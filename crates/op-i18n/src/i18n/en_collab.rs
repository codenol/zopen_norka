//! Collaboration UI strings.

pub fn lookup(key: &str) -> Option<&'static str> {
    Some(match key {
        "collab.topbar.collaborate" => "Collaborate",
        "collab.topbar.starting" => "Starting collaboration…",
        "collab.topbar.joining" => "Joining…",
        "collab.topbar.authenticating" => "Authenticating…",
        "collab.topbar.connected" => "Connected",
        "collab.topbar.reconnecting" => "Reconnecting…",
        "collab.topbar.readOnly" => "Read-only",
        "collab.topbar.ended" => "Session ended",
        "collab.topbar.participants" => "{{count}} participants",
        // Cause-neutral on purpose: this screen covers BOTH a build with no
        // collaboration runtime and a deployment that cannot answer a sign-in
        // (#150), and the panel cannot tell them apart — see
        // `CollabAvailability::Unavailable`. The wording names neither.
        "collab.topbar.unavailable" => "Collaboration is not available here",
        "collab.action.start" => "Create session",
        "collab.action.join" => "Join session",
        "collab.home.hint" => "Create a new session or join with an invite.",
        "collab.create.choose" => "Choose how other people can connect.",
        "collab.action.leave" => "Leave session",
        "collab.action.retry" => "Retry",
        "collab.action.cancel" => "Cancel",
        "collab.action.connect" => "Connect",
        "collab.action.copyInviteCode" => "Copy invite code",
        "collab.action.copyInviteLink" => "Copy invite link",
        "collab.action.findNearby" => "Find nearby",
        "collab.join.code" => "Invite code or IP address",
        "collab.join.codePlaceholder" => "A2C4E6G8J0 or 192.168.1.8:43120",
        "collab.join.publicHint" => "Invite codes connect securely over the internet.",
        "collab.join.nearby" => "Nearby sessions",
        "collab.session.invite" => "Public invite code",
        "collab.session.preparingInvite" => "Preparing a secure invite…",
        "collab.session.publicReady" => "Public relay is ready",
        "collab.session.region" => "Home relay region",
        "collab.connection.lan" => "Local network",
        "collab.connection.relay" => "Public relay",
        "collab.region.china" => "China",
        "collab.region.global" => "Global",
        "collab.error.inviteUnavailable" => "This invite is unavailable or has expired.",
        "collab.error.relayUnavailable" => "The public relay is temporarily unavailable.",
        "collab.error.inviteInvalid" => "This invite code is invalid. Check it and try again.",
        "collab.error.inviteExpired" => "This invite code has expired. Ask the owner for a new one.",
        "collab.error.relayNotConfigured" => "No public relay is configured on this device.",
        "collab.error.regionUnavailable" => "The invite's home relay region is unavailable.",
        "collab.error.secureKeyUnavailable" => "The device's secure key is unavailable. Check system keychain access and try again.",
        "collab.error.rateLimited" => "Too many connection attempts. Try again shortly.",
        "collab.action.discardPending" => "Discard pending edit",
        "collab.action.saveAsFork" => "Save as a fork",
        "collab.action.approveEditor" => "Approve editor",
        "collab.action.approveViewer" => "Approve viewer",
        "collab.action.rejectAdmission" => "Reject",
        "collab.admission.request" => "An authenticated peer is requesting access.",
        "collab.join.title" => "Join a collaboration session",
        "collab.join.discovering" => "Looking for sessions on your local network…",
        "collab.join.noSessions" => "No local sessions found",
        "collab.join.address" => "IP address and port",
        "collab.join.addressPlaceholder" => "192.168.1.8:43120",
        "collab.join.authenticating" => "Verifying the secure session…",
        "collab.join.incompatible" => "This session uses an incompatible version",
        "collab.join.signInRequired" => "Sign in to start or join a session",
        "collab.join.signInUnavailable" => "Collaboration needs an account, and no sign-in is available here.",
        "collab.session.title" => "Collaboration",
        "collab.session.name" => "Session: {{name}}",
        "collab.session.shareAddress" => "Share address",
        "collab.session.role.owner" => "Owner",
        "collab.session.role.editor" => "Editor",
        "collab.session.role.viewer" => "Viewer",
        "collab.session.pending" => "Waiting for the owner to confirm your edit…",
        "collab.status.disconnectedReadOnly" => {
            "Connection lost. Editing is paused while Norka reconnects."
        }
        "collab.status.ticketExpired" => "Your collaboration sign-in expired. Sign in again.",
        "collab.status.ownerLeft" => {
            "The owner left, so this session has ended. You can save a separate copy."
        }
        "collab.status.epochChanged" => {
            "The owner started a new session. Your pending edit was not submitted."
        }
        "collab.status.undoConflict" => {
            "That change cannot be undone because someone edited the same field later."
        }
        "collab.status.unsupportedEdit" => {
            "That edit is not supported in collaboration yet and was not applied."
        }
        "collab.status.profileUnavailable" => "Profile image unavailable; showing initials.",
        "collab.reject.staleBase" => "The document changed first. Catching up before retrying.",
        "collab.reject.readOnly" => "You have view-only access to this session.",
        "collab.reject.unsupported" => "The owner does not support that edit.",
        "collab.reject.conflict" => "That edit conflicts with a newer change.",
        "collab.reject.conflictDetail" => "Discarded: {{fields}} on “{{node}}”.",
        "collab.action.reapply" => "Reapply my edit",
        "collab.reject.resourceLimit" => "That edit is too large for this session.",
        "collab.reject.authentication" => "Your collaboration authorization is no longer valid.",
        "collab.reject.unknown" => "The owner rejected that edit.",
        "collab.gate.pages" => "Page changes are not supported in collaboration yet.",
        "collab.gate.pageBackground" => {
            "Page background changes are not supported in collaboration yet."
        }
        "collab.gate.variablesThemes" => {
            "Variables and themes are not supported in collaboration yet."
        }
        "collab.gate.components" => {
            "Component registry changes are not supported in collaboration yet."
        }
        "collab.gate.uikit" => "UIKit changes are not supported in collaboration yet.",
        "collab.gate.externalAssets" => {
            "Images, SVG, HTML, and other external assets cannot be imported in collaboration yet."
        }
        "collab.gate.clipboardPaste" => {
            "Pasting document content is not supported in collaboration yet."
        }
        "collab.gate.duplicate" => "Duplicating nodes is not supported in collaboration yet.",
        "collab.gate.bulkWrite" => "Bulk document changes are disabled during collaboration.",
        "collab.gate.replaceDocument" => {
            "Replacing the whole document is disabled during collaboration."
        }
        "collab.gate.rootMetadata" => {
            "Document metadata changes are not supported in collaboration yet."
        }
        "collab.gate.typography" => {
            "Typography changes are not supported in collaboration yet."
        }
        "collab.gate.effects" => "Effects are not supported in collaboration yet.",
        "collab.gate.visibilityLocking" => {
            "Visibility and locking changes are not supported in collaboration yet."
        }
        "collab.gate.nodeReplacement" => {
            "Replacing a node is not supported in collaboration yet."
        }
        "collab.gate.nodeProperty" => {
            "That node property is not supported in collaboration yet."
        }
        "collab.gate.nodeKind" => "That node type is not supported in collaboration yet.",
        "collab.gate.sessionTransition" => {
            "Editing is paused while the collaboration session is being prepared."
        }
        "collab.gate.readOnly" => "This collaboration session is read-only.",
        "collab.gate.pendingEdit" => {
            "Wait for your pending edit to be confirmed before making another change."
        }
        "collab.gate.aiMcp" => "AI and MCP document writes are disabled during collaboration.",
        "collab.gate.undoUnavailable" => {
            "Global undo is disabled in collaboration. Only confirmed personal changes can be undone."
        }
        "collab.gate.redoUnavailable" => "Redo is not available in collaboration yet.",
        "collab.gate.ownerOnlySave" => "Only the owner can save the shared source file.",
        "collab.gate.leaveSessionFirst" => {
            "Leave the collaboration session before replacing or opening another document."
        }
        "collab.status.localEditPreserved" => {
            "The remote version was applied. Use Undo to restore your local changes."
        }
        "collab.a11y.participant" => "{{name}}, {{role}}",
        "collab.a11y.remoteCursor" => "{{name}}'s cursor",
        "icon.catalogLoading" => "Icon catalog is still loading…",
        "sceneTemplate.documentUnavailable" => "That template's document could not be loaded. Try again.",
        "sceneTemplate.deleteFailed" => "That template could not be deleted. Try again.",
        "sceneTemplate.item.soundingNavyDeck.title" => "Sounding Chart · Strategy Deck",
        "sceneTemplate.item.soundingNavyDeck.summary" => "Chart-paper white against steel and ink blue: every slide leads with a conclusion and backs it with one sourced reading. Depth-profile bars and a sounding-track agenda carry all seven pages, for consulting deliverables and strategy reviews.",
        "sceneTemplate.item.tidemarkSlateDeck.title" => "Tidemark Slate · Data Review Deck",
        "sceneTemplate.item.tidemarkSlateDeck.summary" => "Overview tiles, a trend line, a detail table, a risk list and a swimlane roadmap on slate grey — seven pages that walk one full data review, for quarterly retrospectives and metric meetings.",
        "sceneTemplate.item.banxinRuleDeck.title" => "Banxin Rule · Chinese Typography Deck",
        "sceneTemplate.item.banxinRuleDeck.summary" => "Built on the classical Chinese page: type block, column rules, marginal notes and a fishtail folio. Body lines stay under thirty characters, with facing columns and a quotation page that reward reading, for lectures and seminars.",
        "sceneTemplate.item.gridpaperGraphiteDeck.title" => "Graphite Grid · Academic Defence Deck",
        "sceneTemplate.item.gridpaperGraphiteDeck.summary" => "Graphite type on grid paper: problem and gap, method, two pages of results, limitations, conclusion and references — eight pages in defence order, for proposals and thesis defences.",
        "sceneTemplate.item.dossierLinenDeck.title" => "Linen Dossier · Memo Deck",
        "sceneTemplate.item.dossierLinenDeck.summary" => "A linen-paper dossier from cover sheet through background, current data, analysis and option comparison to the resolution — eight pages that read as a standalone memo, for decision reviews.",
        "sceneTemplate.item.ledgerTickDeck.title" => "Ledger Tick · Competitive Matrix Deck",
        "sceneTemplate.item.ledgerTickDeck.summary" => "Scoring criteria, the main matrix, quantile scales and a gap-versus-strength read-out on ledger ruling — seven pages that tell a competitive comparison like a balanced account, for vendor selection and market analysis.",
        "sceneTemplate.item.brandConceptSheet.title" => "Brand Concept Sheet",
        "sceneTemplate.item.brandConceptSheet.summary" => "One horizontal review board for the primary lockup, construction rationale, monochrome reversals and minimum-size survival — built for first-round concept handoff.",
        "sceneTemplate.item.logoQaBoard.title" => "Logo Fusion QA Board",
        "sceneTemplate.item.logoQaBoard.summary" => "Four review cells check shared semantic load, structural dependence, silhouette unity and whether the secondary reading feels discovered rather than attached.",
        "account.mcpToken" => "MCP Tokens",
        "menu.saveAsTemplate" => "Save As Template…",
        "menu.saveAsTemplate.saved" => "Saved as a template",
        "menu.saveAsTemplate.failed" => "Could not save the template",
        "assetCenter.template.mine" => "My templates",
        "assetCenter.template.builtIn" => "Built-in templates",
        "ai.designProgress.detail.failureReason" => "Reason: {{reason}}",
        "ai.designProgress.detail.noDiagnostic" => "The agent failed without returning an error description.",
        "ai.designProgress.detail.noResult" => "The agent stopped before returning a result for this section.",
        "ai.designProgress.detail.connectionClosed" => "The agent connection closed before this section returned a result.",
        "ai.designProgress.detail.stoppedByUser" => "Stopped by the user before this section completed.",
        "builtin.modelsOnePerLine" => "Model IDs, one per line",
        "builtin.typeModelManually" => "Failed to fetch the model list. Enter model IDs manually, one per line.",
        "settings.provider.deepSeekHarness" => "DeepSeek Harness models",
        "settings.agents.deleteProvider" => "Delete provider",
        "chat.mcpRequired" => "{cli} needs the Norka MCP integration enabled in Settings",
        "designMd.tab.document" => "Document",
        "designMd.tab.rules" => "Rules",
        "designMd.rules.filter.all" => "All",
        "designMd.rules.filter.global" => "Global",
        "designMd.rules.filter.components" => "Components",
        "designMd.rules.filter.local" => "Local",
        "designMd.rules.new" => "New rule",
        "designMd.rules.empty" => "No rules match this filter",
        "designMd.rules.kind.do" => "Do",
        "designMd.rules.kind.dont" => "Don't",
        "designMd.rules.kind.require" => "Require",
        "designMd.rules.kind.avoid" => "Avoid",
        "designMd.rules.source.library" => "Library",
        "designMd.rules.source.local" => "Local",
        "designMd.rules.source.override" => "Override",
        "designMd.rules.scope.global" => "Global",
        "designMd.rules.scope.component" => "Component type",
        "designMd.rules.form.title" => "Rule title",
        "designMd.rules.form.instruction" => "Instruction",
        "designMd.rules.form.save" => "Save",
        "designMd.rules.form.cancel" => "Cancel",
        "designMd.rules.form.incomplete" => "Title and instruction are required",
        "designMd.rules.form.condition" => "When",
        "designMd.rules.form.conditionHint" => "Optional condition",
        "designMd.rules.aiInstructions" => "AI instructions",
        "recipes.title" => "Recipes",
        "ai.rulesActive" => "Rules · {{count}}",
        "topbar.allFiles" => "All files",
        "layerMenu.copyLink" => "Copy link",
        "layerMenu.linkCopied" => "Link copied to clipboard",
        "topbar.saved" => "Saved",
        "recovery.banner.title" => "Unsaved work found: {{when}}",
        "recovery.banner.restore" => "Restore",
        "recovery.banner.discard" => "Discard",
        "copy.status.behind" => "The daemon's newest change is not on this canvas (canvas v{{shown}}, daemon v{{daemon}})",
        "copy.status.conflict" => "Sync conflict — this canvas is at v{{shown}} and the daemon at v{{daemon}}, so the canvas stopped following the daemon",
        "copy.status.localEdits" => "This canvas has edits the daemon has not confirmed (canvas v{{shown}})",
        "copy.status.silent" => "The daemon is not answering — this canvas is the copy this browser kept {{when}}",
        "copy.status.silentNoCopy" => "The daemon is not answering — this canvas has not received the daemon's document",
        "comments.panel.title" => "Comments",
        "comments.panel.loading" => "Loading comments…",
        "comments.panel.empty" => "No comments on this page yet",
        "comments.panel.error" => "Comments could not be loaded",
        "comments.panel.replyCount" => "{{count}} replies",
        "comments.panel.more" => "{{count}} more",
        "comments.panel.open" => "Open",
        "comments.panel.resolved" => "Resolved",
        "comments.panel.pin" => "Pin {{number}}",
        "comments.panel.unpinned" => "No pin",
        "comments.panel.otherPages" => "{{count}} on other pages",
        "comments.tool.hint" => "Click the canvas to add a comment",
        "comments.composer.placeholder" => "Write a comment…",
        "comments.composer.replyPlaceholder" => "Reply…",
        "comments.composer.newTitle" => "New comment",
        "comments.composer.send" => "Send",
        "comments.action.resolve" => "Resolve",
        "comments.action.reopen" => "Reopen",
        "comments.action.close" => "Close",
        "comments.action.showList" => "All comments",
        "comments.author.you" => "You",
        "comments.author.local" => "Local operator",
        "comments.author.unknown" => "Unknown",
        "comments.error.refused" => "You may not change this thread",
        "comments.error.gone" => "This thread no longer exists",
        "comments.error.rejected" => "The comment was rejected",
        "comments.error.transport" => "The comment could not be sent",
        "comments.error.unsaved" => "Save the document before commenting",
        "comments.resolvedBy" => "Resolved by {{name}}",
        "account.entry.signInTitle" => "Sign in to Norka",
        "account.entry.signInSubtitle" => "Enter your account name and password.",
        "account.entry.username" => "Username",
        "account.entry.password" => "Password",
        "account.entry.signIn" => "Sign in",
        "account.entry.signingIn" => "Signing in…",
        "account.entry.needsFirstAdminTitle" => "No accounts yet",
        "account.entry.needsFirstAdminBody" => "This deployment has no accounts. Set NORKA_ADMIN_USERNAME and NORKA_ADMIN_PASSWORD and restart, or run `op admin create`.",
        "account.entry.inviteTitle" => "Accept your invitation",
        "account.entry.inviteSubtitle" => "Choose a name and a password for your new account.",
        "account.entry.inviteConfirm" => "Repeat password",
        "account.entry.inviteDisplayName" => "Display name (optional)",
        "account.entry.acceptInvite" => "Create account",
        "account.entry.acceptingInvite" => "Creating account…",
        "account.entry.errorRejected" => "That name and password do not open an account.",
        "account.entry.errorDisabled" => "This account is disabled and may not sign in.",
        "account.entry.errorUnprovisioned" => "This deployment has no accounts yet.",
        "account.entry.errorUnavailable" => "The account service is unavailable right now. Try again.",
        "account.entry.errorInvalid" => "That name cannot be used as written.",
        "account.entry.errorEmptyFields" => "Fill in every field.",
        "account.entry.errorPasswordMismatch" => "The two passwords do not match.",
        "account.entry.errorTooManyAttempts" => "Too many failed attempts. Try again in {{seconds}} seconds.",
        "account.entry.errorTooManyAttemptsUnstated" => "Too many failed attempts. Wait a while before trying again.",
        "account.entry.errorWeakPassword" => "The password is too weak: at least 12 characters, no repeats, and it must not contain the account name.",
        "account.entry.errorUsernameTaken" => "That username is already taken.",
        "account.entry.errorInviteNotFound" => "This invitation does not exist.",
        "account.entry.errorInviteExpired" => "This invitation has expired. Ask for a new one.",
        "account.entry.errorInviteAccepted" => "This invitation has already been used.",
        "share.title" => "Share this document",
        "share.action.copyLink" => "Copy link",
        "share.action.copyInviteLink" => "Copy",
        "share.action.invite" => "Invite",
        "share.invite.label" => "Add comma separated emails to invite",
        "share.invite.placeholder" => "name@example.com, account name",
        "share.invite.noMail" => "This deployment sends no email. An invitation creates a link that you pass on yourself.",
        "share.section.whoHasAccess" => "Who has access",
        "share.row.you" => "You",
        "share.row.owner" => "Owner — everything on this document",
        "share.row.anyoneWithLink" => "Anyone with the link",
        // One caption per level, because the row hands out four different
        // powers and this sentence is the only place the row can say which
        // (#131). Every one keeps the deployment's sign-in caveat: without it
        // "anyone with the link" reads as the open internet.
        "share.row.anyoneWithLink.caption.admin" => "Anyone who can sign in may edit this document and re-share it.",
        "share.row.anyoneWithLink.caption.editor" => "Anyone who can sign in may edit this document.",
        "share.row.anyoneWithLink.caption.commenter" => "Anyone who can sign in may comment and invite others.",
        "share.row.anyoneWithLink.caption.viewer" => "Anyone who can sign in may only look at this document.",
        "share.row.invitedBy" => "Added by {{account}}",
        "share.row.invitedByYou" => "Added by you",
        "share.row.invitedUnknown" => "Added before this deployment recorded who",
        "share.row.more" => "+{{count}} more with access",
        "share.level.admin" => "Admin — full access",
        "share.level.admin.hint" => "May edit this document and change who else can open it.",
        "share.level.editor" => "Can edit",
        "share.level.editor.hint" => "May view, comment, invite other people and change the document.",
        "share.level.commenter" => "Can comment and invite",
        "share.level.commenter.hint" => "May view, comment and add other people — but not change the document.",
        "share.level.viewer" => "Can view",
        "share.level.viewer.hint" => "May open the document and read it. Nothing else.",
        "share.link.on" => "On",
        "share.link.off" => "Off",
        "share.notice.issued" => "{{count}} invitations created. No email was sent — copy the link and pass it on.",
        "share.notice.granted" => "{{count}} accounts now have access. No email was sent.",
        "share.notice.copiedLink" => "Link copied",
        "share.notice.linkAccess" => "Link access is {{state}} — {{level}}",
        "share.notice.listUnavailable" => "Could not read the access list. This is not an empty list.",
        "share.notice.noLink" => "This document has no link yet — it has not been saved anywhere.",
        "share.invite.refused.empty" => "Enter at least one email or account name.",
        "share.invite.refused.tooMany" => "Too many addresses ({{count}}). At most 20 can be invited at once.",
        "share.invite.refused.noRight" => "Your role does not allow adding people to this document.",
        "share.invite.refused.levelAboveOwn" => "You cannot grant {{level}}: you hold {{own}}.",
        "share.invite.refused.emailNeedsAdmin" => "Inviting by email creates an account, and only an administrator may do that.",
        "share.invite.refused.alreadyHasAccess" => "{{account}} already has access.",
        "share.invite.refused.unknownAccount" => "There is no account called {{account}}. Check the spelling, or invite by email.",
        "share.invite.refused.shareLimit" => "This document is already shared with {{limit}} accounts. Remove one to add another.",
        "share.invite.refused.server" => "The request could not be completed.",
        "section.title" => "Section",
        "section.readFailed" => "Could not read this section",
        "section.analytics" => "Built from",
        "section.analytics.attach" => "Attach analytics…",
        "section.analytics.none" => "No analytics attached",
        "section.state.inSync" => "In sync",
        "section.state.analyticsMoved" => "The analytics has changed since",
        "section.state.mockupsMoved" => "The screens have changed since",
        "section.state.bothMoved" => "Both the analytics and the screens have changed since",
        "section.state.assetMissing" => "The analytics document is gone",
        "section.state.notReadable" => "You cannot read this analytics document",
        "section.state.checkFailed" => "The analytics document could not be checked",
        "section.notRead" => "Not read yet",
        "section.summary" => "What this section is",
        "section.summary.whatItIs" => "What this is",
        "section.summary.whereToLook" => "Where to look",
        "section.summary.useCases" => "Use cases",
        "section.summary.whatToCheck" => "What to check",
        "section.summary.empty" => "Nothing written down yet",
        "section.flows" => "Flows",
        "section.flows.none" => "No flows yet",
        "section.flows.stepCount" => "{{count}} steps",
        "share.footer.session" => "Live collaboration session…",
        "share.topbar.share" => "Share",
        "share.topbar.level" => "{{level}}",
        // Issue #63 — the reference card beside the canvas: which of the two
        // pictures is the one the user asked the design to match.
        "ai.referenceView.title" => "Reference",
        // The file browser's own strings. The screen used to paint
        // English literals; it is the app's front door now, so its
        // labels come from here like every other surface's.
        "files.title" => "Files",
        "files.new" => "New file",
        "files.search" => "Search files",
        "files.loading" => "Loading files…",
        "files.empty" => "No files yet — create one to get started",
        "files.noMatch" => "Nothing matches that search",
        "files.rename" => "Rename",
        "files.delete" => "Delete",
        "files.editedRecently" => "Edited recently",
        "files.editedJustNow" => "Edited just now",
        "files.editedMinutes" => "Edited {n} min ago",
        "files.editedHours" => "Edited {n} h ago",
        "files.editedDays" => "Edited {n} d ago",
        // The property panel's section tabs: «Обзор» is the analytics half of a
        // section (what it was built from, what it says, its flows); «Дизайн»
        // beside it carries only design. Issue #59's panel, split in two.
        "rightPanel.overview" => "Overview",
        _ => return None,
    })
}
