//! A data directory that deletes itself, for the account store's tests.
//!
//! The store needs a real directory and a real database file. `:memory:` is
//! deliberately not used: the store opens with `journal_mode=WAL` and refuses
//! to continue if it did not get it, and WAL is a property of a database FILE —
//! an in-memory database stays in `memory` journal mode, so a test on one would
//! pass while proving nothing about the mode the product runs in.
//!
//! This is the account store's own copy of the helper `op-host-services` keeps
//! for its document tests, and it exists for the reason the whole crate does:
//! these tests must run without the daemon's closure in the graph. It carries
//! only what the account tests use — a directory that removes itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh, unique directory under the system temp directory.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// Create an empty directory named for `tag`.
    ///
    /// Unique per process AND per call, so two tests in one binary never share
    /// a directory and a run never inherits one from a previous run that
    /// crashed before its cleanup.
    pub(crate) fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "norka-accounts-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The path of a file inside this directory — the database file itself,
    /// for the tests that open it as bytes or as a connection.
    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
