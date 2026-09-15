//! Account / sign-in state for a deployment's own accounts.
//!
//! The session itself lives on the daemon (`/api/auth/*`, one `accounts.db`
//! per deployment); this module carries only the display-state model plus a
//! dev-only fake-login seam, so the topbar avatar button, its dropdown, the
//! sign-in modal and the settings modal's Account tab can be exercised
//! without a daemon behind them.
//!
//! Same wasm32-clean discipline as the other `*_state` mirrors — plain
//! data only, no session/token material.

/// Signed-in / signed-out state for the current user. `SignedIn` carries
/// only display fields; the real OIDC client (not this crate) will own
/// any token/session material.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AccountState {
    #[default]
    Anonymous,
    SignedIn {
        display_name: String,
        username: String,
        /// The STABLE account key the deployment issued (`subject` in
        /// `/api/auth/status`) — the same value every access list, comment and
        /// grant records.
        ///
        /// Carried beside the username because they are different things and
        /// only one of them is an identity: a username is a handle an operator
        /// may change, and an access list that keyed on it would lose every
        /// grant on the day somebody was renamed. `None` for a session that
        /// predates the field, which is why callers that need an identity
        /// treat a missing one as "cannot say" rather than as a name.
        account_id: Option<String>,
    },
}

impl AccountState {
    pub fn is_signed_in(&self) -> bool {
        matches!(self, AccountState::SignedIn { .. })
    }

    /// Build display state from the authenticated profile.
    ///
    /// Usernames are the only values rendered with an `@` prefix. Older
    /// persisted credentials and rolling-upgrade servers may not provide one,
    /// so a missing or blank username falls back to the display name. Email is
    /// deliberately not accepted here: an address is not an account handle.
    pub fn signed_in_profile(display_name: String, username: Option<String>) -> Self {
        Self::signed_in_account(display_name, username, None)
    }

    /// The same, with the deployment's own account key.
    pub fn signed_in_account(
        display_name: String,
        username: Option<String>,
        account_id: Option<String>,
    ) -> Self {
        let username = username
            .and_then(|username| {
                let username = username.trim();
                (!username.is_empty()).then(|| username.to_string())
            })
            .unwrap_or_else(|| display_name.clone());
        Self::SignedIn {
            display_name,
            username,
            account_id: account_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
        }
    }

    /// The deployment's account key, when the session carried one.
    ///
    /// This — never the username — is what an access list, a grant or a section
    /// link is keyed by.
    pub fn account_id(&self) -> Option<&str> {
        match self {
            Self::SignedIn { account_id, .. } => account_id.as_deref(),
            Self::Anonymous => None,
        }
    }

    /// First uppercase character of the display name — the avatar-circle
    /// glyph when signed in. `Anonymous` (or an empty display name) has
    /// no letter to show; callers gate on [`Self::is_signed_in`] first.
    pub fn initial(&self) -> char {
        match self {
            AccountState::SignedIn { display_name, .. } => display_name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase())
                .unwrap_or('?'),
            AccountState::Anonymous => '?',
        }
    }

    /// Dev/test fake sign-in — the fast path gated by
    /// `OPENPENCIL_DEV_FAKE_LOGIN=1` (checked host-side; this crate reads
    /// no env vars so it stays wasm32-clean). Never reachable from the
    /// production sign-in button.
    pub fn dev_fake_signed_in() -> Self {
        AccountState::signed_in_profile("Fini".to_string(), Some("fini".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::AccountState;

    #[test]
    fn signed_in_profile_prefers_the_authenticated_username() {
        assert_eq!(
            AccountState::signed_in_profile("Kay Shen".to_string(), Some("kayshen_7".to_string())),
            AccountState::SignedIn {
                display_name: "Kay Shen".to_string(),
                username: "kayshen_7".to_string(),
            }
        );
    }

    #[test]
    fn signed_in_profile_falls_back_to_display_name_for_missing_or_blank_username() {
        for username in [None, Some(String::new()), Some(" \t ".to_string())] {
            assert_eq!(
                AccountState::signed_in_profile("Kay Shen".to_string(), username),
                AccountState::SignedIn {
                    display_name: "Kay Shen".to_string(),
                    username: "Kay Shen".to_string(),
                }
            );
        }
    }
}

/// One row in the signed-in account dropdown (anchored under the topbar
/// avatar button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountMenuRow {
    /// Opens the settings modal on the Account tab.
    Settings,
    /// Opens the hub portal's MCP-token page in a new tab. Only shown in
    /// the online/hub-served web editor (gated host-side); native never
    /// paints it.
    McpToken,
    /// Clears `AccountState` back to `Anonymous`.
    SignOut,
}

impl AccountMenuRow {
    pub const ALL: [AccountMenuRow; 3] = [
        AccountMenuRow::Settings,
        AccountMenuRow::McpToken,
        AccountMenuRow::SignOut,
    ];
}

/// Which control in the sign-in modal the cursor is over / has pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginModalButton {
    Close,
    /// The primary "Sign in with browser" action. With an auth backend
    /// linked it starts the device-login flow; stub builds show an
    /// honest "coming soon" note instead (see
    /// `AccountState::dev_fake_signed_in` for the dev-only fast path).
    SignIn,
}

/// Progress of an in-flight browser device-login, mirrored into the
/// sign-in modal's note row. Plain display data — the auth client owns
/// the actual protocol state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFlowStatus {
    /// Start request accepted; the system browser is being opened.
    WaitingBrowser,
    /// Browser open — waiting for the user to approve on the web page.
    WaitingApproval,
    /// Approved — exchanging the pairing for a device session.
    Exchanging,
    Failed(LoginFlowError),
}

/// Why a device-login attempt ended without a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFlowError {
    /// The user rejected the pairing on the approval page.
    Denied,
    /// The pairing expired before approval.
    Expired,
    /// Canceled locally (modal closed mid-flow).
    Canceled,
    /// Network / server / protocol trouble talking to the SSO service.
    Unavailable,
}
