//! Component-library loading + merge.
//!
//! A *component library* is an ordinary `.op` / `.lib.op` document whose
//! top-level (or per-page) frames carry `reusable: true`. Loading one into
//! a working document makes those masters addressable by `ref` nodes during
//! AI generation — the "design-kit composition" path.
//!
//! [`merge_library_into_state`] reads such a file via the canonical loader,
//! harvests its reusable masters with
//! [`op_editor_core::ComponentLibrary::from_document`], and merges them into
//! a live [`EditorState`]:
//!
//! 1. the master frames are appended to a dedicated, hidden `Components`
//!    page (deduped by id) — NOT the active design page. This keeps
//!    `active_children()` clean (only the design) so the orchestrator's
//!    dashboard scaffold + role/cleanup passes are unaffected, while the
//!    masters stay discoverable for ref resolution (both
//!    `ComponentLibrary::from_document` and `ref_resolve::resolve_refs_for_canvas`
//!    walk every page, not just the active one),
//! 2. the library's `variables` + `themes` are merged into the doc (so each
//!    master's `$--token` fills resolve), and
//! 3. `state.components` is rebuilt so the runtime registry + the generator's
//!    available-components manifest see the new masters immediately.
//!
//! [`crate::ensure_skala_session`] is the default New / launch path: it
//! merges this library onto a hidden Components store so a blank file still
//! has every Skala master. Direct callers remain for smoke and kit import.

use op_editor_core::{ComponentLibrary, EditorState};

use crate::payload::load_canonical;

/// Why a component-library merge could not run.
///
/// Variants carry the structured pieces (which file, which underlying
/// failure) instead of a pre-formatted sentence; `Display` reproduces the
/// exact strings this module used to return so log lines and callers that
/// interpolate the error do not move by a byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryMergeError {
    /// The `.lib.op` file could not be read off disk.
    Read {
        /// Path the caller asked for.
        path: String,
        /// The underlying `std::io::Error`, rendered.
        error: String,
    },
    /// The file was read but its canonical `.op` JSON did not load.
    Load {
        /// Path the caller asked for.
        path: String,
        /// The [`LibraryMergeError::Parse`] message from the source-string core.
        error: String,
    },
    /// An in-memory library source failed the canonical `.op` parse.
    Parse {
        /// The `jian_ops_schema` load error, rendered.
        error: String,
    },
}

impl std::fmt::Display for LibraryMergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryMergeError::Read { path, error } => write!(f, "read library {path}: {error}"),
            LibraryMergeError::Load { path, error } => write!(f, "load library {path}: {error}"),
            LibraryMergeError::Parse { error } => f.write_str(error),
        }
    }
}

impl std::error::Error for LibraryMergeError {}

/// Keeps `?` working for callers that still collect failures as `String`
/// (e.g. `op-smoke`'s loop setup and the desktop import path).
impl From<LibraryMergeError> for String {
    fn from(error: LibraryMergeError) -> String {
        error.to_string()
    }
}

/// Outcome of a successful library merge — counts for logging / tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryMergeReport {
    /// Reusable master frames appended to the hidden `Components`
    /// page (post-dedup).
    pub masters_added: usize,
    /// Total reusable masters now registered in `state.components`.
    pub component_count: usize,
    /// Variable definitions merged in from the library (newly added only).
    pub variables_added: usize,
    /// Theme axes merged in from the library (newly added only).
    pub themes_added: usize,
}

/// Load the `.lib.op` at `path`, harvest its reusable masters, and merge them
/// (plus its variables + themes) into `state`. Existing document content is
/// preserved; masters/variables/themes whose ids already exist are skipped so
/// re-importing the same library is idempotent.
///
/// Returns the merge report on success, or a [`LibraryMergeError`] when the
/// file can't be read or parsed. On error `state` is left unchanged.
pub fn merge_library_into_state(
    state: &mut EditorState,
    path: &str,
) -> Result<LibraryMergeReport, LibraryMergeError> {
    let src = std::fs::read_to_string(path).map_err(|e| LibraryMergeError::Read {
        path: path.to_string(),
        error: e.to_string(),
    })?;
    merge_library_src_into_state(state, &src).map_err(|e| LibraryMergeError::Load {
        path: path.to_string(),
        error: e.to_string(),
    })
}

/// Source-string variant of [`merge_library_into_state`] — parses canonical
/// `.op` JSON already in memory. Shared core so tests can drive it without a
/// temp file.
pub fn merge_library_src_into_state(
    state: &mut EditorState,
    src: &str,
) -> Result<LibraryMergeReport, LibraryMergeError> {
    let loaded = load_canonical(src).map_err(|e| LibraryMergeError::Parse {
        error: e.to_string(),
    })?;
    let lib_doc = loaded.value;

    // Harvest reusable masters from the library document (top-level + pages).
    let library = ComponentLibrary::from_document(&lib_doc);

    let mut report = LibraryMergeReport::default();

    // 1. Append the master frames onto a dedicated, hidden `Components`
    //    page — NOT the active design page. `append_components_page_masters`
    //    keeps `active_children()` clean (only the design) so scaffolding +
    //    role/cleanup passes are unaffected, preserves the active page index,
    //    keeps master ids verbatim so refs resolve, and dedups by id so a
    //    re-import is idempotent.
    let masters: Vec<_> = library
        .components
        .iter()
        .filter_map(|component| library.resolved_root(&lib_doc, &component.id).cloned())
        .collect();
    report.masters_added = state.append_components_page_masters(masters);

    // 2. Merge the library's variables (only new names) so master `$--token`
    //    fills resolve against concrete values.
    if let Some(lib_vars) = lib_doc.variables.as_ref() {
        let dst = state.doc.variables.get_or_insert_with(Default::default);
        for (name, def) in lib_vars {
            if !dst.contains_key(name) {
                dst.insert(name.clone(), def.clone());
                report.variables_added += 1;
            }
        }
    }

    // 3. Merge the library's theme axes (only new axis names).
    if let Some(lib_themes) = lib_doc.themes.as_ref() {
        let dst = state.doc.themes.get_or_insert_with(Default::default);
        for (axis, values) in lib_themes {
            if !dst.contains_key(axis) {
                dst.insert(axis.clone(), values.clone());
                report.themes_added += 1;
            }
        }
    }

    // 4. Rebuild the runtime component registry off the merged document so the
    //    generator's available-components manifest sees the new masters now.
    state.components = ComponentLibrary::from_document(&state.doc);
    report.component_count = state.components.len();

    if report.masters_added > 0 || report.variables_added > 0 || report.themes_added > 0 {
        state.mark_document_changed();
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::page_mutators::{is_component_store_page, COMPONENTS_PAGE_NAME};
    use op_editor_core::pen_node_ext::PenNodeExt;

    /// Children of every `Components/{Type}` store page (and a leftover
    /// legacy `Components` page, if present).
    fn components_page_children(state: &EditorState) -> Vec<&jian_ops_schema::node::PenNode> {
        state
            .doc
            .pages
            .as_ref()
            .map(|pages| {
                pages
                    .iter()
                    .filter(|p| is_component_store_page(&p.name))
                    .flat_map(|p| p.children.iter())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One reusable master frame as canonical `.op` JSON (a button-like
    /// frame whose fill references a `$--primary` token so the merge's
    /// variable handling matters). Built with `serde_json` so color
    /// literals like `#18181B` don't trip the raw-string `r#` prefix rules.
    fn reusable_frame_json(id: &str, name: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "type": "frame",
            "name": name,
            "reusable": true,
            "width": 120,
            "height": 40,
            "layout": "horizontal",
            "cornerRadius": 8,
            "fill": [{ "type": "solid", "color": "$--primary" }],
            "children": [
                { "id": format!("{id}-label"), "type": "text", "name": "Label",
                  "content": name, "fontSize": 14 }
            ],
        })
    }

    /// A library document (JSON) with `n` reusable masters + variables + a
    /// theme axis. Serialized to a canonical `.op` JSON string so it goes
    /// through the exact same `load_canonical` path a real `.lib.op` file
    /// would.
    fn library_src(n: usize) -> String {
        let children: Vec<serde_json::Value> = (0..n)
            .map(|i| reusable_frame_json(&format!("lib-comp-{i}"), &format!("Component/{i}")))
            .collect();
        let doc = serde_json::json!({
            "version": "1.0",
            "name": "Test Library",
            "themes": { "mode": ["light", "dark"] },
            "variables": {
                "--primary": { "type": "color", "value": "#18181B" },
                "--surface": { "type": "color", "value": "#FAFAFA" },
            },
            "children": children,
        });
        serde_json::to_string(&doc).unwrap()
    }

    #[test]
    fn merges_masters_variables_and_themes_into_empty_state() {
        let mut state = EditorState::new();
        let src = library_src(120);
        let report = merge_library_src_into_state(&mut state, &src).expect("merge");

        assert_eq!(report.masters_added, 120);
        assert_eq!(report.component_count, 120);
        assert!(
            report.component_count > 100,
            "component count must exceed 100"
        );
        assert_eq!(report.variables_added, 2);
        assert_eq!(report.themes_added, 1);
        // The masters landed on the hidden Components page, NOT the
        // active design page. The empty starter doc had no design, so
        // the active page (page 0) stays empty.
        assert_eq!(components_page_children(&state).len(), 120);
        assert!(
            state
                .doc
                .pages
                .as_ref()
                .unwrap()
                .iter()
                .all(|page| page.name != COMPONENTS_PAGE_NAME),
            "legacy combined Components page must be split by type"
        );
        let origins: std::collections::HashSet<(i64, i64)> = components_page_children(&state)
            .iter()
            .map(|node| {
                (
                    node.base().x.unwrap_or(0.0).round() as i64,
                    node.base().y.unwrap_or(0.0).round() as i64,
                )
            })
            .collect();
        assert_eq!(
            origins.len(),
            120,
            "gallery layout must give every master its own origin"
        );
        assert!(
            state.active_children().is_empty(),
            "active design page must stay clean of masters"
        );
        // Variables + themes are present so master `$--token` fills resolve.
        assert!(state
            .doc
            .variables
            .as_ref()
            .unwrap()
            .contains_key("--primary"));
        assert!(state.doc.themes.as_ref().unwrap().contains_key("mode"));
        // The runtime registry sees them too.
        assert_eq!(state.components.len(), 120);
        assert!(state.components.find_by_name("Component/7").is_some());
        let id = op_editor_core::NodeId::new("lib-comp-7");
        let store_children = components_page_children(&state);
        let canonical = store_children
            .iter()
            .find(|node| node.id_str() == id.as_str())
            .copied()
            .expect("canonical merged master");
        assert!(std::ptr::eq(
            state
                .components
                .resolved_root(&state.doc, &id)
                .expect("document-backed master"),
            canonical,
        ));
    }

    #[test]
    fn dedup_keeps_existing_and_is_idempotent() {
        let mut state = EditorState::new();
        let src = library_src(5);
        let first = merge_library_src_into_state(&mut state, &src).expect("first");
        assert_eq!(first.masters_added, 5);
        // Re-importing the same library adds nothing new.
        let second = merge_library_src_into_state(&mut state, &src).expect("second");
        assert_eq!(second.masters_added, 0);
        assert_eq!(second.variables_added, 0);
        assert_eq!(second.themes_added, 0);
        // Component count is stable.
        assert_eq!(second.component_count, 5);
        // Re-import is idempotent on the Components page (no duplicates).
        assert_eq!(components_page_children(&state).len(), 5);
    }

    #[test]
    fn preserves_existing_document_content() {
        // A working document with one ordinary (non-reusable) user frame.
        let user_doc_src = r#"{"version":"1.0","children":[
            {"id":"user-frame","type":"frame","name":"User Frame","width":200,"height":100}
        ]}"#;
        let loaded = load_canonical(user_doc_src).expect("user doc");
        let mut state = EditorState::from_document(loaded.value);
        let src = library_src(3);
        let report = merge_library_src_into_state(&mut state, &src).expect("merge");
        assert_eq!(report.masters_added, 3);
        // The user's design survived untouched on the active page — the
        // masters went to the separate Components page, not beside it.
        assert_eq!(state.active_children().len(), 1);
        assert_eq!(state.active_children()[0].id_str(), "user-frame");
        assert_eq!(components_page_children(&state).len(), 3);
        // No master leaked onto the active design page.
        assert!(state
            .active_children()
            .iter()
            .all(|n| n.id_str() == "user-frame"));
    }

    #[test]
    fn bad_source_leaves_state_unchanged() {
        let mut state = EditorState::new();
        let err = merge_library_src_into_state(&mut state, "not json").unwrap_err();
        assert!(!err.to_string().is_empty());
        assert!(state.doc.children.is_empty());
        assert!(state.components.is_empty());
    }

    /// The root bug: masters appended to the active design page polluted
    /// `active_children()` and broke the dashboard scaffold index math.
    /// This pins the fix end-to-end:
    /// - the active design page stays CLEAN of masters (so scaffold +
    ///   role/cleanup passes work),
    /// - the masters live on a separate hidden `Components` page,
    /// - `ComponentLibrary::from_document` still finds all of them
    ///   (it walks every page, not just the active one), and
    /// - a `ref` on the active page resolves cross-page to a master on
    ///   the Components page via `resolve_refs_for_canvas`.
    #[test]
    fn masters_isolated_on_components_page_but_resolve_cross_page() {
        // Active design page carries one ordinary frame plus a `ref`
        // instance whose target is a library master.
        let user_doc_src = r#"{"version":"1.0","children":[
            {"id":"design-root","type":"frame","name":"Design Root","width":1200,"height":800},
            {"id":"inst-7","type":"ref","ref":"lib-comp-7","x":40,"y":40}
        ]}"#;
        let loaded = load_canonical(user_doc_src).expect("user doc");
        let mut state = EditorState::from_document(loaded.value);

        let src = library_src(46);
        let report = merge_library_src_into_state(&mut state, &src).expect("merge");
        assert_eq!(report.masters_added, 46);

        // 1. The active design page is CLEAN of masters — only the
        //    design root + the ref instance remain.
        let active = state.active_children();
        assert_eq!(active.len(), 2);
        assert!(
            active.iter().all(|n| !matches!(
                n,
                jian_ops_schema::node::PenNode::Frame(f) if f.reusable == Some(true)
            )),
            "no reusable master may leak onto the active design page"
        );
        assert_eq!(
            state.ui.active_page_index, 0,
            "active page stays the design"
        );

        // 2. A separate Components page holds all 46 masters.
        assert_eq!(components_page_children(&state).len(), 46);

        // 3. ComponentLibrary::from_document finds all 46 (it walks
        //    every page, not just the active one).
        let lib = ComponentLibrary::from_document(&state.doc);
        assert_eq!(lib.len(), 46);
        assert!(lib.find_by_name("Component/7").is_some());

        // 4. The active-page ref resolves cross-page to its master on
        //    the Components page via resolve_refs_for_canvas.
        let resolved = op_editor_core::ref_resolve::resolve_refs_for_canvas(&state.doc);
        let resolved_active = resolved
            .pages
            .as_ref()
            .map(|pages| pages[0].children.as_slice())
            .unwrap_or(&resolved.children);
        let expanded = resolved_active
            .iter()
            .find(|n| n.id_str() == "inst-7")
            .expect("ref instance survives resolution");
        // The ref expanded into the master's Frame subtree (it carries
        // the master's label child remapped into the instance id space),
        // i.e. it did NOT drop as an unknown target.
        let children = PenNodeExt::children(expanded)
            .expect("cross-page master resolved into a populated subtree");
        assert!(
            children
                .iter()
                .any(|c| c.id_str() == "inst-7__lib-comp-7-label"),
            "the master on the Components page resolved into the active-page instance"
        );
    }
}
