//! The staged chat attachments of ONE turn — drained once, handed to every
//! route that turn might take.
//!
//! Why this module exists (issue #64): each request builder used to take
//! `ChatState::pending_attachments` for itself. The design builder ran first,
//! so the chat and modify builders read an already-empty list and the turn
//! answered as if nothing had been attached — the picture was gone with no
//! message. The attachments belong to the *turn*, not to one of the routes the
//! turn may take, so the turn drains them exactly once (`launch_if_pending`)
//! and every route's request is built from that same value. No route may take
//! them again: a second take is precisely the reported defect.
//!
//! The split of responsibilities here is deliberate: this module owns *which
//! bytes go to which request*, and the transport owns *whether it can carry
//! them* (`op_host_services::chat_attachment::prompt_with_*`, which names
//! every file the model cannot see — issue #61).

use op_ai::chat_provider::{ChatHistoryRole, ChatRequest, EffortLevel, ThinkingMode};
use op_editor_core::chat::ChatAttachment;
use op_editor_core::EditorState;
use op_host_services::chat_intent::ModifyPlan;
use op_orchestrator::ReferenceAttachment;

/// One turn's staged attachments, owned by the launch path for the whole turn.
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnAttachments {
    staged: Vec<ChatAttachment>,
}

impl TurnAttachments {
    /// Take the turn's staged attachments. This is the launch path's ONLY
    /// drain of `ChatState::pending_attachments`; callers pass the result down
    /// to every route builder instead of taking again.
    pub(crate) fn drain(state: &mut EditorState) -> Self {
        Self {
            staged: std::mem::take(&mut state.chat.pending_attachments),
        }
    }

    /// True when the turn carries nothing the requests must know about.
    pub(crate) fn is_empty(&self) -> bool {
        self.staged.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.staged.len()
    }

    /// The chat / modify channel: every staged file, images and documents
    /// alike. Each transport decides per file how to hand it over (inline image
    /// block, temp-file path, guided Read) and says so in the prompt when it
    /// cannot, so a file that travels this channel is either delivered or
    /// explicitly reported — never silently assumed.
    pub(crate) fn for_chat(&self) -> Vec<ChatAttachment> {
        self.staged.clone()
    }

    /// The design route's reference channel. `DesignRequest::
    /// reference_attachments` is an image type (the multimodal brief pass reads
    /// pictures out of it), so only images can travel here; whatever stays
    /// behind is named by [`Self::design_omission_note`].
    pub(crate) fn for_design(&self) -> Vec<ReferenceAttachment> {
        self.staged
            .iter()
            .filter(|att| att.is_image())
            .map(|att| ReferenceAttachment {
                name: att.name.clone(),
                media_type: att.media_type.clone(),
                data: att.data.clone(),
            })
            .collect()
    }

    /// Names of staged files the design route cannot carry.
    fn design_omissions(&self) -> Vec<&str> {
        self.staged
            .iter()
            .filter(|att| !att.is_image())
            .map(|att| att.name.as_str())
            .collect()
    }

    /// Model-facing line for a design prompt whose turn carried files the
    /// route cannot take. Without it the planner is free to describe a
    /// document it never received — the fabricated-inventory failure that
    /// issue #61 was about.
    pub(crate) fn design_omission_note(&self) -> String {
        let names = self.design_omissions();
        if names.is_empty() {
            return String::new();
        }
        format!(
            "\n\n[attachment NOT carried into this design turn: {} — the design route \
             takes images only, its contents are unavailable to you, do not describe it.]",
            names.join(", ")
        )
    }

    /// Transcript-facing line for a design turn that is about to run without
    /// some of the staged files, so the person is told instead of watching the
    /// attachment disappear. `None` when there is nothing to report.
    pub(crate) fn design_omission_chat_note(&self) -> Option<String> {
        let names = self.design_omissions();
        (!names.is_empty()).then(|| {
            format!(
                "\n\n[not sent: {} — the design route carries images only]",
                names.join(", ")
            )
        })
    }

    /// Transcript-facing line for a turn that never launched at all. The
    /// attachments are already out of `pending_attachments` by then, so the
    /// turn's own error is the last place able to say they did not go
    /// anywhere. Empty when the turn carried none.
    pub(crate) fn unsent_note(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        format!(
            "\n\n{} attachment(s) were not sent — no turn was started.",
            self.len()
        )
    }
}

/// The plain-chat request of a turn.
///
/// Both chat entry points go through here — the CLI standard-mode turn, whose
/// route the classifier only decides later on the worker, and the fallback
/// chat path. The attachments ride along because a chat turn is one of the
/// outcomes either request exists for; a request built without them can only
/// produce an answer about a picture the model never received (issue #64).
#[allow(clippy::too_many_arguments)]
pub(crate) fn plain_chat_request(
    system_prompt: String,
    user_message: String,
    history: Vec<(ChatHistoryRole, String)>,
    thinking: ThinkingMode,
    effort: EffortLevel,
    model: Option<String>,
    attachments: &TurnAttachments,
) -> ChatRequest {
    ChatRequest {
        system_prompt,
        user_message,
        history,
        max_output_tokens: 4096,
        thinking,
        effort,
        attachments: attachments.for_chat(),
        model,
    }
}

/// The `generateDesignModification` request of a turn — the selection-scoped
/// edit route, reached both directly (builtin / ACP selection) and through the
/// CLI standard-mode plan.
///
/// TS sends this single-shot request with no attachments at all. The desktop
/// does not copy that part of it: an attached reference is exactly what "change
/// this the way the picture shows" is asking about, and the transport reports
/// any file it cannot carry rather than letting the picture vanish (issue #64).
pub(crate) fn modify_request(
    plan: ModifyPlan,
    model: Option<String>,
    thinking: ThinkingMode,
    attachments: &TurnAttachments,
) -> ChatRequest {
    ChatRequest {
        system_prompt: plan.system_prompt,
        user_message: plan.user_message,
        max_output_tokens: 8192,
        model,
        thinking,
        attachments: attachments.for_chat(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::chat::MAX_ATTACHMENTS;

    fn image(name: &str) -> ChatAttachment {
        ChatAttachment {
            name: name.into(),
            media_type: "image/png".into(),
            // A real PNG signature: the built-in HTTP transport picks images
            // for inline delivery by sniffing bytes, not by trusting the
            // media-type label.
            data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3],
        }
    }

    fn document(name: &str) -> ChatAttachment {
        ChatAttachment {
            name: name.into(),
            media_type: "application/pdf".into(),
            data: vec![b'%', b'P', b'D', b'F'],
        }
    }

    fn staged(items: Vec<ChatAttachment>) -> TurnAttachments {
        let mut state = EditorState::new();
        for item in items {
            assert!(
                state.chat.add_attachment(item),
                "fixture must fit the per-turn attachment cap of {MAX_ATTACHMENTS}"
            );
        }
        TurnAttachments::drain(&mut state)
    }

    #[test]
    fn drain_takes_the_staged_attachments_and_leaves_the_state_empty() {
        let mut state = EditorState::new();
        assert!(state.chat.add_attachment(image("shot.png")));

        let attachments = TurnAttachments::drain(&mut state);

        assert_eq!(
            attachments.len(),
            1,
            "the turn must own what the user staged"
        );
        assert!(
            state.chat.pending_attachments.is_empty(),
            "a second take would find nothing — which is exactly how the image \
             used to vanish, so the drain must be the last word on the state"
        );
    }

    #[test]
    fn images_travel_both_channels_documents_only_the_chat_one() {
        let attachments = staged(vec![image("shot.png"), document("brief.pdf")]);

        let chat = attachments.for_chat();
        assert_eq!(chat.len(), 2, "the chat/modify channel carries every file");

        let design = attachments.for_design();
        assert_eq!(design.len(), 1, "the design channel is an image type");
        assert_eq!(design[0].name, "shot.png");
        assert_eq!(design[0].data, chat[0].data);
    }

    #[test]
    fn a_document_left_behind_is_named_for_the_model_and_the_person() {
        let attachments = staged(vec![image("shot.png"), document("brief.pdf")]);

        let note = attachments.design_omission_note();
        assert!(note.contains("brief.pdf"), "{note}");
        assert!(
            !note.contains("shot.png"),
            "an image does travel, so it must not be reported as missing: {note}"
        );
        let chat_note = attachments
            .design_omission_chat_note()
            .expect("the person is told too");
        assert!(chat_note.contains("brief.pdf"), "{chat_note}");
    }

    #[test]
    fn an_all_image_turn_has_nothing_to_report() {
        let attachments = staged(vec![image("shot.png")]);

        // Nothing is left behind by either design channel, so neither the
        // planner nor the transcript carries an omission line. The
        // launch-failure note is a different situation (it reports a turn that
        // never started at all) and is covered by its own test.
        assert_eq!(attachments.design_omission_note(), "");
        assert_eq!(attachments.design_omission_chat_note(), None);
    }

    #[test]
    fn an_unlaunched_turn_says_its_attachments_went_nowhere() {
        let attachments = staged(vec![image("shot.png"), image("second.png")]);

        assert!(
            attachments.unsent_note().contains('2'),
            "the count is what tells the person how much was lost: {}",
            attachments.unsent_note()
        );
    }
}
