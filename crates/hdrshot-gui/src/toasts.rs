//! Short messages over the window, and the card for copies too big for Discord.

use super::*;

impl App {
    pub(crate) fn show_toast(&mut self, text: String, ctx: &egui::Context) {
        self.toast = Some(Toast {
            text,
            until: ctx.input(|i| i.time) + TOAST_SECONDS,
        });
    }

    /// Shows the outcome of a clipboard copy. One too large for Discord
    /// stays up with an offer to copy a smaller version.
    pub(crate) fn copied_ui(&mut self, copied: Copied, ctx: &egui::Context) {
        self.too_big = copied.too_big.map(|too_big| (too_big, false));
        self.show_toast(copied.message, ctx);
    }

    pub(crate) fn too_big_ui(&mut self, ctx: &egui::Context) {
        let Some((too_big, working)) = &self.too_big else {
            return;
        };
        let mut make_smaller = false;
        let mut close = false;
        egui::Area::new(egui::Id::unique("too big"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 56.0))
            .show(ctx, |ui| {
                let visuals = ui.visuals();
                egui::Frame::new()
                    .fill(visuals.extreme_bg_color)
                    .stroke(visuals.window_stroke)
                    .corner_radius(8.0)
                    .shadow(visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(card_width(ctx));
                        ui.label(
                            RichText::new(format!(
                                "Copied {}. At {} it is over Discord's 10 MB limit.",
                                too_big.name,
                                hdrshot_core::share::megabytes(too_big.size)
                            ))
                            .size(14.0),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if *working {
                                ui.spinner();
                                ui.label("Making a smaller copy…");
                            } else {
                                make_smaller = ui
                                    .button("Copy a smaller version")
                                    .on_hover_text(
                                        "Copy it at 1440p, or lower if needed, so it fits \
                                         in 10 MB. The saved PNGs stay as they are.",
                                    )
                                    .clicked();
                                close = ui
                                    .button("Keep this one")
                                    .on_hover_text("Keep the full-size copy on the clipboard")
                                    .clicked();
                            }
                        });
                    });
            });
        if close {
            self.too_big = None;
            self.toast = None;
        } else if make_smaller {
            let clipboard = self.clipboard.clone();
            let options = self.convert_options(Existing::Replace);
            let too_big = too_big.clone();
            self.too_big = Some((too_big.clone(), true));
            convert::in_background(self.copied.0.clone(), ctx.clone(), move || {
                Copied::from(convert::copy_smaller(&clipboard, &too_big, &options))
            });
        }
    }

    /// "Removed 5 images" with Undo, for a few seconds after removing
    /// files; longer while the pointer is on it. Returns whether it shows.
    pub(crate) fn undo_ui(&mut self, ctx: &egui::Context) -> bool {
        let running = self.running();
        let Some(removed) = &mut self.removed else {
            return false;
        };
        let now = ctx.input(|i| i.time);
        if now >= removed.until || running {
            self.removed = None;
            return false;
        }
        let count = removed.rows.len();
        let mut undo = false;
        let area = egui::Area::new(egui::Id::unique("undo"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 56.0))
            .show(ctx, |ui| {
                let visuals = ui.visuals();
                egui::Frame::new()
                    .fill(visuals.extreme_bg_color)
                    .stroke(visuals.window_stroke)
                    .corner_radius(8.0)
                    .shadow(visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Removed {}", images(count))).size(14.0),
                            );
                            ui.add_space(8.0);
                            let shortcut = if cfg!(target_os = "macos") {
                                "Cmd+Z"
                            } else {
                                "Ctrl+Z"
                            };
                            undo = ui
                                .button("Undo")
                                .on_hover_text(format!("Put them back ({shortcut})"))
                                .clicked();
                        });
                    });
            });
        if area.response.contains_pointer() {
            removed.until = removed.until.max(now + 2.0);
        }
        if undo {
            self.undo_remove();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(removed.until - now));
        }
        true
    }

    pub(crate) fn toast_ui(&mut self, ctx: &egui::Context) {
        while let Ok(copied) = self.copied.1.try_recv() {
            self.copied_ui(copied, ctx);
        }
        if self.too_big.is_some() {
            self.too_big_ui(ctx);
            return;
        }
        // Below Undo, if it shows.
        let top = if self.undo_ui(ctx) { 104.0 } else { 56.0 };
        let Some(toast) = &self.toast else { return };
        let left = toast.until - ctx.input(|i| i.time);
        if left <= 0.0 {
            self.toast = None;
            return;
        }
        // Fade out over the last half second.
        let opacity = (left / 0.5).min(1.0) as f32;
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, top))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                let visuals = ui.visuals();
                egui::Frame::new()
                    .fill(visuals.extreme_bg_color)
                    .stroke(visuals.window_stroke)
                    .corner_radius(8.0)
                    .shadow(visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(card_width(ctx));
                        ui.label(RichText::new(&toast.text).size(14.0));
                    });
            });
        ctx.request_repaint();
    }
}

pub(crate) struct Toast {
    pub(crate) text: String,
    /// `egui` time at which it disappears.
    pub(crate) until: f64,
}

pub(crate) const TOAST_SECONDS: f64 = 3.0;

/// Widest a toast may be: the window's width less a margin on each side,
/// so long file names wrap instead of running off the window.
pub(crate) fn card_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 2.0 * 16.0 - 2.0 * 14.0).max(120.0)
}
