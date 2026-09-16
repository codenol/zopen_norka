//! `impl Widget for LayerPanel` — the panel's id, layout box, row paint
//! pass and accessibility node.
//!
//! This is the paint side of the panel: `layer_panel.rs` keeps the row
//! model and the constructors that build it, `layer_panel_hit.rs` owns
//! hit-test and drop-target resolution, `layer_panel_walkers.rs` owns the
//! tree walks, and `layer_panel_paint.rs` owns the leaf painters this file
//! calls. Split out of `layer_panel.rs` at the 800-line cap; pure code
//! motion, so the row-paint order here IS the z-order the hit-test mirrors
//! in reverse.

use super::layer_panel::*;
use super::layer_panel_metrics::{
    add_page_target, collapse_target, glyph_rect_in, layer_action_targets, layer_drag_target,
    layer_node_icon_x,
};
use super::layer_panel_paint::{
    layer_content_clip_rect_with_metrics, layer_label_available_width_with_metrics,
    paint_drag_ghost, paint_layer_action_backing, paint_layer_drag_handle, paint_page_rows,
    paint_rename_input_with_metrics, paint_section_header_with_metrics, truncate_to_fit_measured,
};
use super::layer_panel_walkers::visible_row_range;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::{LayoutBox, LayoutCx, PaintCx, Widget, WidgetId};
use crate::{Color, Point2D, Rect, TextLayout};

impl Widget for LayerPanel {
    fn id(&self) -> WidgetId {
        self.id
    }

    fn layout(&self, cx: &LayoutCx) -> LayoutBox {
        LayoutBox {
            rect: Rect {
                origin: Point2D::new(0.0, 0.0),
                size: Point2D::new(cx.available_width, self.intrinsic_height()),
            },
        }
    }

    fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        // Card background.
        cx.backend.fill_rect(rect, self.theme.card);

        // Right-edge hairline so the LayerPanel reads as a
        // distinct surface from the canvas next to it.
        cx.backend.fill_rect(
            Rect {
                origin: Point2D::new(rect.origin.x + rect.size.x - 1.0, rect.origin.y),
                size: Point2D::new(1.0, rect.size.y),
            },
            self.theme.border,
        );

        let r = self.regions(rect);

        // Pages section header.
        paint_section_header_with_metrics(
            cx,
            &self.theme,
            rect.origin.x,
            r.pages_header_y,
            rect.size.x,
            self.pages_label,
            self.metrics,
        );
        // "+" add-page affordance, top-right of header row.
        let plus = glyph_rect_in(
            add_page_target(rect, r.pages_header_y, self.metrics),
            self.metrics.glyph_size,
        );
        draw_icon(
            cx.backend,
            Icon::Plus,
            plus.origin,
            self.metrics.glyph_size,
            self.theme.muted_foreground,
            1.4,
        );
        paint_page_rows(
            cx,
            &self.theme,
            rect,
            &self.pages,
            Some(&self.page_comments),
            r.pages_rows_top,
            r.pages_view_h,
            r.pages.offset,
            self.hovered_page,
            self.rename_input.as_ref(),
            self.now_ms,
            self.metrics,
            true,
        );

        if !self.components.is_empty() {
            cx.backend.fill_rect(
                Rect {
                    origin: Point2D::new(
                        rect.origin.x + self.metrics.row_pad_x,
                        r.components_header_y - self.metrics.section_gap / 2.0,
                    ),
                    size: Point2D::new(rect.size.x - self.metrics.row_pad_x * 2.0, 1.0),
                },
                self.theme.border,
            );
            paint_section_header_with_metrics(
                cx,
                &self.theme,
                rect.origin.x,
                r.components_header_y,
                rect.size.x,
                self.components_label,
                self.metrics,
            );
            paint_page_rows(
                cx,
                &self.theme,
                rect,
                &self.components,
                // A component-store page is a document page like any other, so a
                // conversation pinned on one is shown on its row: the row is
                // where a reviewer looks for the page, whichever section holds
                // it.
                Some(&self.page_comments),
                r.components_rows_top,
                r.components_view_h,
                r.components.offset,
                self.hovered_page,
                None,
                self.now_ms,
                self.metrics,
                false,
            );
        }

        if !self.recipes.is_empty() {
            cx.backend.fill_rect(
                Rect {
                    origin: Point2D::new(
                        rect.origin.x + self.metrics.row_pad_x,
                        r.recipes_header_y - self.metrics.section_gap / 2.0,
                    ),
                    size: Point2D::new(rect.size.x - self.metrics.row_pad_x * 2.0, 1.0),
                },
                self.theme.border,
            );
            paint_section_header_with_metrics(
                cx,
                &self.theme,
                rect.origin.x,
                r.recipes_header_y,
                rect.size.x,
                self.recipes_label,
                self.metrics,
            );
            // Recipe names are longer than page names, so they are clipped
            // by measurement here rather than by the row painter's estimate —
            // an estimated fit let a 26-character name run past the rail.
            let label_x = rect.origin.x + 6.0 + 12.0;
            let label_max_x = rect.origin.x + rect.size.x - self.metrics.row_pad_x - 18.0;
            let available_w = (label_max_x - label_x).max(0.0);
            let recipe_rows: Vec<PageItem> = self
                .recipes
                .iter()
                .map(|recipe| PageItem {
                    page_index: recipe.page_index,
                    label: truncate_to_fit_measured(
                        cx.backend,
                        &recipe.label,
                        self.metrics.row_font,
                        available_w,
                    ),
                    active: false,
                    renaming: false,
                })
                .collect();
            paint_page_rows(
                cx,
                &self.theme,
                rect,
                &recipe_rows,
                // Recipes are shipped kit compositions, not pages of this
                // document: there is no page for a comment to be pinned on, so
                // the section carries no counts at all.
                None,
                r.recipes_rows_top,
                r.recipes_view_h,
                r.recipes.offset,
                self.hovered_page,
                None,
                self.now_ms,
                self.metrics,
                false,
            );
        }

        let mut y = r.layers_header_y;
        // Hairline between Pages and Layers sections — mirrors
        // the TS LayerPanel's `border-t border-border`.
        cx.backend.fill_rect(
            Rect {
                origin: Point2D::new(
                    rect.origin.x + self.metrics.row_pad_x,
                    y - self.metrics.section_gap / 2.0,
                ),
                size: Point2D::new(rect.size.x - self.metrics.row_pad_x * 2.0, 1.0),
            },
            self.theme.border,
        );

        // Layers section header.
        paint_section_header_with_metrics(
            cx,
            &self.theme,
            rect.origin.x,
            y,
            rect.size.x,
            self.layers_label,
            self.metrics,
        );
        // Layer rows — clipped + scrolled inside the bounded viewport.
        cx.backend.save();
        cx.backend.clip_rect(Rect {
            origin: Point2D::new(rect.origin.x, r.layers_rows_top),
            size: Point2D::new(rect.size.x, r.layers_view_h),
        });
        for index in visible_row_range(
            self.items.len(),
            r.layers.offset,
            r.layers_view_h,
            self.metrics.layer_row_height,
        ) {
            let item = &self.items[index];
            // Live styling overlay — never baked into the cached item.
            let selected = self.is_row_selected(&item.node_id);
            let hovered = self.is_row_hovered(&item.node_id);
            y = r.layers_rows_top - r.layers.offset + index as f32 * self.metrics.layer_row_height;
            let row = Rect {
                origin: Point2D::new(rect.origin.x + 6.0, y + 2.0),
                size: Point2D::new(rect.size.x - 12.0, self.metrics.layer_row_height - 4.0),
            };
            if selected {
                // TS uses bg-blue-500/15 + primary text + primary
                // icon for the selected layer row.
                cx.backend
                    .fill_round_rect(row, 6.0, self.theme.row_selected_primary);
            } else if hovered {
                cx.backend
                    .fill_round_rect(row, 6.0, self.theme.button_hover);
            }

            let indent = self.metrics.row_pad_x + item.depth as f32 * 12.0;
            let dim = |c: Color, factor: f32| -> Color {
                Color {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                    a: c.a * factor,
                }
            };
            let dim_factor = if item.hidden { 0.45 } else { 1.0 };
            // Component / instance rows tint purple like TS
            // (`text-purple-400` #a855f7 / instance #9281f7);
            // selection still wins so the selected row reads as one.
            let component_tint = if item.is_reusable {
                Some(crate::widgets::property_panel_inputs::COMPONENT_ACCENT)
            } else if item.is_instance {
                Some(crate::widgets::property_panel_inputs::INSTANCE_ACCENT)
            } else {
                None
            };
            let icon_color = if selected {
                dim(self.theme.primary, dim_factor)
            } else if let Some(tint) = component_tint {
                dim(tint, dim_factor)
            } else {
                dim(self.theme.muted_foreground, dim_factor)
            };
            let content_clip =
                layer_content_clip_rect_with_metrics(row, item.renaming, self.metrics);
            cx.backend.save();
            cx.backend.clip_rect(content_clip);
            cx.backend
                .translate(Point2D::new(-r.layers.horizontal_offset, 0.0));
            if item.has_children {
                let chev_icon = if item.collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                };
                let chevron = glyph_rect_in(
                    collapse_target(row, indent, 0.0, self.metrics),
                    self.metrics.glyph_size,
                );
                draw_icon(
                    cx.backend,
                    chev_icon,
                    chevron.origin,
                    self.metrics.glyph_size,
                    icon_color,
                    1.4,
                );
            }
            let icon_x = layer_node_icon_x(row, indent, self.metrics);
            let icon_y = if self.metrics.touch {
                row.origin.y + (row.size.y - self.metrics.glyph_size) / 2.0
            } else {
                row.origin.y + 6.0
            };
            draw_icon(
                cx.backend,
                item.icon,
                Point2D::new(icon_x, icon_y),
                self.metrics.glyph_size,
                icon_color,
                1.4,
            );
            let label_color = if selected {
                dim(self.theme.primary, dim_factor)
            } else if let Some(tint) = component_tint {
                dim(tint, dim_factor)
            } else {
                dim(self.theme.card_foreground, dim_factor)
            };
            let label_x =
                icon_x + self.metrics.glyph_size + if self.metrics.touch { 8.0 } else { 6.0 };
            let available_w = layer_label_available_width_with_metrics(
                row,
                label_x,
                r.layers.horizontal_offset,
                item.renaming,
                self.metrics,
            );
            if item.renaming {
                paint_rename_input_with_metrics(
                    cx,
                    &self.theme,
                    self.rename_input.as_ref().expect("renaming row has input"),
                    label_x,
                    row.origin.y,
                    available_w,
                    self.now_ms,
                    self.metrics,
                );
            } else {
                let display = truncate_to_fit_measured(
                    cx.backend,
                    &item.label,
                    self.metrics.row_font,
                    available_w,
                );
                let label = TextLayout::single_run(
                    &display,
                    "system-ui",
                    self.metrics.row_font,
                    (label_color).to_jian(),
                    Point2D::new(0.0, 0.0),
                );
                let baseline = if self.metrics.touch {
                    jian_widgets::centered_text_baseline_y(row, self.metrics.row_font)
                } else {
                    row.origin.y + 17.0
                };
                cx.backend
                    .draw_text(&label, Point2D::new(label_x, baseline));
            }
            cx.backend.restore();
            let (eye_target, lock_target) = layer_action_targets(row, self.metrics);
            let drag_target = layer_drag_target(row, self.metrics).filter(|_| !item.renaming);
            let eye_icon = if item.hidden { Icon::EyeOff } else { Icon::Eye };
            let lock_icon = if item.locked {
                Icon::Lock
            } else {
                Icon::LockOpen
            };
            let trailing_default = if selected {
                dim(self.theme.primary, dim_factor)
            } else {
                dim(self.theme.muted_foreground, dim_factor)
            };
            let state_color = |r, g, b| Color { r, g, b, a: 1.0 };
            let eye_hidden = state_color(0.98039216, 0.8, 0.08235294);
            let lock_locked = state_color(0.92, 0.49, 0.20);
            let eye_color = if item.hidden {
                eye_hidden
            } else {
                trailing_default
            };
            let lock_color = if item.locked {
                lock_locked
            } else {
                trailing_default
            };
            let trailing_size = self.metrics.trailing_glyph_size;
            let trailing_stroke = 1.2;
            let eye_glyph = glyph_rect_in(eye_target, trailing_size);
            let lock_glyph = glyph_rect_in(lock_target, trailing_size);
            let show_eye = (hovered || self.metrics.touch) && !item.renaming;
            let show_lock = (hovered || self.metrics.touch) && !item.renaming;
            paint_layer_action_backing(
                cx,
                row,
                self.metrics,
                &self.theme,
                selected,
                hovered,
                drag_target.is_some() || show_eye || show_lock,
            );
            paint_layer_drag_handle(cx, drag_target, trailing_default);
            if show_eye {
                draw_icon(
                    cx.backend,
                    eye_icon,
                    eye_glyph.origin,
                    trailing_size,
                    eye_color,
                    trailing_stroke,
                );
            }
            if show_lock {
                draw_icon(
                    cx.backend,
                    lock_icon,
                    lock_glyph.origin,
                    trailing_size,
                    lock_color,
                    trailing_stroke,
                );
            }
        }

        // Drop-indicator — paints AFTER row chrome so it sits on top.
        // Before/After: a 2 px horizontal line between rows.
        // Into: a 1.5 px outline around the target row (signals
        // "drop becomes child of this container").
        if let Some(drop) = &self.drop_target {
            match drop.position {
                DropPosition::Before | DropPosition::After => {
                    let indicator_rect = Rect {
                        origin: Point2D::new(
                            rect.origin.x + self.metrics.row_pad_x,
                            drop.indicator_y - 1.0,
                        ),
                        size: Point2D::new(rect.size.x - self.metrics.row_pad_x * 2.0, 2.0),
                    };
                    cx.backend.fill_rect(indicator_rect, self.theme.primary);
                }
                DropPosition::Into => {
                    // 1.5 px outline rect spanning the full row.
                    let outline = Rect {
                        origin: Point2D::new(rect.origin.x + 6.0, drop.indicator_y + 2.0),
                        size: Point2D::new(rect.size.x - 12.0, self.metrics.layer_row_height - 4.0),
                    };
                    cx.backend
                        .stroke_round_rect(outline, 6.0, self.theme.primary, 1.5);
                }
            }
        }

        cx.backend.restore();

        if let Some((ghost, cursor_y)) = &self.drag_ghost {
            paint_drag_ghost(cx, &self.theme, ghost, *cursor_y, rect, self.metrics);
        }
    }

    fn access_node(&self) -> accesskit::Node {
        let mut node = accesskit::Node::new(accesskit::Role::Tree);
        node.set_label("Layers");
        node
    }
}
