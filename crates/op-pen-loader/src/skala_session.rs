//! Always-on Skala Spectrum session: compact policy + library merge.
//!
//! The `.lib.op` is **not** embedded. Desktop and the web daemon read it
//! from disk (`OPENPENCIL_SKALA_LIB` or `design/skala-spectrum.lib.op`
//! next to the process). Missing file is a loud log, never a silent
//! fallback to starter/shadcn. wasm32 applies the compact `design.md`
//! policy only — it has no filesystem.

use std::path::{Path, PathBuf};

use op_editor_core::{apply_skala_kit_policy, document_has_skala_masters, EditorState};
// Only the disk path names the kit id in its "not found" message and only the
// disk path merges, so on wasm32 these are dead imports — and a dead import is
// a hard error under `-D warnings` (issue #193).
#[cfg(not(target_arch = "wasm32"))]
use op_editor_core::SKALA_KIT_ID;

#[cfg(not(target_arch = "wasm32"))]
use super::library::{merge_library_into_state, LibraryMergeError};
use super::library::LibraryMergeReport;

/// Outcome of ensuring Skala is attached to a session document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkalaSessionReport {
    /// Compact `design.md` was written because the doc had none.
    pub policy_applied: bool,
    /// Library merge result, if a merge ran.
    pub merge: Option<LibraryMergeReport>,
    /// Why a merge did not run (already present, no file, wasm, …).
    pub skip: Option<SkalaSkip>,
}

/// Why the `.lib.op` was not merged this call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkalaSkip {
    /// Masters with Skala ids are already on the document.
    AlreadyPresent,
    /// wasm32 host — no disk.
    NoFilesystem,
    /// No candidate path existed.
    MissingLibrary { tried: Vec<String> },
    /// File existed but could not be read or parsed.
    MergeFailed(String),
}

/// Blank starter Frame plus Skala policy and, when the `.lib.op` is on
/// disk, the reusable masters on hidden `Components/{Type}` pages.
///
/// This is the document File → New and a launch with no path should open.
pub fn new_skala_editor_state() -> EditorState {
    let mut state = EditorState::starter();
    ensure_skala_session(&mut state);
    state
}

/// Attach Skala policy + library to `state`.
///
/// Safe to call on every New and on documents that already contain the
/// kit: merge is idempotent by master id, and the policy is only written
/// when `design_md` is empty.
pub fn ensure_skala_session(state: &mut EditorState) -> SkalaSessionReport {
    let had_policy = state.doc.design_md.is_some();
    apply_skala_kit_policy(state);
    let policy_applied = !had_policy && state.doc.design_md.is_some();

    if document_has_skala_masters(state) {
        if state.layout_components_page_gallery() {
            state.mark_document_changed();
        }
        return SkalaSessionReport {
            policy_applied,
            merge: None,
            skip: Some(SkalaSkip::AlreadyPresent),
        };
    }

    #[cfg(target_arch = "wasm32")]
    {
        SkalaSessionReport {
            policy_applied,
            merge: None,
            skip: Some(SkalaSkip::NoFilesystem),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        merge_from_disk(state, policy_applied)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn merge_from_disk(state: &mut EditorState, policy_applied: bool) -> SkalaSessionReport {
    let candidates = skala_library_candidates();
    let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
    let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
        eprintln!(
            "openpencil: Skala Spectrum library not found ({SKALA_KIT_ID}). \
             Set OPENPENCIL_SKALA_LIB or run from the repo so design/skala-spectrum.lib.op \
             is visible. Tried: {}",
            tried.join(", ")
        );
        return SkalaSessionReport {
            policy_applied,
            merge: None,
            skip: Some(SkalaSkip::MissingLibrary { tried }),
        };
    };
    match merge_library_into_state(state, path.to_str().unwrap_or_default()) {
        Ok(merge) => {
            eprintln!(
                "openpencil: merged Skala Spectrum from {}: +{} master(s), {} component(s)",
                path.display(),
                merge.masters_added,
                merge.component_count
            );
            SkalaSessionReport {
                policy_applied,
                merge: Some(merge),
                skip: None,
            }
        }
        Err(err) => {
            let msg = library_merge_error_string(&err);
            eprintln!(
                "openpencil: failed to merge Skala Spectrum from {}: {msg}",
                path.display()
            );
            SkalaSessionReport {
                policy_applied,
                merge: None,
                skip: Some(SkalaSkip::MergeFailed(msg)),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn library_merge_error_string(err: &LibraryMergeError) -> String {
    err.to_string()
}

/// Paths we will try, in order. Env wins; then cwd; then next to the binary
/// (workspace `target/debug` → repo `design/`).
pub fn skala_library_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("OPENPENCIL_SKALA_LIB") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    // Test binaries live in `target/debug/deps/`. Never merge the repo kit
    // into unit tests (slow + File → New viewport-fit assertions drift).
    if running_under_cargo_test() {
        return dedupe_paths(out);
    }
    out.push(PathBuf::from("design/skala-spectrum.lib.op"));
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join("design/skala-spectrum.lib.op"));
    }
    if let Ok(exe) = std::env::current_exe() {
        // target/debug/openpencil-desktop → repo/design
        if let Some(repo) = exe.parent().and_then(Path::parent).and_then(Path::parent) {
            out.push(repo.join("design/skala-spectrum.lib.op"));
        }
    }
    dedupe_paths(out)
}

fn dedupe_paths(out: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in out {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn running_under_cargo_test() -> bool {
    std::env::current_exe()
        .map(|exe| {
            exe.components()
                .any(|component| component.as_os_str() == "deps")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_is_applied_on_an_empty_document() {
        let mut state = EditorState::new();
        let report = ensure_skala_session(&mut state);
        assert!(state.doc.design_md.is_some());
        assert!(report.policy_applied);
        // Unit tests do not walk to the repo kit (see candidates()); a
        // missing file is the expected skip unless the caller set env.
        assert!(matches!(
            report.skip,
            Some(SkalaSkip::MissingLibrary { .. })
                | Some(SkalaSkip::AlreadyPresent)
                | Some(SkalaSkip::NoFilesystem)
                | None
        ));
    }

    #[test]
    fn new_skala_editor_state_keeps_the_blank_starter_frame() {
        let state = new_skala_editor_state();
        assert_eq!(state.doc.children.len(), 1);
        assert!(state.doc.design_md.is_some());
        assert!(state.selection.is_empty());
    }
}
