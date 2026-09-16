//! OpenPencil accounts — who somebody is, in SQLite.
//!
//! ## Why this is a crate of its own
//!
//! The store used to live in `op-host-services`, which is the headless
//! daemon's whole library closure: skia-safe, tokio, reqwest, the agent
//! runtimes. Two callers need accounts without needing a daemon. The first is
//! `op admin create`: a deployment with no accounts has nobody who could
//! authorize an HTTP request, so the first administrator has to be creatable
//! before there is a server to ask — and the `op` binary a designer runs (and
//! agents drive over the HTTP MCP transport) should not link a rasteriser to
//! write one row. The second is any future host that only verifies a
//! credential.
//!
//! ## The boundary
//!
//! [`accounts`] is the whole of it: the schema and its migrations, the
//! operations over users, sessions, one-time tokens and invites, the sign-in
//! budget, and the password policy. Nothing in here opens a socket, spawns a
//! process, or knows what a document is. `op-host-services` depends on this
//! crate and re-exports [`accounts`] under its original path, so its own
//! callers (`web_canvas_server`, the CLI) keep importing what they always
//! imported; the dependency never points the other way, because the moment it
//! did, `op admin create` would drag Skia back in through the front door.
//!
//! Accounts are a property of the DEPLOYMENT, not of the artwork, which is why
//! the store lives in its own database under
//! [`accounts::DATA_DIR_ENV`] rather than beside the documents.

pub mod accounts;

/// A temporary data directory for the store's own tests. Test-only: nothing in
/// the product creates one.
#[cfg(test)]
mod test_dir;
