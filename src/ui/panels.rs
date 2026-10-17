use super::*;

impl Editor {
    pub(super) fn top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.strong("SVBB");
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.button("New project").clicked() {
                        self.request_open(None);
                        ui.close_menu();
                    }
                    if ui.button("Open…     Ctrl+O").clicked() {
                        self.open_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Import media…     Ctrl+I").clicked() {
                        self.import_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Save     Ctrl+S").clicked() {
                        self.save(false);
                        ui.close_menu();
                    }
                    if ui.button("Save as…     Ctrl+Shift+S").clicked() {
                        self.save(true);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Render MP4…").clicked() {
                        self.render_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    for (label, a) in [
                        ("Undo", Action::Undo),
                        ("Redo", Action::Redo),
                        ("Split at playhead", Action::Split),
                        ("Delete (leave gap)", Action::Delete),
                        ("Ripple delete", Action::Ripple),
                    ] {
                        if ui.button(label).clicked() {
                            self.action(a);
                            ui.close_menu();
                        }
                    }
                    if ui.button("Close video gaps").clicked() {
                        self.apply(edit::close_gaps);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Project", |ui| {
                    if ui.button("Format and name…").clicked() {
                        self.project_settings = true;
                        ui.close_menu();
                    }
                    if ui.button("Fit timeline").clicked() {
                        self.zoom = ((ui.ctx().screen_rect().width() - 230.)
                            / seconds(self.p.duration()).max(10.) as f32)
                            .clamp(0.05, 1000.);
                        ui.close_menu();
                    }
                });
                if ui.button("Shortcuts / Help").clicked() {
                    self.help = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "{}{}",
                        self.p.name,
                        if self.dirty { " *" } else { "" }
                    ));
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Import…").clicked() {
                    self.import_dialog();
                }
                if ui.button("Save").clicked() {
                    self.save(false);
                }
                ui.separator();
                for (label, a) in [
                    ("Undo", Action::Undo),
                    ("Redo", Action::Redo),
                    ("Split [S]", Action::Split),
                    ("Delete", Action::Delete),
                    ("Ripple delete", Action::Ripple),
                ] {
                    if ui.button(label).clicked() {
                        self.action(a);
                    }
                }
                if ui.button("Close gaps").clicked() {
                    self.apply(edit::close_gaps);
                }
                ui.separator();
                ui.checkbox(&mut self.snapping, "Snap");
                if ui.button("−").clicked() {
                    self.action(Action::ZoomOut);
                }
                if ui.button("+").clicked() {
                    self.action(Action::ZoomIn);
                }
                let fit = ui.button("Fit");
                trace(ui.ctx(), "fit", &fit);
                if fit.clicked() {
                    self.zoom = ((ui.available_width() + 700.)
                        / seconds(self.p.duration()).max(10.) as f32)
                        .clamp(0.05, 1000.);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Render MP4…").clicked() {
                        self.render_dialog = true;
                    }
                });
            });
        });
    }
    pub(super) fn media_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("PROJECT MEDIA");
            ui.label(format!("{} files", self.p.media.len()));
            if self.importing {
                ui.spinner();
            }
        });
        ui.separator();
        let mut append = None;
        let mut relink = None;
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("media-scroll")
            .max_height((ui.available_height() - 92.).max(80.))
            .show(ui, |ui| {
                if self.p.media.is_empty() {
                    ui.add_space(30.);
                    ui.label("Drop video and audio recordings here.");
                    ui.label("Double-click a file to append it.");
                }
                for m in &self.p.media {
                    let selected = self.media_selected.as_ref() == Some(&m.id);
                    let missing = !m.path.is_file();
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 58.),
                        Sense::click_and_drag(),
                    );
                    trace(ui.ctx(), format!("media:{}", m.name), &response);
                    if selected && self.scroll_to_media {
                        response.scroll_to_me(Some(egui::Align::Center));
                        self.scroll_to_media = false;
                    }
                    if selected {
                        ui.painter()
                            .rect_filled(rect, 0., Color32::from_rgb(49, 66, 81));
                    }
                    if let Some(t) = self.thumb.get(&m.id) {
                        ui.painter().image(
                            t.id(),
                            Rect::from_min_size(rect.min + Vec2::new(4., 5.), Vec2::new(76., 43.)),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                            Color32::WHITE,
                        );
                    }
                    let x = if self.thumb.contains_key(&m.id) {
                        88.
                    } else {
                        8.
                    };
                    let color = if missing {
                        Color32::LIGHT_RED
                    } else {
                        Color32::from_gray(218)
                    };
                    let name = if missing {
                        format!("MISSING · {}", m.name)
                    } else {
                        m.name.clone()
                    };
                    ui.painter().text(
                        rect.min + Vec2::new(x, 9.),
                        Align2::LEFT_TOP,
                        name,
                        FontId::proportional(12.),
                        color,
                    );
                    let info = if m.video.is_some() {
                        format!(
                            "{}×{}  {:.2} fps  ·  {:.1}s",
                            m.width,
                            m.height,
                            m.frame_rate.num as f64 / m.frame_rate.den as f64,
                            seconds(m.duration)
                        )
                    } else {
                        format!(
                            "{} Hz  {} ch  ·  {:.1}s",
                            m.sample_rate,
                            m.channels,
                            seconds(m.duration)
                        )
                    };
                    ui.painter().text(
                        rect.min + Vec2::new(x, 29.),
                        Align2::LEFT_TOP,
                        info,
                        FontId::proportional(11.),
                        Color32::from_gray(150),
                    );
                    if response.clicked() {
                        self.media_selected = Some(m.id.clone());
                    }
                    if response.double_clicked() {
                        append = Some((
                            m.id.clone(),
                            if m.video.is_some() {
                                0
                            } else {
                                self.audio_lane
                            },
                        ));
                    }
                    response.dnd_set_drag_payload(MediaDrag(m.id.clone()));
                    response.context_menu(|ui| {
                        if ui.button("Append to video").clicked() {
                            append = Some((m.id.clone(), 0));
                            ui.close_menu();
                        }
                        for lane in 1..3 {
                            if ui.button(format!("Append audio to lane {lane}")).clicked() {
                                append = Some((m.id.clone(), lane));
                                ui.close_menu();
                            }
                        }
                        if ui.button("Relink source…").clicked() {
                            relink = Some(m.id.clone());
                            ui.close_menu();
                        }
                        if ui.button("Remove unused media").clicked() {
                            remove = Some(m.id.clone());
                            ui.close_menu();
                        }
                    });
                    response.on_hover_text(m.path.display().to_string());
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Audio destination");
            ui.selectable_value(&mut self.audio_lane, 1, "1");
            ui.selectable_value(&mut self.audio_lane, 2, "2");
        });
        ui.horizontal(|ui| {
            if ui.button("Append selected").clicked()
                && let Some(m) = self
                    .media_selected
                    .as_ref()
                    .and_then(|id| self.p.media.iter().find(|m| &m.id == id))
            {
                append = Some((
                    m.id.clone(),
                    if m.video.is_some() {
                        0
                    } else {
                        self.audio_lane
                    },
                ));
            }
            if ui.button("Insert at cursor").clicked()
                && let Some(m) = self
                    .media_selected
                    .as_ref()
                    .and_then(|id| self.p.media.iter().find(|m| &m.id == id))
            {
                let (id, lane) = (
                    m.id.clone(),
                    if m.video.is_some() {
                        0
                    } else {
                        self.audio_lane
                    },
                );
                self.add_media(id, lane, Some(self.playhead));
            }
        });
        if let Some((id, lane)) = append {
            self.add_media(id, lane, None);
        }
        if let Some(id) = remove {
            self.apply(|p| {
                anyhow::ensure!(
                    !p.clips.iter().any(|c| c.media == id),
                    "Media is used by timeline clips"
                );
                p.media.retain(|m| m.id != id);
                Ok(())
            });
        }
        if let Some(id) = relink
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Locate original recording")
                .pick_file()
            && let Some(rt) = self.rt.clone()
        {
            let tx = self.tx.clone();
            let old = self.p.media.iter().find(|m| m.id == id).cloned();
            if let Some(old) = old {
                std::thread::spawn(move || {
                    let result = rt.probe(&file).and_then(|mut m| {
                        anyhow::ensure!(
                            m.duration >= old.duration
                                && m.video.is_some() == old.video.is_some()
                                && m.audio.is_some() == old.audio.is_some(),
                            "Replacement has incompatible duration or stream types"
                        );
                        m.id = old.id;
                        Ok(m)
                    });
                    let _ = tx.send(Event::Imported(result));
                });
            }
        }
    }
    pub(super) fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("PREVIEW");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} × {} / {:.3}p",
                    self.p.width,
                    self.p.height,
                    self.p.fps.num as f64 / self.p.fps.den as f64
                ));
            });
        });
        let space = ui.available_size();
        let height = (space.y - 90.).max(40.);
        let size = Vec2::new(space.x, (space.x * 9. / 16.).min(height));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, 0., Color32::BLACK);
        if let Some(t) = &self.texture {
            let mut target = rect;
            let w = rect.height() * 16. / 9.;
            if w < rect.width() {
                target.min.x += (rect.width() - w) / 2.;
                target.max.x = target.min.x + w;
            }
            ui.painter().image(
                t.id(),
                target,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "NO TIMELINE MEDIA",
                FontId::monospace(12.),
                Color32::from_gray(98),
            );
        }
        ui.horizontal(|ui| {
            if ui.button("|◀").on_hover_text("Start [Home]").clicked() {
                self.seek(0);
            }
            if ui
                .button("◀")
                .on_hover_text("Previous frame [Left]")
                .clicked()
            {
                self.action(Action::Left);
            }
            let playing = self.player.as_ref().is_some_and(|p| !p.snapshot().paused);
            if ui
                .button(if playing { "Pause" } else { "Play [Space]" })
                .clicked()
            {
                self.play();
            }
            if ui.button("▶").on_hover_text("Next frame [Right]").clicked() {
                self.action(Action::Right);
            }
            ui.monospace(timecode(self.playhead, self.p.fps));
            ui.label("/");
            ui.monospace(timecode(self.p.duration(), self.p.fps));
            if self.preview_job.is_some() {
                ui.spinner();
                ui.small("Preparing mix");
            }
        });
        let mut time = seconds(self.playhead);
        if ui
            .add(
                egui::Slider::new(&mut time, 0.0..=seconds(self.p.duration()).max(0.001))
                    .show_value(false),
            )
            .changed()
        {
            self.seek(ticks(time));
        }
        ui.horizontal(|ui| {
            ui.small("OUTPUT");
            let peaks = self
                .player
                .as_ref()
                .map(|p| p.snapshot().peak_db)
                .unwrap_or([-120.; 2]);
            for (channel, db) in ["L", "R"].into_iter().zip(peaks) {
                ui.small(channel);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(90., 7.), Sense::hover());
                ui.painter().rect_filled(rect, 0., Color32::from_gray(45));
                let mut fill = rect;
                fill.max.x = fill.min.x + rect.width() * ((db + 60.) / 60.).clamp(0., 1.) as f32;
                ui.painter().rect_filled(
                    fill,
                    0.,
                    if db > -1. {
                        Color32::LIGHT_RED
                    } else {
                        Color32::from_rgb(109, 169, 124)
                    },
                );
                ui.small(format!("{db:.0} dBFS"));
            }
        });
    }
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.strong("CLIP PROPERTIES");
        ui.separator();
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| self.p.clips.iter().find(|c| &c.id == id))
            .cloned();
        if let Some(c) = selected {
            let name = self
                .p
                .media(&c.media)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            ui.label(name);
            let locked = self.p.tracks[c.lane].locked;
            ui.add_enabled_ui(!locked,|ui|{
            let mut start=seconds(c.start);let mut source=seconds(c.source_in);let mut duration=seconds(c.duration);let mut gain=c.gain_db;let mut fi=seconds(c.fade_in);let mut fo=seconds(c.fade_out);let mut mute=c.mute;
            egui::Grid::new("properties").num_columns(2).spacing([8.,8.]).show(ui,|ui|{
                for(label,value)in[("Position (s)",&mut start),("Source in (s)",&mut source),("Duration (s)",&mut duration),("Gain (dB)",&mut gain),("Fade in (s)",&mut fi),("Fade out (s)",&mut fo)]{ui.label(label);let response=ui.add(egui::DragValue::new(value).speed(if label=="Gain (dB)"{0.2}else{1./60.}).max_decimals(3));trace(ui.ctx(),label,&response);
                    // egui persists keyboard text across frames, but DragValue writes
                    // its parsed value into this frame's local copy on commit.
                    if response.changed(){let id=c.id.clone();let new_value=*value;let label=label.to_string();self.apply(move|p|{let fps=p.fps;let cl=p.clips.iter_mut().find(|x|x.id==id).context("Clip removed")?;match label.as_str(){"Position (s)"=>cl.start=fps.align(ticks(new_value)),"Source in (s)"=>cl.source_in=fps.align(ticks(new_value)),"Duration (s)"=>{cl.duration=fps.align(ticks(new_value));cl.clamp_fades();},"Gain (dB)"=>cl.gain_db=new_value,"Fade in (s)"=>cl.fade_in=ticks(new_value),_=>cl.fade_out=ticks(new_value)}Ok(())});}ui.end_row();}
            });
            if ui.checkbox(&mut mute,"Mute clip").changed(){let id=c.id.clone();self.apply(|p|{p.clips.iter_mut().find(|x|x.id==id).unwrap().mute=mute;Ok(())});}
            if ui.button("Match voice loudness · −16 LUFS").on_hover_text("Measure selected source range. Limit boost to +18 dB and true peak to −1 dBTP; silence below −50 LUFS is refused.").clicked(){self.normalize();}
            ui.horizontal(|ui|{
                let earlier=ui.button("Earlier");trace(ui.ctx(),"earlier",&earlier);if earlier.clicked(){self.edit_selected(|p,id|edit::reorder(p,id,-1));}
                if ui.button("Later").clicked(){self.edit_selected(|p,id|edit::reorder(p,id,1));}
            });
            ui.label("Drag clip edges to trim. Primary trims ripple downstream unlocked material.");
        });
            if locked {
                ui.colored_label(Color32::YELLOW, "Track is locked");
            }
        } else {
            ui.label("Select a timeline clip to adjust its source range, gain and fades.");
        }
        ui.separator();
        let mut protect = self.p.peak_protection;
        if ui.checkbox(&mut protect, "Peak-protected mix").changed() {
            self.apply(|p| {
                p.peak_protection = protect;
                Ok(())
            });
        }
        ui.small("Stereo 48 kHz · shared preview/export mix");
    }
}
