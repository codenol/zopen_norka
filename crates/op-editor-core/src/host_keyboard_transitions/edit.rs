//! Keyboard transitions that type into, or delete from, a chrome text
//! input — the `keyboard.rs` / `keyboard_delete.rs` / `keyboard_send.rs`
//! families the native and web hosts dispatch to.
//!
//! Split out of the `host_keyboard_transitions` spine at the 800-line cap;
//! pure code motion, and the spine re-exports every name below so each
//! `op_editor_core::host_keyboard_transitions::*` path still resolves.

use jian_core::text_input::TextInputState;

use crate::editor_ui_state::{EditorUiState, VariableRowFocus};
use crate::state::EditorState;
use crate::ui_draft::PropertyFocus;

/// Mirror `ui.property_input` into the flat legacy draft fields.
pub fn mirror_property_input_legacy(state: &mut EditorState, select_all: bool, now_ms: u64) {
    let ui = &mut state.ui;
    ui.property_input_draft = ui.property_input.text().to_owned();
    ui.property_caret_pos = ui.property_input.caret();
    ui.property_draft_select_all = select_all;
    ui.property_caret_anchor_ms = now_ms;
}

/// Mirror `editor_ui.variables_header_input` into the legacy draft.
pub fn mirror_variables_header_input_legacy(
    state: &mut EditorState,
    select_all: bool,
    now_ms: u64,
) {
    let text = state.editor_ui.variables_header_input.text().to_owned();
    let caret = state.editor_ui.variables_header_input.caret();
    let ui = &mut state.ui;
    ui.property_input_draft = text;
    ui.property_caret_pos = caret;
    ui.property_draft_select_all = select_all;
    ui.property_caret_anchor_ms = now_ms;
}

/// Mirror `editor_ui.variable_row_input` into the legacy draft.
pub fn mirror_variable_row_input_legacy(state: &mut EditorState, select_all: bool, now_ms: u64) {
    let text = state.editor_ui.variable_row_input.text().to_owned();
    let caret = state.editor_ui.variable_row_input.caret();
    let ui = &mut state.ui;
    ui.property_input_draft = text;
    ui.property_caret_pos = caret;
    ui.property_draft_select_all = select_all;
    ui.property_caret_anchor_ms = now_ms;
}

// ─── Focus predicates ──────────────────────────────────────────────────

impl EditorUiState {
    /// Whether the visible collaboration Join field owns the keyboard.
    ///
    /// The focus bit alone is insufficient: an async phase transition or an
    /// externally dismissed panel can leave it stale. Hosts use this
    /// visibility-aware predicate for shortcuts, clipboard, and IME focus.
    pub fn collab_join_input_active(&self) -> bool {
        self.collab.availability == crate::CollabAvailability::Ready
            && matches!(
                self.collab.phase,
                crate::CollabConnectionPhase::Idle | crate::CollabConnectionPhase::Discovering
            )
            && self.collab.panel.open
            && self.collab.panel.view == crate::CollabPanelView::Join
            && self.collab.panel.join_address_focused
    }

    /// Drop even a stale Join-field focus bit when another surface takes
    /// over. The input's selection collapses with it so a later refocus
    /// never resurrects a destructive replace-on-type state.
    pub fn blur_collab_join_input(&mut self) -> bool {
        let input = &mut self.collab.panel.join_input;
        let end = input.text().len();
        input.set_caret(end, 0);
        std::mem::take(&mut self.collab.panel.join_address_focused)
    }

    /// Whether a variables-panel theme-axis / variant header rename
    /// draft owns the keyboard.
    pub fn variables_header_rename_active(&self) -> bool {
        self.variables_theme_rename_axis.is_some() || self.variables_variant_rename_value.is_some()
    }

    /// Whether the variables-panel search input owns the keyboard.
    /// Gated on the panel being open so a stale focus flag can't eat
    /// keystrokes after the panel closes.
    pub fn variables_search_input_active(&self) -> bool {
        self.variables_panel_open && self.variables_search_focus
    }

    /// Blur the variables-panel search box, KEEPING the typed filter —
    /// clearing it would surprise mid-search. Both the Escape rung and
    /// Enter run this same transition.
    pub fn blur_variables_search(&mut self) -> bool {
        if !self.variables_search_input_active() {
            return false;
        }
        self.variables_search_focus = false;
        true
    }
}

// ─── Inline layer rename ───────────────────────────────────────────────

/// Append `c` to the inline layer-rename draft. `None` when no rename
/// is active or `c` is a control char, so the caller falls through to
/// the next router arm.
pub fn rename_text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    if state.ui.layer_rename.is_none() || c.is_control() {
        return None;
    }
    let mut buf = [0u8; 4];
    let ok = state.rename_append(c.encode_utf8(&mut buf));
    if ok {
        if let Some(rename) = state.ui.layer_rename.as_mut() {
            rename.input.touch(now_ms);
        }
    }
    Some(ok)
}

/// Backspace (and forward-Delete — the rename draft has no separate
/// forward deletion) in the inline layer-rename draft.
pub fn rename_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    state.ui.layer_rename.as_ref()?;
    let ok = state.rename_backspace();
    if ok {
        if let Some(rename) = state.ui.layer_rename.as_mut() {
            rename.input.touch(now_ms);
        }
    }
    Some(ok)
}

// ─── Canvas text editing ───────────────────────────────────────────────

/// Insert `c` into the canvas text-edit session.
pub fn text_edit_text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    if state.ui.text_editing.is_none() || c.is_control() {
        return None;
    }
    let mut buf = [0u8; 4];
    Some(state.text_edit_insert(c.encode_utf8(&mut buf), now_ms))
}

/// Backspace in the canvas text-edit session.
pub fn text_edit_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    state.ui.text_editing.as_ref()?;
    Some(state.text_edit_backspace(now_ms))
}

/// Forward-delete in the canvas text-edit session — Delete deletes AT
/// the caret (or removes the active selection), textarea parity.
pub fn text_edit_delete_forward(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    state.ui.text_editing.as_ref()?;
    Some(state.text_edit_delete_forward(now_ms))
}

// ─── Variables-panel search filter ─────────────────────────────────────
//
// A live-filter box with no draft / commit machinery (the TS side is a
// controlled `<input>`), so it appends and pops straight onto
// `editor_ui.variables_search`.

/// Append `c` to the variables-panel search filter.
pub fn variables_search_text(state: &mut EditorState, c: char, now_ms: u64) -> bool {
    if !state.editor_ui.variables_search_input_active() || c.is_control() {
        return false;
    }
    state.editor_ui.variables_search.push(c);
    state.ui.property_caret_anchor_ms = now_ms;
    // A narrower list invalidates the scroll offset — the widget
    // clamps, but reset for a stable reveal-from-top.
    state.editor_ui.variables_scroll.offset = 0.0;
    true
}

/// Pop the last char off the variables-panel search filter.
/// `Some(false)` on an empty filter: the box still owns the key, so the
/// host must not fall through to deleting the selected node.
pub fn variables_search_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    if !state.editor_ui.variables_search_input_active() {
        return None;
    }
    if state.editor_ui.variables_search.pop().is_none() {
        return Some(false);
    }
    state.ui.property_caret_anchor_ms = now_ms;
    state.editor_ui.variables_scroll.offset = 0.0;
    Some(true)
}

// ─── Chat composer ─────────────────────────────────────────────────────

/// Insert `c` into the focused chat input. `false` when the composer
/// is not focused or `c` is a control character.
pub fn chat_input_text(state: &mut EditorState, c: char, now_ms: u64) -> bool {
    if !state.chat.focused || c.is_control() {
        return false;
    }
    let mut buf = [0u8; 4];
    state
        .chat
        .insert_input_text(c.encode_utf8(&mut buf), now_ms);
    true
}

/// Backspace in the focused chat input.
pub fn chat_input_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    if !state.chat.focused {
        return None;
    }
    Some(state.chat.backspace_input(now_ms))
}

// ─── Per-focus character gates ─────────────────────────────────────────

/// Numeric-draft gate (effect-parameter value boxes and any other
/// plain numeric field): digits, a leading `-`, and a single `.`.
pub fn numeric_draft_accepts(input: &TextInputState, c: char) -> bool {
    let replacing_all = input.is_select_all();
    let draft = input.text();
    let pos = if replacing_all {
        0
    } else {
        input.caret().min(draft.len())
    };
    c.is_ascii_digit()
        || (c == '-' && pos == 0 && (replacing_all || !draft.starts_with('-')))
        || (c == '.' && (replacing_all || !draft.contains('.')))
}

/// Property-panel input gate. Free-text rows take any printable char;
/// hex rows cap at the focus's `#RRGGBB(AA)` width with a sticky `#`
/// prefix; everything else is numeric.
///
/// Caret byte-index is also the char index (drafts are ASCII). `-` /
/// `#` are gated on the caret being at the start, NOT on the draft
/// being empty: typing `-` at the head of an existing `40` is a valid
/// edit (`-40`).
pub fn property_focus_accepts(focus: PropertyFocus, input: &TextInputState, c: char) -> bool {
    let replacing_all = input.is_select_all();
    let draft = input.text();
    let pos = if replacing_all {
        0
    } else {
        input.caret().min(draft.len())
    };
    if focus.is_free_text() {
        // Widget text rows (placeholder / value / label / icon names /
        // bind key) take any non-control character.
        !c.is_control()
    } else if focus.is_hex() {
        // Most colour rows cap at `#RRGGBB`; fill and page background
        // rows additionally author `#RRGGBBAA`. Keep that exception on
        // the focus type so adding it cannot silently widen fill /
        // stroke inputs.
        (replacing_all || draft.len() < focus.hex_max_len().unwrap_or(7))
            && (c.is_ascii_hexdigit() || (c == '#' && pos == 0 && !draft.starts_with('#')))
    } else {
        c.is_ascii_digit()
            || (c == '-' && pos == 0 && (replacing_all || !draft.starts_with('-')))
            || (c == '.' && focus.accepts_decimal() && (replacing_all || !draft.contains('.')))
    }
}

/// Variables-panel row / cell draft gate — per-kind (free text,
/// numeric, inline colour hex).
pub fn variable_row_accepts(focus: VariableRowFocus, input: &TextInputState, c: char) -> bool {
    let replacing_all = input.is_select_all();
    let draft = input.text();
    let pos = if replacing_all {
        0
    } else {
        input.caret().min(draft.len())
    };
    match focus {
        VariableRowFocus::Name(_) => !c.is_control(),
        VariableRowFocus::Number(_) | VariableRowFocus::NumberCell { .. } => {
            c.is_ascii_digit()
                || (c == '-' && (replacing_all || (pos == 0 && !draft.starts_with('-'))))
                || (c == '.' && (replacing_all || !draft.contains('.')))
        }
        VariableRowFocus::String(_) | VariableRowFocus::StringCell { .. } => !c.is_control(),
        // Inline colour hex — `#` only at the front, hex digits after,
        // capped at `#rrggbb` (same gating as the property panel's
        // FillHex draft).
        VariableRowFocus::ColorCell { .. } => {
            let len_after_clear = if replacing_all { 0 } else { draft.len() };
            if c == '#' {
                len_after_clear == 0
            } else {
                c.is_ascii_hexdigit() && len_after_clear < 7
            }
        }
    }
}

// ─── Typed-character routing ───────────────────────────────────────────

/// Insert `c` into the focused property-panel / effect-parameter input.
///
/// `None` when neither focus is set. When both are set the property
/// focus wins — an unreachable combination today (the two focuses are
/// set exclusively), kept explicit so the routing order is not
/// accidentally load-bearing.
pub fn property_input_text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    let focus = state.ui.property_focus;
    if focus.is_none() && state.editor_ui.effect_param_focus.is_none() {
        return None;
    }
    if c.is_control() {
        return Some(false);
    }
    let allowed = match focus {
        Some(focus) => property_focus_accepts(focus, &state.ui.property_input, c),
        None => numeric_draft_accepts(&state.ui.property_input, c),
    };
    if !allowed {
        return Some(false);
    }
    let mut buf = [0u8; 4];
    state
        .ui
        .property_input
        .insert_str(c.encode_utf8(&mut buf), now_ms);
    mirror_property_input_legacy(state, false, now_ms);
    Some(true)
}

/// Insert `c` into the focused variables-panel row / cell draft.
/// `None` when no row is focused.
pub fn variable_row_text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    let focus = state.editor_ui.variable_row_focus?;
    if !variable_row_accepts(focus, &state.editor_ui.variable_row_input, c) {
        return Some(false);
    }
    let mut buf = [0u8; 4];
    state
        .editor_ui
        .variable_row_input
        .insert_str(c.encode_utf8(&mut buf), now_ms);
    mirror_variable_row_input_legacy(state, false, now_ms);
    Some(true)
}

/// Insert `c` into the variables-panel theme/variant header rename
/// draft. `false` (fall through) when no rename is active or `c` is a
/// control character.
pub fn variables_header_text(state: &mut EditorState, c: char, now_ms: u64) -> bool {
    if !state.editor_ui.variables_header_rename_active() || c.is_control() {
        return false;
    }
    let mut buf = [0u8; 4];
    state
        .editor_ui
        .variables_header_input
        .insert_str(c.encode_utf8(&mut buf), now_ms);
    mirror_variables_header_input_legacy(state, false, now_ms);
    true
}

// ─── Backspace / forward-delete ────────────────────────────────────────
//
// Each helper compares (text, caret) before and after so a no-op at a
// text boundary reports `false` and the caller can fall through.

macro_rules! edited {
    ($input:expr, $op:expr) => {{
        let before = ($input.text().to_owned(), $input.caret());
        $op;
        (before.0.as_str(), before.1) != ($input.text(), $input.caret())
    }};
}

// ─── Rules-form input ──────────────────────────────────────────────────
//
// The guidelines panel's rule editor is a markdown text area: one field
// holding `## Title` plus the instruction body. It sits above the chat
// but below every modal / popover input, so a keystroke only reaches it
// when nothing more modal is open. Each helper reports `None` while the
// editor is closed, so the host ladder falls through to the next surface.

/// Insert `c` into the rule editor's markdown field.
pub fn design_rule_text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    if c == '\n' {
        draft.focused_input().insert_str("\n", now_ms);
        return Some(true);
    }
    // Every other control character stays with the host's shortcuts.
    if c.is_control() {
        return Some(false);
    }
    let mut buf = [0u8; 4];
    draft
        .focused_input()
        .insert_str(c.encode_utf8(&mut buf), now_ms);
    Some(true)
}

/// Enter in the rule editor — a newline inside the markdown field.
pub fn design_rule_newline(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    draft.focused_input().insert_str("\n", now_ms);
    Some(true)
}

/// Backspace in the rule editor.
pub fn design_rule_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    let input = draft.focused_input();
    let before = (input.text().to_owned(), input.caret());
    input.backspace(now_ms);
    Some((before.0.as_str(), before.1) != (input.text(), input.caret()))
}

/// Forward-delete in the rule editor.
pub fn design_rule_delete_forward(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    let input = draft.focused_input();
    let before = (input.text().to_owned(), input.caret());
    input.delete_forward(now_ms);
    Some((before.0.as_str(), before.1) != (input.text(), input.caret()))
}

/// Backspace in the focused property-panel / effect-parameter input.
pub fn property_input_backspace(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.ui.property_input,
        state.ui.property_input.backspace(now_ms)
    );
    if changed {
        mirror_property_input_legacy(state, false, now_ms);
    }
    changed
}

/// Forward-delete in the focused property-panel / effect-parameter input.
pub fn property_input_delete_forward(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.ui.property_input,
        state.ui.property_input.delete_forward(now_ms)
    );
    if changed {
        mirror_property_input_legacy(state, false, now_ms);
    }
    changed
}

/// Backspace in the variables header rename draft.
pub fn variables_header_backspace(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.editor_ui.variables_header_input,
        state.editor_ui.variables_header_input.backspace(now_ms)
    );
    if changed {
        mirror_variables_header_input_legacy(state, false, now_ms);
    }
    changed
}

/// Forward-delete in the variables header rename draft.
pub fn variables_header_delete_forward(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.editor_ui.variables_header_input,
        state
            .editor_ui
            .variables_header_input
            .delete_forward(now_ms)
    );
    if changed {
        mirror_variables_header_input_legacy(state, false, now_ms);
    }
    changed
}

/// Backspace in the variables row / cell draft.
pub fn variable_row_backspace(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.editor_ui.variable_row_input,
        state.editor_ui.variable_row_input.backspace(now_ms)
    );
    if changed {
        mirror_variable_row_input_legacy(state, false, now_ms);
    }
    changed
}

/// Forward-delete in the variables row / cell draft.
pub fn variable_row_delete_forward(state: &mut EditorState, now_ms: u64) -> bool {
    let changed = edited!(
        state.editor_ui.variable_row_input,
        state.editor_ui.variable_row_input.delete_forward(now_ms)
    );
    if changed {
        mirror_variable_row_input_legacy(state, false, now_ms);
    }
    changed
}

// ─── Caret movement ────────────────────────────────────────────────────

/// Whether a chrome text field / search overlay owns forward-Delete.
///
/// Each of these surfaces either handles Delete itself in an earlier
/// router arm or deliberately swallows it; falling through to
/// [`delete_selection_with_history`] behind one of them would silently
/// drop the node under the focused field.
///
/// The two hosts had drifted to *different* subsets of this list —
/// native was missing the variables search box, web was missing the
/// icon picker and the component browser — so each could destroy a
/// node while the other's overlay was up. This is the union.
///
/// Host-resolved surfaces (font picker, image popovers, settings, Git)
/// are checked host-side before this predicate.
pub fn delete_owned_by_chrome_input(state: &EditorState) -> bool {
    state.editor_ui.collab_join_input_active()
        // The settings-modal input routes Delete in an earlier arm
        // (`host_ui_transitions::settings_delete_forward`); listed here so
        // reordering the arms cannot reopen the node-destroying
        // fall-through — a Delete while the API-key field is focused used
        // to silently remove the selected node behind the modal.
        || state.editor_ui.agent_settings.focus.is_some()
        || state.ui.property_focus.is_some()
        || state.editor_ui.effect_param_focus.is_some()
        || state.editor_ui.variable_row_focus.is_some()
        || state.editor_ui.variables_header_rename_active()
        // Both hosts route the preset draft in an earlier arm; listed
        // here so reordering the arms cannot reopen the node-destroying
        // fall-through this predicate exists to stop.
        || state.editor_ui.preset_name_input_active()
        || state.editor_ui.variables_search_input_active()
        || state.editor_ui.assets_search_input_active()
        || state.editor_ui.icon_picker.open
        || state.editor_ui.prompt_center.open
        || state.editor_ui.chat_model_picker.open
        || state.editor_ui.component_browser_open
        || state.chat.focused
}

/// Delete the selection, pushing the pre-delete snapshot onto history.
pub fn delete_selection_with_history(state: &mut EditorState) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    let snap = state.snapshot_for_history();
    if state.delete_selected() {
        state.history_push_past(snap);
        return true;
    }
    false
}
