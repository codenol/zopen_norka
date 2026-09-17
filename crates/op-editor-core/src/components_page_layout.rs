//! Gallery layout for per-type `Components/{Type}` store pages.
//!
//! Library masters are authored at overlapping origins (often `0,0`). Each
//! component type gets its own page so the left-rail Components list stays
//! short (Button, Logo, …) and opening a row shows only that type.

use super::{component_store_page_name, is_component_store_page, make_page, COMPONENTS_PAGE_NAME};
use crate::id_allocator::{IdAllocError, IdAllocator, SequentialIdAllocator};
use crate::node_id::NodeId;
use crate::pen_node_ext::PenNodeExt;
use crate::state::EditorState;
use jian_ops_schema::node::PenNode;
use std::collections::HashSet;

const ORIGIN_X: f64 = 80.0;
const ORIGIN_Y: f64 = 80.0;
const COLUMN_GAP: f64 = 40.0;
const ROW_GAP: f64 = 56.0;
const GROUP_GAP: f64 = 96.0;
const MAX_ROW_WIDTH: f64 = 1600.0;
const FALLBACK_WIDTH: f64 = 80.0;
const FALLBACK_HEIGHT: f64 = 40.0;

impl EditorState {
    /// Split a legacy single `Components` page (if any) onto per-type pages
    /// and spread each type's masters into a readable gallery.
    /// Returns true when at least one node moved, a page was created, or
    /// sibling order changed.
    pub fn layout_components_page_gallery(&mut self) -> bool {
        let Ok(mut allocator) = SequentialIdAllocator::for_document(&self.doc, 1) else {
            return false;
        };
        let mut taken = self.collect_node_ids();
        let split = split_legacy_components_page(self, &mut allocator, &mut taken).unwrap_or(false);
        let laid_out = layout_all_store_galleries(self);
        split || laid_out
    }

    /// Append masters onto `Components/{Type}` pages, preserving ids.
    pub(super) fn distribute_component_masters(
        &mut self,
        masters: Vec<PenNode>,
        allocator: &mut dyn IdAllocator,
        taken: &mut HashSet<NodeId>,
    ) -> Result<usize, IdAllocError> {
        let mut existing = existing_store_master_ids(self);
        let mut added = 0usize;
        for master in masters {
            let id = master.id_str().to_string();
            if existing.contains(&id) {
                continue;
            }
            existing.insert(id);
            let page_name = component_store_page_name(&group_key(&master));
            ensure_store_page(self, &page_name, allocator, taken)?;
            let pages = self.doc.pages.as_mut().expect("ensure_pages");
            if let Some(page) = pages.iter_mut().find(|page| page.name == page_name) {
                page.children.push(master);
                added += 1;
            }
        }
        split_legacy_components_page(self, allocator, taken)?;
        restack_store_pages(self);
        layout_all_store_galleries(self);
        Ok(added)
    }
}

fn existing_store_master_ids(state: &EditorState) -> HashSet<String> {
    let Some(pages) = state.doc.pages.as_ref() else {
        return HashSet::new();
    };
    pages
        .iter()
        .filter(|page| is_component_store_page(&page.name))
        .flat_map(|page| page.children.iter().map(|node| node.id_str().to_string()))
        .collect()
}

fn ensure_store_page(
    state: &mut EditorState,
    page_name: &str,
    allocator: &mut dyn IdAllocator,
    taken: &mut HashSet<NodeId>,
) -> Result<(), IdAllocError> {
    let exists = state
        .doc
        .pages
        .as_ref()
        .is_some_and(|pages| pages.iter().any(|page| page.name == page_name));
    if exists {
        return Ok(());
    }
    let page_id = allocator.allocate(taken)?;
    state
        .doc
        .pages
        .as_mut()
        .expect("ensure_pages")
        .push(make_page(page_id.into(), page_name.to_string(), Vec::new()));
    Ok(())
}

/// Move children off a legacy `Components` page onto `Components/{Type}`.
fn split_legacy_components_page(
    state: &mut EditorState,
    allocator: &mut dyn IdAllocator,
    taken: &mut HashSet<NodeId>,
) -> Result<bool, IdAllocError> {
    let Some(legacy_idx) = state.doc.pages.as_ref().and_then(|pages| {
        pages
            .iter()
            .position(|page| page.name == COMPONENTS_PAGE_NAME)
    }) else {
        return Ok(false);
    };
    let children = state
        .doc
        .pages
        .as_mut()
        .map(|pages| std::mem::take(&mut pages[legacy_idx].children))
        .unwrap_or_default();
    if children.is_empty() {
        remove_store_page_at(state, legacy_idx);
        return Ok(true);
    }
    let mut changed = false;
    for master in children {
        let page_name = component_store_page_name(&group_key(&master));
        ensure_store_page(state, &page_name, allocator, taken)?;
        let pages = state.doc.pages.as_mut().expect("pages");
        if let Some(page) = pages.iter_mut().find(|page| page.name == page_name) {
            page.children.push(master);
            changed = true;
        }
    }
    if let Some(pages) = state.doc.pages.as_ref() {
        if let Some(idx) = pages
            .iter()
            .position(|page| page.name == COMPONENTS_PAGE_NAME)
        {
            if pages[idx].children.is_empty() {
                remove_store_page_at(state, idx);
                changed = true;
            }
        }
    }
    restack_store_pages(state);
    Ok(changed)
}

fn remove_store_page_at(state: &mut EditorState, idx: usize) {
    let Some(pages) = state.doc.pages.as_mut() else {
        return;
    };
    if idx >= pages.len() || pages.len() <= 1 {
        return;
    }
    pages.remove(idx);
    let len = pages.len();
    if state.ui.active_page_index >= len {
        state.ui.active_page_index = len.saturating_sub(1);
    } else if idx < state.ui.active_page_index {
        state.ui.active_page_index -= 1;
    }
}

fn restack_store_pages(state: &mut EditorState) {
    let new_index = {
        let Some(pages_slot) = state.doc.pages.as_mut() else {
            return;
        };
        let active_id = pages_slot
            .get(state.ui.active_page_index)
            .map(|page| page.id.clone());
        let mut design = Vec::new();
        let mut store = Vec::new();
        for page in pages_slot.drain(..) {
            if is_component_store_page(&page.name) {
                store.push(page);
            } else {
                design.push(page);
            }
        }
        store.sort_by(|a, b| {
            super::component_store_page_label(&a.name)
                .cmp(super::component_store_page_label(&b.name))
        });
        design.append(&mut store);
        let new_index = active_id
            .and_then(|id| design.iter().position(|page| page.id == id))
            .unwrap_or(0)
            .min(design.len().saturating_sub(1));
        *pages_slot = design;
        new_index
    };
    state.ui.active_page_index = new_index;
}

fn layout_all_store_galleries(state: &mut EditorState) -> bool {
    let Some(pages) = state.doc.pages.as_mut() else {
        return false;
    };
    let mut changed = false;
    for page in pages.iter_mut() {
        if !is_component_store_page(&page.name) || page.name == COMPONENTS_PAGE_NAME {
            continue;
        }
        changed |= layout_components_gallery(&mut page.children);
    }
    changed
}

/// Sort masters and place them in wrapping rows with gaps large enough
/// that frame labels do not collide.
pub(super) fn layout_components_gallery(nodes: &mut [PenNode]) -> bool {
    if nodes.is_empty() {
        return false;
    }
    let before: Vec<(String, Option<f64>, Option<f64>)> = nodes
        .iter()
        .map(|node| (node.id_str().to_string(), node.base().x, node.base().y))
        .collect();
    nodes.sort_by(|a, b| {
        group_key(a)
            .cmp(&group_key(b))
            .then_with(|| display_name(a).cmp(&display_name(b)))
            .then_with(|| a.id_str().cmp(b.id_str()))
    });

    let mut x = ORIGIN_X;
    let mut y = ORIGIN_Y;
    let mut row_height = 0.0;
    let mut prev_group: Option<String> = None;
    for node in nodes.iter_mut() {
        let group = group_key(node);
        if prev_group
            .as_ref()
            .is_some_and(|previous| previous != &group)
        {
            y += row_height + GROUP_GAP;
            x = ORIGIN_X;
            row_height = 0.0;
        }
        prev_group = Some(group);
        let (width, height) = cell_size(node);
        if x > ORIGIN_X && x + width > ORIGIN_X + MAX_ROW_WIDTH {
            y += row_height + ROW_GAP;
            x = ORIGIN_X;
            row_height = 0.0;
        }
        node.base_mut().x = Some(x);
        node.base_mut().y = Some(y);
        x += width + COLUMN_GAP;
        row_height = row_height.max(height);
    }
    before
        .iter()
        .zip(nodes.iter())
        .any(|(was, now)| was.0 != now.id_str() || was.1 != now.base().x || was.2 != now.base().y)
}

pub(super) fn group_key(node: &PenNode) -> String {
    if let Some(name) = node.base().name.as_deref() {
        if let Some((head, _)) = name.split_once('/') {
            if !head.is_empty() {
                return head.to_string();
            }
        }
        if !name.is_empty() {
            return name.to_string();
        }
    }
    let id = node.id_str();
    let mut parts = id.split('-');
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => format!("{a}-{b}"),
        _ => id.to_string(),
    }
}

fn display_name(node: &PenNode) -> String {
    node.base()
        .name
        .clone()
        .unwrap_or_else(|| node.id_str().to_string())
}

fn authored_size(node: &PenNode) -> (f64, f64) {
    (
        node.width_px().unwrap_or(FALLBACK_WIDTH).max(1.0),
        node.height_px().unwrap_or(FALLBACK_HEIGHT).max(1.0),
    )
}

/// Spacing cell: frame size plus room for the canvas name label (12 px,
/// ~7 px per character) so variant names do not paint on top of neighbors.
fn cell_size(node: &PenNode) -> (f64, f64) {
    let (width, height) = authored_size(node);
    let label_width = (display_name(node).chars().count() as f64).mul_add(7.0, 8.0);
    (width.max(label_width).max(160.0), height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_mutators::make_page;
    use crate::pen_node_ext::PenNodeExt;

    fn frame(id: &str, name: &str, w: f64, h: f64, x: f64, y: f64) -> PenNode {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "type": "frame",
            "name": name,
            "reusable": true,
            "x": x,
            "y": y,
            "width": w,
            "height": h
        }))
        .expect("frame")
    }

    fn boxes_overlap(a: &PenNode, b: &PenNode) -> bool {
        let (aw, ah) = authored_size(a);
        let (bw, bh) = authored_size(b);
        let ax = a.base().x.unwrap_or(0.0);
        let ay = a.base().y.unwrap_or(0.0);
        let bx = b.base().x.unwrap_or(0.0);
        let by = b.base().y.unwrap_or(0.0);
        ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
    }

    #[test]
    fn overlapping_masters_spread_into_non_overlapping_rows() {
        let mut nodes = vec![
            frame("atom-logo-ai", "Logo/Спектр ИИ", 120.0, 32.0, 0.0, 0.0),
            frame(
                "atom-menu-default",
                "MenuButton/Default",
                40.0,
                40.0,
                0.0,
                0.0,
            ),
            frame("atom-menu-hover", "MenuButton/Hover", 40.0, 40.0, 0.0, 0.0),
            frame(
                "tpl-layout-default",
                "Layout/Default",
                1440.0,
                850.0,
                0.0,
                0.0,
            ),
            frame("atom-status-danger", "Status/Danger", 16.0, 16.0, 0.0, 0.0),
        ];
        assert!(layout_components_gallery(&mut nodes));
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                assert!(
                    !boxes_overlap(&nodes[i], &nodes[j]),
                    "{} overlaps {}",
                    nodes[i].base().name.as_deref().unwrap_or("?"),
                    nodes[j].base().name.as_deref().unwrap_or("?")
                );
            }
        }
        let names: Vec<_> = nodes
            .iter()
            .map(|n| n.base().name.clone().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "Layout/Default",
                "Logo/Спектр ИИ",
                "MenuButton/Default",
                "MenuButton/Hover",
                "Status/Danger",
            ]
        );
        assert_eq!(nodes[2].base().y, nodes[3].base().y);
        let menu_gap = nodes[3].base().x.unwrap() - nodes[2].base().x.unwrap();
        assert!(
            menu_gap >= 160.0,
            "same-type variants must leave room for name labels, got {menu_gap}"
        );
        assert!(nodes[0].base().y.unwrap() < nodes[1].base().y.unwrap());
    }

    #[test]
    fn single_master_moves_to_gallery_origin() {
        let mut nodes = vec![frame("only", "Button/Default", 80.0, 32.0, 12.0, 12.0)];
        assert!(layout_components_gallery(&mut nodes));
        assert_eq!(nodes[0].base().x, Some(ORIGIN_X));
        assert_eq!(nodes[0].base().y, Some(ORIGIN_Y));
    }

    #[test]
    fn mixed_types_land_on_separate_store_pages() {
        let mut state = EditorState::new();
        let added = state.append_components_page_masters(vec![
            frame("atom-logo", "Logo/Спектр", 120.0, 32.0, 0.0, 0.0),
            frame("atom-btn-a", "Button/Default", 80.0, 32.0, 0.0, 0.0),
            frame("atom-btn-b", "Button/Hover", 80.0, 32.0, 0.0, 0.0),
        ]);
        assert_eq!(added, 3);
        let pages = state.doc.pages.as_ref().unwrap();
        let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Components/Button"));
        assert!(names.contains(&"Components/Logo"));
        assert!(!names.contains(&COMPONENTS_PAGE_NAME));
        assert!(!names.contains(&crate::COMPONENTS_PAGE_PREFIX));
        let button = pages
            .iter()
            .find(|p| p.name == "Components/Button")
            .unwrap();
        assert_eq!(button.children.len(), 2);
        assert_ne!(button.children[0].base().x, button.children[1].base().x);
    }

    #[test]
    fn legacy_components_page_splits_on_layout() {
        let mut state = EditorState::new();
        let _ = state.add_page();
        state.ui.active_page_index = 0;
        let pages = state.doc.pages.as_mut().unwrap();
        pages.push(make_page(
            "legacy-comp".into(),
            COMPONENTS_PAGE_NAME.to_string(),
            vec![
                frame("a", "Avatar/Default", 40.0, 40.0, 0.0, 0.0),
                frame("s", "Status/Danger", 16.0, 16.0, 0.0, 0.0),
            ],
        ));
        assert!(state.layout_components_page_gallery());
        let pages = state.doc.pages.as_ref().unwrap();
        assert!(pages.iter().all(|p| p.name != COMPONENTS_PAGE_NAME));
        assert_eq!(
            pages
                .iter()
                .find(|p| p.name == "Components/Avatar")
                .map(|p| p.children.len()),
            Some(1)
        );
        assert_eq!(
            pages
                .iter()
                .find(|p| p.name == "Components/Status")
                .map(|p| p.children.len()),
            Some(1)
        );
        assert_eq!(state.ui.active_page_index, 0);
    }
}
