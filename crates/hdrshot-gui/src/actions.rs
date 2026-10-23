//! The bar under the file list: the banner summing up the last batch, the
//! settings chips, and the Convert button, which shows the progress while
//! a batch runs.

use super::*;

impl App {
    pub(crate) fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        if self.banner && !self.running() {
            self.banner_ui(ui);
            ui.add_space(10.0);
        }
        ui.add_enabled_ui(!self.running(), |ui| self.summary_chips(ui));
        ui.add_space(8.0);
        self.convert_row(ui);
        // Room for the update button, which is taller than the credit line
        // it sits on.
        ui.add_space(10.0);
        let row = credit(ui);
        self.update_button(ui, row);
    }

    /// How the last batch went, with Retry failed and Open folder.
    fn banner_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut close = false;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                let painter = ui.painter();
                let weak = ui.visuals().weak_text_color();
                match self.batch.outcome() {
                    Outcome::Done => icons::done(painter, rect.center(), 10.0, ok_color(ui)),
                    Outcome::Problem => {
                        icons::failed(painter, rect.center(), 10.0, ui.visuals().error_fg_color)
                    }
                    Outcome::Nothing => icons::skipped(painter, rect.center(), 10.0, weak),
                }
                ui.add_space(2.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::small_icon_button(ui, icons::cross)
                        .on_hover_text("Close")
                        .clicked()
                    {
                        close = true;
                    }
                    if let Some(dir) = self.output_folder()
                        && ui
                            .button("Open folder")
                            .on_hover_text(dir.display().to_string())
                            .clicked()
                    {
                        reveal(&dir);
                    }
                    let failed = self.failed();
                    if !failed.is_empty()
                        && ui
                            .add_enabled(self.can_convert(), egui::Button::new("Retry failed"))
                            .on_hover_text("Convert the files that failed again")
                            .clicked()
                    {
                        self.start_some(failed, &ctx);
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(self.batch.title()).font(theme::semibold(14.0)));
                        let details = self.batch.details();
                        if !details.is_empty() {
                            ui.label(RichText::new(details).size(12.5).weak());
                        }
                    });
                });
            });
        });
        if close {
            self.banner = false;
        }
    }

    /// The Convert button, or while a batch runs its progress and Stop.
    fn convert_row(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(job) = &self.job {
            ui.horizontal(|ui| {
                let stopping = job.is_cancelled();
                let label = if stopping { "Stopping…" } else { "Stop" };
                let stop_width = 90.0;
                let width = ui.available_width() - stop_width - ui.spacing().item_spacing.x;
                let now = ctx.input(|i| i.time);
                theme::progress_bar(
                    ui,
                    width,
                    self.batch.fraction(now),
                    &self.batch.progress_text(now),
                );
                if ui
                    .add_enabled(!stopping, theme::secondary_button(label))
                    .on_hover_text("Stop after the current image")
                    .clicked()
                {
                    job.cancel();
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pending = self.pending();
                // Once everything is converted, converting again is still
                // possible, as after changing the settings.
                let again = pending.is_empty() && !self.files.is_empty();
                let button = if again {
                    theme::secondary_button("Convert again")
                } else {
                    let label = match pending.len() {
                        0 => "Convert".to_owned(),
                        n => format!("Convert {}", images(n)),
                    };
                    theme::primary(ui, theme::primary_button(&label))
                };
                let mut response = ui.add_enabled(self.can_convert(), button);
                if again {
                    response = response.on_hover_text("Convert all the files again");
                }
                if self.settings.save_to_folder && self.settings.output_dir.is_none() {
                    response =
                        response.on_disabled_hover_text("Choose a folder to save into first");
                }
                if response.clicked() {
                    if again {
                        self.start(&ctx);
                    } else {
                        self.start_some(pending, &ctx);
                    }
                }
                if let Some(notice) = &self.notice {
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(notice).size(12.5).weak()).wrap());
                    });
                }
            });
        });
    }
}
