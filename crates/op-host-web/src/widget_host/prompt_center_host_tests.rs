use super::WidgetHost;
use op_editor_core::{
    ChatFooterButton, ChatMessage, ComponentBrowserButton, NodeId, PropertyFocus, Viewport,
};
use op_editor_ui::widgets::{IconPickerPanel, PromptCenterPanel};
use op_editor_ui::Point2D;

const VIEWPORT_W: f32 = 1200.0;
const VIEWPORT_H: f32 = 800.0;

fn open_prompt_center() -> WidgetHost {
    let mut host = WidgetHost::new();
    host.last_viewport_w = VIEWPORT_W;
    host.last_viewport_h = VIEWPORT_H;
    host.editor_state.editor_ui.open_prompt_center(10);
    host
}

fn panel_rect(host: &WidgetHost) -> op_editor_ui::Rect {
    host.prompt_center_panel_rect(VIEWPORT_W, VIEWPORT_H)
        .expect("Prompt Center is open")
}

#[test]
fn prompt_center_padding_owns_cursor_and_clears_lower_hover() {
    let mut host = open_prompt_center();
    host.editor_state.editor_ui.prompt_center.hover = Some(0);
    host.editor_state.editor_ui.canvas_hover_node = Some(NodeId::new("covered-canvas-node"));
    host.editor_state.editor_ui.hovered_layer_id = Some(NodeId::new("covered-layer-row"));
    host.editor_state.editor_ui.property_action_hover = Some(2);
    host.editor_state.editor_ui.chat_footer_hover = Some(ChatFooterButton::PromptCenter);
    host.editor_state.editor_ui.component_browser_open = true;
    host.editor_state.editor_ui.component_browser_hover = Some(ComponentBrowserButton::Close);

    let rect = panel_rect(&host);
    let padding = Point2D::new(rect.origin.x + 4.0, rect.origin.y + 4.0);
    assert!(rect.contains(padding));
    assert_eq!(
        PromptCenterPanel::for_editor(&host.editor_state)
            .expect("panel view model")
            .hover_at(rect, padding),
        None,
        "fixture point must be inert panel padding"
    );

    assert!(
        host.apply_cursor_move(padding.x, padding.y),
        "the whole floating panel must own cursor movement"
    );

    let ui = &host.editor_state.editor_ui;
    assert_eq!(ui.prompt_center.hover, None);
    assert_eq!(ui.canvas_hover_node, None);
    assert_eq!(ui.hovered_layer_id, None);
    assert_eq!(ui.property_action_hover, None);
    assert_eq!(ui.chat_footer_hover, None);
    assert_eq!(
        ui.component_browser_hover, None,
        "Prompt Center paints above Component Browser and must clear its hover"
    );
}

#[test]
fn escape_closes_only_prompt_center_before_chat_focus_or_selection() {
    let mut host = open_prompt_center();
    host.editor_state.chat.set_input_text("keep this draft");
    host.editor_state.chat.focus_input_at_end(11);
    host.editor_state
        .set_single_selection(NodeId::new("selected-behind-overlay"));
    let selection_before = host.editor_state.selection.clone();
    let input_before = host.editor_state.chat.input.text().to_owned();
    let caret_before = host.editor_state.chat.input_caret();

    assert!(host.apply_escape());

    assert!(!host.editor_state.editor_ui.prompt_center.open);
    assert!(
        host.editor_state.chat.focused,
        "the same Escape must not continue into chat blur"
    );
    assert_eq!(host.editor_state.chat.input.text(), input_before);
    assert_eq!(host.editor_state.chat.input_caret(), caret_before);
    assert_eq!(
        host.editor_state.selection, selection_before,
        "the same Escape must not continue into selection clearing"
    );
}

#[test]
fn wheel_and_trackpad_scroll_panel_without_moving_viewport() {
    let mut host = open_prompt_center();
    host.editor_state.viewport = Viewport {
        pan_x: 17.0,
        pan_y: -23.0,
        zoom: 1.35,
    };
    let viewport_before = host.editor_state.viewport;
    let rect = panel_rect(&host);
    let padding = Point2D::new(rect.origin.x + 4.0, rect.origin.y + 4.0);
    let max_scroll = PromptCenterPanel::for_editor(&host.editor_state)
        .expect("panel view model")
        .max_scroll(rect);
    assert!(max_scroll > 300.0, "seed catalogue must overflow the grid");

    assert!(host.apply_wheel(padding.x, padding.y, -120.0, VIEWPORT_W, VIEWPORT_H));
    let after_wheel = host.editor_state.editor_ui.prompt_center.scroll.offset;
    assert!(after_wheel > 0.0);
    assert_eq!(host.editor_state.viewport, viewport_before);

    assert!(host.apply_pan_gesture(padding.x, padding.y, 75.0, -90.0, VIEWPORT_W, VIEWPORT_H));
    assert!(host.editor_state.editor_ui.prompt_center.scroll.offset > after_wheel);
    assert_eq!(
        host.editor_state.viewport, viewport_before,
        "Prompt Center trackpad scrolling must not pan the canvas"
    );
}

#[test]
fn wheel_prefers_design_and_icon_panels_painted_above_prompt_center() {
    let mut host = open_prompt_center();
    let mut markdown = String::from("# Design System: Long\n\n## Color Palette\n");
    for index in 0..48 {
        markdown.push_str(&format!(
            "- **color-{index:02}** (#{index:02X}{index:02X}{index:02X}) - role {index}\n"
        ));
    }
    host.editor_state.editor_ui.design_md_panel.open = true;
    host.editor_state.doc.design_md = Some(op_editor_core::parse_design_md(&markdown));
    let design_rect = host
        .design_md_panel_rect(VIEWPORT_W, VIEWPORT_H)
        .expect("Design panel rect");
    let design = op_editor_ui::widgets::DesignMdPanel::for_editor(&host.editor_state)
        .expect("Design panel view model");
    assert!(design.max_rules_scroll(design_rect) > 0.0);
    let point = Point2D::new(
        design_rect.origin.x + design_rect.size.x / 2.0,
        design_rect.origin.y + design_rect.size.y / 2.0,
    );
    assert!(panel_rect(&host).contains(point));

    assert!(host.apply_wheel(point.x, point.y, -120.0, VIEWPORT_W, VIEWPORT_H));
    // The panel scrolls its rules list — the markdown brief it used to
    // scroll is no longer part of the UI.
    assert!(host.editor_state.editor_ui.design_md_panel.rules_scroll.offset > 0.0);
    assert_eq!(
        host.editor_state.editor_ui.prompt_center.scroll.offset, 0.0,
        "the covered Prompt Center must not consume Design-MD wheel input"
    );

    host.editor_state.editor_ui.design_md_panel.open = false;
    host.editor_state.editor_ui.icon_picker.open = true;
    let icon_rect = host
        .icon_picker_panel_rect(VIEWPORT_W, VIEWPORT_H)
        .expect("Icon Picker rect");
    let icon = IconPickerPanel::for_editor(&host.editor_state).expect("Icon Picker view model");
    assert!(icon.icon_picker_max_scroll(icon_rect) > 0.0);
    let point = Point2D::new(
        icon_rect.origin.x + icon_rect.size.x / 2.0,
        icon_rect.origin.y + icon_rect.size.y / 2.0,
    );
    assert!(panel_rect(&host).contains(point));

    assert!(host.apply_wheel(point.x, point.y, -120.0, VIEWPORT_W, VIEWPORT_H));
    assert!(host.editor_state.editor_ui.icon_picker.scroll.offset > 0.0);
    assert_eq!(
        host.editor_state.editor_ui.prompt_center.scroll.offset, 0.0,
        "the covered Prompt Center must not consume Icon Picker wheel input"
    );
}

#[test]
fn builtin_card_press_only_fills_draft_and_closes_center() {
    let mut host = open_prompt_center();
    host.editor_state.chat.set_input_text("replace me");
    host.editor_state.chat.focused = false;
    host.editor_state.chat.pending_send = Some("already queued".to_owned());
    host.editor_state
        .chat
        .messages
        .push(ChatMessage::assistant("existing transcript"));
    let messages_before = host.editor_state.chat.messages.clone();
    let pending_before = host.editor_state.chat.pending_send.clone();

    let rect = panel_rect(&host);
    let panel = PromptCenterPanel::for_editor(&host.editor_state).expect("panel view model");
    let expected_body = panel
        .filtered()
        .first()
        .expect("built-in seed card")
        .body
        .to_owned();
    let card = panel.card_rects(rect).first().expect("first card rect").1;
    let point = Point2D::new(card.origin.x + 12.0, card.origin.y + 12.0);

    assert!(host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H));

    assert_eq!(host.editor_state.chat.input.text(), expected_body);
    assert_eq!(
        host.editor_state.chat.input_caret(),
        expected_body.len(),
        "caret must land at the UTF-8 byte end used by TextInputState"
    );
    assert!(host.editor_state.chat.focused);
    assert!(!host.editor_state.editor_ui.prompt_center.open);
    assert_eq!(host.editor_state.chat.pending_send, pending_before);
    assert_eq!(host.editor_state.chat.messages, messages_before);
}

#[test]
fn prompt_center_selection_wins_over_stale_chat_focus() {
    let mut host = open_prompt_center();
    host.editor_state.chat.set_input_text("keep chat draft");
    host.editor_state.chat.focused = true;
    host.editor_state
        .editor_ui
        .prompt_center
        .search
        .set_text("travel");
    host.editor_state
        .editor_ui
        .prompt_center
        .search
        .select_all();

    assert_eq!(
        host.focused_input_selected_text().as_deref(),
        Some("travel")
    );
    assert!(host.apply_backspace());
    assert!(host
        .editor_state
        .editor_ui
        .prompt_center
        .search
        .text()
        .is_empty());
    assert_eq!(host.editor_state.chat.input.text(), "keep chat draft");
}

#[test]
fn prompt_center_ime_and_editing_override_stale_property_focus() {
    let mut host = open_prompt_center();
    host.editor_state.ui.property_focus = Some(PropertyFocus::SizeW);
    host.editor_state.ui.property_input.set_text("320");
    host.editor_state.chat.set_input_text("keep chat draft");
    host.editor_state.chat.focused = true;
    host.editor_state
        .editor_ui
        .prompt_center
        .search
        .set_text("旅");
    host.editor_state
        .editor_ui
        .prompt_center
        .search
        .set_caret("旅".len(), 0);
    let anchor_before = host.ime_anchor_rect().expect("Prompt Center IME anchor");

    let event = crate::event::ime::composition_end("行".to_owned());
    assert!(host.apply_ime(&event));
    assert_eq!(
        host.editor_state.editor_ui.prompt_center.search.text(),
        "旅行"
    );
    assert_eq!(host.editor_state.ui.property_input.text(), "320");
    assert_eq!(host.editor_state.chat.input.text(), "keep chat draft");
    let anchor_after = host.ime_anchor_rect().expect("Prompt Center IME anchor");
    assert!(anchor_after.origin.x > anchor_before.origin.x);

    assert!(host.apply_select_all());
    assert_eq!(host.focused_input_selected_text().as_deref(), Some("旅行"));
    assert!(host.apply_backspace());
    assert!(host
        .editor_state
        .editor_ui
        .prompt_center
        .search
        .text()
        .is_empty());
    assert_eq!(host.editor_state.ui.property_input.text(), "320");
}
