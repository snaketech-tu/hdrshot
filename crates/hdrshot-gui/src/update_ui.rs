//! The update check: its button, dialog and background work.

use super::*;

impl App {
    pub(crate) fn check_for_updates(&mut self, ctx: &egui::Context) {
        self.update = Update::Checking;
        let sender = self.updates.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(UpdateMessage::Checked(update::check()));
            ctx.request_repaint();
        });
    }

    pub(crate) fn install_update(&mut self, ctx: &egui::Context) {
        let Update::Available { releases, .. } = std::mem::take(&mut self.update) else {
            return;
        };
        let Some(download) = releases[0].download.clone() else {
            return;
        };
        self.update = Update::Installing(releases);
        let sender = self.updates.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(UpdateMessage::Installed(update::install(&download)));
            ctx.request_repaint();
        });
    }

    /// Saves the settings and ends the app, for an update.
    pub(crate) fn quit(&mut self) -> ! {
        storage::save(SETTINGS_KEY, &self.settings);
        std::process::exit(0)
    }

    pub(crate) fn receive_update(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.updates.1.try_recv() {
            match message {
                UpdateMessage::Checked(Ok(update::Checked::UpToDate)) => {
                    self.update = Update::Idle;
                    let text = format!("hdrshot {} is the latest version", update::CURRENT);
                    self.show_toast(text, ctx);
                }
                UpdateMessage::Checked(Ok(update::Checked::Newer(releases))) => {
                    self.update = Update::Available {
                        releases,
                        error: None,
                    };
                }
                UpdateMessage::Checked(Err(e)) => {
                    self.update = Update::Idle;
                    self.show_toast(e, ctx);
                }
                UpdateMessage::Installed(Ok(update::Installed::Restart(exe))) => {
                    match update::restart(&exe) {
                        Ok(()) => self.quit(),
                        Err(_) => {
                            self.update = Update::Idle;
                            let text = "The update is installed. Start hdrshot again to use it.";
                            self.show_toast(text.into(), ctx);
                        }
                    }
                }
                UpdateMessage::Installed(Ok(update::Installed::Opened)) => {
                    self.update = Update::Opened;
                }
                UpdateMessage::Installed(Err(e)) => {
                    if let Update::Installing(releases) = std::mem::take(&mut self.update) {
                        self.update = Update::Available {
                            releases,
                            error: Some(e),
                        };
                    }
                }
            }
        }
    }

    /// The release notes of a newer version, with Update and Skip.
    pub(crate) fn update_dialog(&mut self, ctx: &egui::Context) {
        self.receive_update(ctx);
        #[derive(Clone, Copy)]
        enum Choice {
            Update,
            Page,
            Skip,
            Quit,
        }
        let mut choice = None;
        let running = self.running();
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(16);
        match &self.update {
            Update::Idle | Update::Checking => return,
            Update::Available { releases, error } => {
                let newest = &releases[0];
                let modal = egui::Modal::new(egui::Id::unique("update"))
                    .frame(frame)
                    .show(ctx, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(
                            RichText::new(format!("hdrshot {} is available", newest.version))
                                .strong()
                                .size(15.0),
                        );
                        ui.weak(format!("You have version {}.", update::CURRENT));
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                for release in releases {
                                    if releases.len() > 1 {
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(format!("Version {}", release.version))
                                                .strong()
                                                .size(14.0),
                                        );
                                    }
                                    notes_ui(ui, &release.notes);
                                }
                            });
                        if let Some(error) = error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        let managed = update::package_manager_command();
                        if let Some(command) = managed {
                            ui.label(
                                "This copy was installed with a package manager. Update it with:",
                            );
                            ui.code(command);
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if let Some(command) = managed {
                                if ui.button("Copy command").clicked() {
                                    ctx.copy_text(command.to_owned());
                                    choice = Some(Choice::Skip);
                                }
                            } else if newest.download.is_some() && error.is_none() {
                                let hint = if cfg!(target_os = "macos") {
                                    "Download the new version and open its disk image"
                                } else {
                                    "Download the new version and restart hdrshot"
                                };
                                if ui
                                    .add_enabled(!running, egui::Button::new("Update"))
                                    .on_hover_text(hint)
                                    .on_disabled_hover_text("Wait for the conversion to finish")
                                    .clicked()
                                {
                                    choice = Some(Choice::Update);
                                }
                            } else if ui
                                .button("Open download page")
                                .on_hover_text(&newest.page)
                                .clicked()
                            {
                                choice = Some(Choice::Page);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button("Skip")
                                        .on_hover_text("Keep this version for now")
                                        .clicked()
                                    {
                                        choice = Some(Choice::Skip);
                                    }
                                },
                            );
                        });
                    });
                if choice.is_none() && modal.should_close() {
                    choice = Some(Choice::Skip);
                }
                if let Some(Choice::Page) = choice {
                    ctx.open_url(egui::OpenUrl::new_tab(&newest.page));
                }
            }
            Update::Installing(releases) => {
                egui::Modal::new(egui::Id::unique("update"))
                    .frame(frame)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Downloading hdrshot {}…", releases[0].version));
                        });
                    });
            }
            Update::Opened => {
                let modal = egui::Modal::new(egui::Id::unique("update"))
                    .frame(frame)
                    .show(ctx, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(
                            RichText::new("The new version is ready")
                                .strong()
                                .size(15.0),
                        );
                        ui.label(
                            "Quit hdrshot, then drag hdrshot from the opened disk image \
                             to Applications and choose Replace.",
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button("Quit hdrshot").clicked() {
                                choice = Some(Choice::Quit);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Close").clicked() {
                                        choice = Some(Choice::Skip);
                                    }
                                },
                            );
                        });
                    });
                if choice.is_none() && modal.should_close() {
                    choice = Some(Choice::Skip);
                }
            }
        }
        match choice {
            Some(Choice::Update) => self.install_update(ctx),
            Some(Choice::Page | Choice::Skip) => self.update = Update::Idle,
            Some(Choice::Quit) => self.quit(),
            None => {}
        }
    }

    /// The small blue "Check for updates" button at the right end of the
    /// credit line `row`.
    pub(crate) fn update_button(&mut self, ui: &mut egui::Ui, row: egui::Rect) {
        let checking = matches!(self.update, Update::Checking);
        let text = if checking {
            "Checking…"
        } else {
            "Check for updates"
        };
        let label = RichText::new(text)
            .font(egui::FontId::proportional(12.0))
            .color(theme::accent(ui));
        let width = ui
            .painter()
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(12.0),
                Color32::PLACEHOLDER,
            )
            .size()
            .x
            + 24.0;
        let right = ui.max_rect().right();
        let rect = egui::Rect::from_center_size(
            egui::pos2(right - width / 2.0, row.center().y),
            Vec2::new(width, 22.0),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
        ));
        let button = egui::Button::new(label)
            .fill(theme::accent(ui).gamma_multiply(0.14))
            .stroke(Stroke::NONE)
            .corner_radius(6.0)
            .wrap_mode(egui::TextWrapMode::Extend);
        if child
            .add_enabled(!checking, button)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("You have hdrshot {}", update::CURRENT))
            .clicked()
        {
            let ctx = ui.ctx().clone();
            self.check_for_updates(&ctx);
        }
    }
}

/// Where the check for a newer version stands.
#[derive(Default)]
pub(crate) enum Update {
    #[default]
    Idle,
    Checking,
    /// Newer releases, newest first, and why installing the newest failed.
    Available {
        releases: Vec<update::Release>,
        error: Option<String>,
    },
    Installing(Vec<update::Release>),
    /// The new disk image is open (macOS).
    Opened,
}

pub(crate) enum UpdateMessage {
    Checked(Result<update::Checked, String>),
    Installed(Result<update::Installed, String>),
}

/// Release notes, as headings, list items and paragraphs.
pub(crate) fn notes_ui(ui: &mut egui::Ui, notes: &str) {
    for block in update::parse_notes(notes) {
        match block {
            update::Block::Heading(text) => {
                ui.add_space(2.0);
                ui.label(RichText::new(text).strong());
            }
            update::Block::Text(text) => {
                ui.label(text);
            }
            update::Block::Item(text) => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let height = ui.text_style_height(&egui::TextStyle::Body);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, height), Sense::hover());
                    let color = ui.visuals().text_color();
                    ui.painter().circle_filled(rect.center(), 2.0, color);
                    ui.add(egui::Label::new(text).wrap());
                });
            }
        }
    }
}
