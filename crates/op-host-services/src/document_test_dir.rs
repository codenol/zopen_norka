//! A documents directory that deletes itself, for the store's tests.
//!
//! Both `document_store_tests` and `document_db_tests` need a real directory
//! and a real connection. `:memory:` is deliberately not used: the store opens
//! with `journal_mode=WAL`, which is a property of a database FILE — an
//! in-memory database stays in `memory` journal mode, so a test on one would
//! pass while proving nothing about the mode the product runs in.

use std::path::{Path, PathBuf};

use crate::document_db::DocumentDb;

/// A fresh, unique directory under the system temp directory.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// Create an empty directory named for `tag`.
    ///
    /// Unique per process AND per call, so two tests in one binary never share
    /// a directory and a run never inherits one from a previous run that
    /// crashed before its cleanup.
    pub(crate) fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "norka-docs-{tag}-{}-{}",
            std::process::id(),
            crate::document_store::new_key()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The store for this directory.
    pub(crate) fn open(&self) -> DocumentDb {
        DocumentDb::open(self.path()).expect("open document store")
    }

    /// The path of a file inside this directory.
    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Write a fixture file.
    pub(crate) fn write(&self, name: &str, body: &str) {
        std::fs::write(self.join(name), body).expect("write fixture");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
