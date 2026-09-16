//! The CI contracts this crate depends on, one fragment per workflow family.
//!
//! A workflow is what makes a claim about this crate true or false in CI, and
//! this file is the contract that the workflows keep saying what they said: no
//! retired build path, no unsigned artifact, no lane that publishes where it
//! must not. It is an integration test (one binary, no crate API), so it can
//! only assert on the files themselves.
//!
//! ## How this file is split
//!
//! Five `include!` fragments, one per family of workflows, rather than sibling
//! test files or `mod` siblings: `cargo test -- --list` names a test after the
//! module path it is defined in, and either of those would rename every test
//! here (issue #220). An `include!` fragment is spliced in at its include site,
//! which keeps the declaration order — and therefore the whole test list —
//! byte-identical. The families:
//! `ci_workflow/rust_check.rs` is the pull-request check lane,
//! `release_lane.rs` the tag release and the Windows installers it ships,
//! `web_runner.rs` the browser smoke and the web publish entrypoints,
//! `mobile_lanes.rs` the iOS and Android distribution lanes, and
//! `deploy_lane.rs` the web-deploy lane with its helpers.

// Each include is a whole test item, so the blank line between two of them is
// the blank line that separated them in the one file this used to be — which is
// what makes the fragments reassemble it byte for byte. A fragment is formatted
// (rustfmt over it is a no-op); it is just not part of the module tree
// `cargo fmt` walks.
include!("ci_workflow/rust_check.rs");

include!("ci_workflow/release_lane.rs");

include!("ci_workflow/web_runner.rs");

include!("ci_workflow/mobile_lanes.rs");

include!("ci_workflow/deploy_lane.rs");
