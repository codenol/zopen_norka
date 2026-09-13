//! Roles and rights — the model half of the roles work (#10).
//!
//! op-hub has always known which product roles an account holds and has
//! always sent them on `GET /api/v1/session`; nothing downstream looked, so
//! until now every account was the same account and the operator's access
//! matrix existed only in a document. This module is where that matrix
//! becomes code.
//!
//! It is platform-free and transport-free on purpose: the hub verifier, the
//! REST routes and (later) the chrome all ask the same question, and none of
//! them owns the answer. Nothing here is enforced anywhere yet — the route
//! checks that consume [`RoleSet::rights`] belong to the step that also knows
//! who owns a document.
//!
//! ## The operator's matrix
//!
//! Four authority levels across seven roles:
//!
//! | Level | Roles | May |
//! | --- | --- | --- |
//! | Admin | Админ | everything, including who else is in the workspace |
//! | Editor | UX/UI | edit everything except the account list |
//! | Contributor | ПО, Аналитик, Фронт, Бэк, QA | view, comment, invite |
//! | Guest | by link | view |
//!
//! ## Why an account's rights are the UNION of its roles
//!
//! Holding two roles at once is ordinary, not a misconfiguration — the
//! designer who also covers QA is exactly the person the matrix is about.
//! Intersecting would take away rights the account holds legitimately ("you
//! are UX/UI and QA, so you may do only what both may do" is nobody's
//! intent), and it would let the *unknown* half of a half-understood role
//! list weaken the half that is understood. Union is also the only rule that
//! keeps a wrong alias guess harmless: a role recognised as the wrong
//! non-privileged role cannot reach past its own bucket.
//!
//! ## Why an unrecognised role grants nothing, and is still kept
//!
//! Fail closed: the hub's role vocabulary is not this module's to guess, and
//! a typo, a renamed role, or a role that a newer hub sends must never land
//! in a privileged bucket. So an unknown string contributes no right — but
//! it is preserved in [`RoleSet::unrecognized`] rather than dropped, because
//! silently discarding what the hub said is precisely the failure that made
//! #10 necessary in the first place.

/// Something a caller can ask about.
///
/// The set is deliberately minimal: one variant per decision the operator's
/// four levels actually distinguish. Export is deliberately absent — no level
/// in the matrix differs on it, and a right no level distinguishes would be
/// invented policy rather than modelled policy (it is listed as an open
/// question for the operator instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Right {
    /// Look at a document.
    View,
    /// Leave comments on a document.
    Comment,
    /// Add another account to a document's access list.
    Invite,
    /// Change the document.
    Edit,
    /// Add, remove, or re-role accounts.
    ManageUsers,
}

impl Right {
    /// Every right, in ascending authority.
    pub const ALL: [Self; 5] = [
        Self::View,
        Self::Comment,
        Self::Invite,
        Self::Edit,
        Self::ManageUsers,
    ];

    /// One bit per right — see [`Rights`].
    const fn bit(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Comment => 2,
            Self::Invite => 4,
            Self::Edit => 8,
            Self::ManageUsers => 16,
        }
    }

    /// Stable name for logs and error text.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Comment => "comment",
            Self::Invite => "invite",
            Self::Edit => "edit",
            Self::ManageUsers => "manage-users",
        }
    }
}

/// A set of [`Right`]s, as a bitmask.
///
/// A bitmask rather than a struct of `bool`s because the operation this type
/// exists for is union: several roles each contribute a set and the answer is
/// their combination, and `u8` makes that a one-liner that cannot drift as
/// rights are added (the alternative — a field per right — is a place where a
/// forgotten field silently means "denied", or worse, a forgotten `true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rights(u8);

impl Rights {
    /// Nothing at all.
    pub const NONE: Self = Self(0);

    /// What any verified account may do before roles are considered: look at
    /// its own workspace. This is the baseline [`RoleSet::rights`] floors at,
    /// and it is why an empty role list means "no roles" rather than "no
    /// access" — a `StaticVerifier` deployment (env token table, no hub, and
    /// therefore no roles) must keep working exactly as it did.
    pub const VIEW_ONLY: Self = Self(Right::View.bit());

    /// View, comment, invite — the five non-design product roles.
    pub const CONTRIBUTOR: Self =
        Self(Right::View.bit() | Right::Comment.bit() | Right::Invite.bit());

    /// Contributor plus edit — UX/UI.
    pub const EDITOR: Self = Self(Self::CONTRIBUTOR.0 | Right::Edit.bit());

    /// Everything, including the account list — Админ.
    pub const ADMIN: Self = Self(Self::EDITOR.0 | Right::ManageUsers.bit());

    /// Both sets together. The only combiner this model needs.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn has(self, right: Right) -> bool {
        self.0 & right.bit() != 0
    }

    /// Raw bits, for tests and for wire projections that want the numbers.
    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Look at a document — always true for a verified account.
    pub const fn can_view(self) -> bool {
        self.has(Right::View)
    }

    pub const fn can_comment(self) -> bool {
        self.has(Right::Comment)
    }

    pub const fn can_invite(self) -> bool {
        self.has(Right::Invite)
    }

    /// Change a document. The right every fail-closed decision hinges on.
    pub const fn can_edit(self) -> bool {
        self.has(Right::Edit)
    }

    pub const fn can_manage_users(self) -> bool {
        self.has(Right::ManageUsers)
    }
}

impl std::fmt::Display for Rights {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 == 0 {
            return f.write_str("none");
        }
        let mut first = true;
        for right in Right::ALL {
            if !self.has(right) {
                continue;
            }
            if !first {
                f.write_str("+")?;
            }
            f.write_str(right.as_str())?;
            first = false;
        }
        Ok(())
    }
}

/// One of the seven product roles the operator defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProductRole {
    /// Админ. Everything, including the account list.
    Admin,
    /// UX/UI. Edits everything except the account list.
    UxUi,
    /// ПО — the operator's own abbreviation, kept as written.
    ///
    /// The source list is Russian (Админ / UX/UI / ПО / Аналитик / Фронт /
    /// Бэк / QA) and «ПО» may read either as the software role or as product
    /// owner. Nothing here turns on which: every role that is not `Admin` or
    /// `UxUi` carries [`Rights::CONTRIBUTOR`], so both readings are
    /// byte-identical in behaviour. Rename this variant when the operator
    /// settles the label — no permission changes with it.
    Software,
    /// Аналитик.
    Analyst,
    /// Фронт.
    Frontend,
    /// Бэк.
    Backend,
    /// QA.
    Qa,
}

impl ProductRole {
    /// Every role, in the operator's own order.
    pub const ALL: [Self; 7] = [
        Self::Admin,
        Self::UxUi,
        Self::Software,
        Self::Analyst,
        Self::Frontend,
        Self::Backend,
        Self::Qa,
    ];

    /// This role's canonical hub-facing slug.
    ///
    /// Canonical, not authoritative: the hub owns the vocabulary and
    /// [`Self::from_wire`] accepts the spellings it is likely to use. This is
    /// the spelling *this* code emits when it has to name a role back.
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::UxUi => "ux_ui",
            Self::Software => "software",
            Self::Analyst => "analyst",
            Self::Frontend => "frontend",
            Self::Backend => "backend",
            Self::Qa => "qa",
        }
    }

    /// The rights this role alone confers. See [`rights_for`].
    pub const fn rights(self) -> Rights {
        rights_for(self)
    }

    /// Parse one hub role string.
    ///
    /// Never panics and never guesses: an unknown, empty, or whitespace-only
    /// string is an error, and the caller decides what to do with it
    /// ([`RoleSet::from_wire`] keeps it, a route would log it). Case and
    /// separators are folded, so `UX/UI`, `ux-ui`, `ux ui` and `ux_ui` are
    /// one role — the wire spelling is the hub's business and this module
    /// refuses to depend on it.
    ///
    /// The alias lists are guesses about a vocabulary that lives outside this
    /// repository, and they are safe to guess at because a role can only ever
    /// land in its own bucket. Only `Admin` and `UxUi` are matched by names
    /// that are unambiguous on their own; every other alias leads to
    /// [`Rights::CONTRIBUTOR`], so a wrong guess costs someone a comment
    /// button and can never hand out an edit or the account list.
    pub fn from_wire(raw: &str) -> Result<Self, RoleWireError> {
        let normalized = normalize_wire(raw);
        if normalized.is_empty() {
            return Err(RoleWireError::Blank);
        }
        Ok(match normalized.as_str() {
            "admin"
            | "administrator"
            | "super_admin"
            | "superadmin"
            | "админ"
            | "администратор" => Self::Admin,
            "ux_ui" | "uxui" | "ux" | "ui" | "designer" | "ux_designer" | "ui_designer"
            | "дизайнер" => Self::UxUi,
            "software"
            | "software_engineer"
            | "software_development"
            | "po"
            | "product_owner"
            | "программист"
            | "по" => Self::Software,
            "analyst" | "business_analyst" | "systems_analyst" | "ba" | "sa" | "аналитик" => {
                Self::Analyst
            }
            "frontend" | "front_end" | "front" | "fe" | "фронт" | "фронтенд" => {
                Self::Frontend
            }
            "backend" | "back_end" | "back" | "be" | "бэк" | "бек" | "бэкенд" => {
                Self::Backend
            }
            "qa" | "quality_assurance" | "tester" | "тестировщик" => Self::Qa,
            // Fail closed: an unknown role is not an admin and not an editor.
            _ => {
                return Err(RoleWireError::Unknown {
                    raw: raw.trim().to_string(),
                })
            }
        })
    }
}

/// Why a role string could not be turned into a [`ProductRole`].
///
/// A named error rather than `Option` so the "we understood nothing" case can
/// be reported instead of looking like "the hub sent nothing" — the two mean
/// different things to whoever debugs this next, and a silent `None` is how a
/// vocabulary mismatch stays invisible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleWireError {
    /// The hub sent an empty or whitespace-only role string.
    Blank,
    /// A role this build does not know. Carries the original spelling.
    Unknown { raw: String },
}

impl std::fmt::Display for RoleWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blank => f.write_str("blank role name"),
            Self::Unknown { raw } => write!(f, "unknown product role: {raw}"),
        }
    }
}

impl std::error::Error for RoleWireError {}

/// Everything one account holds, as parsed from the hub's role list.
///
/// The type exists so the raw strings survive the trip: a `Vec<ProductRole>`
/// would be smaller, but it loses exactly the evidence needed when the hub
/// and this build disagree about a role name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleSet {
    known: Vec<ProductRole>,
    unrecognized: Vec<String>,
}

impl RoleSet {
    /// No roles at all — what every credential path that cannot supply them
    /// uses (see [`crate::access`] module docs, and `StaticVerifier`).
    pub const fn empty() -> Self {
        Self {
            known: Vec::new(),
            unrecognized: Vec::new(),
        }
    }

    /// Parse a hub role list, keeping what could not be parsed.
    ///
    /// Order of first appearance is preserved and repeats collapse, so a
    /// diagnostic reads in the order the hub wrote it.
    pub fn from_wire<I, S>(raw: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = Self::empty();
        for entry in raw {
            match ProductRole::from_wire(entry.as_ref()) {
                Ok(role) => set.push_once(role),
                // Recorded, not ignored: a role that arrived and was not
                // understood is the whole subject of #10.
                Err(RoleWireError::Blank) => set.note_unrecognized(entry.as_ref().trim()),
                Err(RoleWireError::Unknown { raw }) => set.note_unrecognized(&raw),
            }
        }
        set
    }

    /// One role, as a set.
    pub fn from_role(role: ProductRole) -> Self {
        let mut set = Self::empty();
        set.known.push(role);
        set
    }

    /// The roles this build recognised.
    pub fn roles(&self) -> &[ProductRole] {
        &self.known
    }

    /// Role strings the hub sent that this build does not know.
    ///
    /// They grant nothing — see the module docs — and exist to be logged.
    pub fn unrecognized(&self) -> &[String] {
        &self.unrecognized
    }

    pub fn contains(&self, role: ProductRole) -> bool {
        self.known.contains(&role)
    }

    /// True when no role was recognised.
    ///
    /// Both an empty hub list and a list of names this build does not know
    /// answer `true` here; [`Self::unrecognized`] tells the two apart. For an
    /// authorization decision that difference does not matter — neither
    /// confers anything.
    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// What this account may do: the union of its roles, floored at
    /// [`Rights::VIEW_ONLY`].
    ///
    /// The floor is the baseline every verified account has for its own
    /// workspace, and it is what keeps "no roles" from reading as "no
    /// access". It is not a privilege: everything that changes state or
    /// widens the account's circle stays `false` unless a recognised role
    /// grants it.
    pub fn rights(&self) -> Rights {
        rights_for_roles(&self.known).union(Rights::VIEW_ONLY)
    }

    fn push_once(&mut self, role: ProductRole) {
        if !self.known.contains(&role) {
            self.known.push(role);
        }
    }

    fn note_unrecognized(&mut self, raw: &str) {
        if !self.unrecognized.iter().any(|seen| seen == raw) {
            self.unrecognized.push(raw.to_string());
        }
    }
}

impl FromIterator<ProductRole> for RoleSet {
    fn from_iter<I: IntoIterator<Item = ProductRole>>(roles: I) -> Self {
        let mut set = Self::empty();
        for role in roles {
            set.push_once(role);
        }
        set
    }
}

/// The rights one product role confers. See the module table.
pub const fn rights_for(role: ProductRole) -> Rights {
    match role {
        ProductRole::Admin => Rights::ADMIN,
        ProductRole::UxUi => Rights::EDITOR,
        ProductRole::Software
        | ProductRole::Analyst
        | ProductRole::Frontend
        | ProductRole::Backend
        | ProductRole::Qa => Rights::CONTRIBUTOR,
    }
}

/// The union of several roles. An empty slice confers nothing — the
/// authenticated baseline is applied by [`RoleSet::rights`], not here, so
/// that this function stays a pure statement about roles.
pub fn rights_for_roles(roles: &[ProductRole]) -> Rights {
    roles
        .iter()
        .fold(Rights::NONE, |acc, role| acc.union(rights_for(*role)))
}

/// Fold a hub role string into a comparable form: trimmed, lower-cased, with
/// `/ - . space` flattened to `_` and runs of `_` collapsed.
///
/// Lower-casing is Unicode-aware on purpose — the operator's list is Russian
/// and a hub panel built from it may well send «Админ».
fn normalize_wire(raw: &str) -> String {
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

#[cfg(test)]
#[path = "access_tests.rs"]
mod tests;
