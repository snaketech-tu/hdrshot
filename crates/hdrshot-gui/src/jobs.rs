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
        self.progress = (0, files.len());
        self.skipped = 0;
        self.notice = None;
        let clipboard = self
            .settings
            .copy_to_clipboard
            .then(|| self.clipboard.clone());
        self.job = Some(Job::start(
            files,
            self.convert_options(existing),
            clipboard,
            ctx.clone(),
        ));
    }

    pub(crate) fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.take() else { return };
        let mut done = false;
        while let Ok(event) = job.events.try_recv() {
            match event {
                Event::Started(i) => self.files[i].status = Status::Converting,
                Event::Finished(i, result) => {
                    self.files[i].status = match result {
                        Ok(outputs) => {
                            if outputs.hdr.is_none() && outputs.sdr.is_none() {
                                self.skipped += 1;
                            }
                            Status::Done(outputs)
                        }
                        Err(e) => Status::Failed(e),
                    };
                    self.progress.0 += 1;
                }
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
            if !ctx.input(|i| i.focused) {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
            let failed = self
                .files
                .iter()
                .filter(|e| matches!(e.status, Status::Failed(_)))
                .count();
            if job.is_cancelled() {
                self.notice = Some("Stopped.".into());
            } else if failed > 0 {
                self.notice = Some(format!(
                    "{failed} file{} could not be converted.",
                    if failed == 1 { "" } else { "s" }
                ));
            } else if self.skipped > 0 {
                let (n, s) = (self.skipped, if self.skipped == 1 { "" } else { "s" });
                self.notice = Some(if n == self.progress.1 {
                    format!("Skipped {n} file{s}: the files already exist.")
                } else {
                    format!("Skipped {n} file{s} whose files already exist.")
                });
            }
        } else {
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
