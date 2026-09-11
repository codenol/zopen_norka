//! Opt-in prompt dumping, for checking what the model actually receives.
//!
//! Off unless `OPENPENCIL_DUMP_PROMPTS` names a directory. The AI paths are
//! long and mostly server-side, so a prompt problem (rules missing, the wrong
//! block winning) is otherwise only visible by reading code and hoping. Set
//! the variable, reproduce the turn, read the files.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

/// Append `text` to `<dir>/<kind>.txt` when dumping is enabled.
///
/// Every failure is silent on purpose: diagnostics must never change what the
/// product does, and a dump path that cannot be written is not a design error.
pub fn dump_prompt(kind: &str, text: &str) {
    let Some(dir) = std::env::var_os("OPENPENCIL_DUMP_PROMPTS") else {
        return;
    };
    let dir = PathBuf::from(dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{kind}.txt"));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "\n===== {} chars =====", text.len());
        let _ = file.write_all(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumping_is_off_without_the_variable() {
        // The only assertion that matters without touching the process
        // environment: the call is a no-op and cannot panic.
        dump_prompt("test-noop", "text");
    }
}
