//! Which tab the property panel is showing, and which tabs a selection offers.
//!
//! Split out of `methods.rs` when the section's second tab pushed that file
//! past the 800-line cap. Two things make this more than a layout question, and
//! they are why it has a file rather than three lines inside the panel:
//!
//! * **A section is not an ordinary selection.** It offers «Обзор» (everything
//!   about flow and analytics) and «Дизайн», and no code inspector: a section is
//!   assembled from analytics and has no generated code to read.
//! * **The retained value outlives the selection.** `property_tab` is one field
//!   for the whole editor, so a value chosen on a text node (Code) has to
//!   present as something a section can show when the selection changes. That
//!   mapping is [`EditorUiState::effective_property_tab`], and it is the single
//!   place paint, hover and the press arm get their answer from.

use super::EditorUiState;

impl EditorUiState {
    /// The generated-code inspector needs more horizontal room than the
    /// Compact phone sheet provides. It remains available on tablet, desktop,
    /// and web layouts.
    pub fn code_property_tab_available(&self) -> bool {
        !self.compact_layout()
    }

    /// Whether the property panel is looking at a section.
    ///
    /// The panel shows a different strip for one: a section is assembled from
    /// analytics and has no generated code to inspect, so it offers «Обзор» and
    /// «Дизайн» and nothing else. The state that knows is the section panel's
    /// own — `SectionPanelState::node` is `Some` exactly while the selection is
    /// a section, which is also when its block paints.
    pub fn selection_is_section(&self) -> bool {
        self.section_panel.node.is_some()
    }

    /// Active PropertyPanel tab after applying responsive availability. A
    /// retained Code value (for example after resizing an iPad split view down
    /// to Compact) presents as Design until enough room is available again.
    ///
    /// A section presents as Overview when the retained value is neither of the
    /// two tabs it offers: a section is not a place the code inspector or the
    /// widget interactions belong, and falling back to Design would hide the
    /// block the section exists for.
    pub fn effective_property_tab(&self) -> crate::PropertyTab {
        if self.selection_is_section() {
            return match self.property_tab {
                crate::PropertyTab::Overview | crate::PropertyTab::Design => self.property_tab,
                _ => crate::PropertyTab::Overview,
            };
        }
        if !self.code_property_tab_available()
            && matches!(self.property_tab, crate::PropertyTab::Code)
        {
            crate::PropertyTab::Design
        } else {
            self.property_tab
        }
    }

    /// Set the active PropertyPanel tab without allowing direct action
    /// dispatch to reopen Code in a Compact layout. Returns whether the stored
    /// value changed.
    pub fn set_property_tab(&mut self, requested: crate::PropertyTab) -> bool {
        // The two values a selection or a layout can refuse: Overview anywhere
        // but on a section, and Code on a Compact phone. Both fall back to
        // Design, which everything has — one condition rather than two branches
        // with the same body.
        let refused = (matches!(requested, crate::PropertyTab::Overview)
            && !self.selection_is_section())
            || (matches!(requested, crate::PropertyTab::Code)
                && !self.code_property_tab_available());
        let next = if refused {
            crate::PropertyTab::Design
        } else {
            requested
        };
        let changed = self.property_tab != next;
        self.property_tab = next;
        if !self.code_property_tab_available()
            && matches!(self.property_tab_hover, Some(crate::PropertyTab::Code))
        {
            self.property_tab_hover = None;
        }
        changed
    }
}
