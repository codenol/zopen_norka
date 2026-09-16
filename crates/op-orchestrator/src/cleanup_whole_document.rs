//! The whole-document cleanup passes — the ones that run ONCE over the finished
//! document after every per-root pass, and whose order is load-bearing (nav
//! anchoring before the trailing-nav reflow, root deconfliction before the
//! shared-chrome unification, routes before the interaction backfill).
//!
//! Carved off `cleanup.rs` to keep that driver under the 800-line cap. The seam
//! is scope, not length: everything here scans `sink.state()` as a whole rather
//! than one `root_ids` entry, which is exactly why the per-root loop above
//! cannot contain it.

use super::*;

/// Finish the document: chrome shape first, then the cross-screen passes, then
/// the routing/interaction wiring. Called after the per-root loop.
pub(super) fn run_whole_document_passes(
    sink: &mut dyn DocSink,
    counter: &mut RepairCounter,
    summary: &mut RepairSummary,
) {
    crate::avatar_repair::repair_avatar_slots_for_all_roots(sink);

    // LAST: structural chrome contract — bottom nav is the mobile root's
    // final child. Runs AFTER the per-root passes (incl. bottom-nav dedup,
    // which keeps the bottom-most duplicate: anchoring first would reorder
    // duplicates and flip which one dedup keeps). A late "catch-up" section
    // appended after the nav is repaired by moving the nav back to the end;
    // where that section belongs is intent — the geometry echo handles it.
    anchor_bottom_nav_last_for_all_roots(sink);
    counter.checkpoint(summary, CheckCategory::Structure, "avatar+nav-anchor");
    crate::mobile_reflow::repair_mobile_trailing_nav_reflow_in_sink(sink);
    counter.checkpoint(summary, CheckCategory::Layout, "mobile-trailing-nav-reflow");

    // Multi-screen root position deconfliction: screen-shaped top-level
    // roots that overlap because one or more never got a canvas position
    // (loop path model didn't call `find_empty_space`, or any other
    // producer skipped positioning) get spread into a left-to-right row.
    // Runs BEFORE `unify_shared_nav` below — a document whose screens are
    // still stacked on top of each other at diagnosis time reads (from a
    // canvas screenshot) like duplicate nav bars crammed into one frame,
    // but the roots are already correctly separated; they just need to
    // stop overlapping before the passes below reason about "which screen
    // owns which nav".
    crate::spread_screen_roots::spread_overlapping_screen_roots(sink);

    // Cross-screen shared-chrome unification: each screen's independently
    // re-generated bottom-nav drifts in icons/labels (measured: Home screen
    // "Home/Search/Library/Premium" vs Library screen's own redraw
    // "Home/Search/Your Library/Premium"). Runs BEFORE `wire_screen_
    // navigation` below so Track A's label↔screen tab-matching sees the
    // POST-unification tree (every screen sharing one tab-label set), not a
    // stale per-screen one.
    crate::unify_shared_nav::unify_shared_nav(sink);

    // Sibling pass to the above: screens missing the pinned status bar
    // entirely (measured: 0718-1-k3-1 — two of three screens had no
    // status-bar subtree at all) get one cloned in from whichever screen
    // already has it. Same "reuse, don't redraw" shape, same shared choke
    // point, so both the classic and loop-finalize paths pick it up.
    crate::unify_shared_status_bar::unify_shared_status_bar(sink);

    // Finalize-time enforcement of the OS status-bar contract: every mobile
    // screen root must carry exactly one canonical status bar (role="status-bar"
    // with Levels child) as its first child. Runs after unify_shared_status_bar
    // to enforce the contract even when root-seeding was escaped (model-built
    // bar, fit_content root, or later batches). Three cases: missing → insert,
    // non-canonical → replace, canonical → untouched.
    finalize_enforce_status_bar::finalize_enforce_status_bar_contract(sink);

    // Establish final screen routes first. The cleanup-only semantic pass can
    // then persist only fact-proven back/card interactions against those real
    // routes, before the label-matching nav fallback. Keeping the semantic pass
    // outside public `wire_screen_navigation` prevents Cmd+P's cloned-state
    // fallback from creating preview-only interactions that never reach the
    // saved document.
    crate::wire_screen_navigation::ensure_screen_routes(sink);
    crate::geometry_validation::wire_interaction_backfill(sink);

    // Track A fallback: wire bottom-nav/sidebar tabs after final chrome shape
    // and semantic interactions are settled. Whole-doc (scans `sink.state()`,
    // not `root_ids`) so it also links pre-existing screens from earlier turns.
    crate::wire_screen_navigation::wire_screen_navigation(sink);
    counter.checkpoint(summary, CheckCategory::Structure, "shared-chrome+nav");
}
