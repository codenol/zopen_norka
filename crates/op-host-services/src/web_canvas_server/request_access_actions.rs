//! What a route asks for — the vocabulary the access decision answers in.
//!
//! A sibling of `request_access` rather than part of it for the 800-line cap,
//! and a coherent half to move: this is the LIST of actions and the words a
//! refusal uses to name one, while the spine is the decision that answers them.
//! The path is unchanged — `request_access::DocumentAction`, re-exported from
//! there — so no caller learns that the file moved.

/// One thing a route asks to do with a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocumentAction {
    /// Look at it: list the store, open a document, read its preview, ask
    /// about the recovery draft, read its comments.
    View,
    /// Take part in the conversation about it: open a thread on an element,
    /// reply, close or reopen one.
    ///
    /// Its own action rather than a spelling of [`DocumentAction::Edit`],
    /// because the operator's matrix has a level that the two split: the five
    /// contributor roles (ПО, Аналитик, Фронт, Бэк, QA) may comment and may not
    /// edit. Asking for `Edit` here would refuse the very people comments exist
    /// for, and asking for `View` would hand them to a guest who was given only
    /// a link to read. The right itself already exists in the model
    /// (`op_editor_core::access::Rights::can_comment`, which is what
    /// `Rights::CONTRIBUTOR` carries); this is the action that reaches it.
    Comment,
    /// Change it: create, save, autosave, rename, or write a draft.
    Edit,
    /// Add somebody else to it: write an entry on this document's access list.
    ///
    /// Its own action rather than a spelling of [`DocumentAction::Edit`],
    /// because the operator's matrix gives the invite right to a level that
    /// does not edit: the five contributor roles may add people and may not
    /// change the document. It is the right the Share dialog's Invite button
    /// asks about, and the same question the `/api/share/*` routes decide —
    /// asked here so there is one answer rather than two.
    Invite,
    /// Remove it from the store.
    Delete,
    /// Adopt the recovery draft as the open document.
    Restore,
    /// Bind a document that belongs to no account to the calling account (#46).
    ///
    /// An action of its own because its authority is the DEPLOYMENT rather than
    /// the document. The row it names has no owner by definition — that is what
    /// makes it claimable — so there is no ownership to compare and no access
    /// list to consult: the question is whether this caller may decide, for the
    /// deployment, who an unattributed file belongs to. That is
    /// [`Rights::can_manage_users`](op_editor_core::access::Rights::can_manage_users),
    /// the right the account list is kept for, and `RequestAccess::decide`
    /// answers it through `RequestAccess::decide_account_administration` so the
    /// two cannot drift apart.
    ///
    /// It is what the routes use to RECOVER documents a deployment inherited —
    /// files placed in the documents directory by hand, and rows left
    /// unattributed by a daemon that ran without accounts. Both are unreachable
    /// online by design (`RequestAccess::reaches_stored_document` refuses a NULL
    /// owner, because handing an unattributed file to whoever names its key is
    /// the leak that check closes), so the recovery has to be a deliberate act
    /// by an authority rather than a side effect of opening or saving one.
    Claim,
}

impl DocumentAction {
    /// Every action, in ascending authority.
    pub const ALL: [Self; 7] = [
        Self::View,
        Self::Comment,
        Self::Invite,
        Self::Edit,
        Self::Delete,
        Self::Restore,
        Self::Claim,
    ];

    /// Stable name, for logs and for a refusal that has to say what was asked.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Comment => "comment",
            Self::Invite => "invite",
            Self::Edit => "edit",
            Self::Delete => "delete",
            Self::Restore => "restore",
            Self::Claim => "claim",
        }
    }

    /// Whether this action changes stored state.
    ///
    /// A comment does, and it is worth being exact about which state: it writes
    /// a row, and it writes it to the conversation rather than to the document
    /// (see `super::comment_routes` — no comment route touches the editor, the
    /// document's version or its file). Reading is still the only action that
    /// changes nothing, which is what this predicate is for.
    pub const fn is_write(self) -> bool {
        !matches!(self, Self::View)
    }
}
