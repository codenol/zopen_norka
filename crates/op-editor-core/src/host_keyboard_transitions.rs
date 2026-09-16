//! Keyboard-driven editor-state transitions shared by the native and
//! web widget hosts (`widget_host/keyboard*.rs` / `shortcuts.rs`).
//! Both hosts used to carry these as copy-pasted method bodies.
//!
//! Hosts stay thin: they own the genuinely platform-specific glue —
//! modifier sourcing (winit `ModifiersChanged` vs the browser
//! `KeyboardEvent`), clipboard routing, and the per-host
//! "which surface owns the keyboard" predicates — then call these and
//! `mark_dirty()` when a transition reports a change.
//!
//! Return conventions:
//! - `bool` — `true` means "this transition acted" (host marks dirty
//!   and reports the key consumed).
//! - `Option<bool>` — `None` means "this surface is not focused, fall
//!   through to the next router arm"; `Some(false)` means "focused but
//!   the keystroke was rejected" (the host stops routing and reports
//!   not-consumed, matching both hosts' original `return false`).
//!
//! This is a spine: the transitions themselves live in the sibling
//! modules below, one per key-binding family, and are re-exported here so
//! every `host_keyboard_transitions::*` import path is unchanged.
//!
//! - `edit` — typing into, and deleting from, a chrome text input
//! - `navigation` — caret movement, nudge, reorder, tool switch
//! - `clipboard` — select-all, duplicate, paste
//! - `escape` — the import-modal gate and the modal commit pass

mod clipboard;
mod edit;
mod escape;
mod navigation;

pub use crate::assets_panel_keyboard::{assets_search_backspace, assets_search_text};
pub use crate::prompt_center_keyboard::{
    backspace as prompt_center_backspace, delete_forward as prompt_center_delete_forward,
    move_caret as prompt_center_caret, select_all as prompt_center_select_all,
    text as prompt_center_text,
};

pub use crate::scene_template_keyboard::{
    backspace as scene_template_backspace, delete_forward as scene_template_delete_forward,
    move_caret as scene_template_caret, paste as scene_template_paste,
    select_all as scene_template_select_all, text as scene_template_text,
};

pub use clipboard::*;
pub use edit::*;
pub use escape::*;
pub use navigation::*;
