//! Editor-state scene builders.
//!
//! The full builder remains available for export, MCP, and preview consumers.
//! Interactive hosts use the active-page builder so inactive pages do not pay
//! ref/token expansion, layout, payload, or scene-allocation costs.

use jian_scene::layout_scene::{LayoutScene, ScenePage};
use op_editor_core::scene_vars::VariableTable;

use crate::editor_state_var_table;
use crate::payload::DocPayload;

/// Build a paint-only [`LayoutScene`] from every page in an editor state.
pub fn editor_state_to_layout_scene(state: &op_editor_core::EditorState) -> LayoutScene {
    let mut prepared = std::borrow::Cow::Borrowed(&state.doc);
    if op_editor_core::ref_resolve::document_has_refs(&prepared) {
        prepared = std::borrow::Cow::Owned(op_editor_core::ref_resolve::resolve_refs_for_canvas(
            &prepared,
        ));
    }
    if op_editor_core::variables_resolve::document_has_tokens(&prepared) {
        prepared = std::borrow::Cow::Owned(
            op_editor_core::variables_resolve::resolve_document_for_canvas(
                &prepared,
                &state.ui.variables.active_theme,
            ),
        );
    }
    let payload: DocPayload = if state.editor_ui.preserve_authored_geometry {
        crate::adapter::pen_document_to_payload_preserving_geometry(&prepared).payload
    } else {
        crate::adapter::pen_document_to_payload(&prepared).payload
    };
    let var_table: VariableTable = editor_state_var_table(state);

    LayoutScene {
        pages: payload
            .pages
            .iter()
            .map(|page| ScenePage {
                id: page.id.clone(),
                name: page.name.clone(),
                children: page
                    .children
                    .iter()
                    .map(|node| crate::layout_scene::node_payload_to_scene(node, &var_table, 1.0))
                    .collect(),
            })
            .collect(),
        active_page_index: state
            .ui
            .active_page_index
            .min(payload.pages.len().saturating_sub(1)),
    }
}

/// Build the interactive editor scene for only the active page.
///
/// Page metadata stays in document order and `active_page_index` keeps its
/// canonical index, but inactive [`ScenePage`] entries have no render children.
pub fn editor_state_to_active_page_layout_scene(
    state: &op_editor_core::EditorState,
) -> LayoutScene {
    let (mut pages, active_page_index, roots): (
        Vec<ScenePage>,
        usize,
        &[jian_ops_schema::node::PenNode],
    ) = if let Some(doc_pages) = state.doc.pages.as_ref() {
        if doc_pages.is_empty() {
            return LayoutScene::default();
        }
        let active_page_index = state
            .ui
            .active_page_index
            .min(doc_pages.len().saturating_sub(1));
        let pages = doc_pages
            .iter()
            .map(|page| ScenePage {
                id: page.id.clone(),
                name: page.name.clone(),
                children: Vec::new(),
            })
            .collect();
        (
            pages,
            active_page_index,
            &doc_pages[active_page_index].children,
        )
    } else {
        // The single-page fallback is not spelled here: `active_page_identity`
        // is the one place that decides what a document without a `pages` array
        // calls its page, and this scene's page id has to be that same string —
        // a page-scoped projection (comment pins, the comment rail) is keyed on
        // it, and two rules would place a marker on a page nobody named.
        let (id, name) = state.active_page_identity();
        (
            vec![ScenePage {
                id,
                name,
                children: Vec::new(),
            }],
            0,
            state.doc.children.as_slice(),
        )
    };

    let mut prepared = std::borrow::Cow::Borrowed(roots);
    if op_editor_core::ref_resolve::roots_have_refs(prepared.as_ref()) {
        prepared = std::borrow::Cow::Owned(if state.document_revision() == 0 {
            // A freshly loaded/imported state's registry was built from this
            // exact document. Reusing it avoids indexing every inactive page
            // on each switch. Once any canonical edit has happened the
            // registry may be a stale prototype snapshot, so preserve the old
            // live-document semantics by taking the full lookup path instead.
            op_editor_core::ref_resolve::resolve_refs_for_canvas_roots_with_components(
                prepared.as_ref(),
                &state.components,
                &state.doc,
            )
        } else {
            op_editor_core::ref_resolve::resolve_refs_for_canvas_roots(
                prepared.as_ref(),
                &state.doc,
            )
        });
    }
    if op_editor_core::variables_resolve::roots_have_tokens(prepared.as_ref()) {
        let mut owned = prepared.into_owned();
        op_editor_core::variables_resolve::resolve_roots_for_canvas(
            &mut owned,
            &state.doc,
            &state.ui.variables.active_theme,
        );
        prepared = std::borrow::Cow::Owned(owned);
    }

    let active_meta = &pages[active_page_index];
    let payload = crate::adapter::pen_roots_to_page_payload(
        &active_meta.id,
        &active_meta.name,
        prepared.as_ref(),
        active_page_index,
        state.editor_ui.preserve_authored_geometry,
    );
    let var_table = editor_state_var_table(state);
    pages[active_page_index].children = payload
        .children
        .iter()
        .map(|node| crate::layout_scene::node_payload_to_scene(node, &var_table, 1.0))
        .collect();

    LayoutScene {
        pages,
        active_page_index,
    }
}
