use super::WidgetHostNative;

/// The rules view's switch must write through the shared flow on the
/// native host too — `EditorCommand` + undo snapshot, gated by collab.
#[test]
fn design_md_rule_switch_press_writes_a_document_override() {
    let mut host = WidgetHostNative::new();
    let (viewport_w, viewport_h) = (1440.0, 900.0);
    {
        let state = host.editor_state_mut();
        state.editor_ui.design_md_panel.open = true;

        state.doc.design_md = Some(op_editor_core::parse_design_md(""));
    }

    let panel_rect = host
        .design_md_panel_rect(viewport_w, viewport_h)
        .expect("design md panel rect");
    let panel = op_editor_ui::widgets::DesignMdPanel::for_editor(host.editor_state())
        .expect("open design md panel");
    let mut point = None;
    let mut y = panel_rect.origin.y;
    while y <= panel_rect.origin.y + panel_rect.size.y && point.is_none() {
        let mut x = panel_rect.origin.x;
        while x <= panel_rect.origin.x + panel_rect.size.x {
            let p = op_editor_ui::Point2D::new(x, y);
            if matches!(
                panel.hit_test(panel_rect, p),
                Some(op_editor_ui::widgets::DesignMdHit::RuleToggle(1))
            ) {
                point = Some(p);
                break;
            }
            x += 3.0;
        }
        y += 3.0;
    }
    let point = point.expect("the first component switch is hittable");

    assert!(host.apply_press(point.x, point.y, viewport_w, viewport_h));

    let rules = &host
        .editor_state()
        .doc
        .design_md
        .as_ref()
        .expect("spec")
        .rules;
    assert_eq!(
        rules.len(),
        1,
        "the switch stores the component document once"
    );
    assert!(!rules[0].enabled);
    assert!(
        matches!(
            rules[0].scope,
            op_editor_core::DesignRuleScope::ComponentType { .. }
        ),
        "a component document is scoped to its component type"
    );
    assert!(host.editor_state().history.can_undo());
}
