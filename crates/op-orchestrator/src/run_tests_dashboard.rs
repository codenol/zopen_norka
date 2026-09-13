//! Dashboard-shell end-to-end tests and the planning retry-before-fallback
//! case.

use super::*;

const DASHBOARD_PLAN_JSON: &str = r##"{
  "rootFrame": { "id": "root", "name": "Barbershop Dashboard", "width": 1200, "height": 0,
                 "layout": "vertical", "gap": 0,
                 "fill": [{ "type": "solid", "color": "#0A0A0A" }] },
  "subtasks": [
    { "id": "sidebar", "label": "Sidebar Navigation", "region": { "width": 260, "height": 900 } },
    { "id": "kpi", "label": "KPI Stat Cards", "region": { "width": 940, "height": 200 } },
    { "id": "clients", "label": "Client Table", "region": { "width": 940, "height": 500 } }
  ]
}"##;

/// The session-kit sentinel, shaped like the shipped `Layout/Default`
/// (`design/skala-spectrum.lib.op`): a full-artboard chassis whose sidebar
/// column is `height: fill_container` with `justifyContent: space_between`,
/// and whose `Main container` is the slot sub-agents fill.
///
/// The sidebar is authored HERE now, not by this crate's scaffold — that is
/// the whole point of the kit chassis, and why the invariant below moved.
fn kit_chassis_state() -> VecDocSink {
    let kit = op_editor_core::session_kit();
    let mut sink = VecDocSink::new();
    let root: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "id": kit.sentinel_master_id,
        "type": "frame",
        "name": "Layout/Default",
        "reusable": true,
        "x": 0,
        "y": 0,
        "width": 1440.0,
        "height": 850.0,
        "layout": "none",
        "children": [
            {
                "id": "tpl-layout-sidebar",
                "type": "frame",
                "name": "Sidebar",
                "x": 20.0,
                "y": 20.0,
                "width": 251.0,
                "height": "fill_container",
                "layout": "vertical",
                "justifyContent": "space_between",
                "children": [
                    {
                        "id": "tpl-sidebar-top",
                        "type": "frame",
                        "name": "Top",
                        "layout": "vertical",
                        "children": [{ "type": "text", "id": "tpl-sidebar-logo", "content": "MAISON" }]
                    },
                    {
                        "id": "tpl-sidebar-owner",
                        "type": "frame",
                        "name": "Owner Card",
                        "layout": "vertical",
                        "children": [{ "type": "text", "id": "tpl-sidebar-owner-line", "content": "Marcus Reed" }]
                    }
                ]
            },
            {
                "id": "tpl-layout-content",
                "type": "frame",
                "name": "Content",
                "x": 287.0,
                "y": 20.0,
                "width": 1137.0,
                "height": 814.0,
                "layout": "vertical",
                "gap": 16.0,
                "alignItems": "start",
                "children": [{
                    "id": "tpl-layout-main",
                    "type": "frame",
                    "name": kit.content_slot_name(),
                    "width": "fill_container",
                    "height": 750.0,
                    "layout": "horizontal",
                    "justifyContent": "center",
                    "alignItems": "center",
                    "children": [{
                        "type": "text",
                        "id": "tpl-layout-main-label",
                        "name": "label",
                        "content": "Main container"
                    }]
                }]
            }
        ]
    }))
    .expect("kit sentinel fixture");
    sink.state.components.insert(op_editor_core::Component {
        id: op_editor_core::NodeId::new(kit.sentinel_master_id.clone()),
        name: "Layout".into(),
        root,
    });
    sink
}

/// Every depth-first descendant name under `node`, the assertion vocabulary
/// for "where did this subtree land".
fn descendant_names(node: &serde_json::Value, out: &mut Vec<String>) {
    for child in node["children"].as_array().into_iter().flatten() {
        out.push(child["name"].as_str().unwrap_or_default().to_string());
        descendant_names(child, out);
    }
}

/// GOLDEN end-to-end (no LLM): dashboard plan → session-kit chassis →
/// scripted subtask → finalize, on a document that carries the kit sentinel.
///
/// The sidebar shell is no longer built by this crate: `plan_repair::
/// finalize_plan` strips kit-owned chrome (sidebar / topbar) from every
/// desktop-screen plan, and the chassis the run puts on the page is the kit's
/// `Layout/Default` sentinel, cloned by `scaffold_kit::kit_chassis_commands`
/// and then filled through its content slot. The user-visible symptom this
/// test was written for — the sidebar footer floating mid-page, reported
/// three times — is now the sentinel's own responsibility, so what the golden
/// guards is that a whole run leaves the cloned chassis' chrome exactly as the
/// kit authored it (`height: fill_container` + `space_between`, footer still
/// inside) while the sub-agent body lands in the content slot rather than in
/// the sidebar.
#[test]
fn run_dashboard_shell_keeps_sidebar_fill_height_end_to_end() {
    // Script-gen, not raw JSON: the sub-agent protocol is `I(parent, obj)`.
    let body_script = r#"I(null, {"type":"frame","name":"Body","width":1100,"height":480,"children":[{"type":"text","content":"Clients","fontSize":18}]});"#;
    let llm = ScriptedLlm::new(vec![
        ScriptResponse::Text(DASHBOARD_PLAN_JSON.into()),
        ScriptResponse::Text(body_script.into()),
    ]);
    let mut sink = kit_chassis_state();
    let mut events: Vec<Progress> = Vec::new();
    let mut on_progress = |p: Progress| events.push(p);
    let mut request = req();
    request.prompt = "barbershop client-management dashboard with a left sidebar".into();
    request.validation_enabled = false;

    let summary = futures::executor::block_on(Orchestrator::new().run(
        request,
        &mut sink,
        &llm,
        &mut on_progress,
        &AbortFlag::new(),
        &stub_providers(),
    ))
    .expect("dashboard run ok");

    assert_eq!(
        sink.state().active_children().len(),
        1,
        "the run must place exactly one root — the cloned kit chassis",
    );
    let root = sink.state.active_children().first().expect("root");
    let v = serde_json::to_value(root).unwrap();
    assert_eq!(
        v["name"],
        serde_json::json!("Layout/Default"),
        "the root is the kit's chassis, not an orchestrator-authored app shell",
    );
    let kids = v["children"].as_array().expect("chassis children");

    let sidebar = kids
        .iter()
        .find(|k| {
            k["name"]
                .as_str()
                .map(|n| n.to_lowercase().contains("sidebar"))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| {
            panic!(
                "sidebar shell present, got children: {:?}",
                kids.iter().map(|k| k["name"].clone()).collect::<Vec<_>>()
            )
        });
    assert_eq!(
        sidebar["height"],
        serde_json::json!("fill_container"),
        "sidebar shell keeps fill height; sidebar = {}",
        serde_json::to_string_pretty(&sidebar)
            .unwrap()
            .chars()
            .take(600)
            .collect::<String>()
    );
    assert_eq!(
        sidebar["justifyContent"],
        serde_json::json!("space_between"),
        "the sidebar keeps the kit's own pin — this is what stops the footer \
         floating mid-page",
    );
    let mut sidebar_names = Vec::new();
    descendant_names(sidebar, &mut sidebar_names);
    assert!(
        sidebar_names.iter().any(|n| n == "Owner Card"),
        "the kit sidebar keeps its footer slot: {sidebar_names:?}",
    );

    // The sub-agent body landed in the content slot, not in the sidebar.
    let slot = kids
        .iter()
        .flat_map(|k| k["children"].as_array().cloned().unwrap_or_default())
        .find(|c| c["name"] == serde_json::json!(op_editor_core::session_kit().content_slot_name()))
        .expect("content slot present inside the chassis");
    let mut slot_names = Vec::new();
    descendant_names(&slot, &mut slot_names);
    assert!(
        slot_names.iter().any(|n| n == "Body"),
        "generated body must land in the content slot: {slot_names:?}",
    );
    assert!(
        !sidebar_names.iter().any(|n| n == "Body"),
        "generated body must never land in the sidebar: {sidebar_names:?}",
    );

    // The run really generated: a plan whose sidebar + KPI sections the
    // normaliser strips leaves one content section, and it must have produced
    // nodes — otherwise the assertions above would hold vacuously.
    assert_eq!(
        summary.subtasks.len(),
        1,
        "the stripped dashboard plan runs exactly its content section",
    );
    assert!(
        summary.total_nodes >= 1 && summary.subtasks[0].node_count >= 1,
        "the content section must have generated into the chassis",
    );
}

#[test]
fn planning_retries_once_before_the_fallback_plan() {
    // A truncated planning response fails the parse; the SECOND attempt
    // returns a valid plan and must be used (no skeleton fallback).
    let llm = ScriptedLlm::new(vec![
        ScriptResponse::Text(r##"{"palette":{"background":"#0B0C0E","surface":"#1A1B"##.into()),
        ScriptResponse::Text(PLAN_JSON.into()),
        ScriptResponse::Text(node_json("hero")),
        ScriptResponse::Text(node_json("feat")),
    ]);
    let mut sink = VecDocSink::new();
    let mut events: Vec<Progress> = Vec::new();
    let mut on_progress = |p: Progress| events.push(p);

    let summary = futures::executor::block_on(Orchestrator::new().run(
        req(),
        &mut sink,
        &llm,
        &mut on_progress,
        &AbortFlag::new(),
        &stub_providers(),
    ))
    .expect("run ok after planning retry");

    // The REAL plan (2 subtasks) landed — not the single-subtask fallback.
    assert_eq!(summary.subtasks.len(), 2, "retried plan used, not fallback");
}
