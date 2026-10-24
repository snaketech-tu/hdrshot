//! Running in the background on Windows: closing the window leaves hdrshot
//! in the tray, where it keeps watching, it can start there at sign-in,
//! and it says with a notification when images were converted.

use tray::{Tray, TrayEvent, WatchItem};

use super::*;

impl App {
    /// Adds or removes the tray icon to match the settings, keeps its
    /// tooltip and menu up to date and acts on what the user did with it.
    pub(crate) fn sync_tray(&mut self, ctx: &egui::Context) {
        if !self.settings.tray {
            self.tray = None;
        } else if self.tray.is_none() && !self.tray_failed {
            self.tray = Tray::new(self.tray_events.0.clone(), ctx.clone());
            self.tray_failed = self.tray.is_none();
        }
        // With no icon to open it from, the window can't stay hidden.
        if self.hidden && self.tray.is_none() {
            self.show_window(ctx);
        }
        let events: Vec<_> = self.tray_events.1.try_iter().collect();
        for event in events {
            match event {
                TrayEvent::Open => self.show_window(ctx),
                TrayEvent::TogglePause => self.watch_paused = !self.watch_paused,
                TrayEvent::Quit => self.quit = true,
            }
        }
        // A question about files that exist already needs the window.
        if self.hidden && self.asking.is_some() {
            self.show_window(ctx);
        }
        if let Some(tray) = &self.tray {
            tray.update(&self.tray_tip(), self.watch_item());
        }
    }

    /// The tray icon's tooltip, e.g. "hdrshot: watching Captures".
    fn tray_tip(&self) -> String {
        if self.running() {
            let current = (self.batch.done + 1).min(self.batch.total);
            return format!("hdrshot: converting {current} of {}", self.batch.total);
        }
        match (&self.settings.watch_dir, self.settings.watch) {
            (Some(_), true) if self.watch_paused => "hdrshot: watching paused".to_owned(),
            (Some(dir), true) => format!("hdrshot: watching {}", folder_name(dir)),
            _ => "hdrshot".to_owned(),
        }
    }

    fn watch_item(&self) -> WatchItem {
        if !self.settings.watch || self.settings.watch_dir.is_none() {
            WatchItem::None
        } else if self.watch_paused {
            WatchItem::Resume
        } else {
            WatchItem::Pause
        }
    }

    /// Shows the window, hidden in the tray or minimized, in front.
    pub(crate) fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// The window was closed and hidden; the first time, a notification
    /// says where hdrshot went.
    pub(crate) fn hidden_in_tray(&mut self) {
        self.hidden = true;
        self.viewer = None;
        if self.settings.tray_note_shown {
            return;
        }
        let Some(tray) = &self.tray else { return };
        let what = match (&self.settings.watch_dir, self.settings.watch) {
            (Some(dir), true) => format!(
                "It keeps converting new images saved in {}. ",
                folder_name(dir)
            ),
            _ => String::new(),
        };
        tray.notify(
            "hdrshot is still running",
            &format!(
                "{what}Click its icon in the notification area to open it again, or \
                 right-click the icon to quit."
            ),
        );
        self.settings.tray_note_shown = true;
    }

    /// Says how the batch that just finished went with a notification,
    /// if the window is hidden or in the background.
    pub(crate) fn notify_batch(&self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };
        let background = self.hidden || !ctx.input(|i| i.focused);
        if !background || !self.settings.notify || self.batch.outcome() == Outcome::Nothing {
            return;
        }
        // One image, as from the watch folder, is named.
        let name = self
            .last_converted
            .as_ref()
            .and_then(|p| p.file_name())
            .filter(|_| self.batch.total == 1 && self.batch.written() == 1);
        let title = match name {
            Some(name) => format!("Converted {}", name.to_string_lossy()),
            None => self.batch.title(),
        };
        tray.notify(&title, &self.batch.details().replace(" · ", "\n"));
    }

    /// The tray, the start at sign-in and the notifications: rows of a
    /// two-column grid, on the Settings page.
    pub(crate) fn background_rows(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let s = &mut self.settings;
        theme::row_label(ui, "Tray");
        if ui
            .checkbox(
                &mut s.tray,
                "Keep running in the tray when the window is closed",
            )
            .on_hover_text(
                "Closing the window leaves hdrshot in the notification area (the \
                 system tray), where it keeps watching the watch folder. Click its \
                 icon to open the window again; right-click it to pause watching or \
                 quit.",
            )
            .changed()
            && !s.tray
            && s.start_with_windows
        {
            s.start_with_windows = false;
            if let Err(e) = startup::set(false) {
                self.show_toast(e, &ctx);
            }
        }
        ui.end_row();

        let s = &mut self.settings;
        let tray = s.tray;
        if startup::SUPPORTED {
            theme::row_label(ui, "Startup");
            let response = ui
                .add_enabled(
                    tray,
                    egui::Checkbox::new(
                        &mut s.start_with_windows,
                        "Start with Windows, in the tray",
                    ),
                )
                .on_hover_text(
                    "Start hdrshot in the tray when you sign in to Windows, so new \
                     screenshots are converted without opening it first",
                )
                .on_disabled_hover_text("Turn on Keep running in the tray first");
            if response.changed() {
                let on = s.start_with_windows;
                if let Err(e) = startup::set(on) {
                    s.start_with_windows = !on;
                    self.show_toast(e, &ctx);
                }
            }
            ui.end_row();
        }

        let s = &mut self.settings;
        theme::row_label(ui, "Notifications");
        ui.add_enabled(
            tray,
            egui::Checkbox::new(
                &mut s.notify,
                "Notify when images are converted in the background",
            ),
        )
        .on_hover_text(
            "When images are converted while the window is closed or behind \
             other windows, say so with a notification. Click it to open hdrshot.",
        )
        .on_disabled_hover_text("Turn on Keep running in the tray first");
        ui.end_row();
    }
}
