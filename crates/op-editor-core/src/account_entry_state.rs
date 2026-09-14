//! Getting into an account: the password form and the invitation form.
//!
//! ## Why this is not `account_state.rs`
//!
//! [`crate::account_state::AccountState`] is who you ARE (a display name, a
//! handle, a signed-in flag) and it is what both hosts paint from. This module
//! is the other half: what a signed-out visitor is currently TYPING, and why
//! their last attempt failed. Keeping them apart is what lets a host repaint
//! the account chrome without carrying a half-finished password around with it.
//!
//! ## Why the password leaves this state at the moment it is sent
//!
//! A form that keeps the typed password in editor state keeps it for the whole
//! session: every snapshot, every dirty-state clone, and every repaint holds a
//! copy of a secret that has already been used. [`AccountEntryState::begin_sign_in`]
//! therefore TAKES the password out of the form (leaving the field empty) and
//! hands it to a [`SignInRequest`] that lives exactly as long as the request
//! being built. Nothing that can be cloned, compared, or logged ever holds it
//! again — [`SignInRequest`] has no `Clone`, and its `Debug` prints no secret.
//!
//! ## Why the errors are typed
//!
//! The daemon answers with a machine-readable `error` code and an English
//! sentence. The sentence is for a log; a browser shows a translated string,
//! and it must show the SAME one for a wrong name and a wrong password (the
//! store deliberately returns one outcome for both, so that a form cannot be
//! used to discover which account names exist). Typing the answers here — and
//! mapping them to one key each — is what keeps that property true at the
//! surface that a person actually reads.

/// Which field of an entry form holds the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountField {
    /// Sign-in form: the account name.
    Username,
    /// Sign-in form: the password.
    Password,
    /// Invitation form: the account name being created.
    InviteUsername,
    /// Invitation form: the password being chosen.
    InvitePassword,
    /// Invitation form: the same password again.
    InvitePasswordConfirm,
    /// Invitation form: the name shown to other people. Optional.
    InviteDisplayName,
}

impl AccountField {
    /// The sign-in form's fields, top to bottom.
    pub const SIGN_IN: [AccountField; 2] = [Self::Username, Self::Password];
    /// The invitation form's fields, top to bottom.
    pub const INVITE: [AccountField; 4] = [
        Self::InviteUsername,
        Self::InvitePassword,
        Self::InvitePasswordConfirm,
        Self::InviteDisplayName,
    ];

    /// Whether the field's text is a secret.
    ///
    /// Painted as bullets and never echoed into an error, a log, or a request
    /// body that outlives the submit.
    pub const fn is_secret(self) -> bool {
        matches!(
            self,
            Self::Password | Self::InvitePassword | Self::InvitePasswordConfirm
        )
    }
}

/// Which account-entry surface a state shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountEntryMode {
    /// Nothing to show: no status yet, no accounts in this deployment, or
    /// somebody is already signed in.
    Hidden,
    /// The name-and-password form.
    SignIn,
    /// The deployment has no accounts at all, so there is nothing to sign in
    /// to. The surface explains how to provision one instead of offering a
    /// form that could only ever refuse.
    Unprovisioned,
    /// The invitation acceptance form, for the token in the address.
    Invite,
}

/// Why an account-entry attempt ended without a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountEntryError {
    /// HTTP 401 — the name and password do not open an account.
    ///
    /// One variant for "no such account" and "wrong password" on purpose: the
    /// store returns one outcome for both, and a surface that split them would
    /// turn the form into a way to enumerate account names.
    Rejected,
    /// HTTP 403 — the account exists and is not allowed to sign in.
    Disabled,
    /// HTTP 503 `accounts-unprovisioned` — this deployment has no accounts.
    Unprovisioned,
    /// HTTP 400 with one of the store's `password-*` codes.
    WeakPassword,
    /// HTTP 400 — a name, or a body, the store will not accept as written.
    InvalidInput,
    /// HTTP 409 `username-taken`.
    UsernameTaken,
    /// HTTP 404 `invite-not-found`.
    InviteNotFound,
    /// HTTP 410 `invite-expired`.
    InviteExpired,
    /// HTTP 409 `invite-already-accepted`.
    InviteAlreadyAccepted,
    /// A field the form needs was left empty — decided here, not asked of the
    /// daemon, because a round trip cannot answer it.
    EmptyFields,
    /// The two passwords in the invitation form differ.
    PasswordMismatch,
    /// No answer the contract describes: a network failure, a proxy, or a
    /// daemon that does not speak this route.
    Unavailable,
}

impl AccountEntryError {
    /// The reason a non-2xx answer carries, read from the daemon's own codes.
    ///
    /// The status code alone is not enough: `409` is both "that name is taken"
    /// and "this invitation has been used", and those are different sentences
    /// for the person reading them. The `error` code decides; the status is
    /// the fallback for a body that does not parse.
    pub fn from_response(status: u16, body: &str) -> Self {
        let code = error_code(body);
        match status {
            401 => Self::Rejected,
            403 => Self::Disabled,
            404 => Self::InviteNotFound,
            409 if code.as_deref() == Some("username-taken") => Self::UsernameTaken,
            409 => Self::InviteAlreadyAccepted,
            410 => Self::InviteExpired,
            400 if code.as_deref().is_some_and(|c| c.starts_with("password-")) => {
                Self::WeakPassword
            }
            400 if code.as_deref() == Some("invite-token-refused") => Self::InviteNotFound,
            400 => Self::InvalidInput,
            503 if code.as_deref() == Some("accounts-unprovisioned") => Self::Unprovisioned,
            _ => Self::Unavailable,
        }
    }

    /// Whether the surface should stop offering a form at all.
    ///
    /// A deployment with no accounts cannot sign anybody in, so the honest
    /// answer is the explanation plus a reload — not another refusal.
    pub const fn leaves_nothing_to_try(self) -> bool {
        matches!(self, Self::Unprovisioned)
    }
}

/// The daemon's machine-readable `error` field, when the body carries one.
fn error_code(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed["error"].as_str().map(str::to_string)
}

/// Everything a signed-out visitor has typed, plus why their last attempt
/// failed. See the module docs for why the secrets do not stay here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountEntryState {
    /// Whether any `/api/auth/status` answer has arrived yet.
    ///
    /// The distinction the form hangs on: an unanswered status means this
    /// deployment's account tier is still unknown, and a form for a deployment
    /// that may have no accounts would be a form that lies.
    pub status_received: bool,
    /// `needs_first_admin` from the last status answer.
    pub needs_first_admin: bool,
    /// The token from `/invite/<token>`, when the address names one.
    pub invite_token: Option<String>,
    /// Sign-in form: the account name.
    pub username: String,
    /// Sign-in form: the password. Empty except between a keystroke and a
    /// submit — see [`Self::begin_sign_in`].
    pub password: String,
    /// Invitation form: the account name being created.
    pub invite_username: String,
    /// Invitation form: the password being chosen.
    pub invite_password: String,
    /// Invitation form: the same password again.
    pub invite_confirm: String,
    /// Invitation form: the name shown to other people.
    pub invite_display_name: String,
    /// Which field holds the caret.
    pub focus: Option<AccountField>,
    /// Why the last attempt failed, painted under the form.
    pub error: Option<AccountEntryError>,
    /// A request is in flight; the submit button says so and is inert.
    pub submitting: bool,
}

impl AccountEntryState {
    /// Which surface this state shows.
    ///
    /// `available` and `signed_in` come from the last `/api/auth/status`
    /// answer, which is why they are parameters rather than fields: the answer
    /// has one owner (`EditorUiState::account`, `account_ui_available`) and a
    /// second copy here is a second thing to keep in step.
    pub fn mode(&self, available: bool, signed_in: bool) -> AccountEntryMode {
        // An invitation is addressed, not inferred: the link is the credential,
        // and it is the one surface that is meaningful before any status answer
        // (the whole point of the link is that nobody is signed in yet).
        if self.invite_token.is_some() {
            return AccountEntryMode::Invite;
        }
        if signed_in || !available || !self.status_received {
            return AccountEntryMode::Hidden;
        }
        if self.needs_first_admin {
            return AccountEntryMode::Unprovisioned;
        }
        AccountEntryMode::SignIn
    }

    /// The text of one field.
    pub fn field(&self, field: AccountField) -> &str {
        match field {
            AccountField::Username => &self.username,
            AccountField::Password => &self.password,
            AccountField::InviteUsername => &self.invite_username,
            AccountField::InvitePassword => &self.invite_password,
            AccountField::InvitePasswordConfirm => &self.invite_confirm,
            AccountField::InviteDisplayName => &self.invite_display_name,
        }
    }

    /// The text of one field, for writing.
    pub fn field_mut(&mut self, field: AccountField) -> &mut String {
        match field {
            AccountField::Username => &mut self.username,
            AccountField::Password => &mut self.password,
            AccountField::InviteUsername => &mut self.invite_username,
            AccountField::InvitePassword => &mut self.invite_password,
            AccountField::InvitePasswordConfirm => &mut self.invite_confirm,
            AccountField::InviteDisplayName => &mut self.invite_display_name,
        }
    }

    /// Move the caret to `field`, clearing the last failure.
    ///
    /// A refusal belongs to the attempt that earned it; leaving it on screen
    /// while somebody corrects a field is how a form appears to reject an
    /// entry nobody has made yet.
    pub fn focus_field(&mut self, field: AccountField) {
        self.focus = Some(field);
        self.error = None;
    }

    /// Append a typed character to the focused field, if there is one.
    /// Returns whether anything changed.
    pub fn push_char(&mut self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        let Some(field) = self.focus else {
            return false;
        };
        // A typed character is also an answer to the previous refusal.
        self.error = None;
        self.field_mut(field).push(c);
        true
    }

    /// Remove the last character of the focused field, if there is one.
    pub fn backspace(&mut self) -> bool {
        let Some(field) = self.focus else {
            return false;
        };
        self.error = None;
        if self.field_mut(field).pop().is_some() {
            return true;
        }
        false
    }

    /// Validate the sign-in form and take its credentials, leaving the
    /// password field empty.
    ///
    /// `Err` is a form-level refusal decided here (an empty field) — the typed
    /// text stays put so the person can fix it without retyping the password.
    /// `Ok` hands out a request that owns the secret for as long as it takes to
    /// serialize it; nothing else in this process will hold it afterwards.
    pub fn begin_sign_in(&mut self) -> Result<SignInRequest, AccountEntryError> {
        let username = self.username.trim().to_string();
        if username.is_empty() || self.password.is_empty() {
            return Err(AccountEntryError::EmptyFields);
        }
        self.error = None;
        self.submitting = true;
        Ok(SignInRequest {
            username,
            password: std::mem::take(&mut self.password),
        })
    }

    /// Validate the invitation form and take what the acceptance needs, leaving
    /// both password fields empty.
    pub fn begin_invite_acceptance(&mut self) -> Result<InviteAcceptance, AccountEntryError> {
        let Some(token) = self.invite_token.clone() else {
            return Err(AccountEntryError::InviteNotFound);
        };
        let username = self.invite_username.trim().to_string();
        if username.is_empty() || self.invite_password.is_empty() || self.invite_confirm.is_empty()
        {
            return Err(AccountEntryError::EmptyFields);
        }
        // Checked here, not at the daemon: the daemon only ever sees one
        // password, so a mismatch is a local mistake and saying so locally
        // avoids creating an account for a password the person mistyped.
        if self.invite_password != self.invite_confirm {
            return Err(AccountEntryError::PasswordMismatch);
        }
        self.error = None;
        self.submitting = true;
        let display_name = self.invite_display_name.trim().to_string();
        Ok(InviteAcceptance {
            token,
            username,
            password: std::mem::take(&mut self.invite_password),
            confirm: std::mem::take(&mut self.invite_confirm),
            display_name: (!display_name.is_empty()).then_some(display_name),
        })
    }

    /// Record a refusal: no request is in flight any more, the reason is on
    /// screen, and every secret the form was holding is gone.
    pub fn fail(&mut self, error: AccountEntryError) {
        self.submitting = false;
        self.error = Some(error);
        self.drop_secrets();
    }

    /// The request came back with a session: clear the form.
    ///
    /// The address still names `/invite/<token>`, and that token has just been
    /// spent — keeping it would repaint an acceptance form for a link that no
    /// longer works.
    pub fn succeed(&mut self) {
        self.submitting = false;
        self.error = None;
        self.invite_token = None;
        self.username.clear();
        self.drop_secrets();
        self.invite_username.clear();
        self.invite_display_name.clear();
        self.focus = None;
    }

    /// Forget every secret this form holds.
    ///
    /// Called on a refusal, on success, and by the host when the tab signs out:
    /// a password field that keeps its text across a round trip is a password
    /// living in editor state for the rest of the session.
    pub fn drop_secrets(&mut self) {
        self.password.clear();
        self.invite_password.clear();
        self.invite_confirm.clear();
    }
}

/// A sign-in about to be sent.
///
/// Deliberately not `Clone` and deliberately opaque: the password has exactly
/// one way out, [`Self::body`], and `Debug` prints a placeholder instead of the
/// secret so a stray log line or a panic message cannot leak it.
pub struct SignInRequest {
    /// The account name, trimmed.
    pub username: String,
    password: String,
}

impl SignInRequest {
    /// The JSON body `POST /api/auth/login` expects.
    pub fn body(&self) -> String {
        // The field names are the daemon's own: `account_routes::LoginRequest`
        // reads exactly these two, and they are spelled out in the route docs
        // of `crate::auth_routes`.
        serde_json::json!({
            "username": self.username,
            "password": self.password,
        })
        .to_string()
    }
}

impl std::fmt::Debug for SignInRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInRequest")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// An invitation acceptance about to be sent. See [`SignInRequest`] for why
/// this is neither `Clone` nor transparent.
pub struct InviteAcceptance {
    token: String,
    /// The account name to create, trimmed.
    pub username: String,
    password: String,
    /// The second copy the form checked against the first.
    ///
    /// Held because it is the ONLY other copy of the secret in the process, and
    /// letting it sit in the form after the acceptance went out would defeat
    /// taking the first one.
    confirm: String,
    /// The name to show instead of the handle. `None` means "use the handle",
    /// which is the same default the daemon applies.
    pub display_name: Option<String>,
}

impl InviteAcceptance {
    /// The JSON body `POST /api/auth/invite/accept` expects.
    pub fn body(&self) -> String {
        let mut body = serde_json::json!({
            "token": self.token,
            "username": self.username,
            "password": self.password,
        });
        if let Some(display_name) = &self.display_name {
            body["display_name"] = serde_json::Value::String(display_name.clone());
        }
        body.to_string()
    }
}

impl std::fmt::Debug for InviteAcceptance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InviteAcceptance")
            .field("token", &"<redacted>")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("display_name", &self.display_name)
            .finish()
    }
}

impl Drop for InviteAcceptance {
    fn drop(&mut self) {
        // Both copies go out of scope together; the second one is dropped here
        // rather than left for whatever the allocator does with the page.
        self.confirm.clear();
    }
}

#[cfg(test)]
#[path = "account_entry_state_tests.rs"]
mod tests;
