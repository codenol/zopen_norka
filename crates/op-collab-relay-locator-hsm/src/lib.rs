//! PKCS#11-backed OPLS signer for Norka relay locators.
//!
//! This is a Unix-only daemon. Every guarantee it makes rests on a POSIX
//! primitive for which no audited Windows equivalent is implemented here:
//!
//! * callers are authenticated by Unix-domain-socket peer credentials
//!   (`SO_PEERCRED` on Linux, `getpeereid` on the BSDs), never by anything the
//!   request payload asserts about itself;
//! * the config and PIN files are accepted only after their owner, mode, and
//!   link count are verified, and are opened with `O_NOFOLLOW` so a swapped
//!   symlink cannot redirect the read;
//! * the listening socket, its `flock` lock file, and its heartbeat file are
//!   created with explicit modes and re-validated against uid/gid/mode
//!   expectations before the signer serves a single request;
//! * even the config schema is POSIX-shaped — it carries the expected client
//!   uid/gid and a socket path bounded by the `sun_path` limit.
//!
//! Building this on Windows would mean either compiling those checks out, which
//! leaves a signer that still looks functional after its file-ownership and
//! peer-identity guarantees are gone, or inventing an unaudited ACL-based
//! substitute for them. Both are worse than not shipping the daemon there, so
//! the entire crate is gated on `cfg(unix)` and the binary exits with an
//! explicit unsupported-platform error elsewhere.

#[cfg(unix)]
pub mod config;
#[cfg(unix)]
pub mod error;
#[cfg(unix)]
pub mod pkcs11;
#[cfg(unix)]
pub mod protocol;
#[cfg(unix)]
pub mod secure_file;
#[cfg(unix)]
pub mod server;

#[cfg(unix)]
pub use config::{KeyConfig, Region, SignerConfig};
#[cfg(unix)]
pub use error::{SignerError, SignerResult};
#[cfg(unix)]
pub use pkcs11::KeyStore;

/// Reported by the binary on platforms where the signer cannot run.
#[cfg(not(unix))]
pub const UNSUPPORTED_PLATFORM: &str = "the Norka relay locator HSM signer requires a Unix \
host: it authenticates callers with Unix-domain-socket peer credentials and guards its config \
and PIN files with POSIX ownership, mode, and link-count checks";
