//! Collaboration cursor and remote-selection paint.
//!
//! Presence is deliberately lossy, bounded in `op-editor-core`, and painted
//! below local selection chrome. Unknown/deleted ids are skipped each frame.

use crate::layout_scene::SceneNode;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};
use op_editor_core::{CollabUiState, PencilCursorStyle, Viewport};

// The doc→screen mapping is shared with the other canvas overlays
// (`canvas_doc_mapping`); a second copy of the pan/zoom arithmetic is how two
// overlays end up disagreeing about where the same element is.
use crate::widgets::canvas_doc_mapping::{doc_point_to_screen, doc_rect_to_screen};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CollabPresencePaint {
    participant_key: String,
    display_name: String,
    color: Color,
    cursor: Option<Point2D>,
    selection: Vec<String>,
}

pub(super) fn snapshot(state: &CollabUiState) -> Vec<CollabPresencePaint> {
    state
        .presence()
        .iter()
        .filter_map(|presence| {
            let participant = state
                .participants()
                .iter()
                .find(|participant| participant.participant_key == presence.participant_key)?;
            if participant.is_self {
                return None;
            }
            Some(CollabPresencePaint {
                participant_key: participant.participant_key.clone(),
                display_name: participant.display_name.clone(),
                color: rgba_u32(participant.color_rgba),
                cursor: presence
                    .cursor
                    .map(|point| Point2D::new(point.x as f32, point.y as f32)),
                selection: presence.selection.as_ref().clone(),
            })
        })
        .collect()
}

pub(super) fn paint(
    cx: &mut PaintCx<'_>,
    roots: &[SceneNode],
    items: &[CollabPresencePaint],
    canvas_rect: Rect,
    viewport: &Viewport,
    cursor_style: PencilCursorStyle,
) {
    for item in items {
        let outline = item.color.with_alpha(0.9);
        for id in &item.selection {
            let Some(node) = find_node(roots, id) else {
                continue;
            };
            if node.hidden {
                continue;
            }
            let bounds = node.aggregate_bounds();
            if bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
                continue;
            }
            let rect = doc_rect_to_screen(bounds, canvas_rect, viewport);
            stroke_outline(cx, rect, outline);
        }
        if let Some(cursor) = item.cursor {
            // Remote collaborators share the agent-cursor look so canvas
            // pointers read as one visual language.
            super::canvas_agent_cursor::paint_presence_cursor(
                cx,
                doc_point_to_screen(cursor, canvas_rect, viewport),
                item.color,
                &item.display_name,
                cursor_style,
            );
        }
    }
}

fn find_node<'a>(roots: &'a [SceneNode], id: &str) -> Option<&'a SceneNode> {
    for root in roots {
        if root.id == id {
            return Some(root);
        }
        if let Some(node) = find_node(&root.children, id) {
            return Some(node);
        }
    }
    None
}

fn stroke_outline(cx: &mut PaintCx<'_>, rect: Rect, color: Color) {
    let left = rect.origin.x;
    let top = rect.origin.y;
    let right = rect.origin.x + rect.size.x;
    let bottom = rect.origin.y + rect.size.y;
    cx.backend.stroke_line(
        Point2D::new(left, top),
        Point2D::new(right, top),
        color,
        1.5,
    );
    cx.backend.stroke_line(
        Point2D::new(right, top),
        Point2D::new(right, bottom),
        color,
        1.5,
    );
    cx.backend.stroke_line(
        Point2D::new(right, bottom),
        Point2D::new(left, bottom),
        color,
        1.5,
    );
    cx.backend.stroke_line(
        Point2D::new(left, bottom),
        Point2D::new(left, top),
        color,
        1.5,
    );
}

fn rgba_u32(value: u32) -> Color {
    Color {
        r: ((value >> 24) & 0xff) as f32 / 255.0,
        g: ((value >> 16) & 0xff) as f32 / 255.0,
        b: ((value >> 8) & 0xff) as f32 / 255.0,
        a: (value & 0xff) as f32 / 255.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::{
        AuthenticatedCollabSession, CollabCanvasPoint, CollabConnectionPhase, CollabParticipantUi,
        CollabUiRole, RemotePresenceUi,
    };

    #[test]
    fn snapshot_excludes_self_and_keeps_epoch_local_remote_identity() {
        let mut state = CollabUiState::default();
        state.set_authenticated_session(
            CollabConnectionPhase::Active,
            AuthenticatedCollabSession {
                session_name: "Design".into(),
                role: CollabUiRole::Owner,
                share_endpoint: None,
            },
            vec![
                CollabParticipantUi::new("self", "Ada", 0x112233ff, CollabUiRole::Owner, true),
                CollabParticipantUi::new(
                    "remote",
                    "Grace",
                    0x445566ff,
                    CollabUiRole::Editor,
                    false,
                ),
            ],
        );
        state.queue_presence_snapshot(vec![
            RemotePresenceUi::bounded(
                "self",
                Some(CollabCanvasPoint { x: 1.0, y: 2.0 }),
                ["n1".into()],
                None,
                1,
            ),
            RemotePresenceUi::bounded(
                "remote",
                Some(CollabCanvasPoint { x: 3.0, y: 4.0 }),
                ["n2".into()],
                None,
                1,
            ),
        ]);
        assert!(state.flush_presence(100));

        let items = snapshot(&state);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].participant_key, "remote");
        assert_eq!(items[0].display_name, "Grace");
        assert_eq!(items[0].selection, ["n2"]);
    }
}
