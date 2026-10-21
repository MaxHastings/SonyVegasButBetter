use super::*;

impl Editor {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut value) = self.goto.clone() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Go to timeline position")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Seconds from timeline start");
                    let response = ui.text_edit_singleline(&mut value);
                    if !response.has_focus() {
                        response.request_focus();
                    }
                    go = ui.button("Go").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel = ui.input(|i| i.key_pressed(Key::Escape));
                });
            if go {
                match value.parse::<f64>() {
                    Ok(s) if s.is_finite() && s >= 0. => {
                        if let Some(player) = &self.player {
                            player.pause();
                        }
                        self.seek(ticks(s));
                        self.goto = None;
                    }
                    _ => self.error = Some("Enter a finite positive number of seconds".into()),
                }
            } else if cancel {
                self.goto = None;
            } else {
                self.goto = Some(value);
            }
        }
        if let Some(error) = self.error.clone() {
            let mut open = true;
            egui::Window::new("SVBB — operation failed")
                .id(Id::new("error"))
                .open(&mut open)
                .collapsible(false)
                .default_width(560.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(300.)
                        .show(ui, |ui| {
                            ui.label(&error);
                        });
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                    if let Some(path) = self.player.as_ref().and_then(Player::log_path) {
                        if ui.button("Copy error details").clicked() {
                            ctx.copy_text(format!(
                                "SVBB {}\n{error}\nPlayback log: {}",
                                env!("CARGO_PKG_VERSION"),
                                path.display()
                            ));
                        }
                        ui.small(format!("Playback log: {}", path.display()));
                    }
                });
            if !open {
                self.error = None;
            }
        }
        if self.recovery_available {
            egui::Window::new("Interrupted session found")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("A recovery project exists. Your manually saved project is separate.");
                    ui.horizontal(|ui| {
                        let recover = ui.button("Recover unsaved session");
                        trace(ui.ctx(), "recover", &recover);
                        if recover.clicked() {
                            if let Some(path) = self.recovery_source.clone() {
                                self.open(path);
                            }
                            self.recovery_available = false;
                        }
                        if ui.button("Dismiss recovery").clicked() {
                            self.recovery_available = false;
                            if let Some(path) = self.recovery_source.take() {
                                let _ = std::fs::remove_file(path);
                            }
                        }
                    });
                });
        }
        if self.confirm_unsaved {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Save your edits before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.confirm_unsaved = false;
                            self.save(false);
                        }
                        if ui.button("Discard").clicked() {
                            self.confirm_unsaved = false;
                            let _ = std::fs::remove_file(&self.recovery);
                            if self.close_pending {
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            } else if let Some(path) = self.pending_open.take() {
                                if let Some(path) = path {
                                    self.open(path);
                                } else {
                                    self.reset(Project::default(), None);
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_unsaved = false;
                            self.close_pending = false;
                            self.pending_open = None;
                        }
                    });
                });
        }
        if self.render_dialog {
            let mut open = true;
            egui::Window::new("Render MP4")
                .open(&mut open)
                .collapsible(false)
                .default_width(540.)
                .show(ctx, |ui| {
                    ui.add_enabled_ui(self.render_job.is_none(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Export goal");
                            ui.selectable_value(&mut self.render_limit, false, "Choose quality");
                            let limit = ui.selectable_value(&mut self.render_limit, true, "Fit under a size limit");
                            trace(ui.ctx(), "size-limit", &limit);
                        });
                        if self.render_limit {
                            ui.horizontal(|ui| {
                                ui.label("Maximum file size");
                                let limit = ui.add(egui::DragValue::new(&mut self.render_limit_mb).range(0.1..=5000.).speed(0.1).max_decimals(2).suffix(" MB"));
                                trace(ui.ctx(), "size-limit-mb", &limit);
                                for mb in [10., 25., 50., 100.] {
                                    if ui.small_button(format!("{mb:.0} MB")).clicked() { self.render_limit_mb = mb; }
                                }
                            });
                            ui.small("MB = 1,000,000 bytes. Video, audio and MP4 overhead all count.");
                            let auto = ui.checkbox(&mut self.render_auto_format, "Automatically reduce resolution / frame rate when needed");
                            trace(ui.ctx(), "auto-size-format", &auto);
                        }
                        ui.add_enabled_ui(!self.render_limit || !self.render_auto_format, |ui| {
                        egui::ComboBox::from_label("Resolution / frame rate")
                            .selected_text(
                                RENDER_PRESETS[self.render_preset],
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in
                                    RENDER_PRESETS.iter().enumerate()
                                {
                                    ui.selectable_value(&mut self.render_preset, i, *name);
                                }
                            });
                        });
                        if !self.render_limit {
                        egui::ComboBox::from_label("Quality")
                            .selected_text(
                                ["Smaller file", "Balanced", "High quality"][self.quality],
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in ["Smaller file", "Balanced", "High quality"]
                                    .iter()
                                    .enumerate()
                                {
                                    ui.selectable_value(&mut self.quality, i, *name);
                                }
                            });
                        }
                        egui::ComboBox::from_label("Video encoder")
                            .selected_text(self.encoder.label())
                            .show_ui(ui, |ui| {
                                for encoder in [
                                    render::Encoder::Auto,
                                    render::Encoder::Software,
                                    render::Encoder::Nvidia,
                                    render::Encoder::Intel,
                                    render::Encoder::Amd,
                                ] {
                                    ui.selectable_value(
                                        &mut self.encoder,
                                        encoder,
                                        encoder.label(),
                                    );
                                }
                                if cfg!(target_os = "macos") {
                                    ui.selectable_value(
                                        &mut self.encoder,
                                        render::Encoder::Apple,
                                        render::Encoder::Apple.label(),
                                    );
                                }
                            });
                        ui.small("Auto tests the GPU encoder and falls back to CPU if needed.");
                        if !self.render_limit {
                        ui.label("Audio: timeline mix · AAC stereo 48 kHz / 192 kbps");
                        } else {
                            ui.small("GPU: faster bitrate-based export. CPU: two-pass encoding for compression quality. Quality varies to fit the limit.");
                            ui.small("The finished file is measured. A size overrun triggers a lower-bitrate retry.");
                            match self.render_settings() {
                                Ok(settings) => {
                                    if let Ok(budget) = render::size_budget(settings.max_bytes.unwrap(), self.p.duration()) {
                                        ui.label(format!("{}×{} · {:.2} fps · {:.1} s", settings.width, settings.height, settings.fps.num as f64 / settings.fps.den as f64, seconds(self.p.duration())));
                                        ui.small(format!("Video budget: {} kbps · audio: AAC {} kbps stereo", budget.video_bps / 1000, budget.audio_bps / 1000));
                                        ui.small("Audio quality adjusts automatically; your mix volume and timing stay intact.");
                                        if budget.video_bps < 300_000 { ui.colored_label(Color32::YELLOW, "Very low video budget: details and motion may look soft. Shorten the edit or raise the limit for better quality."); }
                                    }
                                }
                                Err(e) => { ui.colored_label(Color32::YELLOW, e.to_string()); }
                            }
                        }
                        ui.label(if self.p.peak_protection {
                            "Peak protection enabled"
                        } else {
                            "Peak protection disabled (change in Clip Properties)"
                        });
                        ui.horizontal(|ui| {
                            ui.label("Destination");
                            let destination = ui.text_edit_singleline(&mut self.render_path);
                            trace(ui.ctx(), "render-path", &destination);
                            if ui.button("Browse…").clicked()
                                && let Some(mut path) = rfd::FileDialog::new()
                                    .set_title("Render finished video")
                                    .add_filter("MP4 video", &["mp4"])
                                    .set_file_name("finished-video.mp4")
                                    .save_file()
                            {
                                if path.extension().is_none() {
                                    path.set_extension("mp4");
                                }
                                self.render_path = path.to_string_lossy().into();
                            }
                        });
                        let render = ui.add_enabled(self.render_settings().is_ok() && self.p.duration() > 0, egui::Button::new("Render"));
                        trace(ui.ctx(), "render", &render);
                        if render.clicked() {
                            self.start_render();
                        }
                    });
                    if let Some(cancel) = &self.render_job {
                        ui.add(egui::ProgressBar::new(self.render_progress).show_percentage());
                        ui.label(&self.render_stage);
                        if let Some(encoder) = self.encoder_used {
                            ui.label(encoder.label());
                        }
                        if self.render_progress >= 0.995 {
                            ui.label("Finalizing and validating output…");
                        } else if let Some(start) = self.render_started {
                            let elapsed = start.elapsed().as_secs_f64();
                            let progress = f64::from(self.render_progress);
                            if elapsed > 2. && progress > 0.005 {
                                let remaining = elapsed * (1. - progress) / progress;
                                ui.monospace(format!(
                                    "ETA ~ {} · elapsed {} · {:.1}× realtime",
                                    format_time(remaining),
                                    format_time(elapsed),
                                    seconds(self.render_duration) * progress / elapsed
                                ));
                            } else {
                                ui.label("Estimating remaining time…");
                            }
                        }
                        ui.label("Rendering a snapshot of this edit. You can keep editing.");
                        if ui.button("Cancel render").clicked() {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    if let Some(path) = &self.rendered {
                        ui.separator();
                        ui.colored_label(
                            Color32::LIGHT_GREEN,
                            "Complete — output streams and duration validated.",
                        );
                        if let Ok(meta) = std::fs::metadata(path) {
                            ui.label(format!("File size: {:.3} MB ({} bytes)", meta.len() as f64 / 1_000_000., meta.len()));
                        }
                        ui.label(path.display().to_string());
                        if ui.button("Open finished video").clicked() {
                            open_external(path);
                        }
                    }
                });
            if !open && self.render_job.is_none() {
                self.render_dialog = false;
            }
        }
        if self.help {
            let mut open = true;
            egui::Window::new("SVBB shortcuts and editing behavior").open(&mut open).default_width(660.).show(ctx,|ui|{
            ui.label("Open → Import → Cut → Arrange → Mix → Render");ui.separator();
            for line in ["Space: play / pause · Left / Right: one project frame · Home / End: timeline bounds","Ctrl+Shift+F / Expand: focus Preview; Restore layout returns. Esc closes framing first, then exits focus.","Media / Properties beside Preview hide each sidebar. Drag panel dividers to resize; View → Reset workspace layout restores defaults. Sizes and sidebar visibility are remembered.","Click clip then S: position and split linked clips · Delete: delete and leave a gap · Shift+Delete: ripple delete","Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y): undo / redo · Ctrl+S: save · Ctrl+Shift+S: save as","Ctrl+I: import · Ctrl+O: open · + / −: zoom · Ctrl+wheel: zoom at mouse · wheel: pan · F: fit / restore · Shift+F: zoom selected clip","Alt+Left / Alt+Right: previous / next cut on the selected track (video track if nothing is selected).", "Hold a move or trim near the timeline edge to scroll. F restores the view after zooming to a selection.","Double-click media: append. Drag media to a lane: insert. Drag timeline edges: trim.","Linked audio/video move together. Unlink in Clip Properties or the context menu to edit separately.","Ripple delete removes the selected time range on every unlocked lane. Intersecting audio is trimmed/split, never silently left over the removed time. Lock audio lanes to protect their positions.","Edge trims keep the opposite edge fixed and stop at source boundaries or neighbouring clips. Top-corner handles adjust fades.","Dragging stops flush against neighbours, including linked tracks. Use separate lanes for overlapping sound.","Match loudness measures the selected source range, targets −16 LUFS, caps boost at +48 dB and predicted true peak at −1 dBTP. Manual gain remains editable.","Splits retain the original outer fades and clear the two new internal fades. Video fades go to black; audio fades change volume. All audio edges have a 3 ms safety ramp.","The default master limiter caps sample peaks at 0.98; AAC may have small reconstructed overshoots.","Save creates a .bak of the previous saved version. Recovery saves run every 15 seconds while dirty."]{ui.label(line);ui.add_space(4.);}
        });
            self.help = open;
        }
        if self.project_settings {
            let mut open = true;
            egui::Window::new("Project format").open(&mut open).show(ctx,|ui|{let mut name=self.p.name.clone();if ui.text_edit_singleline(&mut name).changed(){self.apply(|p|{p.name=name;Ok(())});}
            ui.label("Changing the frame rate requires an empty timeline; source timestamps remain independent.");ui.add_enabled_ui(self.p.clips.is_empty(),|ui|{for (label,fps) in [("1080p30",Rational::new(30,1)),("1080p60",Rational::new(60,1)),("1080p59.94",Rational::new(60000,1001))]{if ui.button(label).clicked(){self.apply(|p|{p.width=1920;p.height=1080;p.fps=fps;Ok(())});}}});});
            self.project_settings = open;
        }
    }
}
