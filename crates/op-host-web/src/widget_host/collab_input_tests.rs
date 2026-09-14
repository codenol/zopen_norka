use super::WidgetHost;
use op_editor_core::{
    CollabAvailability, CollabConnectionPhase, CollabPanelHover, CollabPanelView, CollabUiAction,
    EditorState, NodeId, PathAnchorMenuState, PenNodeExt,
};
use op_editor_ui::widgets::{CollabPanel, TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

const TWO_RECTS: &str = r#"{"version":"1.0.0","children":[
  {"type":"rectangle","id":"n1","name":"One","x":10,"y":20,"width":100,"height":50},
  {"type":"rectangle","id":"n2","name":"Two","x":200,"y":20,"width":100,"height":50}
]}"#;

fn focus_join(host: &mut WidgetHost) {
    let collab = &mut host.editor_state.editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.phase = CollabConnectionPhase::Idle;
    collab.panel.open = true;
    collab.panel.view = CollabPanelView::Join;
    collab.panel.join_address_focused = true;
    collab.panel.join_input.set_text("");
}

fn host_with_selected_node() -> WidgetHost {
    let document = jian_ops_schema::load_str(TWO_RECTS)
        .expect("fixture JSON parses")
        .value;
    let mut host = WidgetHost::new();
    host.editor_state = EditorState::from_document(document);
    host.editor_state.set_single_selection(NodeId::new("n1"));
    focus_join(&mut host);
    host
}

#[test]
fn join_field_owns_web_text_ime_paste_backspace_and_enter() {
    let mut host = WidgetHost::new();
    focus_join(&mut host);

    assert!(host.input_active());
    assert!(host.non_chat_input_owns_keyboard());
    assert!(
        host.text_input_focus_active(),
        "hidden IME input must focus"
    );

    assert!(host.apply_paste_text("opc1_Ab-9\n"));
    let ime = crate::event::ime::composition_end("Z".to_string());
    assert!(host.apply_ime(&ime));
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.join_input.text(),
        "opc1_Ab-9Z"
    );
    assert!(host.apply_text('/'), "rejected input is still consumed");
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.join_input.text(),
        "opc1_Ab-9Z"
    );
    assert!(host.apply_backspace());
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.join_input.text(),
        "opc1_Ab-9"
    );

    assert!(host.apply_send());
    assert!(
        !host
            .editor_state
            .editor_ui
            .collab
            .panel
            .join_address_focused
    );
    assert_eq!(
        host.editor_state.editor_ui.collab.pending_action,
        Some(CollabUiAction::JoinAddress {
            endpoint: "opc1_Ab-9".to_string(),
        })
    );
}

#[test]
fn join_field_blocks_web_canvas_shortcuts() {
    let mut host = host_with_selected_node();
    let before = host
        .editor_state
        .selected_node()
        .expect("selected node")
        .base()
        .x;

    assert!(!host.apply_delete());
    assert!(!host.apply_duplicate());
    assert!(!host.apply_nudge(10.0, 0.0));
    assert!(!host.apply_undo());
    assert!(!host.apply_redo());
    assert!(host.apply_select_all(), "Cmd/Ctrl+A remains owned");
    assert!(host.apply_keydown_shortcut("K", true, true, false));
    assert!(!host.editor_state.editor_ui.component_browser_open);
    assert_eq!(host.editor_state.active_children().len(), 2);
    assert_eq!(host.editor_state.selection.set.len(), 1);
    assert_eq!(
        host.editor_state
            .selected_node()
            .expect("selection survives")
            .base()
            .x,
        before
    );
}

#[test]
fn hidden_or_external_join_switch_drops_web_focus() {
    let mut host = WidgetHost::new();
    focus_join(&mut host);
    host.editor_state.editor_ui.collab.panel.open = false;

    assert!(!host.input_active(), "hidden focus cannot own shortcuts");
    assert!(!host.non_chat_input_owns_keyboard());
    assert!(!host.apply_text('x'), "hidden focus cannot receive text");
    assert!(host.blur_text_inputs_on_blank_press());
    assert!(
        !host
            .editor_state
            .editor_ui
            .collab
            .panel
            .join_address_focused
    );
}

#[test]
fn web_cursor_tracks_collab_control_hover_and_clears_on_exit() {
    let mut host = WidgetHost::new();
    host.last_viewport_w = 1200.0;
    host.last_viewport_h = 800.0;
    let collab = &mut host.editor_state.editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.panel.open = true;
    collab.panel.view = CollabPanelView::Home;

    let ui = &host.editor_state.editor_ui;
    let top_bar_rect = Rect::xywh(0.0, 0.0, host.last_viewport_w, TOP_BAR_HEIGHT);
    let top_bar = TopBar::for_editor_ui(ui).with_traffic_controls(false);
    let panel = CollabPanel::for_editor_ui(ui).expect("open collaboration panel");
    let rect = panel.rect_at(
        top_bar.collaboration_chip_rect_estimated(top_bar_rect),
        Rect::xywh(0.0, 0.0, host.last_viewport_w, host.last_viewport_h),
    );
    let close_x = rect.origin.x + rect.size.x - 23.0;
    let close_y = rect.origin.y + 22.0;
    host.editor_state.editor_ui.canvas_hover_node = Some(NodeId::new("stale"));

    assert!(host.apply_cursor_move(close_x, close_y));
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.hover,
        Some(CollabPanelHover::Close)
    );
    assert_eq!(host.editor_state.editor_ui.canvas_hover_node, None);

    assert!(host.apply_cursor_move(2.0, 2.0));
    assert_eq!(host.editor_state.editor_ui.collab.panel.hover, None);
}

#[test]
fn web_path_context_menu_keeps_hover_priority_over_collab_panel() {
    let mut host = WidgetHost::new();
    host.last_viewport_w = 1200.0;
    host.last_viewport_h = 800.0;
    let collab = &mut host.editor_state.editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.panel.open = true;
    collab.panel.view = CollabPanelView::Home;
    collab.panel.hover = Some(CollabPanelHover::Close);

    let ui = &host.editor_state.editor_ui;
    let top_bar_rect = Rect::xywh(0.0, 0.0, host.last_viewport_w, TOP_BAR_HEIGHT);
    let top_bar = TopBar::for_editor_ui(ui).with_traffic_controls(false);
    let panel = CollabPanel::for_editor_ui(ui).unwrap();
    let rect = panel.rect_at(
        top_bar.collaboration_chip_rect_estimated(top_bar_rect),
        Rect::xywh(0.0, 0.0, host.last_viewport_w, host.last_viewport_h),
    );
    let point =
        op_editor_ui::Point2D::new(rect.origin.x + rect.size.x - 23.0, rect.origin.y + 22.0);
    host.editor_state.ui.path_anchor_menu = Some(PathAnchorMenuState {
        node_id: NodeId::new("n1"),
        anchor_index: 0,
        x: point.x - 20.0,
        y: point.y - 10.0,
        menu: Default::default(),
    });

    assert!(host.apply_cursor_move(point.x, point.y));
    assert_eq!(
        host.editor_state
            .ui
            .path_anchor_menu
            .as_ref()
            .unwrap()
            .menu
            .hover,
        Some(0)
    );
    assert_eq!(host.editor_state.editor_ui.collab.panel.hover, None);
}

#[test]
fn web_collab_sign_in_row_cannot_open_the_native_popup_modal() {
    let (viewport_w, viewport_h) = (1200.0, 800.0);
    let mut host = WidgetHost::new();
    let ui = &mut host.editor_state.editor_ui;
    ui.account_ui_available = true;
    // The daemon has answered: this deployment signs people in and this tab is
    // not one of them, so the password form is on screen and is the gate.
    ui.account_entry.status_received = true;
    ui.collab.availability = CollabAvailability::SignInRequired;
    ui.collab.panel.open = true;

    let top_bar_rect = Rect::xywh(0.0, 0.0, viewport_w, TOP_BAR_HEIGHT);
    let panel = CollabPanel::for_editor_ui(&host.editor_state.editor_ui).unwrap();
    let panel_rect = panel.rect_at(
        TopBar::for_editor_ui(&host.editor_state.editor_ui)
            .with_traffic_controls(false)
            .collaboration_chip_rect_estimated(top_bar_rect),
        Rect::xywh(0.0, 0.0, viewport_w, viewport_h),
    );
    let sign_in = Point2D::new(
        panel_rect.origin.x + panel_rect.size.x / 2.0,
        panel_rect.origin.y + 100.0,
    );
    assert!(matches!(
        panel.hit_test(panel_rect, sign_in),
        Some(op_editor_ui::widgets::CollabPanelHit::OpenSignIn)
    ));

    assert!(host.apply_press(sign_in.x, sign_in.y, viewport_w, viewport_h));

    // The form is painted above the panel and consumes every press, scrim
    // included, so the panel neither takes the click nor loses its state.
    assert!(
        !host.editor_state.editor_ui.login_modal_open,
        "the web host has no popup sign-in modal to open"
    );
    assert!(host.editor_state.editor_ui.login_modal_status.is_none());
    assert!(host.editor_state.editor_ui.collab.panel.open);
}

#[test]
fn a_requested_sign_in_surface_becomes_the_caret_in_the_form() {
    // The shared chrome (the collaboration panel and the settings modal's
    // Account tab) asks for a sign-in surface by setting the native host's
    // `login_modal_open`. In the web host that surface is the password form,
    // which is already on screen, so the request is answered by pointing at the
    // field — and never by leaving a flag nothing paints.
    let mut host = WidgetHost::new();
    let ui = &mut host.editor_state.editor_ui;
    ui.account_ui_available = true;
    ui.account_entry.status_received = true;
    ui.login_modal_open = true;

    host.absorb_sign_in_request_into_entry_form();

    assert!(!host.editor_state.editor_ui.login_modal_open);
    assert_eq!(
        host.editor_state.editor_ui.account_entry.focus,
        Some(op_editor_core::AccountField::Username)
    );

    // Before the daemon has answered there is no form to point at, and the flag
    // is still dropped: a surface that does not exist cannot be requested.
    let mut early = WidgetHost::new();
    early.editor_state.editor_ui.account_ui_available = true;
    early.editor_state.editor_ui.login_modal_open = true;
    early.absorb_sign_in_request_into_entry_form();
    assert!(!early.editor_state.editor_ui.login_modal_open);
    assert_eq!(early.editor_state.editor_ui.account_entry.focus, None);
}

#[test]
fn web_join_clipboard_replaces_and_select_all_clears() {
    let mut host = WidgetHost::new();
    focus_join(&mut host);

    assert!(host.apply_clipboard_text("opc1_first-code"));
    assert!(host.apply_clipboard_text("opc1_second-code"));
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.join_input.text(),
        "opc1_second-code",
        "a pasted invite replaces the stale one instead of appending"
    );

    // IME commits keep insert semantics — only the clipboard replaces.
    let ime = crate::event::ime::composition_end("Z".to_string());
    assert!(host.apply_ime(&ime));
    assert_eq!(
        host.editor_state.editor_ui.collab.panel.join_input.text(),
        "opc1_second-codeZ"
    );

    assert!(host.apply_select_all());
    assert!(host
        .editor_state
        .editor_ui
        .collab
        .panel
        .join_input
        .highlight_range()
        .is_some());
    assert!(host.apply_backspace());
    assert!(host
        .editor_state
        .editor_ui
        .collab
        .panel
        .join_input
        .text()
        .is_empty());
}
