//! What the panel says when collaboration is not available here (issue #150).
//!
//! `CollabPanelScreen::Unavailable` covers two causes and the panel cannot tell
//! them apart: a build that links no collaboration runtime, and a deployment
//! whose account tier answers nobody (a `--serve-web` daemon, #148). The
//! sentence used to name the first one — "unavailable in this build" — and was
//! therefore false half the time. These tests pin the other resolution: one
//! cause-neutral sentence, painted for both causes, naming neither.

use super::*;
use crate::widgets::collab_ui::COLLAB_UNAVAILABLE_SENTENCE;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::{CollabAvailability, Locale};

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, 1_000.0, 800.0)
}

/// The panel's painted lines, in paint order, for one cause.
///
/// The cause is expressed the way a host expresses it: `account_ui_available`
/// is what `/api/auth/status` (web) or the linked auth artifact (native) says
/// about whether ANYBODY can sign in here, and it is the one fact that differs
/// between the two states a person can be looking at.
fn painted_lines(account_ui_available: bool) -> (CollabPanelScreen, Vec<String>) {
    let mut ui = EditorUiState {
        locale: Locale::EnUs,
        account_ui_available,
        ..Default::default()
    };
    // The runtime's own verdict, whichever reason produced it: this is the only
    // thing the panel is told.
    ui.collab.availability = CollabAvailability::Unavailable;
    ui.collab.panel.open = true;

    let panel = CollabPanel::for_editor_ui(&ui).expect("the panel is always constructible");
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    (
        panel.model.screen.clone(),
        backend.texts.into_iter().map(|(text, _)| text).collect(),
    )
}

/// The sentence inside the painted lines, with the panel's title dropped.
fn painted_sentence(lines: &[String]) -> String {
    assert_eq!(
        lines.first().map(String::as_str),
        Some("Collaboration"),
        "the title, then the body: {lines:?}"
    );
    lines[1..].join(" ")
}

#[test]
fn both_causes_get_the_same_neutral_sentence() {
    // The BUILD cause: accounts exist and are signable here, so what is missing
    // is the collaboration runtime the build did not link. A build without the
    // ticket ABI publishes exactly this chrome state.
    let (build_screen, build_lines) = painted_lines(true);
    // The DEPLOYMENT cause: nobody can sign in here — `/api/auth/status` says
    // `available: false`, every login route is a 404 (#148) — so the runtime is
    // present and no session can ever be admitted.
    let (deployment_screen, deployment_lines) = painted_lines(false);

    assert_eq!(build_screen, CollabPanelScreen::Unavailable);
    assert_eq!(deployment_screen, CollabPanelScreen::Unavailable);

    let build = painted_sentence(&build_lines);
    let deployment = painted_sentence(&deployment_lines);
    assert_eq!(
        build, deployment,
        "one screen, one sentence: the panel cannot tell these two apart, so a \
         sentence that differed between them would be inventing a cause"
    );
    assert_eq!(
        build,
        op_i18n::translate(Locale::EnUs, COLLAB_UNAVAILABLE_SENTENCE),
        "and it is the catalogued sentence, not a literal"
    );
}

#[test]
fn the_sentence_names_no_cause() {
    let sentence = op_i18n::translate(Locale::EnUs, COLLAB_UNAVAILABLE_SENTENCE);
    let lowered = sentence.to_lowercase();
    for blamed in [
        "build",
        "version",
        "compil",
        "deployment",
        "accounts",
        "sign in",
    ] {
        assert!(
            !lowered.contains(blamed),
            "`{sentence}` still points at a cause; the panel knows none (#150)"
        );
    }
    assert!(
        !sentence.contains('…'),
        "an ellipsized reason is a half-sentence: {sentence:?}"
    );
}
