//! Instance inspection tests (GAP #10): a selected `Ref` builds its
//! panel snapshot from the merged display node so the FULL section
//! set shows, carries the `is_instance` flag for badging, and emits
//! the Go-to-component / Detach-instance action rects. Plus the
//! layer-panel component/instance badging flags.

use super::property_panel::{PropertyPanel, PropertyPanelAction};
use super::property_panel_test_support::{state_from, CountingBackend};
use crate::widgets::layer_panel::LayerPanel;
use crate::widgets::property_panel_visibility::ComponentButtonState;
use crate::widgets::{PaintCx, Widget};
use crate::{Point2D, Rect};
use op_editor_core::NodeId;

const COMPONENT_DOC: &str = r##"{
  "version":"1.0.0",
  "children":[
    {"type":"frame","id":"card","name":"Card","reusable":true,"x":0,"y":0,"width":200,"height":100,
     "fill":[{"type":"solid","color":"#222222"}],
     "children":[{"type":"text","id":"title","name":"Title","content":"Hello"}]},
    {"type":"frame","id":"banner","name":"Banner","reusable":true,
     "x":0,"y":140,"width":280,"height":60,"children":[]},
    {"type":"ref","id":"inst1","ref":"card","x":300,"y":50,
     "descendants":{
       "card":{"fill":[{"type":"solid","color":"#ff8800"}]},
       "title":{"fill":[{"type":"solid","color":"#00aa55"}]}
     }}
  ]
}"##;

fn panel_for(id: &str) -> PropertyPanel {
    let mut state = state_from(COMPONENT_DOC);
    state.set_single_selection(NodeId::new(id));
    PropertyPanel::for_selection(&state).expect("panel builds")
}

#[test]
fn instance_snapshot_merges_display_node_and_flags_instance() {
    let panel = panel_for("inst1");
    let snap = &panel.snapshot;
    assert!(snap.is_instance, "Ref selection flags is_instance");
    assert!(!snap.is_reusable);
    // Merged display node: component geometry + instance position.
    assert_eq!(snap.x, 300);
    assert_eq!(snap.width, 200);
    // descendants[card] top-level fill override shows in the panel.
    let fill = snap.fill.expect("merged fill present");
    assert!(fill.r > 0.9 && fill.g > 0.4 && fill.b < 0.1, "#ff8800");
    // The merged node takes the component's Frame kind — the FULL
    // section set (fill / stroke / layout) is exposed, not the old
    // near-empty `Other("ref")` mask.
    assert!(matches!(
        snap.kind_variant,
        crate::layout_scene::NodeKind::Frame
    ));
}

#[test]
fn instance_snapshot_exposes_sorted_swap_options_and_current_target() {
    let mut state = state_from(COMPONENT_DOC);
    state.set_single_selection(NodeId::new("inst1"));
    let panel = PropertyPanel::for_selection(&state).expect("panel builds");
    assert_eq!(panel.instance_component_target.as_deref(), Some("card"));
    let options: Vec<_> = panel
        .instance_component_options
        .iter()
        .map(|option| (option.id.as_str(), option.name.as_str()))
        .collect();
    assert_eq!(options, [("banner", "Banner"), ("card", "Card")]);
    assert!(!panel.instance_component_picker_open);

    let rebuilt = PropertyPanel::for_selection(&state).expect("panel rebuilds");
    assert!(std::sync::Arc::ptr_eq(
        &panel.instance_component_options,
        &rebuilt.instance_component_options,
    ));
}

#[test]
fn expanded_swap_list_paints_only_viewport_sized_candidate_window() {
    let state = state_from(COMPONENT_DOC);
    let labels = super::property_panel_sections::PropertyLabels::for_editor_ui(&state.editor_ui);
    let theme = super::editor_state_ext::theme_for(&state.editor_ui);
    let options: Vec<_> = (0..10_000)
        .map(|index| op_editor_core::ComponentOption {
            id: format!("component-{index}"),
            name: format!("Candidate {index}"),
        })
        .collect();
    let mut backend = CountingBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    super::property_panel_instance::paint_component_block(
        &mut cx,
        &theme,
        &labels,
        ComponentButtonState::Instance {
            component_count: options.len(),
            picker_open: true,
        },
        &options,
        Some("component-0"),
        100_000.0,
        100_400.0,
        0.0,
        0.0,
        280.0,
    );

    let candidates: Vec<_> = backend
        .texts
        .iter()
        .filter(|text| text.starts_with("Candidate "))
        .collect();
    assert!(
        !candidates.is_empty(),
        "the visible candidate window paints"
    );
    assert!(
        candidates.len() <= 12,
        "10k candidates must not become 10k paint calls: {}",
        candidates.len()
    );
}

#[test]
fn dangling_ref_can_be_repaired_from_the_swap_control() {
    let mut state = state_from(
        r##"{
          "version":"1.0.0",
          "children":[
            {"type":"frame","id":"banner","name":"Banner","reusable":true,
             "width":200,"height":60,"children":[]},
            {"type":"ref","id":"inst1","ref":"missing","x":20,"y":30}
          ]
        }"##,
    );
    state.set_single_selection(NodeId::new("inst1"));
    let panel = PropertyPanel::for_selection(&state).expect("dangling Ref still inspects");
    assert!(panel.snapshot.is_instance);
    assert_eq!(panel.instance_component_options.len(), 1);
    assert!(panel.visible_sections().create_component);
}

#[test]
fn swap_control_stays_hidden_without_an_alternative_component() {
    let mut state = state_from(
        r##"{
          "version":"1.0.0",
          "children":[
            {"type":"frame","id":"card","name":"Card","reusable":true,
             "width":200,"height":60,"children":[]},
            {"type":"ref","id":"inst1","ref":"card","x":20,"y":30}
          ]
        }"##,
    );
    state.set_single_selection(NodeId::new("inst1"));
    let panel = PropertyPanel::for_selection(&state).expect("Ref panel builds");
    assert!(panel.instance_component_options.is_empty());
    let actions: Vec<_> = super::property_panel_layout::action_button_rects(
        Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(280.0, 1000.0),
        },
        panel.visible_sections(),
        &panel.snapshot.effects,
        &panel.snapshot.fills,
        &panel.snapshot.interactions,
    )
    .into_iter()
    .map(|(action, _)| action)
    .collect();
    assert!(!actions.contains(&PropertyPanelAction::ToggleInstanceComponentPicker));
}

#[test]
fn expanded_instance_swap_rows_paint_and_hit_the_component_target() {
    let mut state = state_from(COMPONENT_DOC);
    state.set_single_selection(NodeId::new("inst1"));
    state.editor_ui.toggle_instance_component_picker("inst1");
    // The row's button elides a label that does not fit, and this test asserts
    // the whole label is painted: it is about the row, not about how a longer
    // translation is shortened, so it states the locale it needs.
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    let panel = PropertyPanel::for_selection(&state).expect("panel builds");
    let labels = super::property_panel_sections::PropertyLabels::for_editor_ui(&state.editor_ui);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(280.0, 1200.0),
    };
    let button_state = panel.visible_sections().component_button;
    let block_y =
        super::property_panel_inputs::TAB_HEIGHT + super::property_panel_inputs::HEADER_HEIGHT;
    let rows = super::property_panel_instance::block_rects(0.0, block_y, 280.0, button_state);
    let banner = rows.option_rect(0).expect("first option row");
    let banner_center = Point2D::new(
        banner.origin.x + banner.size.x / 2.0,
        banner.origin.y + banner.size.y / 2.0,
    );
    assert_eq!(
        panel.hit_test_action(rect, banner_center),
        Some(PropertyPanelAction::SetInstanceComponent(
            "banner".to_string()
        ))
    );

    let mut backend = CountingBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    let swap_label = format!("{}: Card", labels.swap_component);
    assert!(backend.texts.iter().any(|text| text == &swap_label));
    assert!(backend.texts.iter().any(|text| text == "Banner"));
    assert!(backend.texts.iter().any(|text| text == "Card"));
}

#[test]
fn virtual_child_snapshot_shows_effective_descendant_values() {
    let panel = panel_for("inst1__title");
    let snap = &panel.snapshot;
    assert_eq!(snap.name, "Title");
    assert!(matches!(
        snap.kind_variant,
        crate::layout_scene::NodeKind::Text
    ));
    let fill = snap.fill.expect("descendant override fill shown");
    assert!(fill.g > 0.55 && fill.r < 0.2, "#00aa55, got {fill:?}");
    assert!(
        !snap.is_instance,
        "instance lifecycle actions belong to the Ref root, not its virtual child"
    );
}

#[test]
fn reusable_component_snapshot_flags_is_reusable() {
    let panel = panel_for("card");
    assert!(panel.snapshot.is_reusable);
    assert!(!panel.snapshot.is_instance);
}

#[test]
fn registered_group_component_snapshot_flags_is_reusable() {
    let mut state = state_from(
        r##"{
          "version":"1.0.0",
          "children":[
            {"type":"group","id":"text_group","name":"Text Group",
             "children":[{"type":"text","id":"label","name":"Label","content":"Hello"}]}
          ]
        }"##,
    );
    state.set_single_selection(NodeId::new("text_group"));
    assert!(state.create_component_from_node_name(&NodeId::new("text_group")));

    let panel = PropertyPanel::for_selection(&state).expect("group panel builds");
    assert!(panel.snapshot.is_reusable);
    assert!(!panel.snapshot.is_instance);
    assert_eq!(
        panel.visible_sections().component_button,
        ComponentButtonState::DetachComponent
    );
}

#[test]
fn instance_panel_emits_go_to_component_and_detach_rows() {
    let panel = panel_for("inst1");
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(280.0, 2000.0),
    };
    let actions: Vec<PropertyPanelAction> = super::property_panel_layout::action_button_rects(
        rect,
        super::property_panel_test_support::visible_for(&panel),
        &panel.snapshot.effects,
        &panel.snapshot.fills,
        &panel.snapshot.interactions,
    )
    .into_iter()
    .map(|(a, _)| a)
    .collect();
    assert!(actions.contains(&PropertyPanelAction::GoToComponent));
    assert!(actions.contains(&PropertyPanelAction::DetachInstance));
    assert!(actions.contains(&PropertyPanelAction::ToggleInstanceComponentPicker));
    assert!(!actions.contains(&PropertyPanelAction::CreateComponent));
}

#[test]
fn reusable_panel_emits_detach_component_action() {
    let panel = panel_for("card");
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(280.0, 2000.0),
    };
    let actions: Vec<PropertyPanelAction> = super::property_panel_layout::action_button_rects(
        rect,
        super::property_panel_test_support::visible_for(&panel),
        &panel.snapshot.effects,
        &panel.snapshot.fills,
        &panel.snapshot.interactions,
    )
    .into_iter()
    .map(|(a, _)| a)
    .collect();
    assert!(actions.contains(&PropertyPanelAction::DetachComponent));
    assert!(!actions.contains(&PropertyPanelAction::CreateComponent));
}

#[test]
fn instance_block_height_matches_two_rows() {
    use crate::widgets::property_panel_inputs::{
        create_component_block_height, CREATE_COMPONENT_BLOCK_H, CREATE_COMPONENT_BTN_H,
        CREATE_COMPONENT_ROW_GAP,
    };
    assert_eq!(
        create_component_block_height(ComponentButtonState::Create),
        CREATE_COMPONENT_BLOCK_H
    );
    assert_eq!(
        create_component_block_height(ComponentButtonState::Instance {
            component_count: 0,
            picker_open: false,
        }),
        CREATE_COMPONENT_BLOCK_H + CREATE_COMPONENT_BTN_H + CREATE_COMPONENT_ROW_GAP
    );
    assert_eq!(
        create_component_block_height(ComponentButtonState::Instance {
            component_count: 2,
            picker_open: true,
        }),
        CREATE_COMPONENT_BLOCK_H + 4.0 * (CREATE_COMPONENT_BTN_H + CREATE_COMPONENT_ROW_GAP)
    );
}

#[test]
fn layer_panel_rows_flag_components_and_instances() {
    let state = state_from(COMPONENT_DOC);
    let panel = LayerPanel::from_editor(&state);
    let card = panel
        .items
        .iter()
        .find(|i| i.node_id.as_str() == "card")
        .expect("card row");
    assert!(card.is_reusable && !card.is_instance);
    assert!(matches!(card.icon, crate::widgets::icons::Icon::Diamond));
    let inst = panel
        .items
        .iter()
        .find(|i| i.node_id.as_str() == "inst1")
        .expect("instance row");
    assert!(inst.is_instance && !inst.is_reusable);
    assert!(matches!(inst.icon, crate::widgets::icons::Icon::Diamond));
}
