use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::{Color, RenderBackend, TextLayout};
use op_editor_core::{
    AuthenticatedCollabSession, CollabAdmissionRequestKey, CollabAvailability,
    CollabConnectionPathUi, CollabConnectionPhase, CollabInviteCode, CollabPanelHover,
    CollabPanelView, CollabParticipantUi, CollabPendingEditUi, CollabRelayRegion,
    CollabShareEndpoint, CollabUiAction, CollabUiRole, DiscoveredCollabEndpoint,
};
use std::sync::Arc;

#[derive(Default)]
struct ClipCaptureBackend {
    active_clip: Option<Rect>,
    clip_stack: Vec<Option<Rect>>,
    clips: Vec<Rect>,
    clipped_round_fills: Vec<(Rect, Rect)>,
    unclipped_round_fills: Vec<Rect>,
}

impl RenderBackend for ClipCaptureBackend {
    fn begin_frame(&mut self) {}

    fn end_frame(&mut self) {}

    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}

    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}

    fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}

    fn clip_rect(&mut self, rect: Rect) {
        self.active_clip = Some(rect);
        self.clips.push(rect);
    }

    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}

    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        if let Some(clip) = self.active_clip {
            self.clipped_round_fills.push((rect, clip));
        } else {
            self.unclipped_round_fills.push(rect);
        }
    }

    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}

    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }

    fn save(&mut self) {
        self.clip_stack.push(self.active_clip);
    }

    fn restore(&mut self) {
        self.active_clip = self.clip_stack.pop().flatten();
    }

    fn translate(&mut self, _offset: Point2D) {}

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, 1_000.0, 800.0)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn active_ui(role: CollabUiRole, share_endpoint: Option<CollabShareEndpoint>) -> EditorUiState {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: "Design".into(),
            role,
            share_endpoint,
        },
        Vec::new(),
    );
    ui
}

#[test]
fn sign_in_and_close_targets_expose_hover_feedback() {
    // A sign-in surface is what makes the row exist at all — see
    // `sign_in_row_exists_only_where_a_press_on_it_can_be_answered`.
    let mut ui = EditorUiState {
        account_ui_available: true,
        ..Default::default()
    };
    ui.collab.availability = CollabAvailability::SignInRequired;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());

    assert_eq!(
        panel.hover_at(rect, center(panel.close_rect(rect))),
        Some(CollabPanelHover::Close)
    );
    assert_eq!(
        panel.hover_at(rect, center(panel.sign_in_rect(rect, panel.body_top(rect)))),
        Some(CollabPanelHover::OpenSignIn)
    );
}

#[test]
fn home_and_create_actions_expose_distinct_hover_feedback() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let expected = [
        (CollabUiAction::OpenCreate, CollabPanelHover::OpenCreate),
        (CollabUiAction::OpenJoin, CollabPanelHover::OpenJoin),
    ];

    for (action, hover) in expected {
        let button = panel
            .action_rects(rect)
            .into_iter()
            .find_map(|(button, model)| (model.action == action).then_some(button))
            .expect("home action button");
        assert_eq!(panel.hover_at(rect, center(button)), Some(hover));
    }

    ui.collab.panel.view = CollabPanelView::Create;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    for (action, hover) in [
        (CollabUiAction::Start, CollabPanelHover::Start),
        (CollabUiAction::StartLan, CollabPanelHover::StartLan),
        (CollabUiAction::Cancel, CollabPanelHover::Cancel),
    ] {
        let button = panel
            .action_rects(rect)
            .into_iter()
            .find_map(|(button, model)| (model.action == action).then_some(button))
            .expect("create action button");
        assert_eq!(panel.hover_at(rect, center(button)), Some(hover));
    }
}

#[test]
fn join_controls_hover_only_when_they_are_interactive() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.panel.view = CollabPanelView::Join;
    ui.collab.panel.join_input.set_text("opc1_public-invite");
    ui.collab.panel.discovered = Arc::new(vec![
        DiscoveredCollabEndpoint {
            discovery_id: "compatible".into(),
            endpoint: "192.168.1.8:43120".into(),
            compatible: true,
        },
        DiscoveredCollabEndpoint {
            discovery_id: "incompatible".into(),
            endpoint: "192.168.1.9:43120".into(),
            compatible: false,
        },
    ]);
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let body_top = panel.body_top(rect);

    assert_eq!(
        panel.hover_at(rect, center(panel.address_rect(rect, body_top + 22.0))),
        Some(CollabPanelHover::JoinAddress)
    );

    let compatible = Rect::xywh(
        rect.origin.x + PAD,
        body_top + 106.0,
        rect.size.x - PAD * 2.0,
        ROW_HEIGHT,
    );
    let incompatible = Rect::xywh(
        compatible.origin.x,
        compatible.origin.y + ROW_HEIGHT,
        compatible.size.x,
        ROW_HEIGHT,
    );
    assert_eq!(
        panel.hover_at(rect, center(compatible)),
        Some(CollabPanelHover::Discovered(0))
    );
    assert_eq!(panel.hover_at(rect, center(incompatible)), None);

    let expected = [
        (
            CollabUiAction::JoinAddress {
                endpoint: "opc1_public-invite".into(),
            },
            CollabPanelHover::Connect,
        ),
        (
            CollabUiAction::BeginDiscovery,
            CollabPanelHover::BeginDiscovery,
        ),
        (CollabUiAction::Cancel, CollabPanelHover::Cancel),
    ];
    for (action, hover) in expected {
        let button = panel
            .action_rects(rect)
            .into_iter()
            .find_map(|(button, model)| (model.action == action).then_some(button))
            .expect("join action button");
        assert_eq!(panel.hover_at(rect, center(button)), Some(hover));
    }
}

#[test]
fn session_copy_and_leave_targets_expose_hover_feedback() {
    let mut ui = active_ui(
        CollabUiRole::Owner,
        CollabShareEndpoint::new("192.168.1.8:43120"),
    );
    ui.collab.set_public_session(
        CollabInviteCode::new("opc1_public-invite"),
        CollabConnectionPathUi::Relay {
            home_region: CollabRelayRegion::China,
        },
    );
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());

    assert_eq!(
        panel.hover_at(
            rect,
            center(panel.invite_copy_rect(rect).expect("invite copy target"))
        ),
        Some(CollabPanelHover::CopyInvite)
    );
    assert_eq!(
        panel.hover_at(
            rect,
            center(
                panel
                    .share_endpoint_copy_rect(rect)
                    .expect("share endpoint copy target")
            )
        ),
        Some(CollabPanelHover::CopyShareEndpoint)
    );
    let leave = panel
        .action_rects(rect)
        .into_iter()
        .find_map(|(button, model)| (model.action == CollabUiAction::Leave).then_some(button))
        .expect("leave action");
    assert_eq!(
        panel.hover_at(rect, center(leave)),
        Some(CollabPanelHover::Leave)
    );
}

#[test]
fn reconnect_and_ended_actions_expose_hover_feedback() {
    let mut reconnecting = EditorUiState::default();
    reconnecting.collab.availability = CollabAvailability::Ready;
    reconnecting.collab.panel.open = true;
    reconnecting.collab.set_authenticated_session(
        CollabConnectionPhase::Reconnecting,
        AuthenticatedCollabSession {
            session_name: "Design".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        Vec::new(),
    );
    let panel = CollabPanel::for_editor_ui(&reconnecting).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let retry = panel
        .action_rects(rect)
        .into_iter()
        .find_map(|(button, model)| (model.action == CollabUiAction::Retry).then_some(button))
        .expect("retry action");
    assert_eq!(
        panel.hover_at(rect, center(retry)),
        Some(CollabPanelHover::Retry)
    );

    let mut ended = EditorUiState::default();
    ended.collab.availability = CollabAvailability::Ready;
    ended.collab.panel.open = true;
    ended.collab.set_authenticated_session(
        CollabConnectionPhase::Ended,
        AuthenticatedCollabSession {
            session_name: "Design".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        Vec::new(),
    );
    ended.collab.pending_edit = CollabPendingEditUi::Conflict;
    let panel = CollabPanel::for_editor_ui(&ended).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    for (action, hover) in [
        (
            CollabUiAction::DiscardPending,
            CollabPanelHover::DiscardPending,
        ),
        (CollabUiAction::SaveAsFork, CollabPanelHover::SaveAsFork),
    ] {
        let button = panel
            .action_rects(rect)
            .into_iter()
            .find_map(|(button, model)| (model.action == action).then_some(button))
            .expect("ended-session action");
        assert_eq!(panel.hover_at(rect, center(button)), Some(hover));
    }
}

#[test]
fn admission_actions_expose_role_specific_hover_feedback() {
    let mut ui = active_ui(CollabUiRole::Owner, None);
    let request_key = CollabAdmissionRequestKey::new("request-42").unwrap();
    assert!(ui.collab.publish_pending_admission(request_key, None));
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let CollabPanelScreen::Session {
        admission_request: Some(request),
        ..
    } = &panel.model.screen
    else {
        panic!("expected owner admission request");
    };

    let expected = [
        (
            "approve editor",
            CollabPanelHover::ApproveAdmissionEditor,
            0,
        ),
        (
            "approve viewer",
            CollabPanelHover::ApproveAdmissionViewer,
            1,
        ),
        ("reject", CollabPanelHover::RejectAdmission, 2),
    ];
    let buttons = panel.admission_action_rects(rect, panel.body_top(rect), request);
    for (label, hover, index) in expected {
        let (button, _) = buttons
            .get(index)
            .unwrap_or_else(|| panic!("{label} button"));
        assert_eq!(
            panel.hover_at(rect, center(*button)),
            Some(hover),
            "{label}"
        );
    }
}

#[test]
fn panel_background_and_outside_do_not_expose_hover_feedback() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());

    assert_eq!(
        panel.hover_at(
            rect,
            Point2D::new(rect.origin.x + PAD, rect.origin.y + HEADER_HEIGHT + 8.0)
        ),
        None
    );
    assert_eq!(panel.hover_at(rect, Point2D::new(2.0, 700.0)), None);
}

#[test]
fn painted_close_and_action_controls_use_the_hover_token() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.panel.view = CollabPanelView::Create;
    ui.collab.panel.hover = Some(CollabPanelHover::Start);
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let start = panel
        .action_rects(rect)
        .into_iter()
        .find_map(|(button, action)| (action.action == CollabUiAction::Start).then_some(button))
        .unwrap();
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(backend
        .round_fills
        .contains(&(start, 6.0, panel.theme.button_hover)));

    ui.collab.panel.hover = Some(CollabPanelHover::Close);
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(backend
        .round_fills
        .contains(&(panel.close_rect(rect), 6.0, panel.theme.button_hover)));
}

#[test]
fn pending_action_does_not_paint_a_disabled_hover_wash() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.panel.view = CollabPanelView::Create;
    ui.collab.panel.hover = Some(CollabPanelHover::Start);
    ui.collab.pending_action = Some(CollabUiAction::Start);
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let start = panel
        .action_rects(rect)
        .into_iter()
        .find_map(|(button, action)| (action.action == CollabUiAction::Start).then_some(button))
        .unwrap();
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(!backend
        .round_fills
        .contains(&(start, 6.0, panel.theme.button_hover)));
}

#[test]
fn authenticated_panel_exposes_real_leave_action() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: "Design".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        vec![CollabParticipantUi::new(
            "p1",
            "Ada",
            0x3366ffff,
            CollabUiRole::Editor,
            true,
        )],
    );
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let (leave, _) = panel
        .action_rects(rect)
        .into_iter()
        .find(|(_, action)| action.action == CollabUiAction::Leave)
        .expect("active session has leave");
    assert_eq!(
        panel.hit_test(
            rect,
            Point2D::new(
                leave.origin.x + leave.size.x / 2.0,
                leave.origin.y + leave.size.y / 2.0,
            )
        ),
        Some(CollabPanelHit::Action(CollabUiAction::Leave))
    );
}

#[test]
fn outside_point_is_not_claimed() {
    let mut ui = EditorUiState::default();
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    assert_eq!(panel.hit_test(rect, Point2D::new(2.0, 700.0)), None);
}

#[test]
fn empty_join_results_reserve_a_row_above_the_actions() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.phase = CollabConnectionPhase::Discovering;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let first_result_y = panel.body_top(rect) + 106.0;
    let first_action_y = panel
        .action_rects(rect)
        .into_iter()
        .map(|(button, _)| button.origin.y)
        .reduce(f32::min)
        .expect("join screen has a cancel action");

    assert!(
        first_action_y >= first_result_y + ROW_HEIGHT,
        "empty-state copy must not be covered by the action row"
    );
}

#[test]
fn height_clamped_body_paint_is_clipped_above_action_row() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.panel.view = CollabPanelView::Join;
    ui.collab.panel.join_input.set_text("198.51.100.42:443");
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(
        Rect::xywh(600.0, 8.0, 100.0, 26.0),
        Rect::xywh(0.0, 0.0, 760.0, 180.0),
    );
    let (button, _) = panel
        .action_rects(rect)
        .into_iter()
        .next()
        .expect("join screen has actions");
    let input = panel.address_rect(rect, panel.body_top(rect) + 22.0);
    let body_clip = panel.body_clip_rect(rect);
    assert!(
        body_clip.origin.y + body_clip.size.y <= button.origin.y - ACTION_GAP,
        "body clip must stop before the fixed action row"
    );

    let mut backend = ClipCaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(backend.clips.contains(&body_clip));
    assert!(
        backend.clipped_round_fills.contains(&(input, body_clip)),
        "join input must paint under the body clip"
    );
    assert!(
        backend.unclipped_round_fills.contains(&button),
        "fixed action row must paint after restoring the body clip"
    );
}

#[test]
fn owner_can_hit_approve_viewer_for_pending_request() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: "Design".into(),
            role: CollabUiRole::Owner,
            share_endpoint: CollabShareEndpoint::new("192.168.1.8:43120"),
        },
        Vec::new(),
    );
    let request_key = CollabAdmissionRequestKey::new("request-42").unwrap();
    assert!(ui
        .collab
        .publish_pending_admission(request_key.clone(), None));
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let CollabPanelScreen::Session {
        admission_request: Some(request),
        ..
    } = &panel.model.screen
    else {
        panic!("expected owner admission request");
    };
    let expected = CollabUiAction::ApproveAdmissionViewer { request_key };
    let (button, _) = panel
        .admission_action_rects(rect, panel.body_top(rect), request)
        .into_iter()
        .find(|(_, action)| action.action == expected)
        .expect("viewer approval button");
    assert_eq!(
        panel.hit_test(
            rect,
            Point2D::new(
                button.origin.x + button.size.x / 2.0,
                button.origin.y + button.size.y / 2.0,
            )
        ),
        Some(CollabPanelHit::Action(expected))
    );
}

#[test]
fn owner_share_address_extends_panel_geometry() {
    let endpoint = CollabShareEndpoint::new("192.168.1.8:43120").unwrap();
    let owner_without_ui = active_ui(CollabUiRole::Owner, None);
    let owner_with_ui = active_ui(CollabUiRole::Owner, Some(endpoint));
    let owner_without = CollabPanel::for_editor_ui(&owner_without_ui).unwrap();
    let owner_with = CollabPanel::for_editor_ui(&owner_with_ui).unwrap();
    assert_eq!(
        owner_with.panel_height(),
        owner_without.panel_height() + SHARE_ENDPOINT_HEIGHT
    );
    assert_eq!(owner_with.session_share_height(), SHARE_ENDPOINT_HEIGHT);

    let guest_ui = active_ui(
        CollabUiRole::Viewer,
        CollabShareEndpoint::new("192.168.1.8:43120"),
    );
    let guest = CollabPanel::for_editor_ui(&guest_ui).unwrap();
    assert_eq!(guest.session_share_height(), 0.0);
    assert_eq!(guest.panel_height(), owner_without.panel_height());
}

#[test]
fn share_address_copy_hit_is_owner_only_and_redacts_debug() {
    let endpoint = "192.168.1.8:43120";
    let owner_ui = active_ui(CollabUiRole::Owner, CollabShareEndpoint::new(endpoint));
    let owner = CollabPanel::for_editor_ui(&owner_ui).unwrap();
    let panel_rect = owner.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let copy = owner
        .share_endpoint_copy_rect(panel_rect)
        .expect("owner has copy target");
    let hit = owner
        .hit_test(
            panel_rect,
            Point2D::new(
                copy.origin.x + copy.size.x / 2.0,
                copy.origin.y + copy.size.y / 2.0,
            ),
        )
        .expect("copy target is hit");
    assert_eq!(hit, CollabPanelHit::CopyShareEndpoint(endpoint.to_string()));
    assert!(!format!("{hit:?}").contains(endpoint));

    let guest_ui = active_ui(CollabUiRole::Viewer, CollabShareEndpoint::new(endpoint));
    let guest = CollabPanel::for_editor_ui(&guest_ui).unwrap();
    assert!(guest.share_endpoint_copy_rect(panel_rect).is_none());
}

#[test]
fn public_invite_copy_hit_is_owner_only_and_redacts_debug() {
    let raw_invite = "opc1_secret-route";
    let baseline_ui = active_ui(CollabUiRole::Owner, None);
    let baseline = CollabPanel::for_editor_ui(&baseline_ui)
        .unwrap()
        .panel_height();
    let mut owner_ui = active_ui(CollabUiRole::Owner, None);
    owner_ui.collab.set_public_session(
        CollabInviteCode::new(raw_invite),
        CollabConnectionPathUi::Relay {
            home_region: CollabRelayRegion::China,
        },
    );
    let owner = CollabPanel::for_editor_ui(&owner_ui).unwrap();
    assert_eq!(
        owner.panel_height(),
        baseline + CONNECTION_PATH_HEIGHT + INVITE_HEIGHT
    );
    let panel_rect = owner.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let copy = owner
        .invite_copy_rect(panel_rect)
        .expect("owner invite target");
    let hit = owner
        .hit_test(
            panel_rect,
            Point2D::new(
                copy.origin.x + copy.size.x / 2.0,
                copy.origin.y + copy.size.y / 2.0,
            ),
        )
        .expect("invite copy target is hit");
    assert_eq!(hit, CollabPanelHit::CopyInvite(raw_invite.to_string()));
    assert!(!format!("{hit:?}").contains(raw_invite));

    let mut guest_ui = active_ui(CollabUiRole::Viewer, None);
    guest_ui.collab.set_public_session(
        CollabInviteCode::new(raw_invite),
        CollabConnectionPathUi::Relay {
            home_region: CollabRelayRegion::China,
        },
    );
    let guest = CollabPanel::for_editor_ui(&guest_ui).unwrap();
    assert!(guest.invite_copy_rect(panel_rect).is_none());
}

#[test]
fn share_address_label_is_localized() {
    assert_eq!(
        op_i18n::translate(op_editor_core::Locale::EnUs, "collab.session.shareAddress"),
        "Share address"
    );
    assert_eq!(
        op_i18n::translate(op_editor_core::Locale::ZhCn, "collab.session.shareAddress"),
        "分享地址"
    );
}

#[test]
fn join_clear_button_wins_inside_the_input_and_needs_content() {
    let mut ui = EditorUiState::default();
    ui.collab.availability = CollabAvailability::Ready;
    ui.collab.panel.open = true;
    ui.collab.panel.view = CollabPanelView::Join;
    ui.collab.panel.join_input.set_text("opc1_public-invite");

    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let body_top = panel.body_top(rect);
    let clear = panel
        .clear_join_rect(rect, body_top + 22.0)
        .expect("non-empty field exposes the clear affordance");
    assert!(
        panel
            .address_rect(rect, body_top + 22.0)
            .contains(center(clear)),
        "clear button sits inside the input"
    );
    assert_eq!(
        panel.hit_test(rect, center(clear)),
        Some(CollabPanelHit::ClearJoinAddress)
    );
    assert_eq!(
        panel.hover_at(rect, center(clear)),
        Some(CollabPanelHover::ClearJoinAddress)
    );
    // Outside the button the input still takes focus.
    let input = panel.address_rect(rect, body_top + 22.0);
    let left = Point2D::new(input.origin.x + 8.0, input.origin.y + input.size.y / 2.0);
    assert_eq!(
        panel.hit_test(rect, left),
        Some(CollabPanelHit::FocusJoinAddress)
    );

    // An empty field paints and hit-tests no dead button.
    let mut empty = EditorUiState::default();
    empty.collab.availability = CollabAvailability::Ready;
    empty.collab.panel.open = true;
    empty.collab.panel.view = CollabPanelView::Join;
    let panel = CollabPanel::for_editor_ui(&empty).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let body_top = panel.body_top(rect);
    assert!(panel.clear_join_rect(rect, body_top + 22.0).is_none());
}
