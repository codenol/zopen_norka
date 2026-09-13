//! Press handling for the `/files` screen.
//!
//! The screen is painted by a platform-free widget, which by design cannot
//! make requests. A press therefore only records intent on the state
//! (`server_files_open_request` / `server_files_create_request`) and the frame
//! performs it — the same split the rest of the shell uses for file actions.

use op_editor_core::AppScreen;
use op_editor_ui::widgets::files_screen::{FileMenuAction, FilesScreen};
use op_editor_ui::Rect;

impl super::WidgetHost {
    /// Handle a press while the file browser is showing.
    ///
    /// Returns whether the press was consumed; on this screen it always is,
    /// because there is no editor underneath to fall through to.
    pub(in crate::widget_host) fn press_files_screen(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        debug_assert_eq!(self.editor_state.editor_ui.screen, AppScreen::Files);
        let screen_rect = Rect {
            origin: op_editor_ui::Point2D::new(0.0, 0.0),
            size: op_editor_ui::Point2D::new(viewport_width, viewport_height),
        };
        let point = op_editor_ui::Point2D::new(x, y);

        // An open menu owns the press: pick a row, or dismiss by pressing
        // anywhere else.
        if let Some(menu) = self.editor_state.editor_ui.server_files_menu.clone() {
            let menu_rect =
                FilesScreen::menu_rect(screen_rect, op_editor_ui::Point2D::new(menu.x, menu.y));
            let mut chosen = None;
            for (rect, action) in FilesScreen::menu_rows(menu_rect) {
                if contains(rect, point) {
                    chosen = Some(action);
                }
            }
            self.editor_state.editor_ui.server_files_menu = None;
            self.mark_editor_state_dirty();
            match chosen {
                Some(FileMenuAction::Rename) => {
                    let name = self
                        .editor_state
                        .editor_ui
                        .server_files
                        .iter()
                        .find(|file| file.key == menu.key)
                        .map(|file| file.name.clone())
                        .unwrap_or_default();
                    self.editor_state.editor_ui.server_files_rename =
                        Some(op_editor_core::ServerFileRename {
                            key: menu.key,
                            draft: name,
                        });
                    self.editor_state.editor_ui.server_files_search_focused = false;
                }
                Some(FileMenuAction::Delete) => {
                    self.editor_state.editor_ui.server_files_delete_request = Some(menu.key);
                }
                None => {}
            }
            return true;
        }

        // A rename in progress swallows everything: its field is over the card
        // and clicking away commits nothing behind it.
        if self.editor_state.editor_ui.server_files_rename.is_some() {
            return true;
        }

        if contains(FilesScreen::new_button_rect(screen_rect), point) {
            self.editor_state.editor_ui.server_files_create_request = true;
            self.mark_editor_state_dirty();
            return true;
        }

        // The search field takes the keyboard here; a press outside it gives
        // the keyboard back.
        if contains(FilesScreen::search_rect(screen_rect), point) {
            self.set_file_search_focused(true);
            return true;
        }
        self.set_file_search_focused(false);

        let screen = self.files_screen(screen_rect);
        for card in screen.cards(screen_rect) {
            if contains(card.rect, point) {
                if let Some(file) = self.editor_state.editor_ui.server_files.get(card.index) {
                    let (key, name) = (file.key.clone(), file.name.clone());
                    self.editor_state.editor_ui.server_files_open_request = Some(key);
                    // The card already knows the name, so the tab is titled from
                    // the click rather than from a second round-trip.
                    self.editor_state.editor_ui.file_name_display = Some(name);
                    self.mark_editor_state_dirty();
                }
                return true;
            }
        }
        true
    }
}

impl super::WidgetHost {
    /// Right press on the file screen: open the menu of the card under it.
    pub(in crate::widget_host) fn right_press_files_screen(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let screen_rect = Rect {
            origin: op_editor_ui::Point2D::new(0.0, 0.0),
            size: op_editor_ui::Point2D::new(viewport_width, viewport_height),
        };
        let point = op_editor_ui::Point2D::new(x, y);
        let screen = self.files_screen(screen_rect);
        for card in screen.cards(screen_rect) {
            if contains(card.rect, point) {
                if let Some(file) = self.editor_state.editor_ui.server_files.get(card.index) {
                    // Anchor just below the press, so the menu does not cover
                    // the card's own name.
                    self.editor_state.editor_ui.server_files_menu =
                        Some(op_editor_core::ServerFileMenu {
                            key: file.key.clone(),
                            x: x,
                            y: y + 6.0,
                        });
                    self.mark_editor_state_dirty();
                }
                return true;
            }
        }
        self.editor_state.editor_ui.server_files_menu = None;
        true
    }

    /// The screen as it is currently configured — paint and hit-test share it.
    pub(in crate::widget_host) fn files_screen(
        &self,
        rect: Rect,
    ) -> op_editor_ui::widgets::files_screen::FilesScreen<'_> {
        let _ = rect;
        op_editor_ui::widgets::files_screen::FilesScreen {
            now_unix_ms: self.editor_state.editor_ui.now_unix_ms,
            theme: &self.theme,
            files: &self.editor_state.editor_ui.server_files,
            search_focused: self.editor_state.editor_ui.server_files_search_focused,
            loading: self.editor_state.editor_ui.server_files_loading,
            error: self.editor_state.editor_ui.server_files_error.as_deref(),
            query: &self.editor_state.editor_ui.server_files_query,
            menu: self.editor_state.editor_ui.server_files_menu.as_ref(),
            rename: self.editor_state.editor_ui.server_files_rename.as_ref(),
        }
    }
}

fn contains(rect: Rect, point: op_editor_ui::Point2D) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.x
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.y
}
