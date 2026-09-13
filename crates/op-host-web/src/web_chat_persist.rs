//! Chat history that survives a page reload.
//!
//! The transcript lives in the editor state, which in the browser is a wasm
//! value: a refresh threw it away, and the user came back to an empty panel
//! after every reload. This mirrors the transcript into `localStorage` — the
//! same adapter the browser settings already use — and restores it on mount.
//!
//! Only what the transcript shows is kept: role, text, and the agent label.
//! Tool calls, activities and thinking are per-turn detail that would bloat
//! the entry (and the panel re-derives them while a turn streams anyway).

use op_editor_core::chat::{ChatMessage, ChatRole};
use op_editor_core::EditorState;

use crate::web_storage::{storage_get, storage_set_checked};

/// Versioned so a future shape change drops the old entry instead of
/// mis-reading it.
const KEY: &str = "norka.chat.transcript.v1";
/// A long session must not push the key past the browser's quota; the
/// transcript is capped to its most recent messages.
const MAX_MESSAGES: usize = 200;

thread_local! {
    /// Fingerprint of the transcript last written, so the per-frame check
    /// costs a length/byte comparison instead of a serialization.
    static LAST_SAVED: std::cell::RefCell<Option<(usize, usize)>> =
        const { std::cell::RefCell::new(None) };
}

fn fingerprint(messages: &[ChatMessage]) -> (usize, usize) {
    (
        messages.len(),
        messages.iter().map(|m| m.content.len()).sum(),
    )
}

/// Restore the saved transcript when the editor state has none.
///
/// A document opened with history already (a fork, a shared session) wins:
/// the saved browser transcript is only a fallback for a fresh page.
pub(crate) fn restore(state: &mut EditorState) {
    if !state.chat.messages.is_empty() {
        return;
    }
    let Some(raw) = storage_get(KEY) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(entries) = value.as_array() else {
        return;
    };
    let messages: Vec<ChatMessage> = entries
        .iter()
        .filter_map(|entry| {
            let content = entry.get("c")?.as_str()?.to_string();
            let mut message = match entry.get("r").and_then(|r| r.as_str()) {
                Some("a") => ChatMessage::assistant(content),
                _ => ChatMessage::user(content),
            };
            if let Some(name) = entry.get("n").and_then(|n| n.as_str()) {
                message.agent_name = Some(name.to_string());
            }
            Some(message)
        })
        .collect();
    if messages.is_empty() {
        return;
    }
    state.chat.messages = messages;
}

/// Mirror the transcript when it changed. Cheap enough for the frame loop.
pub(crate) fn persist_if_changed(state: &EditorState) {
    let messages = &state.chat.messages;
    let now = fingerprint(messages);
    if LAST_SAVED.with(|saved| *saved.borrow() == Some(now)) {
        return;
    }
    let tail = if messages.len() > MAX_MESSAGES {
        &messages[messages.len() - MAX_MESSAGES..]
    } else {
        messages
    };
    let entries: Vec<serde_json::Value> = tail
        .iter()
        .map(|message| {
            let mut entry = serde_json::Map::new();
            entry.insert(
                "r".into(),
                serde_json::Value::String(match message.role {
                    ChatRole::Assistant => "a".into(),
                    ChatRole::User => "u".into(),
                }),
            );
            entry.insert(
                "c".into(),
                serde_json::Value::String(message.content.clone()),
            );
            if let Some(name) = message.agent_name.as_deref() {
                entry.insert("n".into(), serde_json::Value::String(name.to_string()));
            }
            serde_json::Value::Object(entry)
        })
        .collect();
    let Ok(raw) = serde_json::to_string(&entries) else {
        return;
    };
    if storage_set_checked(KEY, &raw) {
        LAST_SAVED.with(|saved| *saved.borrow_mut() = Some(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_tracks_count_and_size() {
        let mut state = EditorState::new();
        let empty = fingerprint(&state.chat.messages);
        state.chat.messages.push(ChatMessage::user("привет"));
        assert_ne!(fingerprint(&state.chat.messages), empty);
        let after = fingerprint(&state.chat.messages);
        state.chat.messages[0].content.push_str("!");
        assert_ne!(fingerprint(&state.chat.messages), after);
    }

    /// Without a browser there is nothing to read or write, and neither call
    /// may panic — the test path is every non-wasm build.
    #[test]
    fn both_calls_are_safe_without_a_browser() {
        let mut state = EditorState::new();
        state.chat.messages.push(ChatMessage::user("keep"));
        restore(&mut state);
        persist_if_changed(&state);
        assert_eq!(state.chat.messages.len(), 1);
    }
}
