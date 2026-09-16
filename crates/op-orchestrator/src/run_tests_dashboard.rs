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
/// the sidebar. The sidebar it inspects is the KIT's, not the orchestrator's —
/// the orchestrator's own two-column scaffold is covered by
/// [`pipeline_builds_the_two_column_scaffold_for_a_weak_nav_dashboard_plan`],
/// which runs without a kit sentinel.
#[test]
fn run_leaves_the_kit_chassis_sidebar_intact_and_fills_its_content_slot() {
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

/// A named body frame — one per subtask, so an assertion can say WHICH column
/// a section landed in.
fn named_body_script(name: &str) -> String {
    format!(
        r#"I(null, {{"type":"frame","name":"{name}","width":1100,"height":320,"children":[{{"type":"text","content":"{name}","fontSize":18}}]}});"#
    )
}

/// A plan with a WEAK nav first subtask: `nav` + `Navigation` reaches
/// `dashboard_columns::is_sidebar_subtask` (the `nav` keyword) without matching
/// `is_strong_sidebar_subtask` (no `sidebar` / `rail` / `side nav` token), so
/// `plan_repair::finalize_plan`'s kit-chrome strip leaves it in place.
const WEAK_NAV_DASHBOARD_PLAN_JSON: &str = r##"{
  "rootFrame": { "id": "root", "name": "Ops Dashboard", "width": 1200, "height": 800,
                 "layout": "vertical", "gap": 0,
                 "fill": [{ "type": "solid", "color": "#0A0A0A" }] },
  "subtasks": [
    { "id": "nav", "label": "Navigation", "region": { "width": 260, "height": 900 } },
    { "id": "table", "label": "Client Table", "region": { "width": 940, "height": 500 } },
    { "id": "stats", "label": "Stats", "region": { "width": 940, "height": 200 } }
  ]
}"##;

/// Reachability guard for `scaffold::plan_is_sidebar_dashboard` and the
/// two-column root it pre-builds (`scaffold::build_two_column_root_node`).
///
/// `codenol/zopen_norka#30` claimed the pair is dead code: that `finalize_plan`
/// strips every sidebar from a desktop-screen plan, so the predicate can only
/// be satisfied by plans the pipeline never produces, and that the predicate's
/// own unit tests hide this by building plans directly and skipping
/// `finalize_plan`. This test drives the REAL run instead — planning JSON →
/// `repair_plan_object` → `finalize_plan` → `normalize` → scaffold → subtasks —
/// on a document with no kit sentinel, and asserts the shape end to end.
///
/// What makes the route live: `finalize_plan` only strips a STRONG sidebar
/// signal (`is_strong_sidebar_subtask`: `sidebar` / `side bar` / `side nav` /
/// `left rail` / `nav rail`, `plan_repair.rs::is_kit_owned_chrome`). A nav
/// subtask the planner named "Navigation" is a WEAK signal, survives the strip,
/// and satisfies the predicate's ambiguous-nav branch once two data sections
/// back it. Verified: the run below emits `horizontal [Sidebar | Main Content]`
/// and routes the nav section into the sidebar and both data sections into the
/// content column.
///
/// The complement is covered too: with a STRONG `Sidebar Navigation` first
/// subtask the strip does remove it and the run stays on the single-root path —
/// that half of #30 is accurate, and it is why this fixture uses the weak form.
#[test]
fn pipeline_builds_the_two_column_scaffold_for_a_weak_nav_dashboard_plan() {
    let llm = ScriptedLlm::new(vec![
        ScriptResponse::Text(WEAK_NAV_DASHBOARD_PLAN_JSON.into()),
        ScriptResponse::Text(named_body_script("Nav Body")),
        ScriptResponse::Text(named_body_script("Table Body")),
        ScriptResponse::Text(named_body_script("Stats Body")),
    ]);
    // No kit sentinel: `scaffold_kit::kit_chassis_commands` returns `None`, so
    // the orchestrator builds its own scaffold instead of cloning the chassis.
    let mut sink = VecDocSink::new();
    let mut on_progress = |_| {};
    let mut request = req();
    request.prompt = "an analytics admin dashboard".into();
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
        summary.subtasks.len(),
        3,
        "the weak-nav plan survives finalize_plan whole — this is the \
         reachability claim of #30 under test",
    );

    let root = sink.state.active_children().first().expect("scaffold root");
    let v = serde_json::to_value(root).unwrap();
    assert_eq!(v["layout"], "horizontal", "two-column root: {v}");
    let kids = v["children"].as_array().expect("columns");
    let column_names: Vec<&str> = kids.iter().filter_map(|k| k["name"].as_str()).collect();
    assert_eq!(
        column_names,
        vec!["Sidebar", "Main Content"],
        "pre-built columns: {v}",
    );
    assert_eq!(kids[0]["width"], serde_json::json!(260.0));
    assert_eq!(kids[0]["height"], serde_json::json!("fill_container"));

    // The run loop re-resolves the columns by name and routes each subtask into
    // one of them (`run_orchestrator.rs`, the `two_col` branch) — so this also
    // proves that call site is live, not just the scaffold predicate.
    let names_in = |column: &serde_json::Value| {
        let mut out = Vec::new();
        descendant_names(column, &mut out);
        out
    };
    let sidebar_names = names_in(&kids[0]);
    let content_names = names_in(&kids[1]);
    assert!(
        sidebar_names.iter().any(|n| n == "Nav Body"),
        "the nav section generated into the sidebar column: {sidebar_names:?}",
    );
    assert!(
        content_names.iter().any(|n| n == "Table Body")
            && content_names.iter().any(|n| n == "Stats Body"),
        "both data sections generated into the content column: {content_names:?}",
    );
    assert!(
        !content_names.iter().any(|n| n == "Nav Body")
            && !sidebar_names.iter().any(|n| n == "Table Body"),
        "sections must not cross columns: sidebar={sidebar_names:?} content={content_names:?}",
    );
}
