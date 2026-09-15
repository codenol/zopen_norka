//! Who may open a document, and as what — the model the Share dialog speaks.
//!
//! ## Why the dialog names four levels and not Figma's two
//!
//! Figma offers "can view" and "can edit". The operator's matrix
//! ([`crate::access`]) has four authority levels across seven roles, and two
//! of its rows are not expressible as view-or-edit at all: the five
//! contributor roles (ПО, Аналитик, Фронт, Бэк, QA) may **comment and invite**
//! without editing, and the admin role additionally owns who else is in the
//! workspace. A dialog offering only "view/edit" would hide both — the person
//! picking a level could not give a colleague the one thing the matrix says
//! they are for, and could not tell a commenter apart from a guest.
//!
//! So the levels here are the operator's four, and each one is named for the
//! rights it carries rather than for Figma's vocabulary:
//!
//! | Level | Carries | UI label |
//! | --- | --- | --- |
//! | [`ShareLevel::Admin`] | edit **and** the access list of this document | "Admin — full access" |
//! | [`ShareLevel::Editor`] | view, comment, invite, edit | "Can edit" |
//! | [`ShareLevel::Commenter`] | view, comment, invite | "Can comment and invite" |
//! | [`ShareLevel::Viewer`] | view | "Can view" |
//!
//! ## Why [`ShareLevel::Commenter`] is not called "commenter" in the UI
//!
//! Because the label is the only place a person can learn what the level
//! actually grants, and this level grants two things. The operator's matrix
//! puts commenting and inviting in ONE bucket ([`Rights::CONTRIBUTOR`]):
//! there is no role that may invite and not comment, so a dialog that offered
//! them separately would be inventing a split the authority model does not
//! have — and would have to answer what "may invite but not comment" even
//! means for a person. One level, both rights, both named.
//!
//! ## Why a level is a ceiling, not a grant
//!
//! A level says what the document's access list allows; it does not create
//! authority. The effective rights of somebody who was given access are the
//! **intersection** of their own product roles with the level they were given:
//! a level cannot promote an account past its roles, and an editor's roles do
//! not silently raise a grant that says "can view". [`ShareLevel::document_rights`]
//! is the document-scoped half of that intersection, and it deliberately
//! excludes [`crate::access::Right::ManageUsers`]: the account list belongs to
//! the deployment, not to a document, so no document grant may reach it.
//!
//! ## Why a grant records who made it
//!
//! Because anyone holding the invite right may add somebody, not only the
//! owner. Once several people can do that, "who added this person" stops being
//! trivia and becomes the question that gets asked when access is wrong — so
//! the grant carries it ([`ShareGrant::invited_by`]) and the dialog shows it.
//! A grant from before attribution existed has `None` and reads as unknown;
//! guessing a name there would be worse than admitting the gap.

use crate::access::{ProductRole, Rights};

/// Authority one account may hold over one document.
///
/// See the module docs for why these four and not Figma's two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShareLevel {
    /// Edit, plus the right to change this document's access list.
    Admin,
    /// View, comment, invite, edit — the UX/UI row of the matrix.
    Editor,
    /// View, comment, invite — the five contributor roles.
    Commenter,
    /// View, and nothing that changes anything.
    Viewer,
}

impl ShareLevel {
    /// Every level, from the strongest down.
    ///
    /// Descending because every list the dialog paints reads that way: the
    /// level picker, the "who has access" rows and the label tables all show
    /// authority first. `ALL[0]` is the level a grant ceiling is compared
    /// against, and a constant that changes order would change that answer.
    pub const ALL: [Self; 4] = [Self::Admin, Self::Editor, Self::Commenter, Self::Viewer];

    /// The level a grant gets when the client names none.
    ///
    /// [`Self::Viewer`], deliberately — the fail-closed direction. A request
    /// that forgot to say, or a client older than this model, must not be able
    /// to hand out editing by omission.
    pub const DEFAULT: Self = Self::Viewer;

    /// Stable name for the wire, for logs, and for persisted ACL entries.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Commenter => "commenter",
            Self::Viewer => "viewer",
        }
    }

    /// The i18n key for this level's label.
    pub const fn i18n_key(self) -> &'static str {
        match self {
            Self::Admin => "share.level.admin",
            Self::Editor => "share.level.editor",
            Self::Commenter => "share.level.commenter",
            Self::Viewer => "share.level.viewer",
        }
    }

    /// One-line explanation of what the level grants, as an i18n key.
    ///
    /// Separate from [`Self::i18n_key`] because a picker shows the short label
    /// and a list needs the sentence: "Can comment and invite" is a name, and
    /// "may leave comments and add other people to this document" is what it
    /// means.
    pub const fn hint_i18n_key(self) -> &'static str {
        match self {
            Self::Admin => "share.level.admin.hint",
            Self::Editor => "share.level.editor.hint",
            Self::Commenter => "share.level.commenter.hint",
            Self::Viewer => "share.level.viewer.hint",
        }
    }

    /// How much authority the level carries, for comparing a grant against a
    /// grantor. Higher is stronger.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Admin => 3,
            Self::Editor => 2,
            Self::Commenter => 1,
            Self::Viewer => 0,
        }
    }

    /// The operator's role bucket this level names.
    ///
    /// The four levels ARE the matrix's four rows (see [`crate::access`]'s
    /// module table), so this is a reading of the operator's document rather
    /// than a mapping invented here. It is what an email invitation has to
    /// send: `POST /api/auth/admin/invites` creates an ACCOUNT and takes
    /// product roles, so a level chosen in the dialog becomes the roles of the
    /// account being invited. [`Self::Viewer`] is empty on purpose — a viewer
    /// is the account with no product role at all, which is exactly what
    /// [`Rights::VIEW_ONLY`] (the floor every verified account sits on) means.
    pub const fn roles(self) -> &'static [ProductRole] {
        match self {
            Self::Admin => &[ProductRole::Admin],
            Self::Editor => &[ProductRole::UxUi],
            Self::Commenter => &[
                ProductRole::Software,
                ProductRole::Analyst,
                ProductRole::Frontend,
                ProductRole::Backend,
                ProductRole::Qa,
            ],
            Self::Viewer => &[],
        }
    }

    /// The wire spellings of [`Self::roles`].
    pub fn role_wires(self) -> Vec<&'static str> {
        self.roles().iter().map(|role| role.as_wire()).collect()
    }

    /// What a grant at this level allows **on one document**.
    ///
    /// [`crate::access::Right::ManageUsers`] is absent from every arm,
    /// including [`Self::Admin`], and that is the load-bearing part of this
    /// function. The account list is the deployment's, not a document's, so a
    /// level granted on a file must never be a way to acquire it: a grant is a
    /// statement about one document and nothing else. That is also why the
    /// workspace and account-list questions are asked of an account's ROLES
    /// rather than of the effective rights this function feeds — see
    /// `RequestAccess::role_rights`.
    /// What an admin holds over *this* document is its access list, which is
    /// [`Self::manages_this_document`], not the deployment's.
    pub const fn document_rights(self) -> Rights {
        match self {
            Self::Admin | Self::Editor => Rights::EDITOR,
            Self::Commenter => Rights::CONTRIBUTOR,
            Self::Viewer => Rights::VIEW_ONLY,
        }
    }

    /// Whether a holder may change THIS document's access list — add somebody,
    /// remove somebody, or re-level somebody.
    ///
    /// Kept apart from [`Self::document_rights`] rather than folded into it as
    /// [`crate::access::Right::ManageUsers`], because `Rights` is the
    /// deployment-wide
    /// vocabulary and reusing its `ManageUsers` bit here would make "may
    /// manage this file's sharing" and "may re-role every account on the
    /// deployment" the same question. They are not, and one of the two is
    /// reachable by a grant on a file.
    pub const fn manages_this_document(self) -> bool {
        matches!(self, Self::Admin)
    }

    /// The strongest level whose rights the caller fully holds.
    ///
    /// Used to answer "what may this person hand out": the dialog offers a
    /// level only when the person opening it holds that level, and the server
    /// refuses a grant above it. The scan runs strongest-first and returns the
    /// first level the rights cover, so a caller holding `Rights::ADMIN` reads
    /// as [`Self::Admin`] and one holding nothing at all reads as
    /// [`Self::Viewer`] — the floor, which is the honest answer for an account
    /// that may look at a document and nothing else.
    pub fn from_rights(rights: Rights) -> Self {
        Self::ALL
            .into_iter()
            .find(|level| rights.contains_all(level.role_rights()))
            .unwrap_or(Self::Viewer)
    }

    /// The full bucket this level names in the operator's matrix.
    ///
    /// [`Self::document_rights`] is the same thing minus the deployment-scoped
    /// right; this one is what a caller's own product roles must cover before
    /// the level may be handed out.
    pub const fn role_rights(self) -> Rights {
        match self {
            Self::Admin => Rights::ADMIN,
            Self::Editor => Rights::EDITOR,
            Self::Commenter => Rights::CONTRIBUTOR,
            Self::Viewer => Rights::VIEW_ONLY,
        }
    }

    /// Whether `granter` may hand out this level.
    ///
    /// "You may not give away what you do not hold" — the rule that keeps a
    /// commenter from creating an admin, and the only ceiling a grant needs.
    /// Ownership is applied by the caller (`RequestAccess` already treats an
    /// owner as holding editing rights over its own document), so this stays a
    /// pure statement about rights.
    pub fn is_grantable_by(self, granter: Rights) -> bool {
        granter.contains_all(self.role_rights())
    }

    /// Parse one wire or persisted spelling.
    ///
    /// Case and separators fold (`Can Edit`, `can-edit` and `can_edit` are one
    /// level) because the spellings this reads come from three places that were
    /// written at different times: a persisted `acl.json`, a request body, and
    /// a test. Nothing privileged rests on the guess — an unrecognised level is
    /// an error, and the caller falls back to [`Self::DEFAULT`], which is the
    /// weakest level rather than a privileged one.
    pub fn from_wire(raw: &str) -> Result<Self, ShareLevelError> {
        let normalized = normalize(raw);
        if normalized.is_empty() {
            return Err(ShareLevelError::Blank);
        }
        Ok(match normalized.as_str() {
            "admin" | "administrator" | "админ" | "администратор" => Self::Admin,
            "editor" | "edit" | "can_edit" | "canedit" | "ux_ui" | "uxui" | "дизайнер" => {
                Self::Editor
            }
            "commenter" | "comment" | "can_comment" | "cancomment" | "inviter" | "contributor" => {
                Self::Commenter
            }
            "viewer" | "view" | "can_view" | "canview" | "guest" => Self::Viewer,
            _ => {
                return Err(ShareLevelError::Unknown {
                    raw: raw.trim().to_string(),
                })
            }
        })
    }
}

/// Why a level string could not be parsed.
///
/// A named error rather than `Option` for the same reason
/// [`crate::access::RoleWireError`] is one: "we understood nothing" and "we
/// were sent nothing" lead to different bug reports, and a silent `None`
/// erases the difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareLevelError {
    Blank,
    Unknown { raw: String },
}

impl std::fmt::Display for ShareLevelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blank => f.write_str("blank share level"),
            Self::Unknown { raw } => write!(f, "unknown share level: {raw}"),
        }
    }
}

impl std::error::Error for ShareLevelError {}

fn normalize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        let ch = match ch {
            '/' | '-' | '.' | ' ' | '\t' => '_',
            other => other,
        };
        if ch == '_' && (out.is_empty() || out.ends_with('_')) {
            continue;
        }
        out.extend(ch.to_lowercase());
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// One account's access to one document.
///
/// `account` is the account id — the same opaque string the access list has
/// always held. `invited_by` is the account that added it, `None` for a grant
/// made before attribution was recorded; see the module docs for why that is
/// shown as unknown rather than guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareGrant {
    pub account: String,
    pub level: ShareLevel,
    pub invited_by: Option<String>,
    /// The name to show for `account`, when the deployment's directory has one.
    ///
    /// A list of opaque account ids is a list of strangers: "Who has access"
    /// painted `u_262d2b166fbbd9f1c4b1c9650271610d` where a person belongs, and
    /// the only way to read it was to already know the ids (issue #119).
    pub display_name: Option<String>,
    /// The sign-in handle, carried beside the display name because it is how
    /// somebody is addressed when inviting them.
    pub username: Option<String>,
}

impl ShareGrant {
    /// A grant with no attribution — the shape every legacy entry has.
    pub fn unattributed(account: impl Into<String>, level: ShareLevel) -> Self {
        Self {
            account: account.into(),
            level,
            invited_by: None,
            display_name: None,
            username: None,
        }
    }

    /// How to name this account to a person.
    ///
    /// The display name first, then the handle, and the raw id only when the
    /// directory knows nothing — an id is still better than a blank row, and it
    /// is what somebody would have to quote to a different account.
    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(self
                .username
                .as_deref()
                .filter(|name| !name.trim().is_empty()))
            .unwrap_or(self.account.as_str())
    }

    /// Read one entry of a `sharedWith` array.
    ///
    /// Two shapes are accepted because the two exist in the field: the current
    /// object (`{"account","level","invitedBy"}`) and the bare account-id
    /// string every list written before levels existed holds. The string form
    /// reads as [`ShareLevel::DEFAULT`] with no attribution — fail closed, and
    /// the truth: whoever it names was granted access, and nothing recorded
    /// said how much.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        if let Some(account) = value.as_str() {
            let account = account.trim();
            return (!account.is_empty()).then(|| Self::unattributed(account, ShareLevel::DEFAULT));
        }
        let account = value.get("account")?.as_str()?.trim();
        if account.is_empty() {
            return None;
        }
        let level = value
            .get("level")
            .and_then(|level| level.as_str())
            .and_then(|level| ShareLevel::from_wire(level).ok())
            .unwrap_or(ShareLevel::DEFAULT);
        let invited_by = value
            .get("invitedBy")
            .and_then(|by| by.as_str())
            .map(str::to_string);
        let name = |field: &str| {
            value
                .get(field)
                .and_then(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            account: account.to_string(),
            level,
            invited_by,
            display_name: name("displayName"),
            username: name("username"),
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "account": self.account,
            "level": self.level.wire(),
            "invitedBy": self.invited_by,
            "displayName": self.display_name,
            "username": self.username,
        })
    }
}

/// What `GET /api/share/list` answered.
///
/// Parsed here rather than in the web host because both halves of the product
/// read it — the browser shell paints the dialog from it today and the desktop
/// app will — and a second parser is a second place for "who has access" to
/// mean something different.
/// One document shared with the asking account, and how much of it.
///
/// A guest who is never told what they hold cannot act on it — the dialog drew
/// them as view-only whatever the grant said (#121) — so the level travels with
/// the owner's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedOwner {
    /// The account whose document it is.
    pub owner: String,
    /// What the owner's access list gives the asker.
    pub level: ShareLevel,
    /// The name to show for the owner, when the directory has one.
    pub display_name: Option<String>,
    /// The owner's sign-in handle.
    pub username: Option<String>,
}

impl SharedOwner {
    /// How to name the owner to a person. See [`ShareGrant::label`].
    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(self
                .username
                .as_deref()
                .filter(|name| !name.trim().is_empty()))
            .unwrap_or(self.owner.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareListSnapshot {
    /// Who may open the caller's document, with their levels.
    pub shared_with: Vec<ShareGrant>,
    /// Whose documents the caller may open, with the level each one gives
    /// them. Read by the dialog to say what the caller holds in a document
    /// somebody else owns.
    pub shared_with_me: Vec<SharedOwner>,
    /// Whether the deployment offers sharing at all.
    pub available: bool,
}

impl ShareListSnapshot {
    /// Parse the answer. `available` is false for anything that is not a
    /// well-formed success — a refusal body, a network error page, or an
    /// empty body all mean "we do not know who has access", which the dialog
    /// must not render as "nobody does".
    pub fn parse(body: &str) -> Self {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
            return Self::default();
        };
        if parsed.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
            return Self::default();
        }
        let shared_with = parsed
            .get("sharedWith")
            .and_then(|value| value.as_array())
            .map(|entries| entries.iter().filter_map(ShareGrant::from_json).collect())
            .unwrap_or_default();
        let shared_with_me = parsed
            .get("sharedWithMe")
            .and_then(|value| value.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        // The bare-id shape is what a deployment older than
                        // levels answered. It reads as the weakest level, which
                        // is the fail-closed direction and the truth: whoever it
                        // names was given access, and nothing written says how
                        // much.
                        if let Some(owner) = entry.as_str() {
                            let owner = owner.trim();
                            return (!owner.is_empty()).then(|| SharedOwner {
                                owner: owner.to_string(),
                                level: ShareLevel::DEFAULT,
                                display_name: None,
                                username: None,
                            });
                        }
                        let owner = entry.get("owner")?.as_str()?.trim();
                        if owner.is_empty() {
                            return None;
                        }
                        let level = entry
                            .get("level")
                            .and_then(|level| level.as_str())
                            .and_then(|level| ShareLevel::from_wire(level).ok())
                            .unwrap_or(ShareLevel::DEFAULT);
                        let name = |field: &str| {
                            entry
                                .get(field)
                                .and_then(|name| name.as_str())
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(str::to_string)
                        };
                        Some(SharedOwner {
                            owner: owner.to_string(),
                            level,
                            display_name: name("displayName"),
                            username: name("username"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            shared_with,
            shared_with_me,
            available: true,
        }
    }

    /// What this snapshot says the caller holds in `owner`'s document.
    ///
    /// `None` when the list does not name that owner at all, which the dialog
    /// must not read as "view only": it means the answer has not arrived.
    pub fn level_from(&self, owner: &str) -> Option<ShareLevel> {
        self.shared_with_me
            .iter()
            .find(|shared| shared.owner == owner)
            .map(|shared| shared.level)
    }

    /// The level `account` holds, if the list names it.
    pub fn level_of(&self, account: &str) -> Option<ShareLevel> {
        self.shared_with
            .iter()
            .find(|grant| grant.account == account)
            .map(|grant| grant.level)
    }

    /// How many accounts beyond one hold each level.
    ///
    /// The count the dialog shows next to a level row ("3 people"); computed
    /// here rather than in paint so the number and the rows cannot disagree.
    pub fn count_at(&self, level: ShareLevel) -> usize {
        self.shared_with
            .iter()
            .filter(|grant| grant.level == level)
            .count()
    }
}

/// Largest number of addresses one Invite press accepts.
///
/// A paste of a mailing list is a mistake, and an invitation is a link a human
/// has to carry to a human: past a handful the request is refused rather than
/// quietly truncated, so nobody believes an address was invited when it was
/// dropped before the first `POST`.
pub const MAX_INVITE_ENTRIES: usize = 20;

/// Split an invite field into the entries it names.
///
/// Comma- and newline-separated (the label says comma, but a pasted column of
/// addresses arrives newline-separated and refusing it would be pedantry),
/// trimmed, empties dropped, repeats collapsed, each entry truncated to
/// [`MAX_INVITE_ENTRY_CHARS`]. Order of first appearance is kept so the dialog
/// reports results in the order the person typed them.
pub fn parse_invite_entries(raw: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for entry in raw.split([',', '\n', '\r', ';']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let entry = truncate_chars(entry, MAX_INVITE_ENTRY_CHARS);
        if !entries.iter().any(|seen| seen == &entry) {
            entries.push(entry);
        }
        if entries.len() >= MAX_INVITE_ENTRIES {
            break;
        }
    }
    entries
}

/// Longest single invite entry accepted.
///
/// An email address is at most 254 characters by RFC 5321; the bound is here
/// so a megabyte pasted into a one-line field cannot become a request body.
pub const MAX_INVITE_ENTRY_CHARS: usize = 254;

/// Whether an invite entry addresses a person by email rather than by account.
///
/// Deliberately a shape test, not a validator: an address that is malformed
/// enough to be useless is rejected by the server that tries to use it, and a
/// client that "corrects" an address is a client that invites the wrong
/// person. All this decides is which of the two routes the entry goes down —
/// an invitation (which mints an account) or a grant (which names one).
pub fn looks_like_email(entry: &str) -> bool {
    let Some((local, domain)) = entry.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !entry.chars().any(char::is_whitespace)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Why an invite press could not do what it said.
///
/// Every variant is shown to the person who pressed Invite: the product has no
/// mail, so the only way a refusal can be honest is to say it out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareInviteRefusal {
    /// The field named nothing to invite.
    EmptyField,
    /// The field named more than [`MAX_INVITE_ENTRIES`] addresses.
    TooManyEntries { count: usize },
    /// This account may not hand out this document's access list at all.
    ///
    /// The operator's matrix gives the invite right to the owner, to an admin
    /// and to the five contributor roles; an account holding none of them (the
    /// view-only floor) reaches this.
    NoInviteRight,
    /// The account may invite, but not at this level — you cannot give away
    /// what you do not hold.
    LevelAboveOwn { level: ShareLevel, own: ShareLevel },
    /// An email entry was typed and this account may not create accounts.
    ///
    /// Not a spelling of [`Self::NoInviteRight`]: adding an existing account to
    /// a document and creating a new account on the deployment are different
    /// powers, held by different levels, and the person reading the message
    /// needs to know which one they are missing.
    NotAnAdministratorForEmail,
    /// A named account already holds access at this level.
    AlreadyOnList { account: String },
    /// The server refused the write, with a reason this build has no sentence
    /// of its own for.
    ///
    /// The daemon's vocabulary is wider than the dialog's — a full access list,
    /// a write that could not be persisted, an account that may not be managed
    /// — and inventing a translation for a code this build does not know would
    /// be a guess printed to the user. The code is carried so that a log line
    /// or a bug report can say which refusal it was.
    RefusedByServer { code: String },
}

impl ShareInviteRefusal {
    /// Stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyField => "empty-invite-field",
            Self::TooManyEntries { .. } => "too-many-invite-entries",
            Self::NoInviteRight => "invite-role-required",
            Self::LevelAboveOwn { .. } => "level-above-your-own",
            Self::NotAnAdministratorForEmail => "admin-role-required-for-email",
            Self::AlreadyOnList { .. } => "already-has-access",
            Self::RefusedByServer { .. } => "server-refused",
        }
    }

    /// The i18n key carrying the sentence shown to the user.
    pub const fn i18n_key(&self) -> &'static str {
        match self {
            Self::EmptyField => "share.invite.refused.empty",
            Self::TooManyEntries { .. } => "share.invite.refused.tooMany",
            Self::NoInviteRight => "share.invite.refused.noRight",
            Self::LevelAboveOwn { .. } => "share.invite.refused.levelAboveOwn",
            Self::NotAnAdministratorForEmail => "share.invite.refused.emailNeedsAdmin",
            Self::AlreadyOnList { .. } => "share.invite.refused.alreadyHasAccess",
            Self::RefusedByServer { .. } => "share.invite.refused.server",
        }
    }

    /// Whether `rights` is enough to invite at all.
    ///
    /// One function so the dialog's disabled button, the message it shows and
    /// the server's refusal cannot drift: they all ask this.
    pub fn check_invite_right(rights: Rights) -> Result<(), Self> {
        if rights.can_invite() {
            Ok(())
        } else {
            Err(Self::NoInviteRight)
        }
    }

    /// Whether `rights` may hand out `level`.
    pub fn check_level(rights: Rights, level: ShareLevel) -> Result<(), Self> {
        if level.is_grantable_by(rights) {
            Ok(())
        } else {
            Err(Self::LevelAboveOwn {
                level,
                own: ShareLevel::from_rights(rights),
            })
        }
    }
}

impl std::fmt::Display for ShareInviteRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField => f.write_str("no address was entered to invite"),
            Self::TooManyEntries { count } => write!(
                f,
                "{count} addresses were entered; at most {MAX_INVITE_ENTRIES} are accepted at once"
            ),
            Self::NoInviteRight => {
                f.write_str("your roles do not allow adding people to this document")
            }
            Self::LevelAboveOwn { level, own } => write!(
                f,
                "you may not grant the {} level; you hold {}",
                level.wire(),
                own.wire()
            ),
            Self::NotAnAdministratorForEmail => f.write_str(
                "inviting by email creates an account, which only an account with the \
                 user-management right may do",
            ),
            Self::AlreadyOnList { account } => {
                write!(f, "{account} already has access to this document")
            }
            Self::RefusedByServer { code } => {
                write!(f, "the server refused the request ({code})")
            }
        }
    }
}

impl std::error::Error for ShareInviteRefusal {}

#[cfg(test)]
#[path = "share_access_tests.rs"]
mod tests;
