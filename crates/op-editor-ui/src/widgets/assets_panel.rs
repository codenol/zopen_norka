//! Left-rail Assets tab — Skala Spectrum type index, search, Insert.
//!
//! Starter/shadcn kits are not listed. Insert queues
//! `pending_skala_insert` with the type's default master id; hosts drain
//! it via `InstantiateComponent`. Details opens the Design-MD panel.

use op_editor_core::{skala_kit, AssetsHit, EditorState, KitLayer, KitType};

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::property_panel_text_input::paint_text_input_view;
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect, TextLayout};

const PAD: f32 = 10.0;
const SEARCH_H: f32 = 32.0;
const SECTION_H: f32 = 22.0;
const ROW_H: f32 = 44.0;
const INSERT_W: f32 = 56.0;
const FONT: f32 = 12.0;
const BADGE_FONT: f32 = 10.0;

#[derive(Debug, Clone, Copy)]
struct RowGeom {
    row: Rect,
    insert: Rect,
}

/// Layout + paint for the Assets rail content (below the tab row).
pub struct AssetsPanel<'a> {
    pub theme: Theme,
    pub search: &'a str,
    pub search_focused: bool,
    pub hover: Option<AssetsHit>,
    pub now_ms: u64,
    types: Vec<&'a KitType>,
}

impl<'a> AssetsPanel<'a> {
    pub fn from_editor(state: &'a EditorState) -> Self {
        let query = state.editor_ui.assets_panel.search.to_ascii_lowercase();
        let kit = skala_kit();
        let types: Vec<&KitType> = kit
            .types
            .iter()
            .filter(|ty| {
                query.is_empty()
                    || ty.name.to_ascii_lowercase().contains(&query)
                    || ty.id.contains(&query)
            })
            .collect();
        Self {
            theme: theme_for(&state.editor_ui),
            search: &state.editor_ui.assets_panel.search,
            search_focused: state.editor_ui.assets_panel.search_focused,
            hover: state.editor_ui.assets_panel.hover,
            now_ms: 0,
            types,
        }
    }

    pub fn filtered_len(&self) -> usize {
        self.types.len()
    }

    pub fn type_at(&self, index: usize) -> Option<&KitType> {
        self.types.get(index).copied()
    }

    fn search_rect(content: Rect) -> Rect {
        Rect {
            origin: Point2D::new(content.origin.x + PAD, content.origin.y + PAD),
            size: Point2D::new((content.size.x - PAD * 2.0).max(0.0), SEARCH_H),
        }
    }

    fn list_origin_y(content: Rect) -> f32 {
        content.origin.y + PAD + SEARCH_H + 8.0
    }

    fn rows(&self, content: Rect, scroll: f32) -> Vec<(usize, KitLayer, RowGeom)> {
        let mut y = Self::list_origin_y(content) - scroll;
        let mut out = Vec::new();
        let mut last_layer: Option<KitLayer> = None;
        let x = content.origin.x + PAD;
        let w = (content.size.x - PAD * 2.0).max(0.0);
        for (index, ty) in self.types.iter().enumerate() {
            if last_layer != Some(ty.layer) {
                y += SECTION_H;
                last_layer = Some(ty.layer);
            }
            let row = Rect {
                origin: Point2D::new(x, y),
                size: Point2D::new(w, ROW_H),
            };
            let insert = Rect {
                origin: Point2D::new(x + w - INSERT_W, y + 8.0),
                size: Point2D::new(INSERT_W, ROW_H - 16.0),
            };
            out.push((index, ty.layer, RowGeom { row, insert }));
            y += ROW_H + 4.0;
        }
        out
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let rows = self.rows(content, 0.0);
        let bottom = rows
            .last()
            .map(|(_, _, g)| g.row.origin.y + g.row.size.y)
            .unwrap_or(Self::list_origin_y(content));
        (bottom - content.origin.y + PAD).max(content.size.y)
    }

    pub fn hit(&self, content: Rect, point: Point2D, scroll: f32) -> Option<AssetsHit> {
        if contains(Self::search_rect(content), point) {
            return Some(AssetsHit::Search);
        }
        for (index, _, geom) in self.rows(content, scroll) {
            if contains(geom.insert, point) {
                return Some(AssetsHit::Insert(index));
            }
            if contains(geom.row, point) {
                return Some(AssetsHit::Row(index));
            }
        }
        None
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, content: Rect, state: &EditorState) {
        cx.backend.fill_rect(content, self.theme.card);
        cx.backend.fill_rect(
            Rect {
                origin: Point2D::new(content.origin.x + content.size.x - 1.0, content.origin.y),
                size: Point2D::new(1.0, content.size.y),
            },
            self.theme.border,
        );
        let search = Self::search_rect(content);
        cx.backend.fill_round_rect(search, 6.0, self.theme.muted);
        let placeholder =
            crate::widgets::editor_state_ext::translate(&state.editor_ui, "assetsPanel.search");
        paint_text_input_view(
            cx,
            &self.theme,
            &state.editor_ui.assets_panel.search_input,
            search,
            FONT,
            10.0,
            search.origin.y + search.size.y / 2.0 + FONT / 2.0 - 1.5,
            self.now_ms,
            placeholder,
            self.search_focused,
        );

        let scroll = state.editor_ui.assets_panel.scroll.offset;
        let mut last_layer: Option<KitLayer> = None;
        let insert_label =
            crate::widgets::editor_state_ext::translate(&state.editor_ui, "assetsPanel.insert");
        for (index, layer, geom) in self.rows(content, scroll) {
            if last_layer != Some(layer) {
                let header_y = geom.row.origin.y - SECTION_H + 4.0;
                if header_y > content.origin.y + SEARCH_H {
                    let heading = layer_label(&state.editor_ui, layer);
                    cx.backend.draw_text(
                        &TextLayout::single_run(
                            heading,
                            "system-ui",
                            BADGE_FONT,
                            self.theme.muted_foreground.to_jian(),
                            Point2D::ZERO,
                        )
                        .with_font_weight(600),
                        Point2D::new(geom.row.origin.x, header_y + BADGE_FONT),
                    );
                }
                last_layer = Some(layer);
            }
            if geom.row.origin.y + geom.row.size.y < content.origin.y
                || geom.row.origin.y > content.origin.y + content.size.y
            {
                continue;
            }
            let hovered = matches!(
                self.hover,
                Some(AssetsHit::Row(i) | AssetsHit::Insert(i) | AssetsHit::Details(i))
                    if i == index
            );
            if hovered {
                cx.backend
                    .fill_round_rect(geom.row, 8.0, self.theme.button_hover);
            }
            let Some(ty) = self.types.get(index) else {
                continue;
            };
            let badge = format!("{}", ty.variant_count);
            draw_icon(
                cx.backend,
                Icon::Component,
                Point2D::new(geom.row.origin.x + 8.0, geom.row.origin.y + 14.0),
                16.0,
                self.theme.foreground,
                1.6,
            );
            cx.backend.draw_text(
                &TextLayout::single_run(
                    &ty.name,
                    "system-ui",
                    FONT,
                    self.theme.foreground.to_jian(),
                    Point2D::ZERO,
                )
                .with_font_weight(500),
                Point2D::new(geom.row.origin.x + 32.0, geom.row.origin.y + 18.0),
            );
            let badge_w =
                text_metrics::measure_chrome_weighted(cx.backend, &badge, BADGE_FONT, 400);
            cx.backend.draw_text(
                &TextLayout::single_run(
                    &badge,
                    "system-ui",
                    BADGE_FONT,
                    self.theme.muted_foreground.to_jian(),
                    Point2D::ZERO,
                ),
                Point2D::new(geom.row.origin.x + 32.0, geom.row.origin.y + 34.0),
            );
            let _ = badge_w;
            let insert_hover = self.hover == Some(AssetsHit::Insert(index));
            if insert_hover {
                cx.backend
                    .fill_round_rect(geom.insert, 6.0, self.theme.button_hover);
            } else {
                cx.backend
                    .fill_round_rect(geom.insert, 6.0, self.theme.muted);
            }
            let iw = text_metrics::measure_chrome_weighted(cx.backend, insert_label, FONT, 500);
            cx.backend.draw_text(
                &TextLayout::single_run(
                    insert_label,
                    "system-ui",
                    FONT,
                    self.theme.foreground.to_jian(),
                    Point2D::ZERO,
                )
                .with_font_weight(500),
                Point2D::new(
                    geom.insert.origin.x + (geom.insert.size.x - iw) / 2.0,
                    geom.insert.origin.y + geom.insert.size.y / 2.0 + FONT / 2.0 - 1.5,
                ),
            );
        }
        if self.types.is_empty() {
            let empty =
                crate::widgets::editor_state_ext::translate(&state.editor_ui, "assetsPanel.empty");
            cx.backend.draw_text(
                &TextLayout::single_run(
                    empty,
                    "system-ui",
                    FONT,
                    self.theme.muted_foreground.to_jian(),
                    Point2D::ZERO,
                ),
                Point2D::new(content.origin.x + PAD, Self::list_origin_y(content) + FONT),
            );
        }
    }
}

fn layer_label(
    ui: &op_editor_core::editor_ui_state::EditorUiState,
    layer: KitLayer,
) -> &'static str {
    let key = match layer {
        KitLayer::Atom => "assetsPanel.layer.atom",
        KitLayer::Molecule => "assetsPanel.layer.molecule",
        KitLayer::Organism => "assetsPanel.layer.organism",
        KitLayer::Template => "assetsPanel.layer.template",
        KitLayer::Recipe => "assetsPanel.layer.template",
    };
    crate::widgets::editor_state_ext::translate(ui, key)
}

fn contains(rect: Rect, point: Point2D) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.x
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.y
}

/// Press / hover / release for the Assets rail body (not the tab row).
pub fn assets_press(state: &mut EditorState, content: Rect, point: Point2D) -> bool {
    let panel = AssetsPanel::from_editor(state);
    let hit = panel.hit(content, point, state.editor_ui.assets_panel.scroll.offset);
    state.editor_ui.assets_panel.pressed = hit;
    state.editor_ui.assets_panel.hover = hit;
    if hit == Some(AssetsHit::Search) {
        state.editor_ui.assets_panel.search_focused = true;
    } else if hit.is_some() {
        state.editor_ui.assets_panel.search_focused = false;
    }
    hit.is_some() || contains(content, point)
}

pub fn assets_hover(state: &mut EditorState, content: Rect, point: Point2D) -> bool {
    let panel = AssetsPanel::from_editor(state);
    let hit = panel.hit(content, point, state.editor_ui.assets_panel.scroll.offset);
    let changed = state.editor_ui.assets_panel.hover != hit;
    state.editor_ui.assets_panel.hover = hit;
    changed
}

pub fn assets_release(state: &mut EditorState) -> Option<AssetsHit> {
    let hover = state.editor_ui.assets_panel.hover;
    let pressed = state.editor_ui.assets_panel.pressed.take()?;
    if hover != Some(pressed) {
        return None;
    }
    match pressed {
        AssetsHit::Insert(i) => {
            if let Some(ty) = AssetsPanel::from_editor(state).type_at(i) {
                state.editor_ui.pending_skala_insert = Some(ty.default_master_id.clone());
            }
            Some(pressed)
        }
        AssetsHit::Row(i) | AssetsHit::Details(i) => {
            state.editor_ui.design_md_panel.open = true;
            let _ = i;
            Some(pressed)
        }
        AssetsHit::Search => Some(pressed),
    }
}

pub fn assets_scroll(state: &mut EditorState, content: Rect, delta_y: f32) -> bool {
    let panel = AssetsPanel::from_editor(state);
    let max = (panel.content_height(content) - content.size.y).max(0.0);
    crate::util::scroll_by_max(&mut state.editor_ui.assets_panel.scroll, delta_y, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::EditorState;

    #[test]
    fn skala_types_are_listed_and_search_filters_them() {
        let mut state = EditorState::starter();
        let all = AssetsPanel::from_editor(&state).filtered_len();
        assert!(all >= 8);
        state.editor_ui.assets_panel.search = "button".into();
        let filtered = AssetsPanel::from_editor(&state);
        let ids: Vec<&str> = (0..filtered.filtered_len())
            .filter_map(|i| filtered.type_at(i).map(|t| t.id.as_str()))
            .collect();
        assert!(ids.contains(&"button"), "{ids:?}");
        assert!(
            ids.iter().all(|id| id.contains("button")),
            "search must not leak unrelated types: {ids:?}"
        );
        assert!(filtered.filtered_len() < all);
    }

    #[test]
    fn insert_queues_the_types_default_master() {
        let mut state = EditorState::starter();
        let button = (0..AssetsPanel::from_editor(&state).filtered_len())
            .find(|&i| AssetsPanel::from_editor(&state).type_at(i).unwrap().id == "button")
            .expect("button type");
        state.editor_ui.assets_panel.pressed = Some(AssetsHit::Insert(button));
        state.editor_ui.assets_panel.hover = Some(AssetsHit::Insert(button));
        assert_eq!(assets_release(&mut state), Some(AssetsHit::Insert(button)));
        assert_eq!(
            state.editor_ui.pending_skala_insert.as_deref(),
            Some("atom-button-filled-large-accent-default-text")
        );
    }
}
