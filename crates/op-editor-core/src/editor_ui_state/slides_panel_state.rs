//! Left-rail tab selection + the slides tab's pointer bookkeeping.
//!
//! The left rail is a two-tab surface for scenario documents: the
//! ordinary Pages + Layers tree, and a page navigator that lists one
//! thumbnail per top-level board. Which tab is showing, which row the
//! cursor is on, and the reorder gesture in flight are all transient UI
//! state and live here.
//!
//! The slide ORDER is deliberately absent: it is the document's own
//! top-level child order (`crate::preview_slideshow::active_page_boards`)
//! and a copy here would be a second answer to "what order are the
//! slides in".

use jian_core::scroll::ScrollState;

/// Which tab the left rail is showing.
///
/// `Layers` is the default. `Assets` is always offered. `Slides` is
/// offered only when the page has boards to navigate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LeftPanelTab {
    #[default]
    Layers,
    Slides,
    Assets,
}

/// A slides-list reorder gesture in flight.
///
/// `press_y` is kept alongside the live `pointer_y` so the release can
/// tell a click (jump to the slide) from a drag (reorder the deck) —
/// the two are the same gesture until the pointer has travelled far
/// enough, and deciding on release is what lets a shaky click still
/// navigate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlidesDrag {
    /// Index of the slide being dragged, in page order.
    pub from: usize,
    /// Where the press landed, in screen px.
    pub press_y: f32,
    /// Where the cursor is now, in screen px.
    pub pointer_y: f32,
}

/// What a press on the slides panel landed on. Hover and press both
/// carry one of these so the two never disagree about what is under
/// the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlidesPanelTarget {
    /// The "Layers" tab in the tab row.
    LayersTab,
    /// The "Slides" / "Cards" tab in the tab row.
    SlidesTab,
    /// The "Assets" tab in the tab row — Skala kit insert.
    AssetsTab,
    /// A slide row, by index in page order.
    Slide(usize),
    /// The action bar's present button.
    Present,
    /// The action bar's `Export PDF ⌄` button — toggles the menu below.
    ExportMenu,
    /// Export-menu row: every slide on the page.
    ExportAllSlides,
    /// Export-menu row: only the slides the selection covers. Only ever
    /// produced while that row is ENABLED, so a disabled row can neither
    /// be hovered nor activated.
    ExportSelectedSlides,
}

/// Left-rail tab selection + the slides tab's transient pointer state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlidesPanelState {
    /// Which tab the rail is showing.
    pub tab: LeftPanelTab,
    /// What the cursor is over — drives every hover wash in the panel.
    pub hover: Option<SlidesPanelTarget>,
    /// What a press landed on. Activates on RELEASE while the cursor is
    /// still on it, the same contract the presenting toolbar uses.
    pub pressed: Option<SlidesPanelTarget>,
    /// The reorder gesture in flight, once the pointer has moved.
    pub drag: Option<SlidesDrag>,
    /// Whether the action bar's export dropdown is showing.
    pub export_menu_open: bool,
    /// Vertical scroll of the slide list.
    pub scroll: ScrollState,
}

/// What a press on the Assets rail landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetsHit {
    /// The search field.
    Search,
    /// Insert the type's default master, by index into the filtered list.
    Insert(usize),
    /// Open the Design-MD panel on this type.
    Details(usize),
    /// The rest of the type row (same as Details).
    Row(usize),
}

/// Search / scroll / pointer state for the left-rail Assets tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetsPanelState {
    /// Substring filter over kit type names.
    pub search: String,
    pub search_input: jian_core::text_input::TextInputState,
    pub search_focused: bool,
    pub hover: Option<AssetsHit>,
    pub pressed: Option<AssetsHit>,
    pub scroll: jian_core::scroll::ScrollState,
}

impl AssetsPanelState {
    pub fn clear_pointer(&mut self) -> bool {
        let live = self.hover.is_some() || self.pressed.is_some();
        self.hover = None;
        self.pressed = None;
        live
    }
}

impl SlidesPanelState {
    /// Forget every in-flight pointer interaction, keeping the selected
    /// tab and the scroll position. Returns whether anything was live —
    /// hosts use that as the repaint signal.
    ///
    /// Called whenever the panel stops being reachable (the sidebar
    /// closes, a presentation starts), so a gesture cannot survive the
    /// surface it belongs to. The export dropdown goes with it for the
    /// same reason: it is painted INTO the panel, so a rail that stops
    /// being drawn would otherwise strand an open menu that reappears —
    /// still open — the next time the tab is shown.
    pub fn clear_pointer(&mut self) -> bool {
        let live = self.hover.is_some() || self.pressed.is_some() || self.drag.is_some();
        self.hover = None;
        self.pressed = None;
        self.drag = None;
        self.close_export_menu() || live
    }

    /// Close the export dropdown. Returns whether it was open, which is
    /// what makes this usable as an Escape-ladder rung: exactly one
    /// layer is dismissed per press.
    pub fn close_export_menu(&mut self) -> bool {
        let was_open = self.export_menu_open;
        self.export_menu_open = false;
        // A hover left on a menu row belongs to a menu that no longer
        // exists; it would light the row up again the moment the menu
        // reopened, before the cursor had moved.
        if was_open
            && matches!(
                self.hover,
                Some(SlidesPanelTarget::ExportAllSlides | SlidesPanelTarget::ExportSelectedSlides)
            )
        {
            self.hover = None;
        }
        was_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layers_tab_is_the_default() {
        assert_eq!(SlidesPanelState::default().tab, LeftPanelTab::Layers);
    }

    #[test]
    fn clearing_pointer_state_keeps_the_tab_and_scroll() {
        let mut state = SlidesPanelState {
            tab: LeftPanelTab::Slides,
            hover: Some(SlidesPanelTarget::Slide(2)),
            pressed: Some(SlidesPanelTarget::Slide(2)),
            drag: Some(SlidesDrag {
                from: 2,
                press_y: 10.0,
                pointer_y: 40.0,
            }),
            export_menu_open: false,
            scroll: ScrollState { offset: 24.0 },
        };
        assert!(state.clear_pointer());
        assert_eq!(state.tab, LeftPanelTab::Slides);
        assert_eq!(state.scroll.offset, 24.0);
        assert_eq!(state.hover, None);
        assert_eq!(state.pressed, None);
        assert_eq!(state.drag, None);
        // Idle again — nothing to repaint for.
        assert!(!state.clear_pointer());
    }

    #[test]
    fn clearing_pointer_state_closes_the_export_menu() {
        let mut state = SlidesPanelState {
            tab: LeftPanelTab::Slides,
            export_menu_open: true,
            ..SlidesPanelState::default()
        };
        // Live even though no hover / press / drag is in flight: the open
        // menu is itself something the next paint has to stop drawing.
        assert!(state.clear_pointer());
        assert!(!state.export_menu_open);
    }

    #[test]
    fn closing_the_export_menu_drops_a_hover_on_one_of_its_rows() {
        let mut state = SlidesPanelState {
            export_menu_open: true,
            hover: Some(SlidesPanelTarget::ExportSelectedSlides),
            ..SlidesPanelState::default()
        };
        assert!(state.close_export_menu());
        assert_eq!(state.hover, None);
        // Already closed — nothing left to dismiss, so the Escape ladder
        // moves on to the next rung.
        assert!(!state.close_export_menu());
    }

    #[test]
    fn closing_the_export_menu_keeps_a_hover_outside_it() {
        let mut state = SlidesPanelState {
            export_menu_open: true,
            hover: Some(SlidesPanelTarget::Present),
            ..SlidesPanelState::default()
        };
        assert!(state.close_export_menu());
        assert_eq!(state.hover, Some(SlidesPanelTarget::Present));
    }
}
