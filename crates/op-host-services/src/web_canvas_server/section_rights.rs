//! Who may change what a section carries.
//!
//! [`super::request_access::RequestAccess::decide`] answers "may this caller do
//! this to the DOCUMENT", and its answer is a right on the document: view,
//! comment, edit. A section carries things that are not the screens — the
//! analytics it was built from, what the analytics means, the UX flow drawn from
//! it — and the operator's rule is that the right to change one of those comes
//! from **what is being changed**, not from who is asking:
//!
//! | What is changed | Who may |
//! | --- | --- |
//! | Analytics (the markdown) | admin, UX/UI, analyst |
//! | The summary — what this is, where to look, use cases, what to check | analyst (and admin) |
//! | The UX flow | UX/UI (and admin) |
//! | The screens, including building them into a section from analytics | admin, UX/UI |
//! | Reading any of it | anyone who may read the document |
//!
//! ## Why this is not a wider `Rights` bitmask
//!
//! `op_editor_core::access::Rights` is the authority an account holds over a
//! DOCUMENT, and it is deliberately untouched by this module: nothing here adds
//! a bit to a role, and `Rights::can_edit` still answers `false` for an analyst.
//! That is not squeamishness. Adding an "edit analytics" bit to
//! `rights_for(ProductRole::Analyst)` would widen the analyst's document rights
//! as a side effect, and whether the analyst role should hold more than viewing
//! is the operator's decision to make, not a consequence of this feature. So the
//! subject matrix lives beside the document matrix, asks its own question, and
//! the two answers are combined by the caller: a write to an analytics ASSET is
//! [`SubjectAction::AnalyticsWrite`] **and** the asset-ownership check, and a
//! write to a screen is [`DocumentAction::Edit`] and nothing else.
//!
//! ## Why ownership does not answer the summary
//!
//! [`RequestAccess::decide`] grants an owner everything on their own document,
//! deliberately — the file is theirs to work on. That rule is NOT extended here,
//! and the asymmetry is the whole point of the table above: the summary is a
//! READING of the analytics, so the person who owns the analytics owns it. A
//! designer whose document it is may read the summary and may not rewrite it,
//! which is the right way round — the person who wrote down why should not have
//! their reasoning quietly edited by the person building from it. Screens are
//! the other side of the same line and keep the owner rule, because a screen is
//! the owner's document.
//!
//! ## The agent confers nothing
//!
//! Every decision here is taken with the CALLER's access. An agent turn that
//! edits the document or an asset must be authorized with the access of whoever
//! asked for it, and the subject of the edit; there is no service identity and
//! no code path in this module that grants anything to "the agent". That is what
//! closes the hole #33 had to close for the AI turn: an analyst asking for
//! screens to be built gets [`AccessRefusal::ReadOnly`], because building them
//! is [`DocumentAction::Edit`] and an analyst does not hold it — routing the
//! request through an agent must not be a way round the matrix.
//!
//! ## What this does not yet consult
//!
//! A document GRANT — the share level a visitor was given, whose effective
//! authority is the intersection of that level with their roles
//! (`op_editor_core::share_access`) — is not read here, because when this was
//! written the daemon had no grant to read (issue #106). The question that
//! leaves open is narrow and worth stating: the summary is stored on the
//! document, so a "can view" grant arguably has to cap
//! [`SubjectAction::SummaryWrite`], while [`SubjectAction::AnalyticsWrite`]
//! touches a separate asset that a document grant has no business capping.
//! Whoever lands the grant plumbing should decide it there rather than discover
//! it here.

use op_editor_core::access::ProductRole;

use super::request_access::{AccessRefusal, DocumentAction, RequestAccess};

/// One of the subjects a section carries.
///
/// An action is added when its answer genuinely differs from every existing
/// one, not to describe what a handler happens to do — the same rule
/// [`DocumentAction`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubjectAction {
    /// Read the analytics a section references.
    AnalyticsRead,
    /// Load, edit or replace the analytics markdown, and link or unlink it.
    AnalyticsWrite,
    /// Change the section's summary: what this is, where to look, use cases,
    /// what to check.
    SummaryWrite,
    /// Change the section's UX flows.
    UxFlowWrite,
    /// Change the screens — the same act as editing the document, asked about
    /// by subject so a caller does not have to know which document action
    /// "building a screen here" is.
    MockupsWrite,
}

impl SubjectAction {
    /// Every action, in ascending authority.
    pub const ALL: [Self; 5] = [
        Self::AnalyticsRead,
        Self::AnalyticsWrite,
        Self::SummaryWrite,
        Self::UxFlowWrite,
        Self::MockupsWrite,
    ];

    /// Stable name, for logs and for a refusal that has to say what was asked.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnalyticsRead => "analytics-read",
            Self::AnalyticsWrite => "analytics-write",
            Self::SummaryWrite => "summary-write",
            Self::UxFlowWrite => "ux-flow-write",
            Self::MockupsWrite => "mockups-write",
        }
    }

    /// Whether this action changes stored state.
    pub const fn is_write(self) -> bool {
        !matches!(self, Self::AnalyticsRead)
    }

    /// The roles whose work this subject is. See the module table.
    fn roles(self) -> &'static [ProductRole] {
        match self {
            Self::AnalyticsRead => &[],
            Self::AnalyticsWrite => &[ProductRole::Admin, ProductRole::UxUi, ProductRole::Analyst],
            Self::SummaryWrite => &[ProductRole::Admin, ProductRole::Analyst],
            Self::UxFlowWrite => &[ProductRole::Admin, ProductRole::UxUi],
            Self::MockupsWrite => &[ProductRole::Admin, ProductRole::UxUi],
        }
    }
}

impl<'a> RequestAccess<'a> {
    /// May this caller change (or read) what a section carries?
    ///
    /// The order is the one [`RequestAccess::decide`] established, and for the
    /// same reason: reach first, so a caller with no business with the document
    /// learns nothing about it, and so a stranger gets the same answer for a
    /// read as for a write.
    pub fn decide_subject(&self, action: SubjectAction) -> Result<(), AccessRefusal> {
        // A deployment with no accounts has nothing to decide: one operator,
        // their own files. This is the branch that lets the section work land
        // without touching local editing.
        if !self.mode().is_online() {
            return Ok(());
        }
        // Question one — the document this section lives in. A section is part
        // of a document, so whoever may not open the document may not read what
        // it says about itself either.
        if !self.reaches_document() {
            return Err(AccessRefusal::NotShared);
        }
        // Question two — the subject. Reading needs only question one: the point
        // of analytics is that a reader who was not in the conversation can get
        // to the reasoning without asking anybody.
        if action == SubjectAction::AnalyticsRead {
            return Ok(());
        }
        match action {
            // A screen is the document, so the document's own answer stands —
            // including the owner rule `decide` states.
            SubjectAction::MockupsWrite => self.decide(DocumentAction::Edit),
            // Everything else is answered by the subject and never by
            // ownership; see the module docs for why that asymmetry is the
            // point rather than an oversight.
            _ => {
                if self
                    .caller_roles()
                    .iter()
                    .any(|role| action.roles().contains(role))
                {
                    Ok(())
                } else {
                    Err(AccessRefusal::ReadOnly)
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "section_rights_tests.rs"]
mod tests;
