//! Browser-IO regression tests (task: web IME commit, paste-text
//! routing, canonical Save serialization, Figma-node paste). Gated on
//! `codegen` because the serialize / ingest helpers live behind that
//! feature; everything here exercises the pure host / helper layer —
//! no DOM.

use super::WidgetHost;
use crate::repaint_ctx::RepaintContext;
use op_editor_core::editor_ui_state::{DesignMdRequest, FileAction, RecentFile};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsValue;

struct TestRepaintContext {
    host: WidgetHost,
    repaint_count: usize,
}

impl TestRepaintContext {
    fn new(host: WidgetHost) -> Self {
        Self {
            host,
            repaint_count: 0,
        }
    }
}

impl RepaintContext for TestRepaintContext {
    fn host(&self) -> &WidgetHost {
        &self.host
    }

    fn host_mut(&mut self) -> &mut WidgetHost {
        &mut self.host
    }

    fn viewport_size(&self) -> (f32, f32) {
        (1200.0, 800.0)
    }

    fn register_system_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font_from_bytes(&mut self, _bytes: &[u8]) -> Option<String> {
        None
    }

    fn imported_family_list(&self) -> Vec<String> {
        Vec::new()
    }

    fn remove_imported_font(&mut self, _family: &str) {}

    fn repaint(&mut self) -> Result<(), JsValue> {
        self.repaint_count += 1;
        Ok(())
    }
}

#[test]
fn ime_commit_lands_in_focused_chat_input() {
    let mut host = WidgetHost::new();
    host.editor_state.chat.focused = true;
    let evt = crate::event::ime::composition_end("你好".to_string());
    assert!(host.apply_ime(&evt));
    assert_eq!(host.editor_state.chat.input.text(), "你好");
}

#[test]
fn ime_preedit_updates_do_not_mutate_the_input() {
    let mut host = WidgetHost::new();
    host.editor_state.chat.focused = true;
    let start = crate::event::ime::composition_start();
    let update = crate::event::ime::composition_update("ni".to_string(), None);
    assert!(!host.apply_ime(&start));
    assert!(!host.apply_ime(&update));
    assert!(host.editor_state.chat.input.text().is_empty());
}

#[test]
fn ime_commit_without_any_focused_input_is_a_no_op() {
    let mut host = WidgetHost::new();
    let evt = crate::event::ime::composition_end("漢字".to_string());
    assert!(!host.apply_ime(&evt));
    assert!(host.editor_state.chat.input.text().is_empty());
}

#[test]
fn paste_text_routes_to_focused_rename() {
    let mut host = WidgetHost::new();
    // Begin an inline rename on the blank starter frame like a
    // layer-row double-click would.
    host.editor_state
        .set_single_selection(op_editor_core::NodeId::new("n10"));
    let id = host.editor_state.selection.anchor.clone();
    assert!(host.editor_state.start_rename_layer(id));
    // Select-all so the paste replaces the seeded name deterministically.
    host.editor_state
        .ui
        .layer_rename
        .as_mut()
        .expect("rename active")
        .input
        .select_all();
    assert!(host.apply_paste_text("Hero Section"));
    assert_eq!(
        host.editor_state
            .ui
            .layer_rename
            .as_ref()
            .expect("rename still active")
            .input
            .text(),
        "Hero Section"
    );
    // The paste went into the rename draft, NOT the chat input.
    assert!(host.editor_state.chat.input.text().is_empty());
}

#[test]
fn paste_text_routes_to_focused_chat_input() {
    let mut host = WidgetHost::new();
    host.editor_state.chat.focused = true;
    assert!(host.apply_paste_text("hello web"));
    assert_eq!(host.editor_state.chat.input.text(), "hello web");
}

#[test]
fn paste_text_without_any_focused_input_is_a_no_op() {
    let mut host = WidgetHost::new();
    assert!(!host.apply_paste_text("nowhere to go"));
}

#[test]
fn save_serializes_the_canonical_document_json() {
    let host = WidgetHost::new();
    let json =
        crate::file_actions::serialize_document(host.editor_state()).expect("serialize succeeds");
    // The Save payload is the canonical document JSON plus the same embedded
    // active-page metadata the desktop writer persists.
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("saved JSON");
    let expected = serde_json::to_value(&host.editor_state().doc).expect("serde serializes");
    assert_eq!(parsed["version"], expected["version"]);
    assert_eq!(parsed["children"], expected["children"]);
    assert_eq!(parsed["editorMeta"]["activePageIndex"], 0);
    assert_eq!(parsed["editorMeta"]["preserveAuthoredGeometry"], false);
    assert!(json.contains("\"version\""));
    // And it round-trips through the same canonical loader the web
    // Open path uses.
    let ingested = crate::file_actions::ingest_op_source(&json, host.editor_state())
        .expect("canonical round-trip parses");
    assert_eq!(
        ingested.state.doc.children.len(),
        host.editor_state().doc.children.len()
    );
}

#[test]
fn figma_preserve_geometry_survives_web_save_open_round_trip() {
    let mut host = WidgetHost::new();
    host.editor_state.add_page();
    host.editor_state.ui.active_page_index = 1;
    host.editor_state.editor_ui.preserve_authored_geometry = true;

    let json =
        crate::file_actions::serialize_document(host.editor_state()).expect("serialize succeeds");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("saved JSON");
    assert_eq!(parsed["editorMeta"]["preserveAuthoredGeometry"], true);

    let ingested = crate::file_actions::ingest_op_source(&json, host.editor_state())
        .expect("canonical round-trip parses");
    assert_eq!(ingested.state.ui.active_page_index, 1);
    assert!(ingested.state.editor_ui.preserve_authored_geometry);
}

#[test]
fn legacy_op_without_preserve_metadata_reopens_in_layout_mode() {
    let mut host = WidgetHost::new();
    // The previous document must not leak its Figma-import mode into an old
    // `.op` that predates preserveAuthoredGeometry.
    host.editor_state.editor_ui.preserve_authored_geometry = true;
    let legacy = r#"{
      "version":"1.0.0",
      "children":[],
      "pages":[
        {"id":"p1","name":"One","children":[]},
        {"id":"p2","name":"Two","children":[]}
      ],
      "editorMeta":{"activePageIndex":1}
    }"#;

    let ingested = crate::file_actions::ingest_op_source(legacy, host.editor_state())
        .expect("legacy canonical document parses");
    assert_eq!(ingested.state.ui.active_page_index, 1);
    assert!(!ingested.state.editor_ui.preserve_authored_geometry);
}

#[test]
fn drain_file_action_clear_recent_matches_desktop_state_change() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.recent_files = vec![
        RecentFile {
            path: "/tmp/a.op".to_string(),
            modified_at: 1,
        },
        RecentFile {
            path: "/tmp/b.op".to_string(),
            modified_at: 2,
        },
    ];
    host.editor_state.editor_ui.pending_file_action = Some(FileAction::ClearRecent);
    host.editor_state_dirty = false;
    let inner = Rc::new(RefCell::new(TestRepaintContext::new(host)));

    crate::dom_io::drain_pending_file_action(&inner);

    let borrowed = inner.borrow();
    assert!(borrowed.host.editor_state.editor_ui.recent_files.is_empty());
    assert!(borrowed
        .host
        .editor_state
        .editor_ui
        .pending_file_action
        .is_none());
    assert!(borrowed.host.editor_state_dirty);
    assert_eq!(borrowed.repaint_count, 1);
}

#[test]
fn drain_design_md_auto_generate_consumes_empty_document_request() {
    let mut host = WidgetHost::new();
    host.editor_state.doc.children.clear();
    host.editor_state.editor_ui.design_md_panel.request = Some(DesignMdRequest::AutoGenerate);
    host.editor_state.editor_ui.design_md_panel.generating = false;
    let inner = Rc::new(RefCell::new(TestRepaintContext::new(host)));

    crate::web_design_md::drain_design_md_action(&inner);

    let borrowed = inner.borrow();
    assert!(borrowed
        .host
        .editor_state
        .editor_ui
        .design_md_panel
        .request
        .is_none());
    assert!(
        !borrowed
            .host
            .editor_state
            .editor_ui
            .design_md_panel
            .generating
    );
    assert_eq!(borrowed.repaint_count, 0);
}

#[test]
fn open_recent_success_response_touches_local_recent_entry() {
    let mut host = WidgetHost::new();
    // A tab showing a stored document that asks the daemon to open a PATH: the
    // document it ends up with is that file, so the stored document's key must
    // not survive it (issue #92).
    host.editor_state
        .editor_ui
        .set_document_key(Some("oldkey".to_string()));
    host.editor_state.editor_ui.recent_files = vec![
        RecentFile {
            path: "/tmp/a.op".to_string(),
            modified_at: 1,
        },
        RecentFile {
            path: "/tmp/b.op".to_string(),
            modified_at: 2,
        },
    ];

    let changed = crate::file_actions::apply_open_recent_response(
        host.editor_state_mut(),
        "/tmp/b.op",
        r#"{"ok":true,"version":3}"#,
        99,
    );

    assert!(changed);
    assert_eq!(
        host.editor_state.editor_ui.recent_files[0].path,
        "/tmp/b.op"
    );
    assert_eq!(host.editor_state.editor_ui.recent_files[0].modified_at, 99);
    assert_eq!(host.editor_state.editor_ui.recent_files.len(), 2);
    assert_eq!(
        host.editor_state.editor_ui.file_name_display.as_deref(),
        Some("b.op")
    );
    assert_eq!(
        host.editor_state().editor_ui.file_key,
        None,
        "the open document is now the daemon's local file, not the stored one"
    );
}

#[test]
fn open_recent_stale_response_prunes_local_recent_entry() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.recent_files = vec![RecentFile {
        path: "/tmp/missing.op".to_string(),
        modified_at: 1,
    }];

    let changed = crate::file_actions::apply_open_recent_response(
        host.editor_state_mut(),
        "/tmp/missing.op",
        r#"{"ok":false,"pruned":true,"error":"missing"}"#,
        99,
    );

    assert!(changed);
    assert!(host.editor_state.editor_ui.recent_files.is_empty());
}

#[test]
fn ingest_preserves_app_preferences_from_the_previous_state() {
    use op_editor_core::editor_ui_state::ThemeMode;
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.theme_mode = ThemeMode::Light;
    host.editor_state.editor_ui.locale = op_editor_core::Locale::ZhCn;
    let json =
        crate::file_actions::serialize_document(host.editor_state()).expect("serialize succeeds");
    let ingested = crate::file_actions::ingest_op_source(&json, host.editor_state())
        .expect("canonical parse succeeds");
    assert_eq!(ingested.state.editor_ui.theme_mode, ThemeMode::Light);
    assert_eq!(
        ingested.state.editor_ui.locale,
        op_editor_core::Locale::ZhCn
    );
}

#[test]
fn save_file_name_falls_back_to_untitled() {
    let mut host = WidgetHost::new();
    assert_eq!(
        crate::file_actions::save_file_name(host.editor_state()),
        "untitled.op"
    );
    host.editor_state.editor_ui.file_name_display = Some("login.op".to_string());
    assert_eq!(
        crate::file_actions::save_file_name(host.editor_state()),
        "login.op"
    );
}

#[test]
fn drop_kind_classifies_by_extension_case_insensitively() {
    use crate::file_actions::{drop_kind, DropKind};
    assert_eq!(drop_kind("design.op"), DropKind::Document);
    assert_eq!(drop_kind("Design.PEN"), DropKind::Document);
    assert_eq!(drop_kind("Mockup.FIG"), DropKind::Figma);
    assert_eq!(drop_kind("icon.svg"), DropKind::Svg);
    assert_eq!(drop_kind("photo.JPEG"), DropKind::Image);
    assert_eq!(drop_kind("notes.txt"), DropKind::HtmlResource);
    assert_eq!(drop_kind("notes.md"), DropKind::Unsupported);
}

#[test]
fn paste_figma_nodes_inserts_clones_and_selects_them() {
    let mut host = WidgetHost::new();
    let before = host.editor_state.doc.children.len();
    // Donor nodes — the starter frame from a second state stands in
    // for a decoded Figma clipboard payload (same `PenNode` type).
    let donor = op_editor_core::EditorState::starter();
    let nodes = donor.doc.children.clone();
    assert!(!nodes.is_empty(), "starter doc has a frame to donate");
    assert!(host.paste_figma_nodes(nodes, 960.0, 640.0));
    assert_eq!(host.editor_state.doc.children.len(), before + 1);
    assert!(host.editor_state.selection.anchor.is_real());
}

#[test]
fn paste_figma_nodes_with_no_nodes_is_a_no_op() {
    let mut host = WidgetHost::new();
    let before = host.editor_state.doc.children.len();
    assert!(!host.paste_figma_nodes(Vec::new(), 960.0, 640.0));
    assert_eq!(host.editor_state.doc.children.len(), before);
}

#[test]
fn install_ingested_state_preserves_live_chrome_and_clears_progress() {
    use op_editor_core::editor_ui_state::ThemeMode;
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.theme_mode = ThemeMode::Light;
    host.editor_state.editor_ui.figma_import_in_progress = true;
    host.editor_state.editor_ui.file_name_display = Some("old.op".to_string());
    host.editor_state.chat.title = "Existing conversation".to_string();

    let mut incoming = op_editor_core::EditorState::starter();
    incoming.editor_ui.preserve_authored_geometry = true;
    host.install_ingested_state(incoming);

    let ui = &host.editor_state.editor_ui;
    assert_eq!(ui.theme_mode, ThemeMode::Light, "live theme survives");
    assert!(!ui.figma_import_in_progress, "progress overlay clears");
    assert!(ui.preserve_authored_geometry, "import geometry flag wins");
    assert_eq!(
        ui.file_name_display, None,
        "imported documents start untitled"
    );
    assert_eq!(
        host.editor_state.chat.title, "Existing conversation",
        "document imports preserve chat sessions"
    );
}

#[test]
fn unsaved_import_is_dirty_while_an_opened_document_install_stays_clean() {
    let mut imported = WidgetHost::new();
    let imported_epoch = imported.document_epoch();
    imported.install_unsaved_ingested_state(op_editor_core::EditorState::starter());
    assert_ne!(imported.document_epoch(), imported_epoch);
    assert!(imported.editor_state().is_dirty());
    assert!(imported.editor_state().editor_ui.document_dirty);

    let mut opened = WidgetHost::new();
    let opened_epoch = opened.document_epoch();
    opened.install_ingested_state(op_editor_core::EditorState::starter());
    assert_ne!(opened.document_epoch(), opened_epoch);
    assert!(!opened.editor_state().is_dirty());
    assert!(!opened.editor_state().editor_ui.document_dirty);
}

#[test]
fn opened_op_state_arms_missing_font_detection_through_ingest_install() {
    let mut host = WidgetHost::new();
    let source = r#"{"version":"0.8.0","children":[
        {"type":"text","id":"t1","name":"Title","x":0,"y":0,"width":100,"height":24,
         "content":"Hello","fontFamily":"__WebOpenedOpMissingFont__"}
    ]}"#;
    let mut ingested = crate::file_actions::ingest_op_source(source, host.editor_state())
        .expect("canonical .op source");
    ingested.state.editor_ui.file_name_display = Some("font-check.op".to_string());

    host.install_ingested_state(ingested.state);

    assert!(host.editor_state.editor_ui.missing_fonts_pending_detect);
    assert_eq!(
        host.editor_state.editor_ui.file_name_display.as_deref(),
        Some("font-check.op")
    );
}

#[test]
fn export_svg_document_returns_vector_markup() {
    let host = WidgetHost::new();
    let svg = crate::file_actions::export_svg_document(&host.editor_state)
        .expect("starter document exports svg");

    assert!(svg.starts_with("<svg "), "missing SVG root: {svg}");
    assert!(
        svg.contains("<rect ") || svg.contains("<text "),
        "starter export should contain vector elements: {svg}"
    );
    assert!(svg.ends_with("</svg>"), "missing SVG close: {svg}");
}

#[test]
fn export_svg_document_uses_single_selection() {
    let mut state = op_editor_core::EditorState::sample();
    state.set_single_selection(op_editor_core::NodeId::new("n11"));

    let svg = crate::file_actions::export_svg_document(&state).expect("selected svg");

    assert!(svg.contains("n11"), "selected node missing: {svg}");
    assert!(!svg.contains("n13"), "unselected sibling leaked: {svg}");
}

/// One comment thread, enough to see whether a conversation survived an install.
fn a_thread() -> op_editor_core::editor_ui_state::CommentThread {
    use op_editor_core::editor_ui_state::{Comment, CommentAnchor, CommentThread};
    CommentThread {
        id: 1,
        anchor: Some(CommentAnchor::new("p1", 10.0, 20.0)),
        comments: vec![Comment::default()],
        ..CommentThread::default()
    }
}

/// A tab showing the stored document `key1`, with a conversation and a name.
fn tab_on_a_stored_document() -> WidgetHost {
    let mut host = WidgetHost::new();
    {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.set_document_key(Some("key1".to_string()));
        ui.file_name_display = Some("server.op".to_string());
        ui.comments
            .install_threads_for_key(Some("key1".to_string()), vec![a_thread()]);
    }
    host
}

#[test]
fn installing_a_local_file_drops_the_previous_server_key() {
    // Issue #92. The document's server key names what Save, autosave and the
    // comment routes address (`/api/files/<key>/…`), so a document read from the
    // user's disk must not inherit the key of the stored document it replaced —
    // otherwise its contents are written into a file the user never opened.
    let mut host = tab_on_a_stored_document();
    assert_eq!(
        op_editor_core::route::file_from_key(host.editor_state()),
        op_editor_core::route::RouteFile::Key("key1".to_string()),
        "the tab starts out as the stored document"
    );

    host.install_ingested_state(op_editor_core::EditorState::starter());

    assert_eq!(
        host.editor_state().editor_ui.file_key,
        None,
        "a local document has no server identity to inherit"
    );
    // The address bar is derived from the same state, so this is also what stops
    // the tab advertising `/f/<old key>` — a reload of that link would open a
    // document the user is not editing.
    assert_eq!(
        op_editor_core::route::file_from_key(host.editor_state()),
        op_editor_core::route::RouteFile::Untitled
    );
}

#[test]
fn installing_a_local_file_leaves_the_replaced_documents_conversation_behind() {
    // Same rule, one step further out: a thread is anchored to a node of ONE
    // document, so the replaced document's pins must not be painted over the
    // local file's pages — and, with no key left, there is no route to read or
    // write them under either.
    let mut host = tab_on_a_stored_document();
    assert_eq!(host.editor_state().editor_ui.comments.thread_ids(), vec![1]);

    host.install_ingested_state(op_editor_core::EditorState::starter());

    let comments = &host.editor_state().editor_ui.comments;
    assert!(
        comments.threads.is_empty(),
        "the previous document's conversation is not this document's"
    );
    assert_eq!(comments.document_key(), None);
}

#[test]
fn an_import_into_a_tab_showing_a_stored_document_also_drops_the_key() {
    // Every import seam is the same seam: `install_unsaved_ingested_state`
    // (Figma / HTML) goes through the install above, so an import cannot keep a
    // server identity either — an imported document has no server file at all.
    let mut host = tab_on_a_stored_document();

    host.install_unsaved_ingested_state(op_editor_core::EditorState::starter());

    assert_eq!(host.editor_state().editor_ui.file_key, None);
    assert!(host.editor_state().editor_ui.comments.threads.is_empty());
}

#[test]
fn a_daemon_apply_of_the_open_document_keeps_its_key() {
    // The other half of the rule, and the reason the key is NOT cleared in
    // `clear_document_derived`: the daemon re-installs its own copy of the same
    // document on every version bump (an AI turn, an MCP write), and the tab
    // must stay the same document across all of them.
    let mut host = tab_on_a_stored_document();

    host.replace_document_from_sync(op_editor_core::EditorState::starter().doc, false);

    assert_eq!(
        host.editor_state().editor_ui.file_key.as_deref(),
        Some("key1"),
        "a live-sync apply of the same document must not erase its identity"
    );
    assert_eq!(
        host.editor_state().editor_ui.comments.thread_ids(),
        vec![1],
        "and the conversation filed under that key is still its own"
    );
}
