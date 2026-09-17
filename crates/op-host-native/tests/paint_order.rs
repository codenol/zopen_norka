//! The native paint pass's band order.
//!
//! `src/widget_host/paint.rs` is the pass's **spine**: it holds the sequence of
//! sub-pass calls, and that sequence *is* the z-order — it is the same order the
//! press ladder mirrors in reverse. Each band's body, however, may live in any
//! sibling of the `widget_host` module, because the pass has been split at the
//! 800-line cap more than once (`paint_panel_arms.rs`, `paint_rail.rs`,
//! `paint_topmost_overlays.rs`, …).
//!
//! So the order lives in the spine and the bodies live wherever a split put
//! them. This file therefore does not carry a list of files to look in — a list
//! is exactly what rots the moment the next split moves a body (issue #224).
//! Instead it **linearises the pass in paint order**: it walks the spine and, at
//! every `self.paint_*(…)` call site, splices in the body of the file that
//! defines that method, recursively. Searching the result for the band markers
//! then answers the only question that matters — in what order do these bands
//! paint — no matter which file each body currently lives in.
//!
//! Two consequences worth knowing before editing `paint.rs`:
//!
//! * Reordering the sub-pass calls in the spine reorders the linearised pass, so
//!   the assertions here fail. That is the point: the call order is behaviour.
//! * Moving a band's body into a file that no `self.paint_*` call reaches makes
//!   its marker unfindable, and the test fails loudly rather than passing
//!   vacuously. See `a_band_no_call_reaches_cannot_be_seen` below.

use std::path::{Path, PathBuf};

/// How deep the spine → sibling → grandchild splice may recurse. The pass is
/// two levels deep today (spine → `paint_panel_arms`); the cap only exists so a
/// pathological module cannot loop.
const MAX_SPLIT_DEPTH: usize = 8;

/// The `src/widget_host/` directory holding the module's spine and siblings.
fn widget_host_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/widget_host")
}

/// The paint pass's spine, located through its `mod paint;` declaration in
/// `src/widget_host.rs` — so even the entry point is not a hardcoded path.
fn paint_spine_path() -> PathBuf {
    let dir = widget_host_dir();
    let module_path = dir
        .parent()
        .expect("src/widget_host has a parent directory")
        .join("widget_host.rs");
    let module = std::fs::read_to_string(&module_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", module_path.display()));
    assert!(
        module.lines().any(|line| line.trim() == "mod paint;"),
        "{} no longer declares `mod paint;` — the paint pass this test guards \
         is either gone or has been renamed",
        module_path.display()
    );
    dir.join("paint.rs")
}

/// Every `.rs` file under `src/widget_host/`, recursively, as
/// `(path, source)`. The pass's bodies may sit in any of them.
fn module_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(source) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), source));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&widget_host_dir(), &mut out);
    assert!(
        !out.is_empty(),
        "no Rust sources under {} — the test cannot see the paint pass",
        widget_host_dir().display()
    );
    out
}

/// The pass linearised in paint order — see the module doc.
fn paint_pass_order() -> String {
    let spine_path = paint_spine_path();
    let spine = std::fs::read_to_string(&spine_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", spine_path.display()));
    expand_paint_pass(&spine, &module_sources())
}

/// Splice every `self.paint_*(…)` call in `spine` with the body of the file
/// that defines that method, recursively, keeping each body at its call site.
fn expand_paint_pass(spine: &str, sources: &[(String, String)]) -> String {
    let mut stack = Vec::new();
    expand_calls(spine, sources, &mut stack, 0)
}

fn expand_calls(
    text: &str,
    sources: &[(String, String)],
    stack: &mut Vec<String>,
    depth: usize,
) -> String {
    if depth >= MAX_SPLIT_DEPTH {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some((call_start, name, call_end)) = next_sub_pass_call(text, cursor) {
        out.push_str(&text[cursor..call_start]);
        match find_definition(sources, &name) {
            // A self-recursive call would expand forever; leave it as text.
            Some((path, body)) if !stack.iter().any(|seen| seen == path) => {
                stack.push(path.clone());
                out.push_str(&expand_calls(body, sources, stack, depth + 1));
                stack.pop();
            }
            _ => out.push_str(&text[call_start..call_end]),
        }
        cursor = call_end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// The next `self.paint_<name>(…)` call at or after `from`, as
/// `(start, name, end)` with `end` just past the closing parenthesis.
fn next_sub_pass_call(text: &str, from: usize) -> Option<(usize, String, usize)> {
    let mut search = from;
    while let Some(offset) = text[search..].find("self.paint_") {
        let start = search + offset;
        let name_start = start + "self.".len();
        let name_end = name_start
            + text[name_start..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(text.len() - name_start);
        if text[name_end..].starts_with('(') {
            if let Some(end) = skip_balanced_parens(text, name_end) {
                return Some((start, text[name_start..name_end].to_owned(), end));
            }
        }
        search = name_end.max(start + 1);
    }
    None
}

/// Index just past the `)` matching the `(` at `open`.
fn skip_balanced_parens(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The file that declares `fn <name>(`, with its source.
fn find_definition<'a>(
    sources: &'a [(String, String)],
    name: &str,
) -> Option<(&'a String, &'a String)> {
    let needle = format!("fn {name}(");
    sources
        .iter()
        .find(|(_, source)| source.contains(&needle))
        .map(|(path, source)| (path, source))
}

/// Where `marker` sits in the linearised pass. A marker that cannot be reached
/// is a hard failure: it means the band moved somewhere the spine's calls do not
/// lead, and silently skipping it would turn this suite into a no-op.
fn band_position(order: &str, marker: &str) -> usize {
    order.find(marker).unwrap_or_else(|| {
        panic!(
            "paint band marker {marker:?} is not reachable in paint order.\n\
             Either the band's body moved into a file that no `self.paint_*` call in \
             `src/widget_host/paint.rs` reaches, or the marker text changed. Give the band a \
             sub-pass call in the spine so its position is expressible, and keep the marker."
        )
    })
}

#[test]
fn canvas_paints_before_property_panel_overlays() {
    let order = paint_pass_order();

    let canvas = band_position(&order, "CanvasViewport — middle band");
    let property = band_position(&order, "PropertyPanel — only when selection");

    assert!(
        canvas < property,
        "PropertyPanel must paint after CanvasViewport so popovers extending into the canvas are \
         not covered (order comes from the sub-pass call sequence in `paint.rs`, each call \
         followed into the file its body lives in)"
    );
}

#[test]
fn image_fill_popover_paints_above_status_bar() {
    let order = paint_pass_order();

    let status = band_position(&order, "StatusBar — floating bottom-right");
    let overlays = band_position(&order, "PropertyPanel overlays");

    assert!(
        status < overlays,
        "image-fill popover must paint after StatusBar so the zoom pill cannot cover adjustment \
         rows (order comes from the sub-pass call sequence in `paint.rs`, each call followed into \
         the file its body lives in)"
    );
}

/// The two tests above each pin one pair. This one states the whole band
/// sequence the numbering in the pass documents — canvas, then the
/// PropertyPanel, then the StatusBar, then the PropertyPanel's own overlays —
/// so a reshuffle that happens to satisfy both pairs still fails here.
#[test]
fn every_band_paints_in_the_documented_z_order() {
    let order = paint_pass_order();

    let bands = [
        "CanvasViewport — middle band",
        "PropertyPanel — only when selection",
        "StatusBar — floating bottom-right",
        "PropertyPanel overlays",
    ];
    let positions: Vec<(&str, usize)> = bands
        .iter()
        .map(|band| (*band, band_position(&order, band)))
        .collect();

    for pair in positions.windows(2) {
        let (earlier, earlier_at) = pair[0];
        let (later, later_at) = pair[1];
        assert!(
            earlier_at < later_at,
            "{later:?} paints at {later_at} but must paint after {earlier:?} at {earlier_at} — \
             the sub-pass call order in `paint.rs` encodes the z-order"
        );
    }
}

/// The resolver's contract, on a synthetic spine: a sub-pass call is replaced by
/// the body of the file that defines it, in place, so a marker inside that body
/// lands exactly where the call sits.
#[test]
fn the_linearised_pass_follows_a_call_into_its_body_file() {
    let sources = vec![(
        "paint_body.rs".to_owned(),
        "pub fn paint_body(&mut self) {\n    // BODY\n}\n".to_owned(),
    )];
    let spine =
        "pub fn paint(&mut self) {\n    // HEAD\n    self.paint_body(frame);\n    // TAIL\n}\n";

    let order = expand_paint_pass(spine, &sources);

    let head = band_position(&order, "// HEAD");
    let body = band_position(&order, "// BODY");
    let tail = band_position(&order, "// TAIL");
    assert!(
        head < body,
        "the body must land after the call's preceding text"
    );
    assert!(
        body < tail,
        "the body must land before the call's following text"
    );
}

/// And it is order-sensitive, which is the guarantee the suite rests on: moving
/// a call in the spine moves the band it paints, and the markers invert.
#[test]
fn moving_a_sub_pass_call_in_the_spine_moves_its_band() {
    let sources = vec![(
        "paint_body.rs".to_owned(),
        "pub fn paint_body(&mut self) {\n    // BODY\n}\n".to_owned(),
    )];
    let before = "pub fn paint(&mut self) {\n    // HEAD\n    self.paint_body(frame);\n}\n";
    let after = "pub fn paint(&mut self) {\n    self.paint_body(frame);\n    // HEAD\n}\n";

    let before = expand_paint_pass(before, &sources);
    let after = expand_paint_pass(after, &sources);

    assert!(before.find("// HEAD").unwrap() < before.find("// BODY").unwrap());
    assert!(
        after.find("// BODY").unwrap() < after.find("// HEAD").unwrap(),
        "a call hoisted above the canvas band must report the band as painting earlier"
    );
}

/// The failure mode the band assertions rely on: a body no call reaches is not
/// in the order, so `band_position` panics instead of quietly passing.
#[test]
fn a_band_no_call_reaches_cannot_be_seen() {
    let sources = vec![(
        "paint_orphan.rs".to_owned(),
        "pub fn paint_orphan(&mut self) {\n    // ORPHAN\n}\n".to_owned(),
    )];
    let spine = "pub fn paint(&mut self) {\n    // HEAD\n}\n";

    let order = expand_paint_pass(spine, &sources);

    assert!(
        !order.contains("// ORPHAN"),
        "an unreachable body must be absent, not guessed at"
    );
}
