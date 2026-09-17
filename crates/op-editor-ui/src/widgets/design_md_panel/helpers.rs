//! The panel's one shared helper.
//!
//! `hex_to_color` used to live here too — a private hex → `Color` wrapper over
//! `op_editor_core::parse_hex_rgb`, kept from before `op-util` became the
//! workspace's single source for hex parsing. Nothing in the panel ever called
//! it, which is how a second copy of the same rule sat next to the real parser.
//!
//! The `tick_layout_call` / `layout_call_count` pair went the same way: it
//! counted `DesignMdPanel::layout` calls so a test could prove one
//! paint/hit-test pass resolved the layout once instead of up to three times.
//! The cache it guarded (`design_md_line_cache`, added in `11ae71fe`) no longer
//! exists, the counter was never wired into `layout`, and no test read it — so
//! it measured nothing and asserted nothing.

pub(super) use crate::util::truncate_ellipsis as truncate;
