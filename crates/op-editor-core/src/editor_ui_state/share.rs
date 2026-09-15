//! Share-dialog state: who has access, at what level, and what one Invite press
//! would do.
//!
//! ## Why the planning happens here and not in the widget
//!
//! The dialog's Invite button has one job with three possible answers: issue
//! invitations, grant access to accounts that already exist, or refuse and say
//! why. Which of the three it is depends on the text in the field, on the
//! caller's rights and on the level being handed out — none of which are
//! things a paint routine can decide, and all of which must give the same
//! answer when the server is asked. [`ShareUiState::plan_invite`] is that
//! answer, in the platform-free half, so the widget only draws it and the
//! tests can ask it directly.
//!
//! ## Why the field is split rather than posted as one list
//!
//! The product sends no mail. An entry that is an email address can therefore
//! never be "delivered" — the only thing that can happen to it is an
//! INVITATION: an account is created (by an administrator) and a link comes
//! back for a human to carry. An entry that is an account id is a different
//! act on a different subject: the account already exists and is added to this
//! document's access list. Splitting them here is what keeps the dialog from
//! reporting "invited" for something no invitation was ever issued for.

use jian_core::text_input::TextInputState;
use op_i18n::Locale;

use crate::access::Rights;
use crate::share_access::{
    looks_like_email, parse_invite_entries, ShareGrant, ShareInviteRefusal, ShareLevel,
    ShareListSnapshot, MAX_INVITE_ENTRIES,
};

/// Which row of the dialog the pointer is over, or which control is active.
///
/// A closed set of rows rather than rects: paint and hit-test both walk this,
/// so a row that is painted is a row that can be clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareRow {
    Close,
    CopyLink,
    /// The comma-separated invite field.
    InviteField,
    /// The Invite button.
    Invite,
    /// The "Anyone with the link" switch.
    LinkAccess,
    /// The level control of the "Anyone with the link" row.
    LinkLevel,
    /// The level control of the Nth person row.
    PersonLevel(usize),
    /// The remove control of the Nth person row.
    PersonRemove(usize),
    /// The Nth option of an open level picker.
    LevelOption(usize),
    /// A footer row that opens the live collaboration panel.
    ///
    /// Present because the top-bar chip used to be the only way into it and
    /// now opens this dialog: without a row here the live-session screen would
    /// be unreachable, which is not a trade this dialog is entitled to make.
    OpenSession,
}

/// Which level control a picker is open for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareLevelTarget {
    /// The level the next invitation or grant carries.
    Invite,
    Link,
    Person(usize),
}

/// What one Invite press turned out to be.
///
/// The two lists are different acts and are reported separately; see the
/// module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareInvitePlan {
    /// Email entries: an invitation is created and its link handed back.
    pub invitations: Vec<String>,
    /// Account entries: added to this document's access list directly.
    pub grants: Vec<String>,
}

impl ShareInvitePlan {
    pub fn is_empty(&self) -> bool {
        self.invitations.is_empty() && self.grants.is_empty()
    }
}

/// One invitation the server issued, with the link only a human can deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareIssuedInvite {
    /// The address the invitation was issued for, exactly as it was typed.
    pub email: String,
    /// The path the server answered with (`/invite/<token>`).
    pub path: String,
}

/// What the dialog last did, said out loud.
///
/// Everything here is either a fact the server confirmed or a refusal. There
/// is deliberately no "sent" variant: nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareNotice {
    /// The press could not be honoured, and why.
    Refused(ShareInviteRefusal),
    /// Invitations were created. Their links must be passed on by hand.
    Issued(Vec<ShareIssuedInvite>),
    /// Accounts were added to this document's access list.
    Granted(Vec<ShareGrant>),
    /// The document link is in the clipboard.
    CopiedLink,
    /// The anyone-with-the-link switch changed.
    LinkAccess { enabled: bool, level: ShareLevel },
    /// The access list could not be read, so the dialog is showing nothing
    /// rather than an empty list it cannot vouch for.
    ListUnavailable,
    /// Copy link was pressed on a document that has no link to copy.
    ///
    /// A document the daemon never stored — a starter document, or one held in
    /// a tab that has not saved it — has no address, so there is nothing to put
    /// on the clipboard. Saying so is the whole point: the alternative is a
    /// button that appears to work and a clipboard that did not change.
    NoDocumentLink,
}

impl ShareNotice {
    /// The i18n key of the sentence this notice shows.
    pub const fn i18n_key(&self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.i18n_key(),
            Self::Issued(_) => "share.notice.issued",
            Self::Granted(_) => "share.notice.granted",
            Self::CopiedLink => "share.notice.copiedLink",
            Self::LinkAccess { .. } => "share.notice.linkAccess",
            Self::ListUnavailable => "share.notice.listUnavailable",
            Self::NoDocumentLink => "share.notice.noLink",
        }
    }

    /// Whether the notice reports something the user should see as a failure.
    pub const fn is_refusal(&self) -> bool {
        matches!(self, Self::Refused(_) | Self::ListUnavailable)
    }
}

/// One thing the dialog asks the host to do.
///
/// The widget layer has no HTTP and no clipboard, so every press that needs
/// either queues one of these and lets the host drain it — the same
/// arrangement `CollabUiAction` uses. A queue rather than a single slot
/// because one Invite press can be several requests: three addresses are three
/// invitations, and a single slot would silently drop two of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareAction {
    /// Read `GET /api/share/list` and feed the answer back through
    /// [`ShareUiState::apply_list`].
    LoadList,
    /// Add an account to this document's access list.
    Grant { account: String, level: ShareLevel },
    /// Remove an account from it.
    Revoke { account: String },
    /// Create an invitation and hand its link back.
    ///
    /// Nothing is sent to the address — the product has no mail — so this is
    /// the whole of what "invite by email" can mean here.
    IssueInvitation { email: String, level: ShareLevel },
    /// Turn link access on or off, or change the level it hands out.
    SetLinkAccess { enabled: bool, level: ShareLevel },
    /// Put the document link in the clipboard.
    CopyLink,
    /// Put one issued invitation's link in the clipboard.
    ///
    /// Its own action rather than a reuse of [`Self::CopyLink`] because the two
    /// put different strings on the clipboard, and a host that had to decide
    /// which by looking at the dialog's notice would be reading paint state to
    /// work out what to copy.
    CopyInviteLink { path: String },
    /// Open the live collaboration panel.
    OpenSession,
}

/// Everything the Share dialog paints and dispatches from.
#[derive(Debug, Clone)]
pub struct ShareUiState {
    pub open: bool,
    /// The "Add comma separated emails to invite" field.
    pub invite_input: TextInputState,
    pub invite_focused: bool,
    /// The level the next invitation or grant will carry.
    pub invite_level: ShareLevel,
    /// The level "Anyone with the link" currently hands out.
    pub link_level: ShareLevel,
    /// Whether anyone holding the link may open this document. Written from the
    /// server's answer, never optimistically: a switch that reads "on" while
    /// the document is closed is worse than one that lags a moment.
    pub link_enabled: bool,
    /// Who has access, as the server last answered.
    pub list: ShareListSnapshot,
    /// The caller's own rights in the document being shared. Gates Invite and
    /// every level picker.
    pub own_rights: Rights,
    /// Whether [`Self::own_rights`] is a fact or still the fail-closed default.
    ///
    /// The chip in the top bar shows the caller's level only when this is true,
    /// because the alternative is showing "Can view" to somebody who has not
    /// been asked yet — a wrong answer rather than a missing one.
    pub rights_known: bool,
    /// Whether the caller owns this document.
    pub is_owner: bool,
    /// What the owner's access list gives the caller, when the document is
    /// somebody else's. Read from `sharedWithMe`, and `None` while no answer has
    /// arrived — which is why the row falls back to the rights rather than
    /// claiming the weakest level.
    pub granted_level: Option<ShareLevel>,
    /// The caller's own account id, for the "you" row and for refusing a
    /// self-invite before it becomes a `cannot-share-with-self` round trip.
    pub self_account: Option<String>,
    /// The absolute document link Copy link copies; `None` when there is no
    /// document to link to yet.
    pub link: Option<String>,
    /// The last thing the dialog did.
    pub notice: Option<ShareNotice>,
    /// Row under the pointer.
    pub hover: Option<ShareRow>,
    /// Open level picker, and whose.
    pub level_picker: Option<ShareLevelTarget>,
    /// A request is in flight; the Invite button is inert until it settles.
    pub busy: bool,
    /// Host-owned work this dialog has asked for and not yet had drained.
    pub pending: Vec<ShareAction>,
}

impl Default for ShareUiState {
    fn default() -> Self {
        Self {
            open: false,
            invite_input: TextInputState::default(),
            invite_focused: false,
            // The default level a new invitation carries. Viewer, matching
            // `ShareLevel::DEFAULT`: the least a press could hand out.
            invite_level: ShareLevel::DEFAULT,
            link_level: ShareLevel::DEFAULT,
            link_enabled: false,
            list: ShareListSnapshot::default(),
            // No roles read yet, so nothing may be handed out. The host
            // replaces this with the verified rights as soon as it has them.
            own_rights: Rights::VIEW_ONLY,
            rights_known: false,
            is_owner: false,
            granted_level: None,
            self_account: None,
            link: None,
            notice: None,
            hover: None,
            level_picker: None,
            busy: false,
            pending: Vec::new(),
        }
    }
}

impl ShareUiState {
    /// Open the dialog over a freshly computed link.
    ///
    /// The access list is NOT cleared here: reopening the dialog while its
    /// list is one request old shows the list, because "we have not asked
    /// again yet" and "nobody has access" are different statements and only
    /// one of them is true.
    pub fn open_with(&mut self, link: Option<String>) {
        self.open = true;
        self.link = link;
        self.hover = None;
        self.level_picker = None;
        self.notice = None;
        self.invite_focused = false;
    }

    /// Close the dialog and drop everything transient about it.
    ///
    /// The notice goes with it: a message about a press belongs to the dialog
    /// that was open when the press happened, and a refusal that reappears
    /// over an unrelated document is a bug the user cannot clear.
    pub fn close(&mut self) {
        self.open = false;
        self.hover = None;
        self.level_picker = None;
        self.invite_focused = false;
        self.notice = None;
        self.busy = false;
        self.invite_input.reset_transient();
    }

    /// Whether this account may add anybody to this document at all.
    pub fn can_invite(&self) -> bool {
        ShareInviteRefusal::check_invite_right(self.own_rights).is_ok()
    }

    /// The level this account itself holds in the document, once that is known.
    ///
    /// The owner holds everything on its own document, so its level is the
    /// strongest there is; everybody else holds the strongest level their
    /// product roles cover. `None` until the roles have been read — see
    /// [`Self::rights_known`].
    pub fn own_level(&self) -> Option<ShareLevel> {
        self.rights_known
            .then(|| ShareLevel::from_rights(self.own_rights))
    }

    /// Whether this account may create an account by inviting an email.
    ///
    /// A different power from [`Self::can_invite`] and held by a different
    /// level: adding somebody who exists is the invite right, creating somebody
    /// who does not is the account list (`Rights::can_manage_users`).
    pub fn can_invite_by_email(&self) -> bool {
        self.own_rights.can_manage_users()
    }

    /// Levels this account may hand out, strongest first.
    pub fn grantable_levels(&self) -> Vec<ShareLevel> {
        ShareLevel::ALL
            .into_iter()
            .filter(|level| level.is_grantable_by(self.own_rights))
            .collect()
    }

    /// The entries the field currently names.
    pub fn invite_entries(&self) -> Vec<String> {
        parse_invite_entries(self.invite_input.text())
    }

    /// What one Invite press would do — or why it will not.
    ///
    /// Checked in the order a person would experience the failure: is there
    /// anything to invite, is there too much of it, may this account invite at
    /// all, may it hand out THIS level, and only then — is an email entry
    /// asking for a power it does not hold.
    pub fn plan_invite(&self) -> Result<ShareInvitePlan, ShareInviteRefusal> {
        let entries = self.invite_entries();
        if entries.is_empty() {
            return Err(ShareInviteRefusal::EmptyField);
        }
        // `parse_invite_entries` truncates at the cap, so the "too many" case
        // is detected on the raw field rather than on the parsed list.
        let typed = parse_invite_entries_unbounded(self.invite_input.text());
        if typed > MAX_INVITE_ENTRIES {
            return Err(ShareInviteRefusal::TooManyEntries { count: typed });
        }
        ShareInviteRefusal::check_invite_right(self.own_rights)?;
        ShareInviteRefusal::check_level(self.own_rights, self.invite_level)?;

        let mut plan = ShareInvitePlan::default();
        for entry in entries {
            if looks_like_email(&entry) {
                // An email entry is an INVITATION, and an invitation creates an
                // account — a power of the deployment's account list, not of a
                // document. So it needs `can_manage_users` whatever level is
                // being handed out: even a viewer invitation mints an account
                // that did not exist, and minting accounts is one power.
                if !self.can_invite_by_email() {
                    return Err(ShareInviteRefusal::NotAnAdministratorForEmail);
                }
                plan.invitations.push(entry);
                continue;
            }
            if self.self_account.as_deref() == Some(entry.as_str()) {
                return Err(ShareInviteRefusal::AlreadyOnList {
                    account: entry.clone(),
                });
            }
            if plan.grants.iter().any(|seen| seen == &entry) {
                continue;
            }
            plan.grants.push(entry);
        }
        if plan.is_empty() {
            return Err(ShareInviteRefusal::EmptyField);
        }
        Ok(plan)
    }

    /// Record the server's answer to `GET /api/share/list`.
    pub fn apply_list(&mut self, snapshot: ShareListSnapshot) {
        if snapshot.available {
            self.list = snapshot;
            if self.notice == Some(ShareNotice::ListUnavailable) {
                self.notice = None;
            }
        } else {
            // The list is left as it was — see `open_with` — and the dialog
            // says it could not read one rather than claiming an empty one.
            self.notice = Some(ShareNotice::ListUnavailable);
        }
    }

    /// Every account the document is shared with, in list order.
    pub fn people(&self) -> &[ShareGrant] {
        &self.list.shared_with
    }

    /// How many people hold each level, for the count a level row shows.
    pub fn count_at(&self, level: ShareLevel) -> usize {
        self.list.count_at(level)
    }

    /// The label for one person row.
    ///
    /// The account id is all this deployment has: there is no directory the
    /// dialog may read (`accounts*` belongs to the account-administration
    /// surface and is not this route's to open), and inventing a display name
    /// from an id would be a guess printed as a fact. The caller's own row is
    /// the one exception, and says "You" because that is not a guess.
    pub fn person_label(&self, locale: Locale, granted: &ShareGrant) -> String {
        if self.self_account.as_deref() == Some(granted.account.as_str()) {
            return op_i18n::translate(locale, "share.row.you").to_string();
        }
        granted.account.clone()
    }

    /// The sentence describing who added somebody, or the honest gap.
    pub fn attribution_label(&self, locale: Locale, granted: &ShareGrant) -> String {
        match granted.invited_by.as_deref() {
            Some(inviter) if Some(inviter) == self.self_account.as_deref() => {
                op_i18n::translate(locale, "share.row.invitedByYou").to_string()
            }
            Some(inviter) => {
                op_i18n::translate(locale, "share.row.invitedBy").replace("{{account}}", inviter)
            }
            // A grant from before attribution was recorded. Saying so is the
            // whole point of the field existing.
            None => op_i18n::translate(locale, "share.row.invitedUnknown").to_string(),
        }
    }

    /// Refuse a level this account may not hand out, before a request is made.
    pub fn set_invite_level(&mut self, level: ShareLevel) -> Result<(), ShareInviteRefusal> {
        ShareInviteRefusal::check_level(self.own_rights, level)?;
        self.invite_level = level;
        self.level_picker = None;
        Ok(())
    }

    /// Set the level "Anyone with the link" hands out.
    pub fn set_link_level(&mut self, level: ShareLevel) -> Result<(), ShareInviteRefusal> {
        ShareInviteRefusal::check_level(self.own_rights, level)?;
        self.link_level = level;
        self.level_picker = None;
        Ok(())
    }

    /// Set one person's level.
    ///
    /// Re-leveling somebody else is [`ShareLevel::manages_this_document`] —
    /// the one thing the admin level has over the editor level — so the check
    /// is not the same one the invite button makes.
    pub fn set_person_level(
        &mut self,
        index: usize,
        level: ShareLevel,
    ) -> Result<(), ShareInviteRefusal> {
        if !self.is_owner && !ShareLevel::from_rights(self.own_rights).manages_this_document() {
            return Err(ShareInviteRefusal::NoInviteRight);
        }
        ShareInviteRefusal::check_level(self.own_rights, level)?;
        if let Some(grant) = self.list.shared_with.get_mut(index) {
            grant.level = level;
        }
        self.level_picker = None;
        Ok(())
    }

    /// Forget one person locally, once the server has confirmed the revoke.
    pub fn forget_person(&mut self, index: usize) {
        if index < self.list.shared_with.len() {
            self.list.shared_with.remove(index);
        }
        self.level_picker = None;
    }

    /// Replace one pending invitation's link with the one the server answered.
    pub fn record_issued(&mut self, invites: Vec<ShareIssuedInvite>) {
        self.busy = false;
        if invites.is_empty() {
            return;
        }
        self.invite_input.set_text("");
        self.invite_focused = true;
        self.notice = Some(ShareNotice::Issued(invites));
    }

    /// Record accounts that were granted access.
    pub fn record_granted(&mut self, granted: Vec<ShareGrant>) {
        self.busy = false;
        if granted.is_empty() {
            return;
        }
        for grant in &granted {
            match self
                .list
                .shared_with
                .iter_mut()
                .find(|seen| seen.account == grant.account)
            {
                Some(existing) => *existing = grant.clone(),
                None => self.list.shared_with.push(grant.clone()),
            }
        }
        self.invite_input.set_text("");
        self.invite_focused = true;
        self.notice = Some(ShareNotice::Granted(granted));
    }

    /// Record a refusal the server sent back.
    pub fn record_refusal(&mut self, refusal: ShareInviteRefusal) {
        self.busy = false;
        self.notice = Some(ShareNotice::Refused(refusal));
    }

    /// Queue host-owned work. Returns whether it was queued.
    ///
    /// The dialog stays open and usable across a request, so this appends
    /// rather than overwriting: an invite of three addresses queues three
    /// requests, and the host drains them in order.
    pub fn request(&mut self, action: ShareAction) -> bool {
        self.pending.push(action);
        true
    }
}

/// Count how many entries the field names, ignoring the cap.
///
/// [`parse_invite_entries`] stops at [`MAX_INVITE_ENTRIES`] because that is
/// what will actually be acted on; the refusal needs the real number so the
/// message can say how many were typed rather than how many fit.
fn parse_invite_entries_unbounded(raw: &str) -> usize {
    raw.split([',', '\n', '\r', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .count()
}

#[cfg(test)]
#[path = "share_tests.rs"]
mod tests;
