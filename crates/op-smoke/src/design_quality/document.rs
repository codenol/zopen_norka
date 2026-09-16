//! Reading a `/api/mcp/document` payload: what is on the page, and which of it
//! this turn put there.
//!
//! Two things the hand prototypes (`.openpencil-tmp/gq2/analyze.py`,
//! `harness.py`) established and this module keeps:
//!
//! 1. **Everything counted is `pages[0]`** — the page the user actually sees.
//!    The same payload carries the kit's `Components/*` pages, which hold
//!    thousands of library nodes; counting those would make every number a
//!    number about the library instead of about the canvas.
//! 2. **Landed content is attributed by NAME, never by a before/after node
//!    diff.** The daemon is shared (a browser tab can hold a live-sync session),
//!    so a bare diff can credit this turn with another writer's nodes. The
//!    top-level frame names this turn's own DSL emitted are looked up in the
//!    after-document, and only those frames (and their subtrees) count as this
//!    turn's nodes.

use serde::Serialize;
use serde_json::Value;

/// One top-level node of the active page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrameInfo {
    pub(crate) name: String,
    pub(crate) kind: String,
    /// Nodes in this node's subtree, the node itself included.
    pub(crate) nodes: u32,
}

/// Page-0 inventory of one read-back document.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInventory {
    /// Nodes under every top-level node of the active page.
    pub(crate) nodes: u32,
    /// Top-level nodes, in document order.
    pub(crate) frames: Vec<FrameInfo>,
    /// Total pages in the document (kit pages included) — reported so a
    /// surprising count can be seen rather than guessed.
    pub(crate) pages: usize,
}

impl PageInventory {
    /// Top-level frame names, in document order.
    pub(crate) fn frame_names(&self) -> Vec<String> {
        self.frames.iter().map(|f| f.name.clone()).collect()
    }
}

/// Counts a node's subtree, the node itself included.
fn subtree_count(node: &Value) -> u32 {
    1 + node
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().map(subtree_count).sum())
        .unwrap_or(0)
}

/// Reads `pages[0]` out of a `/api/mcp/document` payload.
///
/// Accepts both the wrapped payload (`{"document": {…}, "version": N}`) and a
/// bare `PenDocument`, so the same reader serves the live route and a `.op`
/// file.
pub(crate) fn page0(payload: &Value, page_index: usize) -> PageInventory {
    let document = payload.get("document").unwrap_or(payload);
    let pages = document
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let page = pages.get(page_index);
    let frames: Vec<FrameInfo> = page
        .and_then(|p| p.get("children"))
        .and_then(Value::as_array)
        .map(|children| {
            children
                .iter()
                .map(|node| FrameInfo {
                    name: node
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string(),
                    kind: node
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string(),
                    nodes: subtree_count(node),
                })
                .collect()
        })
        .unwrap_or_default();
    PageInventory {
        nodes: frames.iter().map(|f| f.nodes).sum(),
        frames,
        pages: pages.len(),
    }
}

/// One root-level DSL statement the reply emitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DslStatement {
    /// The binding token the statement parents to (`null` for a root).
    pub(crate) parent: String,
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) name: String,
}

/// Scans a reply for the Pencil DSL statements that start a line.
///
/// The hand prototype's regex was
/// `^I\(([^,]*),\s*\{\s*id:"([^"]+)",\s*type:"([^"]+)",\s*name:"([^"]*)"` — a
/// statement at the start of a line whose object opens with `id`, `type`,
/// `name` in that order. This scanner keeps the line-start `I(` rule and the
/// `id`/`type`/`name` requirement but does not demand that exact adjacency, so
/// a reply that orders the keys differently is still attributed instead of
/// silently counting as "nothing landed". It never invents a name: a statement
/// missing `name` is skipped, exactly as the regex would have skipped it.
pub(crate) fn scan_dsl_statements(reply: &str) -> Vec<DslStatement> {
    let mut out = Vec::new();
    for line in reply.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("I(") else {
            continue;
        };
        let Some((parent, tail)) = rest.split_once(',') else {
            continue;
        };
        // The statement's own object literal. The prototype's rule was
        // line-anchored, and staying on the line is what keeps a nested
        // statement on the next line from donating its keys to this one.
        let body = tail;
        let Some(id) = field(body, "id") else {
            continue;
        };
        let Some(kind) = field(body, "type") else {
            continue;
        };
        let Some(name) = field(body, "name") else {
            continue;
        };
        out.push(DslStatement {
            parent: parent.trim().to_string(),
            id,
            kind,
            name,
        });
    }
    out
}

/// `field:"value"` out of a statement body, unescaped for the common cases.
fn field(body: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let mut search = body;
    loop {
        let at = search.find(&needle)?;
        // Guard against matching inside another key (`subtype:` for `type:`).
        let boundary_ok = at == 0
            || !search[..at]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric() || c == '_')
                .unwrap_or(false);
        let after = &search[at + needle.len()..];
        let after = after.trim_start();
        if boundary_ok {
            if let Some(value) = after.strip_prefix('"') {
                if let Some(end) = value.find('"') {
                    return Some(value[..end].replace("\\\"", "\"").replace("\\\\", "\\"));
                }
            }
            return None;
        }
        search = after;
    }
}

/// A frame this turn's own DSL emitted AND that is present in the document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LandedFrame {
    pub(crate) name: String,
    pub(crate) nodes: u32,
}

/// Matches the emitted statements' names against the page's top-level frames.
///
/// Returns the landed frames and the node total of their subtrees — the
/// "nodes landed by this turn" number both hand passes reported.
pub(crate) fn attribution(
    statements: &[DslStatement],
    inventory: &PageInventory,
) -> (Vec<LandedFrame>, u32) {
    let mut landed = Vec::new();
    for statement in statements {
        if let Some(frame) = inventory
            .frames
            .iter()
            .find(|frame| frame.name == statement.name)
        {
            if !landed.iter().any(|f: &LandedFrame| f.name == frame.name) {
                landed.push(LandedFrame {
                    name: frame.name.clone(),
                    nodes: frame.nodes,
                });
            }
        }
    }
    let nodes = landed.iter().map(|f| f.nodes).sum();
    (landed, nodes)
}

/// Root-level statements (`I(null, {…})`) in the emitted set — the count
/// `harness.py` reported as `turn_root_statements`.
pub(crate) fn root_statement_count(statements: &[DslStatement]) -> u32 {
    statements
        .iter()
        .filter(|s| s.parent == "null" || s.parent.is_empty())
        .count() as u32
}
