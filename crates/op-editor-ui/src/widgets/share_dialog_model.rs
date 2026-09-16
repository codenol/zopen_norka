//! The Share dialog's strings and rows, computed once per frame.
//!
//! ## Why the model is separate from the paint
//!
//! Because every sentence in this dialog is a claim about access, and a claim
//! is testable while a paint call is not. "Who added this person", "can this
//! account invite at all", "why is the button inert" and "did anything get
//! sent" are all answered here, from state, with no backend; the paint routine
//! then only decides where to put them.

use op_editor_core::editor_ui_state::share::{
    ShareInvitePlan, ShareLevelTarget, ShareNotice, ShareUiState,
};
use op_editor_core::{EditorState, ShareInviteRefusal, ShareLevel};
use op_i18n::Locale;

use crate::widgets::share_dialog_layout::{
    LevelAnchor, LevelMenuSpec, ShareLayoutSpec, MAX_INVITE_LINK_ROWS, MAX_PERSON_ROWS,
};

/// How many invitation links a state is holding.
fn model_issued_links(ui: &ShareUiState) -> usize {
    match ui.notice.as_ref() {
        Some(ShareNotice::Issued(invites)) => invites.len(),
        _ => 0,
    }
}

/// One person's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharePersonModel {
    /// Index into the state's list — what a press on the row acts on.
    pub index: usize,
    pub account: String,
    pub label: String,
    pub level: ShareLevel,
    pub level_label: String,
    pub attribution: String,
}

/// Everything the dialog says, decided once.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareDialogModel {
    pub title: String,
    pub copy_link: String,
    pub invite_label: String,
    pub invite_placeholder: String,
    pub invite_value: String,
    pub invite_button: String,
    pub invite_level_label: String,
    /// What the level currently chosen for the next invitation means.
    pub invite_level_hint: String,
    /// The caption on an issued invitation's copy control.
    pub copy_invite_label: String,
    /// Whether pressing Invite could do anything at all.
    pub invite_enabled: bool,
    /// Why it could not, when the field already says enough to know.
    pub invite_blocked: Option<String>,
    /// The no-mail sentence, shown under the field. Always painted: it is the
    /// whole answer to "what does Invite send", and every person asks it once.
    pub no_mail_hint: String,
    pub notice: Option<String>,
    pub notice_is_refusal: bool,
    /// The invitation links the last press produced: address, then path.
    pub issued_links: Vec<(String, String)>,
    pub section: String,
    pub you_label: String,
    pub you_caption: String,
    pub link_label: String,
    pub link_caption: String,
    pub link_enabled: bool,
    pub link_level_label: String,
    /// Whether the link's level lets everybody who signs in change the
    /// document. Paint draws the caption at full contrast when it does: a
    /// sentence is easy to skim past, and this is the one state in the dialog
    /// that publishes writing to the whole deployment at once.
    pub link_level_writes: bool,
    pub people: Vec<SharePersonModel>,
    pub overflow_text: Option<String>,
    pub footer: String,
    /// The levels the open picker offers, with their label and hint.
    pub level_options: Vec<(ShareLevel, String, String)>,
    /// What the layout needs in order to place all of the above.
    pub spec: ShareLayoutSpec,
    /// What an Invite press would do right now, or why it would refuse.
    pub plan: Result<ShareInvitePlan, ShareInviteRefusal>,
}

impl ShareDialogModel {
    /// Build from the editor's share state.
    pub fn for_state(state: &EditorState) -> Self {
        let ui = &state.editor_ui.share;
        let locale = state.editor_ui.effective_locale();
        let t = |key: &'static str| op_i18n::translate(locale, key).to_string();

        // The plan is computed FIRST: the button's enabled state, the notice
        // and the host's request builder all read it, and computing it three
        // times is three places for them to disagree.
        let plan = ui.plan_invite();

        let people: Vec<SharePersonModel> = ui
            .people()
            .iter()
            .take(MAX_PERSON_ROWS)
            .enumerate()
            .map(|(index, grant)| SharePersonModel {
                index,
                account: grant.account.clone(),
                label: ui.person_label(locale, grant),
                level: grant.level,
                level_label: t(grant.level.i18n_key()),
                attribution: ui.attribution_label(locale, grant),
            })
            .collect();
        let hidden = ui.people().len().saturating_sub(people.len());
        let rows = people.len();

        // The reason a press would refuse, shown before the press only once
        // the field says something: an "enter an address" complaint under an
        // untouched field is noise, while "you may not grant that level" is
        // worth reading early.
        let blocked = (!ui.invite_entries().is_empty())
            .then(|| {
                plan.as_ref()
                    .err()
                    .map(|refusal| refusal_text(locale, refusal))
            })
            .flatten();

        // A picker offers only what this account may hand out; the caller's own
        // level is the ceiling. Same set for every control, because the ceiling
        // is a property of the person, not of the row.
        let grantable = ui.grantable_levels();
        let selected = match ui.level_picker {
            Some(ShareLevelTarget::Invite) => ui.invite_level,
            Some(ShareLevelTarget::Link) => ui.link_level,
            Some(ShareLevelTarget::Person(index)) => ui
                .people()
                .get(index)
                .map(|grant| grant.level)
                .unwrap_or(ui.invite_level),
            None => ui.invite_level,
        };
        let menu = ui.level_picker.map(|target| LevelMenuSpec {
            options: grantable.len(),
            selected: grantable
                .iter()
                .position(|level| *level == selected)
                .unwrap_or(0),
            anchor: match target {
                ShareLevelTarget::Invite => LevelAnchor::Invite,
                ShareLevelTarget::Link => LevelAnchor::Link,
                ShareLevelTarget::Person(index) => LevelAnchor::Person(index),
            },
        });

        Self {
            title: t("share.title"),
            copy_link: t("share.action.copyLink"),
            invite_label: t("share.invite.label"),
            invite_placeholder: t("share.invite.placeholder"),
            invite_value: ui.invite_input.effective_text(),
            invite_button: t("share.action.invite"),
            invite_level_label: t(ui.invite_level.i18n_key()),
            invite_level_hint: t(ui.invite_level.hint_i18n_key()),
            copy_invite_label: t("share.action.copyInviteLink"),
            // The button is live exactly when a press would be honoured — the
            // same predicate the press consults, so the two cannot disagree.
            invite_enabled: plan.is_ok(),
            invite_blocked: blocked,
            no_mail_hint: t("share.invite.noMail"),
            notice: ui.notice.as_ref().map(|notice| notice_text(locale, notice)),
            notice_is_refusal: ui.notice.as_ref().is_some_and(ShareNotice::is_refusal),
            issued_links: match ui.notice.as_ref() {
                Some(ShareNotice::Issued(invites)) => invites
                    .iter()
                    .map(|invite| (invite.email.clone(), invite.path.clone()))
                    .collect(),
                _ => Vec::new(),
            },
            section: t("share.section.whoHasAccess"),
            you_label: t("share.row.you"),
            you_caption: if ui.is_owner {
                t("share.row.owner")
            } else {
                // What the owner's list gives the caller, when that is known.
                // Without it a guest was drawn as view-only whatever the grant
                // said (#121).
                t(ui.granted_level
                    .unwrap_or_else(|| ShareLevel::from_rights(ui.own_rights))
                    .i18n_key())
            },
            link_label: t("share.row.anyoneWithLink"),
            // The caption follows the level the link hands out, so the row says
            // what "anyone with the link" MEANS here rather than only who may
            // arrive (#131). A row that reads the same at "Can view" and "Can
            // edit" is a row whose widest state a person cannot see.
            link_caption: ui.link_caption(locale),
            link_enabled: ui.link_enabled,
            link_level_label: t(ui.link_level.i18n_key()),
            // Whether the level in force lets everybody who signs in rewrite
            // the document. Paint raises the caption's contrast on it.
            link_level_writes: ui.link_level.changes_the_document(),
            people,
            overflow_text: (hidden > 0).then(|| {
                op_i18n::translate(locale, "share.row.more")
                    .replace("{{count}}", &hidden.to_string())
            }),
            footer: t("share.footer.session"),
            level_options: grantable
                .iter()
                .map(|level| {
                    (
                        level.to_owned(),
                        t(level.i18n_key()),
                        t(level.hint_i18n_key()),
                    )
                })
                .collect(),
            spec: ShareLayoutSpec {
                person_rows: rows,
                overflow: hidden > 0,
                // At most a few links are shown; the rest are already in the
                // notice's count, and a card that grew with a paste of twenty
                // addresses would run off the bottom of the window.
                invite_links: model_issued_links(ui).min(MAX_INVITE_LINK_ROWS),
                level_menu: menu,
            },
            plan,
        }
    }

    /// What the layout needs — for a caller that has only the model.
    pub const fn spec(&self) -> ShareLayoutSpec {
        self.spec
    }
}

/// The sentence for a notice.
fn notice_text(locale: Locale, notice: &ShareNotice) -> String {
    match notice {
        ShareNotice::Refused(refusal) => refusal_text(locale, refusal),
        ShareNotice::Issued(invites) => op_i18n::translate(locale, notice.i18n_key())
            .replace("{{count}}", &invites.len().to_string()),
        ShareNotice::Granted(granted) => op_i18n::translate(locale, notice.i18n_key())
            .replace("{{count}}", &granted.len().to_string()),
        ShareNotice::LinkAccess { level, enabled } => op_i18n::translate(locale, notice.i18n_key())
            .replace(
                "{{state}}",
                op_i18n::translate(
                    locale,
                    if *enabled {
                        "share.link.on"
                    } else {
                        "share.link.off"
                    },
                ),
            )
            .replace("{{level}}", op_i18n::translate(locale, level.i18n_key())),
        ShareNotice::CopiedLink | ShareNotice::ListUnavailable | ShareNotice::NoDocumentLink => {
            op_i18n::translate(locale, notice.i18n_key()).to_string()
        }
    }
}

/// The sentence for a refusal, with its placeholders filled.
///
/// One function so the button's caption, the notice band and any host-side
/// message word the same refusal the same way.
pub fn refusal_text(locale: Locale, refusal: &ShareInviteRefusal) -> String {
    use ShareInviteRefusal as R;
    let raw = op_i18n::translate(locale, refusal.i18n_key());
    match refusal {
        R::TooManyEntries { count } => raw.replace("{{count}}", &count.to_string()),
        R::LevelAboveOwn { level, own } => raw
            .replace("{{level}}", op_i18n::translate(locale, level.i18n_key()))
            .replace("{{own}}", op_i18n::translate(locale, own.i18n_key())),
        R::AlreadyOnList { account } => raw.replace("{{account}}", account),
        // The two refusals a person actually meets in this dialog (#146): the
        // entry that named nobody, and the list that is full. Both sentences
        // need the figure the refusal is about — the spelling to check, the
        // number of accounts already on the list — so both are filled here
        // rather than left showing a placeholder.
        R::UnknownAccount { account } => raw.replace("{{account}}", account),
        R::ShareLimitReached { limit } => raw.replace("{{limit}}", &limit.to_string()),
        // The daemon's own refusal: the sentence is generic on purpose, and the
        // code travels in the variant for a log line rather than the card.
        R::EmptyField
        | R::NoInviteRight
        | R::NotAnAdministratorForEmail
        | R::RefusedByServer { .. } => raw.to_string(),
    }
}
