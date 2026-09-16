//! `LayerPanel` — left-rail document tree (Pages + Layers sections).
//!
//! Its page rows and depth-flattened layer rows are built from the canonical
//! `PenNode` tree on `op_editor_core::EditorState`.
//!
//! This is the panel's spine: the row model, the constructors that build it
//! (through [`layer_panel_cache`]), and the read-only accessors the hosts
//! call. The row paint pass lives in `layer_panel_paint_pass.rs`, hit-test
//! and drop-target resolution in `layer_panel_hit.rs`, the tree walks in
//! `layer_panel_walkers.rs`, and the leaf painters in `layer_panel_paint.rs`.

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::icons::Icon;
use crate::widgets::layer_panel_cache::{self, CachedLayerRows};
use crate::widgets::layer_panel_metrics::LayerPanelMetrics;
use crate::widgets::layer_panel_page_comments::page_comment_counts;
use crate::widgets::layer_panel_walkers::{
    apply_layer_rename, build_layer_rows, components_from_state, icon_for_node, kind_label,
    layer_regions, layers_content_width, pages_content_width, pages_from_state, walk_excluding,
    LayerRegionInput, LayerRegions, LayerScrollSnapshot, RenameView, WalkCx,
};
use crate::widgets::WidgetId;
use crate::Rect;
use jian_core::text_input::TextInputState;
use op_editor_core::{NodeId, SelectionState};
use std::rc::Rc;

use jian_ops_schema::node::PenNode;
use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::EditorState;

/// Outer panel width; host layout uses this.
pub const LAYER_PANEL_WIDTH: f32 = 240.0;

/// Translate a chrome string key against the active editor locale.
fn t(ui: &EditorUiState, key: &'static str) -> &'static str {
    crate::widgets::editor_state_ext::translate(ui, key)
}

pub(crate) const ROW_PAD_X: f32 = 12.0;

/// One row in the layers tree — flat depth-walked view.
#[derive(Debug, Clone)]
pub struct LayerItem {
    pub node_id: NodeId,
    pub label: String,
    pub kind_label: String,
    pub icon: Icon,
    pub depth: usize,
    pub has_children: bool,
    pub hidden: bool,
    pub locked: bool,
    pub collapsed: bool,
    /// True when the row can host children (Frame/Group); gates
    /// the middle drag-Into band.
    pub is_container: bool,
    /// Active inline-rename target — paints the input instead.
    pub renaming: bool,
    /// Reusable COMPONENT definition — Diamond icon + #a855f7 tint.
    pub is_reusable: bool,
    /// Component INSTANCE (`Ref`) — Diamond icon + #9281f7 tint.
    pub is_instance: bool,
}

/// Pages-section row.
#[derive(Debug, Clone)]
pub struct PageItem {
    pub page_index: usize,
    pub label: String,
    pub active: bool,
    pub renaming: bool,
}

pub struct LayerPanel {
    pub id: WidgetId,
    /// Cached, styling-neutral row models (shared via `Rc`; a cache hit
    /// is a refcount bump, not a per-row re-allocation).
    pub pages: Rc<Vec<PageItem>>,
    pub components: Rc<Vec<PageItem>>,
    /// Shipped recipes — one row per ready-made composition document.
    pub recipes: Rc<Vec<PageItem>>,
    pub items: Rc<Vec<LayerItem>>,
    /// Open comment threads pinned on each document page, indexed by
    /// `PageItem::page_index` — the number a page row's marker shows.
    ///
    /// A live overlay rather than part of the cached row model, for the reason
    /// `layer_panel_page_comments` states: a comment arriving or being resolved
    /// does not touch the document revision the row cache keys on, so a count
    /// baked into the rows would go stale without anything to invalidate it.
    pub page_comments: Rc<Vec<usize>>,
    pub theme: Theme,
    pub(crate) metrics: LayerPanelMetrics,
    pub pages_label: &'static str,
    pub components_label: &'static str,
    pub recipes_label: &'static str,
    pub layers_label: &'static str,
    pub drop_target: Option<DropTarget>,
    pub drag_ghost: Option<(LayerItem, f32)>,
    pub now_ms: u64,
    pub rename_input: Option<TextInputState>,
    /// Live styling overlay — selection + hover are applied at paint /
    /// hit-test time so they never invalidate the cached row model.
    pub selection: SelectionState,
    pub hovered_layer: Option<NodeId>,
    pub hovered_page: Option<usize>,
    /// Scroll state for the bounded Pages / Layers regions.
    pub pages_scroll: LayerScrollSnapshot,
    pub recipes_scroll: LayerScrollSnapshot,
    pub components_scroll: LayerScrollSnapshot,
    pub layers_scroll: LayerScrollSnapshot,
}

impl LayerPanel {
    /// Allocate a process-unique owner id for a persistent host's
    /// LayerPanel cache slot (mirrors `AIChatPlaceholder::next_owner`).
    /// A host pulls an id at construction, passes it to
    /// [`Self::from_editor_owned`] on paint and event paths, and pulls a FRESH
    /// id after every whole-document replacement
    /// (`force_rotate_layer_panel_owner`) — ids are stable only between
    /// replacements.
    pub fn next_layer_panel_owner() -> u64 {
        layer_panel_cache::next_owner()
    }

    /// Build the panel from the editor state via the CACHE-BYPASS path —
    /// always a fresh walk that never touches the shared slot. Used primarily
    /// by unit tests and one-off callers without a persistent owner. Hosts call
    /// [`Self::from_editor_owned`] for paint and event-time hit tests so those
    /// paths share the same row model.
    pub fn from_editor(state: &EditorState) -> Self {
        Self::from_editor_owned(state, layer_panel_cache::UNOWNED)
    }

    /// Owner-scoped build for a persistent host's paint and event paths.
    /// Resolves the row model through the thread-local cache scoped to
    /// `owner`; rebuilds only when the document revision / active page /
    /// collapsed set / rename draft change. Selection + hover are applied as
    /// a live overlay (see `layer_panel_cache`), so a selection-/hover-only
    /// change reuses the cached rows.
    pub fn from_editor_owned(state: &EditorState, owner: u64) -> Self {
        let metrics = LayerPanelMetrics::for_ui(&state.editor_ui);
        let rows =
            layer_panel_cache::resolve_owned(owner, state, || build_layer_rows(state, metrics));
        // Recipes come from the session kit, not from the document: they are
        // shipped compositions, so a brand-new empty file lists them too.
        let recipes = Self::recipes_from_kit();
        Self::assemble(state, metrics, rows, recipes)
    }

    /// Live selection overlay — true when `id` is in the editor
    /// selection. Paint / hit-test read this instead of a baked flag so
    /// selection changes never invalidate the cached row model.
    pub fn is_row_selected(&self, id: &NodeId) -> bool {
        self.selection.contains(id)
    }

    /// Live hover overlay for a layer row.
    pub fn is_row_hovered(&self, id: &NodeId) -> bool {
        self.hovered_layer.as_ref() == Some(id)
    }

    /// Live hover overlay for a page row.
    pub fn is_page_hovered(&self, page_index: usize) -> bool {
        self.hovered_page == Some(page_index)
    }

    /// Assemble a panel from an already-built (cached or fresh) row model,
    /// stamping the live styling overlay + scroll snapshots + chrome.
    ///
    /// The row model arrives as the `CachedLayerRows` the cache resolved
    /// rather than as its seven fields taken apart: the rows and their content
    /// widths are produced together by one walk and only ever make sense
    /// together, and a caller that passed them in a different order would be
    /// assembling a panel from two different documents.
    fn assemble(
        state: &EditorState,
        metrics: LayerPanelMetrics,
        rows: Rc<CachedLayerRows>,
        recipes: Rc<Vec<PageItem>>,
    ) -> Self {
        Self {
            id: WidgetId::new(1000),
            pages: rows.pages.clone(),
            components: rows.components.clone(),
            recipes,
            items: rows.items.clone(),
            page_comments: page_comment_counts(state),
            theme: theme_for(&state.editor_ui),
            metrics,
            pages_label: t(&state.editor_ui, "pages.title"),
            components_label: t(&state.editor_ui, "components.title"),
            recipes_label: t(&state.editor_ui, "recipes.title"),
            layers_label: t(&state.editor_ui, "layers.title"),
            drop_target: None,
            drag_ghost: None,
            now_ms: 0,
            rename_input: state.ui.layer_rename.as_ref().map(|r| r.input.clone()),
            selection: state.selection.clone(),
            hovered_layer: state.editor_ui.hovered_layer_id.clone(),
            hovered_page: state.editor_ui.hovered_page_index,
            pages_scroll: LayerScrollSnapshot::new(
                state.editor_ui.layer_pages_scroll,
                state.editor_ui.layer_pages_h_scroll,
                rows.pages_content_width,
            ),
            components_scroll: LayerScrollSnapshot::new(
                state.editor_ui.layer_components_scroll,
                state.editor_ui.layer_components_h_scroll,
                rows.components_content_width,
            ),
            recipes_scroll: LayerScrollSnapshot::new(
                state.editor_ui.layer_components_scroll,
                state.editor_ui.layer_components_h_scroll,
                rows.components_content_width,
            ),
            layers_scroll: LayerScrollSnapshot::new(
                state.editor_ui.layer_layers_scroll,
                state.editor_ui.layer_layers_h_scroll,
                rows.layers_content_width,
            ),
        }
    }

    /// The session kit's recipes as panel rows.
    ///
    /// They come from the kit rather than the document, so an empty new file
    /// lists them exactly like an opened one.
    fn recipes_from_kit() -> Rc<Vec<PageItem>> {
        Rc::new(
            op_editor_core::session_kit()
                .recipes
                .iter()
                .enumerate()
                .map(|(index, recipe)| PageItem {
                    page_index: index,
                    label: recipe.name.clone(),
                    active: false,
                    renaming: false,
                })
                .collect(),
        )
    }

    /// Floating ghost row for the dragged source — host paints it
    /// at the cursor's y. None when the source isn't on the
    /// active page.
    pub fn ghost_item_for(state: &EditorState, source: &NodeId) -> Option<LayerItem> {
        let node = op_editor_core::walkers::find_node(state.active_children(), source)?;
        let base = node.base();
        Some(LayerItem {
            node_id: source.clone(),
            // Name-or-kind fallback, same as `item_for` (TS `name ?? type`)
            // so a nameless node's drag ghost isn't blank.
            label: base
                .name
                .clone()
                .unwrap_or_else(|| kind_label(node).to_string()),
            kind_label: kind_label(node).to_string(),
            icon: icon_for_node(node),
            depth: 0,
            has_children: node.children().map(|c| !c.is_empty()).unwrap_or(false),
            hidden: base.visible == Some(false),
            locked: base.locked.unwrap_or(false),
            collapsed: state.editor_ui.collapsed_layers.contains(source),
            // Reparent-into drop targets match TS CONTAINER_TYPES
            // (layer-panel.tsx:14 — frame/group/rectangle/ref).
            is_container: matches!(
                node,
                PenNode::Frame(_) | PenNode::Group(_) | PenNode::Rectangle(_) | PenNode::Ref(_)
            ),
            renaming: false,
            is_reusable: matches!(node, PenNode::Frame(f) if f.reusable == Some(true)),
            is_instance: matches!(node, PenNode::Ref(_)),
        })
    }

    /// Panel for a drag-in-progress — layer list excludes the
    /// dragged source's subtree, mirroring the post-commit layout
    /// so `drop_target_at` returns y values that match where the
    /// source lands after reorder.
    pub fn from_editor_with_drag_source(state: &EditorState, drag_source: &NodeId) -> Self {
        // Active drag rows bypass the idle cache because their source subtree
        // is excluded; selection and hover still overlay via `assemble`.
        let rename = RenameView::from_state(state);
        let pages = pages_from_state(state, &rename);
        let components = components_from_state(state);
        let cx = WalkCx::from_state(state);
        let mut items = Vec::new();
        for child in state.active_children() {
            walk_excluding(child, &cx, drag_source, 0, &mut items);
        }
        apply_layer_rename(&mut items, &rename);
        let metrics = LayerPanelMetrics::for_ui(&state.editor_ui);
        let pages_w = pages_content_width(&pages, LAYER_PANEL_WIDTH, metrics);
        let components_w = pages_content_width(&components, LAYER_PANEL_WIDTH, metrics);
        let layers_w = layers_content_width(&items, LAYER_PANEL_WIDTH, metrics);
        Self::assemble(
            state,
            metrics,
            Rc::new(CachedLayerRows {
                pages: Rc::new(pages),
                components: Rc::new(components),
                items: Rc::new(items),
                pages_content_width: pages_w,
                components_content_width: components_w,
                layers_content_width: layers_w,
            }),
            Self::recipes_from_kit(),
        )
    }

    pub fn empty() -> Self {
        Self {
            id: WidgetId::new(1000),
            pages: Rc::new(Vec::new()),
            components: Rc::new(Vec::new()),
            recipes: Rc::new(Vec::new()),
            items: Rc::new(Vec::new()),
            page_comments: Rc::new(Vec::new()),
            theme: Theme::dark(),
            metrics: LayerPanelMetrics::DESKTOP,
            // No editor state (and therefore no locale) is reachable in
            // the empty skeleton — route through the canonical English
            // table instead of hardcoding literals.
            pages_label: op_i18n::translate(op_editor_core::Locale::EnUs, "pages.title"),
            components_label: op_i18n::translate(op_editor_core::Locale::EnUs, "components.title"),
            recipes_label: op_i18n::translate(op_editor_core::Locale::EnUs, "recipes.title"),
            layers_label: op_i18n::translate(op_editor_core::Locale::EnUs, "layers.title"),
            drop_target: None,
            drag_ghost: None,
            now_ms: 0,
            rename_input: None,
            selection: SelectionState::empty(),
            hovered_layer: None,
            hovered_page: None,
            pages_scroll: LayerScrollSnapshot::default(),
            components_scroll: LayerScrollSnapshot::default(),
            recipes_scroll: LayerScrollSnapshot::default(),
            layers_scroll: LayerScrollSnapshot::default(),
        }
    }

    pub(crate) fn intrinsic_height(&self) -> f32 {
        let pages_h = self.metrics.section_header_height
            + self.pages.len() as f32 * self.metrics.page_row_height;
        let components_h = if self.components.is_empty() {
            0.0
        } else {
            self.metrics.section_gap
                + self.metrics.section_header_height
                + self.components.len() as f32 * self.metrics.page_row_height
        };
        let recipes_h = if self.recipes.is_empty() {
            0.0
        } else {
            self.metrics.section_gap
                + self.metrics.section_header_height
                + self.recipes.len() as f32 * self.metrics.page_row_height
        };
        let layers_h = self.metrics.section_header_height
            + self.items.len().max(1) as f32 * self.metrics.layer_row_height;
        pages_h + components_h + recipes_h + self.metrics.section_gap + layers_h + 16.0
    }

    /// Bounded Pages / Layers scroll-region geometry for `rect` —
    /// the single source paint + hit-test + drop-target derive from.
    /// `pub` so the host's wheel handler can route a scroll to the
    /// region under the cursor.
    pub fn regions(&self, rect: Rect) -> LayerRegions {
        layer_regions(LayerRegionInput {
            rect,
            pages_len: self.pages.len(),
            components_len: self.components.len(),
            recipes_len: self.recipes.len(),
            items_len: self.items.len(),
            pages: self.pages_scroll,
            components: self.components_scroll,
            recipes: self.recipes_scroll,
            layers: self.layers_scroll,
            metrics: self.metrics,
        })
    }

    /// Return the layer-list scroll offset that keeps `node_id`'s
    /// row visible inside the clipped Layers viewport.
    pub fn layers_offset_revealing(&self, rect: Rect, node_id: &NodeId) -> Option<f32> {
        let index = self
            .items
            .iter()
            .position(|item| &item.node_id == node_id)?;
        let r = self.regions(rect);
        if r.layers_view_h <= 0.0 {
            return None;
        }
        let row_top = index as f32 * self.metrics.layer_row_height;
        let row_bottom = row_top + self.metrics.layer_row_height;
        let view_top = r.layers.offset;
        let view_bottom = view_top + r.layers_view_h;
        let next = if row_top < view_top {
            row_top
        } else if row_bottom > view_bottom {
            row_bottom - r.layers_view_h
        } else {
            view_top
        };
        Some(next.clamp(0.0, r.layers.max_offset))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerPanelHit {
    Page(usize),
    /// A shipped recipe row — index into the kit's recipe list.
    Recipe(usize),
    Layer(NodeId),
    ToggleHidden(NodeId),
    ToggleLocked(NodeId),
    ToggleCollapsed(NodeId),
    AddPage,
    DeletePage(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    After,
    Into,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DropTarget {
    pub anchor: NodeId,
    pub position: DropPosition,
    pub indicator_y: f32,
}
