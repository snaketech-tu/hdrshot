//! Starting and following a batch of conversions.

use super::*;

impl App {
    pub(crate) fn convert_options(&self, existing: Existing) -> ConvertOptions {
        let t = &self.tuning;
        ConvertOptions {
            mode: self.settings.mode.into(),
            input: InputOptions {
                white_nits: t.fixed_input_white.then_some(t.input_white),
            },
            output_dir: if self.settings.save_to_folder {
                self.settings.output_dir.clone()
            } else {
                None
            },
            naming: self.settings.naming(),
            hdr: HdrOptions {
                format: self.settings.hdr_format.into(),
                peak_nits: t.limit_hdr_peak.then_some(t.hdr_peak),
                for_discord: self.settings.discord_png,
                ..Default::default()
            },
            sdr: SdrOptions {
                white_nits: t.sdr_white,
                peak_nits: t.fixed_sdr_peak.then_some(t.sdr_peak),
            },
            sdr_format: self.settings.sdr_format.into(),
            max_height: self.max_height,
            existing,
            keep_date: self.settings.keep_date,
        }
    }

    /// Rows not converted yet: waiting, or failed last time.
    pub(crate) fn pending(&self) -> Vec<usize> {
        (0..self.files.len())
            .filter(|&i| matches!(self.files[i].status, Status::Waiting | Status::Failed(_)))
            .collect()
    }

    pub(crate) fn can_convert(&self) -> bool {
        !self.files.is_empty()
            && !self.running()
            && self.asking.is_none()
            && (!self.settings.save_to_folder || self.settings.output_dir.is_some())
    }

    /// Converts every listed file.
    pub(crate) fn start(&mut self, ctx: &egui::Context) {
        self.start_some((0..self.files.len()).collect(), ctx);
    }

    /// Converts the files at `indices`, leaving the other rows as they are.
    /// If some of their PNGs already exist, asks first what to do. A file
    /// that would make the same output file as an earlier one in the batch
    /// fails instead of replacing that one's result.
    pub(crate) fn start_some(&mut self, indices: Vec<usize>, ctx: &egui::Context) {
        let options = self.convert_options(Existing::Skip);
        let paths: Vec<PathBuf> = indices
            .iter()
            .map(|&i| self.files[i].path.clone())
            .collect();
        let mut kept = Vec::with_capacity(indices.len());
        let mut conflicts = Vec::new();
        for (&i, conflict) in indices
            .iter()
            .zip(hdrshot_core::output_conflicts(&paths, &options))
        {
            match conflict {
                Some((output, earlier)) => conflicts.push((
                    i,
                    format!(
                        "Not converted: {} makes the same file, {}. Save to subfolders \
                         or convert them separately.",
                        paths[earlier].display(),
                        output.display()
                    ),
                )),
                None => kept.push(i),
            }
        }
        if kept.is_empty() {
            return;
        }
        let existing = existing_outputs(kept.iter().map(|&i| &self.files[i].path), &options);
        if existing.is_empty() {
            // Nothing to overwrite, so the choice doesn't matter.
            self.run(kept, conflicts, Existing::Skip, ctx);
        } else {
            self.asking = Some(Asking {
                indices: kept,
                conflicts,
                existing,
            });
        }
    }

    /// Converts the rows `indices` and marks the rows `conflicts` failed.
    pub(crate) fn run(
        &mut self,
        indices: Vec<usize>,
        conflicts: Vec<(usize, String)>,
        existing: Existing,
        ctx: &egui::Context,
    ) {
        for (i, reason) in conflicts {
            self.files[i].status = Status::Failed(reason);
        }
        let files: Vec<_> = indices
            .into_iter()
            .map(|i| {
                let entry = &mut self.files[i];
                entry.status = Status::Waiting;
                (i, entry.path.clone())
            })
            .collect();
        let place = match (&self.settings.output_dir, self.settings.save_to_folder) {
            (Some(dir), true) => format!("in {}", folder_name(dir)),
            _ if files.len() == 1 => "next to the original".to_owned(),
            _ => "next to the originals".to_owned(),
        };
        self.batch = Batch {
            total: files.len(),
            place,
            started: ctx.input(|i| i.time),
            ..Default::default()
        };
        self.banner = false;
        self.notice = None;
        // Undo would put rows back between the batch's rows.
        self.removed = None;
        let clipboard = self
            .settings
            .copy_to_clipboard
            .then(|| self.clipboard.clone());
        self.job = Some(Job::start(
            files,
            self.convert_options(existing),
            clipboard,
            self.settings.trash_originals,
            ctx.clone(),
        ));
    }

    pub(crate) fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.take() else { return };
        let mut done = false;
        let now = ctx.input(|i| i.time);
        while let Ok(event) = job.events.try_recv() {
            match event {
                Event::Started(i) => self.files[i].status = Status::Converting,
                Event::Finished(i, result) => {
                    let entry = &mut self.files[i];
                    entry.status = match result {
                        Ok(outputs) => {
                            let (saved, bytes) = saved_summary(&outputs);
                            entry.saved = saved;
                            self.batch.bytes += bytes;
                            if outputs.hdr.is_none() && outputs.sdr.is_none() {
                                self.batch.skipped += 1;
                            } else {
                                self.last_converted = Some(entry.path.clone());
                            }
                            Status::Done(outputs)
                        }
                        Err(e) => {
                            self.batch.failed += 1;
                            Status::Failed(e)
                        }
                    };
                    self.batch.done += 1;
                    self.batch.last_finished = now;
                }
                Event::Trashed(i, result) => match result {
                    Ok(()) => self.batch.trashed += 1,
                    Err(e) => self
                        .batch
                        .trash_errors
                        .push(format!("{}: {e}", self.files[i].path.display())),
                },
                Event::Done(copied) => {
                    done = true;
                    if let Some(copied) = copied {
                        self.copied_ui(copied, ctx);
                    }
                }
            }
        }
        if done {
            // Flash the taskbar button (bounce the Dock icon on macOS) if
            // the user switched to something else meanwhile.
            if !self.hidden && !ctx.input(|i| i.focused) {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
            self.batch.stopped = job.is_cancelled();
            self.banner = true;
            self.notify_batch(ctx);
        } else {
            // For the time left and the progress between files.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            self.job = Some(job);
        }
    }

    /// Asks once per batch what to do with PNGs that already exist.
    pub(crate) fn existing_dialog(&mut self, ctx: &egui::Context) {
        let Some(asking) = &self.asking else { return };
        let mut choice = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("existing"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(16))
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                let count = asking.existing.len();
                let title = if count == 1 {
                    let name = asking.existing[0].file_name().unwrap_or_default();
                    format!("{} already exists", name.to_string_lossy())
                } else {
                    format!("{count} files already exist")
                };
                ui.label(RichText::new(title).strong().size(15.0));
                ui.label("What should happen to the files that are already there?");
                if count > 1 {
                    let names: Vec<String> = asking
                        .existing
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    ui.weak("Show files").on_hover_text(names.join("\n"));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Replace")
                        .on_hover_text("Overwrite them with the new files")
                        .clicked()
                    {
                        choice = Some(Existing::Replace);
                    }
                    if ui
                        .button("Keep both")
                        .on_hover_text("Save the new files with a number, e.g. shot_hdr (2).png")
                        .clicked()
                    {
                        choice = Some(Existing::KeepBoth);
                    }
                    if ui
                        .button("Skip")
                        .on_hover_text("Leave them alone and only make the files that are missing")
                        .clicked()
                    {
                        choice = Some(Existing::Skip);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            });
        if let Some(existing) = choice
            && let Some(asking) = self.asking.take()
        {
            self.run(asking.indices, asking.conflicts, existing, ctx);
        } else if cancel || modal.should_close() {
            self.asking = None;
        }
    }
}

/// What the system calls the place deleted files go.
pub(crate) const RECYCLE_BIN: &str = if cfg!(windows) {
    "Recycle Bin"
} else {
    "Trash"
};

/// The running batch, or the last one: what the progress and the banner
/// after it show.
#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) total: usize,
    /// Files finished, however they went.
    pub(crate) done: usize,
    pub(crate) failed: usize,
    /// Files whose outputs all existed already.
    pub(crate) skipped: usize,
    /// Size of the files written.
    pub(crate) bytes: u64,
    /// Originals moved to the Recycle Bin, and why others couldn't be.
    pub(crate) trashed: usize,
    pub(crate) trash_errors: Vec<String>,
    pub(crate) stopped: bool,
    /// Where the files went, e.g. "next to the originals".
    pub(crate) place: String,
    /// `egui` times the batch started and its last file finished.
    pub(crate) started: f64,
    pub(crate) last_finished: f64,
}

/// How a batch went, for the banner's icon.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Done,
    Problem,
    Nothing,
}

impl Batch {
    /// Files written, at least partly.
    pub(crate) fn written(&self) -> usize {
        self.done - self.failed - self.skipped
    }

    /// Average time per file so far.
    fn seconds_per_file(&self) -> Option<f64> {
        (self.done > 0).then(|| (self.last_finished - self.started) / self.done as f64)
    }

    /// How far along the batch is at `now`, from 0 to 1. The file being
    /// converted counts in part, by how long files have taken so far.
    pub(crate) fn fraction(&self, now: f64) -> f32 {
        let partial = self.seconds_per_file().map_or(0.0, |per_file| {
            ((now - self.last_finished) / per_file.max(0.001)).clamp(0.0, 0.9)
        });
        let done = (self.done as f64 + partial).min(self.total as f64);
        (done / self.total.max(1) as f64) as f32
    }

    /// "Converting 3 of 6 · about 40 s left", once a file has finished to
    /// go by.
    pub(crate) fn progress_text(&self, now: f64) -> String {
        let current = (self.done + 1).min(self.total);
        let mut text = format!("Converting {current} of {}", self.total);
        if let Some(per_file) = self.seconds_per_file() {
            let left = (self.total - self.done) as f64 * per_file - (now - self.last_finished);
            text = format!("{text} · {}", time_left(left));
        }
        text
    }

    pub(crate) fn outcome(&self) -> Outcome {
        if self.failed > 0 || !self.trash_errors.is_empty() {
            Outcome::Problem
        } else if self.stopped || self.written() == 0 {
            Outcome::Nothing
        } else {
            Outcome::Done
        }
    }

    /// The banner's title, e.g. "Converted 6 images".
    pub(crate) fn title(&self) -> String {
        let written = self.written();
        if self.stopped {
            if written == 0 {
                "Stopped".to_owned()
            } else {
                format!("Stopped after {}", images(written))
            }
        } else if written == 0 && self.failed > 0 {
            if self.failed == 1 {
                "Couldn't convert the image".to_owned()
            } else {
                format!("Couldn't convert {}", images(self.failed))
            }
        } else if written == 0 {
            "Nothing new to save".to_owned()
        } else {
            format!("Converted {}", images(written))
        }
    }

    /// The line under the banner's title: the size, where the files went
    /// and anything that didn't go to plan.
    pub(crate) fn details(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let mut parts = Vec::new();
        if self.bytes > 0 {
            parts.push(format!(
                "{}, saved {}",
                hdrshot_core::share::megabytes(self.bytes),
                self.place
            ));
        }
        if self.failed > 0 && self.written() == 0 && !self.stopped {
            // The title already says none could be converted.
            let why = if self.failed == 1 {
                "Its row says why"
            } else {
                "Their rows say why"
            };
            parts.push(why.to_owned());
        } else if self.failed > 0 {
            parts.push(plural(
                self.failed,
                "failed: its row says why",
                "failed: their rows say why",
            ));
        }
        if self.skipped > 0 {
            parts.push(plural(
                self.skipped,
                "skipped, as its files already exist",
                "skipped, as their files already exist",
            ));
        }
        if self.trashed > 0 {
            let originals = plural(self.trashed, "original", "originals");
            parts.push(format!("{originals} moved to the {RECYCLE_BIN}"));
        }
        if let Some(error) = self.trash_errors.first() {
            let originals = plural(self.trash_errors.len(), "original", "originals");
            parts.push(format!(
                "Couldn't move {originals} to the {RECYCLE_BIN}: {error}"
            ));
        }
        parts.join(" · ")
    }
}

/// "1 image" or "3 images".
pub(crate) fn images(n: usize) -> String {
    if n == 1 {
        "1 image".to_owned()
    } else {
        format!("{n} images")
    }
}

/// A rough time left, rounded so it doesn't flicker.
pub(crate) fn time_left(seconds: f64) -> String {
    if seconds < 8.0 {
        "a few seconds left".to_owned()
    } else if seconds < 60.0 {
        format!("about {} s left", ((seconds / 5.0).round() * 5.0) as u32)
    } else {
        format!("about {} min left", (seconds / 60.0).ceil() as u32)
    }
}

/// A folder's name rather than its whole path, for short labels.
pub(crate) fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into())
}

pub(crate) struct Asking {
    pub(crate) indices: Vec<usize>,
    /// Rows left out of the batch, and why (see `App::start_some`).
    pub(crate) conflicts: Vec<(usize, String)>,
    /// The output files that already exist.
    pub(crate) existing: Vec<PathBuf>,
}

/// The planned output files of `inputs` that already exist.
pub(crate) fn existing_outputs<'a>(
    inputs: impl Iterator<Item = &'a PathBuf>,
    options: &ConvertOptions,
) -> Vec<PathBuf> {
    inputs
        .flat_map(|input| {
            let planned = hdrshot_core::output_paths(input, options);
            [planned.hdr, planned.sdr]
        })
        .flatten()
        .filter(|p| p.exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_sums_up_the_batch() {
        let mut batch = Batch {
            total: 6,
            done: 6,
            bytes: 48_200_000,
            place: "next to the originals".into(),
            ..Default::default()
        };
        assert_eq!(batch.title(), "Converted 6 images");
        assert_eq!(batch.details(), "48.2 MB, saved next to the originals");
        assert_eq!(batch.outcome(), Outcome::Done);

        batch.failed = 1;
        batch.skipped = 2;
        assert_eq!(batch.title(), "Converted 3 images");
        assert_eq!(
            batch.details(),
            "48.2 MB, saved next to the originals · 1 failed: its row says why · \
             2 skipped, as their files already exist"
        );
        assert_eq!(batch.outcome(), Outcome::Problem);

        let skipped = Batch {
            total: 1,
            done: 1,
            skipped: 1,
            ..Default::default()
        };
        assert_eq!(skipped.title(), "Nothing new to save");
        assert_eq!(skipped.outcome(), Outcome::Nothing);

        let failed = Batch {
            total: 2,
            done: 2,
            failed: 2,
            ..Default::default()
        };
        assert_eq!(failed.title(), "Couldn't convert 2 images");
        assert_eq!(failed.details(), "Their rows say why");

        let stopped = Batch {
            total: 5,
            done: 2,
            stopped: true,
            ..Default::default()
        };
        assert_eq!(stopped.title(), "Stopped after 2 images");
    }

    #[test]
    fn progress_counts_the_file_being_converted() {
        let mut batch = Batch {
            total: 4,
            ..Default::default()
        };
        assert_eq!(batch.progress_text(1.0), "Converting 1 of 4");
        assert_eq!(batch.fraction(1.0), 0.0);
        batch.done = 2;
        batch.last_finished = 20.0;
        // 10 s per file, 5 s into the third.
        assert_eq!(batch.fraction(25.0), 2.5 / 4.0);
        assert_eq!(
            batch.progress_text(25.0),
            "Converting 3 of 4 · about 15 s left"
        );
        assert_eq!(time_left(3.0), "a few seconds left");
        assert_eq!(time_left(61.0), "about 2 min left");
    }

    #[test]
    fn existing_outputs_lists_only_files_on_disk() {
        let dir = std::env::temp_dir().join(format!("hdrshot-gui-existing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a_hdr.png"), b"png").unwrap();
        let inputs = [dir.join("a.jxr"), dir.join("b.jxr")];
        let options = ConvertOptions::default();
        assert_eq!(
            existing_outputs(inputs.iter(), &options),
            [dir.join("a_hdr.png")]
        );
        let sdr_only = ConvertOptions {
            mode: OutputMode::Sdr,
            ..Default::default()
        };
        assert!(existing_outputs(inputs.iter(), &sdr_only).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
