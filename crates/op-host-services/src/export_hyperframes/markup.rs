//! The composition page and its companion notes.
//!
//! Split from the exporter next door so that module stays about the
//! timeline (what holds for how long) and this one about the file (what
//! that timeline is spelled as). Everything is inline: the composition
//! is handed to a renderer that opens it in a sandboxed browser, and a
//! stylesheet it would have to fetch is a frame it would have to
//! capture without.

use op_util::xml_escape::escape_html;
use std::fmt::Write as _;

use super::{css_num, fit, seconds, Scene, FADE_TICKS, FPS};

/// Build the whole composition page.
///
/// `canvas_w` / `canvas_h` are the video frame size — the first visible
/// board's size, which for a deck is every board's size. Boards that
/// differ are fitted into it, letterboxed against the root's black.
///
/// # What the renderer requires, and why each piece is here
///
/// Three attributes are not decoration — each one is a lint error or a
/// wasted render without it, and all three were confirmed against
/// `hyperframes lint` rather than assumed:
///
/// - **`class="clip"` on every timed element.** The runtime keys its
///   show/hide off that class, NOT off `data-start` alone. A timed
///   element without it stays on screen for the whole composition, so
///   the deck would render as every slide stacked on the last one.
/// - **`data-no-timeline` on the root.** The renderer otherwise polls
///   for a `window.__timelines` registration for 45 seconds before
///   giving up — a 45 s tax on every render for a composition that has
///   no scripted timeline to register. Ours has none by construction:
///   the animation is CSS, which the frame clock drives directly.
/// - **`data-fps`.** Pinned rather than left to the CLI default, so the
///   frame grid the renderer walks is a property of the artifact and a
///   later default change cannot silently resample the deck.
///
/// Scene ids are emitted for the same reason a slide has a name: they
/// are the stable handle the renderer's studio and its agent tooling
/// address a scene by.
pub fn render_composition(
    title: &str,
    composition_id: &str,
    canvas_w: f32,
    canvas_h: f32,
    scenes: &[Scene],
) -> String {
    let mut tracks = String::new();
    for (i, scene) in scenes.iter().enumerate() {
        let (scale, left, top) = fit(scene.width, scene.height, canvas_w, canvas_h);
        let start = seconds(scene.start_ticks);
        let _ = write!(
            tracks,
            "\n<div class=\"clip hf-scene\" id=\"scene-{n}\" role=\"group\" \
             aria-label=\"{label}\" \
             data-start=\"{start}\" data-duration=\"{duration}\" data-track-index=\"0\" \
             style=\"--hf-start:{start}s;animation-delay:{start}s;\
             animation-duration:{duration}s\">\
             <div class=\"hf-slide\" style=\"left:{left}px;top:{top}px;\
             width:{w}px;height:{h}px;transform:scale({scale})\">{body}</div></div>",
            n = i + 1,
            label = escape_html(&scene.name),
            duration = seconds(scene.duration_ticks),
            left = css_num(left),
            top = css_num(top),
            w = css_num(scene.width),
            h = css_num(scene.height),
            scale = css_num(scale),
            body = scene.body,
        );
    }
    let total = seconds(scenes.iter().map(|scene| scene.duration_ticks).sum());
    let width = css_num(canvas_w);
    let height = css_num(canvas_h);
    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <title>{title}</title>\n\
         <style>{css}</style>\n\
         </head>\n\
         <body>\n\
         <div id=\"root\" data-composition-id=\"{id}\" data-no-timeline data-start=\"0\" \
         data-duration=\"{total}\" data-fps=\"{FPS}\" \
         data-width=\"{width}\" data-height=\"{height}\" \
         style=\"width:{width}px;height:{height}px\">{tracks}\n</div>\n\
         </body>\n\
         </html>\n",
        title = escape_html(title),
        id = escape_html(composition_id),
        css = composition_css(),
    )
}

/// The composition's stylesheet.
///
/// Two shared `@keyframes` carry the whole timeline: every scene uses
/// the same pair and differs only in the inline `animation-delay` /
/// `animation-duration` the emitter wrote. Per-scene keyframes would
/// grow the file with one rule per slide and say nothing extra.
///
/// `hf-hold` animates `visibility` from visible to visible, which reads
/// as a no-op and is the point: paired with `animation-fill-mode:none`
/// (never `both`), the scene takes the animated value only INSIDE its
/// window and falls back to the hidden base rule on either side. That
/// is what makes each scene's own attributes the single source of when
/// it is on screen — the failure mode this replaces is the middle
/// scenes of a composition all staying stacked on top of each other.
///
/// The entrance fade deliberately does NOT touch the slide box. Fading
/// the box in from zero was the first attempt, and rendering it showed
/// what that actually means: at every cut the frame is the root's
/// black, because the incoming slide's own background is inside the
/// thing being faded. So the box hard-cuts and only its CONTENTS
/// animate — `.hf-slide > .n > *` is the board's children, i.e.
/// everything except the board's own fill. The cut lands on a fully
/// painted slide, and the content arrives over the next 0.3 s.
///
/// The per-scene delay reaches those children through the inherited
/// `--hf-start` custom property, because animation longhands do not
/// inherit and the children are written by the shared slide emitter,
/// which knows nothing about timelines and must not have to.
fn composition_css() -> String {
    format!(
        "html,body{{margin:0;background:#000}}\
         #root{{position:relative;overflow:hidden;background:#000}}\
         .hf-scene{{position:absolute;left:0;top:0;width:100%;height:100%;\
         visibility:hidden;animation-name:hf-hold;animation-timing-function:linear;\
         animation-fill-mode:none}}\
         .hf-slide{{position:absolute;transform-origin:0 0;overflow:hidden;background:#fff}}\
         .hf-slide .n{{position:absolute;box-sizing:border-box}}\
         .hf-slide .k{{pointer-events:none}}\
         .hf-slide > .n > *{{animation-name:hf-enter;animation-duration:{fade}s;\
         animation-timing-function:ease-out;animation-fill-mode:none;\
         animation-delay:var(--hf-start,0s)}}\
         @keyframes hf-hold{{from{{visibility:visible}}to{{visibility:visible}}}}\
         @keyframes hf-enter{{from{{opacity:0}}to{{opacity:1}}}}",
        fade = seconds(FADE_TICKS),
    )
}

/// The `RENDER.md` written beside the composition.
///
/// Deliberately two commands and the facts needed to sanity-check the
/// result: the renderer is a Node tool this host does not bundle, so
/// what ships is the instruction, not the MP4.
pub fn render_notes(title: &str, total_ticks: u32, scenes: usize) -> String {
    format!(
        "# Render `{title}`\n\
         \n\
         {scenes} scene(s), {total}s total, {FPS} fps. `index.html` is a Hyperframes\n\
         composition: plain HTML with `data-start` / `data-duration` on each scene and\n\
         no external resource of any kind, so it renders offline and frame for frame\n\
         the same on every run.\n\
         \n\
         Run both commands from THIS directory — the renderer takes a project\n\
         directory and reads `index.html` out of it, not an HTML path.\n\
         \n\
         ```sh\n\
         npx hyperframes render . --output deck.mp4\n\
         npx hyperframes preview .\n\
         ```\n\
         \n\
         Needs Node 22 or newer. Edit the deck in Norka and export again — the\n\
         composition is generated, not authored, so edits made to `index.html` are\n\
         lost on the next export.\n",
        total = seconds(total_ticks),
    )
}
