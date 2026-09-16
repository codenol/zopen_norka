//! Root-height evidence taken from the measured corpus in
//! `.openpencil-tmp/gq/` (issue #185) plus the fixture that reproduces it.
//!
//! The corpus is a gitignored scratch directory, so the corpus tests skip
//! themselves when it is absent; the fixture test always runs and asserts the
//! same invariant on the same shape (an app-shell root whose body sits under a
//! `clipContent` frame far below the visible content).

use super::*;
use crate::cleanup::run_cleanup_passes;
use crate::plan::{OrchestratorPlan, RootFrameSpec};
use crate::test_support::VecDocSink;
use jian_ops_schema::node::PenNode;
use op_editor_core::{EditorCommand, NodeId, PenNodeExt};
use serde_json::json;

/// The kit chassis' authored root height, before any post-pass touched it
/// (`Components/Layout` → `tpl-layout-default` in every corpus document).
const CHASSIS_ROOT_HEIGHT: f64 = 850.0;

/// One corpus document, exactly as delivered.
fn corpus_state(rel: &str) -> Option<EditorState> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    let text = std::fs::read_to_string(path).ok()?;
    let doc = jian_ops_schema::load_str(&text).ok()?.value;
    Some(EditorState::from_document(doc))
}

fn chassis_root_id(state: &EditorState) -> Option<String> {
    state
        .active_children()
        .iter()
        .find(|node| node.base().name.as_deref() == Some("Layout/Default"))
        .map(|node| node.id_str().to_string())
}

fn delivered_root_height(state: &EditorState) -> Option<f64> {
    let id = chassis_root_id(state)?;
    state
        .active_children()
        .iter()
        .find(|node| node.id_str() == id)
        .and_then(PenNodeExt::height_px)
}

/// Deepest descendant bottom in the resolved scene that a CLIPPING ancestor
/// crops away — the invisible extent the root-height repair used to be handed,
/// with the id of the frame that crops it.
fn deepest_cropped_descendant(state: &EditorState, root_id: &str) -> Option<(String, f64, String)> {
    fn walk(node: &SceneNode, clipper: Option<&str>, best: &mut Option<(String, f64, String)>) {
        let bottom = f64::from(node.bounds.origin.y) + f64::from(node.bounds.size.y);
        if let Some(clipper) = clipper {
            if bottom.is_finite() {
                let better = best
                    .as_ref()
                    .map(|(_, best_bottom, _)| bottom > *best_bottom)
                    .unwrap_or(true);
                if better {
                    *best = Some((node.id.clone(), bottom, clipper.to_string()));
                }
            }
        }
        // A clipping node crops its whole subtree: nothing under it paints, but
        // the pass's raw walk descended through it anyway — this helper does the
        // same so the measurement matches what the pass was given.
        let clipper = if node.clip_content {
            Some(node.id.as_str())
        } else {
            clipper
        };
        for child in &node.children {
            walk(child, clipper, best);
        }
    }
    let scene = op_pen_loader::editor_state_to_active_page_layout_scene(state);
    let page = scene.active_page()?;
    let root = page.children.iter().find(|node| node.id == root_id)?;
    let mut best = None;
    for child in &root.children {
        walk(child, None, &mut best);
    }
    best
}

fn shell_plan(root_id: &str) -> OrchestratorPlan {
    OrchestratorPlan {
        root_frame: RootFrameSpec {
            id: root_id.to_string(),
            name: "Layout/Default".into(),
            width: 1440.0,
            height: CHASSIS_ROOT_HEIGHT,
            layout: Some("none".into()),
            gap: None,
            padding: None,
            fill: None,
        },
        subtasks: Vec::new(),
        style_guide_name: None,
    }
}

/// WHO WRITES THE ROOT HEIGHT (issue #185).
///
/// The orchestrator's own repair log for these runs says:
///
/// ```text
/// layout · radial+root-height · Layout/Default [n212] · height 850 → 1184   (05)
/// layout · radial+root-height · Layout/Default [n193] · height 850 → 5408   (06)
/// layout · radial+root-height · Layout/Default [n197] · height 850 → 1513   (08)
/// ```
///
/// The checkpoint `radial+root-height` (`cleanup::run_cleanup_passes`) has one
/// writer of a root height — `cleanup_root_and_nav::adjust_root_height_to_content`
/// — fed by `resolved_node_height`, which walked EVERY descendant's raw absolute
/// bottom THROUGH clipping ancestors. Run against these documents before the
/// clipping guard, that walk returned exactly the number the pass wrote:
///
/// ```text
///                                            pre-fix walk   delivered root
/// 05-dark-theme-ru  deepest descendant n217      1184px     →     1184px
/// 08-pricing-en     deepest descendant n330      1505px     →     1513px
/// 06-vague-ru       body inside clipped n197     5296px     →     5408px
/// ```
///
/// (06's body resolved at 5296px inside the `clipContent` `Main container`; the
/// root's 5408 is that bottom plus the shell's trailing padding. The dump that
/// measured it is in the issue's comment.)
///
/// The invariant that has to hold either way: the root is sized to what the
/// canvas can PAINT, never to a descendant some frame crops away. Per document:
/// `visible` is the visible content bottom after the fix, and `oversized` marks
/// the documents whose delivered root really is a defect.
///
/// `05-dark-theme-ru` is **not** one of them: its dashboard body legitimately
/// reaches 1184px, so the pass' 1184 was right for that document — the defect is
/// the 5408px root of a 1026px screen (`06`) and the 1513px root of the same
/// screen (`08`), where the repair sized the root to a body INSIDE a clipped
/// `Main container`.
#[test]
fn the_root_is_sized_to_visible_content_not_to_cropped_content() {
    for (file, written, visible_bottom, oversized) in [
        (
            ".openpencil-tmp/gq/05-dark-theme-ru.op",
            1184.0,
            1190.0,
            false,
        ),
        (".openpencil-tmp/gq/06-vague-ru.op", 5408.0, 1100.0, true),
        (".openpencil-tmp/gq/08-pricing-en.op", 1513.0, 1100.0, true),
    ] {
        let Some(state) = corpus_state(file) else {
            eprintln!("[SKIP] {file} is not present (gitignored corpus)");
            return;
        };
        let Some(root_id) = chassis_root_id(&state) else {
            return;
        };
        let delivered = delivered_root_height(&state).expect("delivered root height");
        assert_eq!(
            delivered, written,
            "{file}: the delivered root height changed; update this test's evidence"
        );

        let visible = resolved_node_height(&state, &root_id).expect("resolved extent");
        let cropped = deepest_cropped_descendant(&state, &root_id);
        eprintln!(
            "[PROBE] {file}: delivered root {delivered}px; visible extent {visible}px; \
             deepest cropped descendant {cropped:?}"
        );

        assert!(
            visible <= visible_bottom + 4.0,
            "{file}: the repair must size the root to the visible content bottom \
             (<= {visible_bottom}px), got {visible}px"
        );
        if oversized {
            assert!(
                delivered > visible + 100.0,
                "{file}: the delivered root {delivered}px is far taller than the {visible}px \
                 of visible content — that gap is the defect this test pins"
            );
        }
    }
}

/// The fixture derived from `06-vague-ru.op`'s real shape, with the deep body
/// made deterministic so the test does not need the scratch corpus.
///
/// root 1440x850 (layout none, clip) → [sidebar 251x814 @20,20, Content
/// 1137x1006 @287,20 (vertical, gap 16) → [breadcrumbs 48, Main container
/// (fill x fill, clipContent, padding 24, gap 16) → body: a fit_content stack
/// resolving ~5.7k]]. The body is CROPPED by `Main container`, so not one pixel
/// of it can reach the canvas — the root must stay at the visible content's
/// bottom (~1026) instead of growing to the hidden extent.
#[test]
fn a_clipping_frame_cannot_inflate_the_root() {
    let mut sink = VecDocSink::new();
    let rows: Vec<serde_json::Value> = (0..12)
        .map(|i| {
            json!({
                "type": "frame",
                "id": format!("row-{i}"),
                "name": format!("Row {i}"),
                "width": "fill_container",
                "height": 440
            })
        })
        .collect();
    let tree: PenNode = serde_json::from_value(json!({
        "type": "frame",
        "id": "root",
        "name": "Layout/Default",
        "width": 1440,
        "height": CHASSIS_ROOT_HEIGHT,
        "layout": "none",
        "clipContent": true,
        "children": [
            {
                "type": "frame", "id": "sidebar", "name": "Sidebar/Default",
                "x": 20, "y": 20, "width": 251, "height": 814, "layout": "none"
            },
            {
                "type": "frame", "id": "content", "name": "Content",
                "x": 287, "y": 20, "width": 1137, "height": 1006,
                "layout": "vertical", "gap": 16, "alignItems": "start",
                "children": [
                    {
                        "type": "frame", "id": "breadcrumbs", "name": "Breadcrumbs",
                        "width": "fill_container", "height": 48
                    },
                    {
                        "type": "frame", "id": "main", "name": "Main container",
                        "width": "fill_container", "height": "fill_container",
                        "clipContent": true, "layout": "vertical", "gap": 16,
                        "padding": [24, 24],
                        "children": [{
                            "type": "frame", "id": "body", "name": "Body",
                            "width": "fill_container", "height": "fit_content",
                            "layout": "vertical", "gap": 24,
                            "padding": [28, 0, 28, 0],
                            "children": rows
                        }]
                    }
                ]
            }
        ]
    }))
    .expect("shell fixture json");
    sink.state.apply(EditorCommand::InsertSubtree {
        nodes: vec![tree],
        parent_id: NodeId::NONE,
        page_id: None,
    });
    let root_id = sink.state.active_children()[0].id_str().to_string();

    let visible = resolved_node_height(&sink.state, &root_id).expect("resolved extent");
    let (deepest_id, deepest, _) =
        deepest_cropped_descendant(&sink.state, &root_id).expect("cropped descendant");
    eprintln!("[PROBE] fixture: visible {visible}px, cropped deepest {deepest_id} at {deepest}px");
    assert!(
        deepest > 5000.0,
        "the cropped body must still resolve far below the fold, got {deepest}px"
    );

    sink.applied.clear();
    run_cleanup_passes(&mut sink, &shell_plan(&root_id), &[&root_id]);

    let root = sink
        .state
        .active_children()
        .iter()
        .find(|node| node.id_str() == root_id)
        .or_else(|| {
            sink.state
                .active_children()
                .iter()
                .find(|node| node.base().name.as_deref() == Some("Layout/Default"))
        })
        .expect("root survives cleanup");
    let grown = root.height_px().expect("numeric root height");
    eprintln!("[PROBE] fixture: root height after cleanup {grown}px");
    assert!(
        grown > CHASSIS_ROOT_HEIGHT,
        "the root still grows to fit what IS visible, got {grown}px"
    );
    assert!(
        grown < 1200.0,
        "a 5.7k-px body cropped by `Main container` must not inflate the root; got {grown}px"
    );
}
