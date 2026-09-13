//! The `/files` screen — the browser's own file list.
//!
//! Figma's file browser is a screen, not a panel: cards, a search field and a
//! new-file button, with the editor one document away. This paints that
//! screen; the host owns when it is shown (`/files` in the address) and what
//! its contents are (the daemon's `/api/files`).
//!
//! Geometry is computed by pure functions so the hit-test and the paint can
//! never disagree about where a card is.

use op_editor_core::ServerFile;

use crate::theme::Theme;
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

/// Outer padding of the screen.
const PAD: f32 = 32.0;
/// Card size and gutters.
const CARD_W: f32 = 248.0;
const CARD_H: f32 = 156.0;
const CARD_GAP: f32 = 20.0;
/// Thumbnail band inside a card.
const THUMB_H: f32 = 92.0;

/// A card's rectangle plus the index of the file it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileCard {
    pub rect: Rect,
    pub index: usize,
}

/// The file browser's pieces.
pub struct FilesScreen<'a> {
    pub theme: &'a Theme,
    pub files: &'a [ServerFile],
    pub loading: bool,
    pub error: Option<&'a str>,
    pub query: &'a str,
}

impl FilesScreen<'_> {
    /// Header height: title row + search field.
    fn header_height() -> f32 {
        112.0
    }

    /// Where the "new file" button sits.
    pub fn new_button_rect(rect: Rect) -> Rect {
        Rect {
            origin: Point2D::new(rect.origin.x + rect.size.x - PAD - 152.0, rect.origin.y + PAD),
            size: Point2D::new(152.0, 36.0),
        }
    }

    /// Where the search field sits.
    pub fn search_rect(rect: Rect) -> Rect {
        Rect {
            origin: Point2D::new(rect.origin.x + PAD, rect.origin.y + PAD + 52.0),
            size: Point2D::new((rect.size.x - PAD * 2.0).min(360.0), 32.0),
        }
    }

    /// How many card columns fit the available width.
    pub fn columns(rect: Rect) -> usize {
        let usable = (rect.size.x - PAD * 2.0).max(CARD_W);
        (((usable + CARD_GAP) / (CARD_W + CARD_GAP)).floor() as usize).max(1)
    }

    /// Every card, in paint order, with the index of the file it shows.
    pub fn cards(&self, rect: Rect) -> Vec<FileCard> {
        let columns = Self::columns(rect);
        let origin_x = rect.origin.x + PAD;
        let origin_y = rect.origin.y + PAD + Self::header_height();
        self.files
            .iter()
            .enumerate()
            .filter_map(|(index, _)| {
                let column = index % columns;
                let row = index / columns;
                let card = Rect {
                    origin: Point2D::new(
                        origin_x + column as f32 * (CARD_W + CARD_GAP),
                        origin_y + row as f32 * (CARD_H + CARD_GAP),
                    ),
                    size: Point2D::new(CARD_W, CARD_H),
                };
                // A card past the bottom of the viewport is not painted (and so
                // not clickable) — the list scrolls by search, not by pixels.
                (card.origin.y + card.size.y <= rect.origin.y + rect.size.y).then_some(FileCard {
                    rect: card,
                    index,
                })
            })
            .collect()
    }

    /// Paint the screen.
    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = self.theme;
        cx.backend.fill_rect(rect, theme.background);

        self.paint_title(cx, rect);
        self.paint_search(cx, rect);

        if let Some(error) = self.error {
            self.paint_note(cx, rect, error, theme.destructive);
            return;
        }
        if self.loading && self.files.is_empty() {
            self.paint_note(cx, rect, "Loading files…", theme.muted_foreground);
            return;
        }
        if self.files.is_empty() {
            let note = if self.query.is_empty() {
                "No files yet — create one to get started"
            } else {
                "Nothing matches that search"
            };
            self.paint_note(cx, rect, note, theme.muted_foreground);
            return;
        }
        for card in self.cards(rect) {
            self.paint_card(cx, card, &self.files[card.index]);
        }
    }

    fn paint_title(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = self.theme;
        let title = TextLayout::single_run(
            "Files",
            "system-ui",
            22.0,
            theme.foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend
            .draw_text(&title, Point2D::new(rect.origin.x + PAD, rect.origin.y + PAD + 22.0));

        // The new-file button: filled, the way a primary action reads.
        let button = Self::new_button_rect(rect);
        cx.backend.fill_rect(button, theme.primary);
        let label = "New file";
        let width = text_metrics::measure_chrome(cx.backend, label, 13.0);
        let text = TextLayout::single_run(
            label,
            "system-ui",
            13.0,
            theme.primary_foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                button.origin.x + (button.size.x - width) / 2.0,
                button.origin.y + button.size.y / 2.0 + 4.5,
            ),
        );
    }

    fn paint_search(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = self.theme;
        let field = Self::search_rect(rect);
        cx.backend.fill_rect(field, theme.input);
        cx.backend.stroke_rect(field, theme.border, 1.0);
        let (text, color) = if self.query.is_empty() {
            ("Search files", theme.muted_foreground)
        } else {
            (self.query, theme.foreground)
        };
        let layout = TextLayout::single_run(
            text,
            "system-ui",
            13.0,
            color.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &layout,
            Point2D::new(field.origin.x + 10.0, field.origin.y + field.size.y / 2.0 + 4.5),
        );
    }

    fn paint_card(&self, cx: &mut PaintCx<'_>, card: FileCard, file: &ServerFile) {
        let theme = self.theme;
        let rect = card.rect;
        cx.backend.fill_rect(rect, theme.card);
        cx.backend.stroke_rect(rect, theme.border, 1.0);

        // Thumbnail band: a document preview is not painted yet, so the band
        // stays a quiet placeholder rather than a misleading picture.
        let thumb = Rect {
            origin: Point2D::new(rect.origin.x, rect.origin.y),
            size: Point2D::new(rect.size.x, THUMB_H),
        };
        cx.backend.fill_rect(thumb, theme.muted);
        cx.backend.stroke_line(
            Point2D::new(thumb.origin.x, thumb.origin.y + thumb.size.y),
            Point2D::new(thumb.origin.x + thumb.size.x, thumb.origin.y + thumb.size.y),
            theme.border,
            1.0,
        );

        let name = TextLayout::single_run(
            &file.name,
            "system-ui",
            13.0,
            theme.card_foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.save();
        cx.backend.clip_rect(Rect {
            origin: Point2D::new(rect.origin.x + 12.0, rect.origin.y + THUMB_H),
            size: Point2D::new(rect.size.x - 24.0, rect.size.y - THUMB_H),
        });
        cx.backend.draw_text(
            &name,
            Point2D::new(rect.origin.x + 12.0, rect.origin.y + THUMB_H + 24.0),
        );
        cx.backend.restore();

        let meta = updated_label(file.updated_at);
        let meta_layout = TextLayout::single_run(
            &meta,
            "system-ui",
            11.0,
            theme.muted_foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &meta_layout,
            Point2D::new(rect.origin.x + 12.0, rect.origin.y + THUMB_H + 46.0),
        );
    }

    fn paint_note(&self, cx: &mut PaintCx<'_>, rect: Rect, message: &str, color: Color) {
        let layout = TextLayout::single_run(
            message,
            "system-ui",
            13.0,
            color.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &layout,
            Point2D::new(
                rect.origin.x + PAD,
                rect.origin.y + PAD + Self::header_height() + 24.0,
            ),
        );
    }
}

/// A short, human "when" for a card.
///
/// Deliberately coarse: the file browser answers "which one was I working on",
/// and a minute-accurate timestamp answers nothing the name does not.
pub fn updated_label(updated_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if updated_at == 0 || now == 0 || updated_at > now {
        return "Edited recently".to_string();
    }
    let age = now - updated_at;
    match age {
        0..=59 => "Edited just now".to_string(),
        60..=3_599 => format!("Edited {} min ago", age / 60),
        3_600..=86_399 => format!("Edited {} h ago", age / 3_600),
        86_400..=604_799 => format!("Edited {} d ago", age / 86_400),
        _ => format!("Edited {} d ago", age / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_rect() -> Rect {
        Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(1_264.0, 800.0),
        }
    }

    fn files(count: usize) -> Vec<ServerFile> {
        (0..count)
            .map(|index| ServerFile {
                key: format!("key{index}"),
                name: format!("File {index}"),
                updated_at: 1_700_000_000,
                size: 10,
            })
            .collect()
    }

    #[test]
    fn columns_follow_the_available_width() {
        assert_eq!(FilesScreen::columns(screen_rect()), 4);
        let narrow = Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(300.0, 800.0),
        };
        assert_eq!(FilesScreen::columns(narrow), 1);
    }

    #[test]
    fn cards_are_laid_out_in_a_grid_without_overlap() {
        let files = files(5);
        let screen = FilesScreen {
            theme: &Theme::default(),
            files: &files,
            loading: false,
            error: None,
            query: "",
        };
        let cards = screen.cards(screen_rect());
        assert_eq!(cards.len(), 5);
        assert_eq!(cards[0].rect.origin, Point2D::new(PAD, PAD + 112.0));
        // Second card sits one column to the right, fifth starts row two.
        assert_eq!(cards[1].rect.origin.x, PAD + 248.0 + 20.0);
        assert_eq!(cards[4].rect.origin.y, cards[0].rect.origin.y + 156.0 + 20.0);
        assert_eq!(cards[4].rect.origin.x, cards[0].rect.origin.x);
    }

    #[test]
    fn cards_below_the_viewport_are_not_clickable() {
        let files = files(40);
        let screen = FilesScreen {
            theme: &Theme::default(),
            files: &files,
            loading: false,
            error: None,
            query: "",
        };
        let cards = screen.cards(screen_rect());
        assert!(cards.len() < files.len(), "some cards must fall outside");
        for card in &cards {
            assert!(card.rect.origin.y + card.rect.size.y <= 800.0);
        }
    }

    #[test]
    fn the_header_holds_a_search_field_and_a_new_button_inside_the_screen() {
        let rect = screen_rect();
        let search = FilesScreen::search_rect(rect);
        let button = FilesScreen::new_button_rect(rect);
        assert!(search.origin.x >= rect.origin.x);
        assert!(search.origin.y > rect.origin.y);
        assert!(button.origin.x + button.size.x <= rect.origin.x + rect.size.x);
        // They must not overlap: the search field ends well before the button.
        assert!(search.origin.x + search.size.x <= button.origin.x);
    }

    #[test]
    fn the_label_reads_as_a_person_would_say_it() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        assert_eq!(updated_label(now - 10), "Edited just now");
        assert_eq!(updated_label(now - 600), "Edited 10 min ago");
        assert_eq!(updated_label(now - 7_200), "Edited 2 h ago");
        assert_eq!(updated_label(now - 172_800), "Edited 2 d ago");
        assert_eq!(updated_label(0), "Edited recently");
    }
}
