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
                        for lane in 1..LANES {
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
            ui.selectable_value(&mut self.audio_lane, 1, "A1");
            ui.selectable_value(&mut self.audio_lane, 2, "A2");
            ui.selectable_value(&mut self.audio_lane, 3, "A3");
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
            if self.source_preview {
                ui.small("TRIM · source frame");
            }
            ui.label("/");
            ui.monospace(timecode(self.p.duration(), self.p.fps));
            if self.preview_job.is_some()
                || self.player.as_ref().is_some_and(|p| !p.snapshot().loaded)
                    && self.p.duration() > 0
            {
                ui.spinner();
                ui.small("Opening sources…");
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
            ui.label(
                self.p
                    .media(&c.media)
                    .map(|m| m.name.clone())
                    .unwrap_or_default(),
            );
            let locked = self.p.tracks[c.lane].locked;
            ui.add_enabled_ui(!locked, |ui| {
                egui::Grid::new("properties").num_columns(2).spacing([8.,8.]).show(ui, |ui| {
                    for (label, mut value) in [("Position (s)",seconds(c.start)),("Source in (s)",seconds(c.source_in)),("Duration (s)",seconds(c.duration)),("Gain (dB)",c.gain_db),("Fade in (s)",seconds(c.fade_in)),("Fade out (s)",seconds(c.fade_out))] {
                        if label == "Gain (dB)" && c.lane == 0 {continue;}
                        ui.label(label);
                        let response=ui.add(egui::DragValue::new(&mut value).speed(if label=="Gain (dB)" {0.2} else {1./60.}).range(if label=="Gain (dB)" {-60.0..=60.0} else {0.0..=86400.0}).max_decimals(3));
                        trace(ui.ctx(),label,&response);
                        #[cfg(feature="gui-test")]
                        if label=="Duration (s)" {let info=format!("value={value} focus={} changed={} clicked={} hovered={} rect={:?} ptr={:?}",response.has_focus(),response.changed(),response.clicked(),response.hovered(),response.rect,ui.ctx().pointer_latest_pos());ui.ctx().data_mut(|d| d.insert_temp(Id::new("duration-debug"), info));}
                        if response.changed() {
                            let id=c.id.clone();
                            self.apply(|p| {
                                let at=p.fps.align(ticks(value));
                                match label {
                                    "Position (s)"=>edit::move_clip(p,&id,at,c.lane)?,
                                    "Source in (s)"=>edit::trim(p,&id,at,c.duration,false)?,
                                    "Duration (s)"=>edit::trim(p,&id,c.source_in,at,false)?,
                                    _=> {let item=p.clips.iter_mut().find(|x|x.id==id).context("Clip removed")?;
                                        match label {"Gain (dB)"=>item.gain_db=value,"Fade in (s)"=>item.fade_in=ticks(value).min(item.duration),_=>item.fade_out=ticks(value).min(item.duration)}
                                    }
                                }
                                Ok(())
                            });
                        }
                        ui.end_row();
                    }
                });
                let mut mute=c.mute;
                if ui.checkbox(&mut mute,if c.lane==0 {"Hide video"} else {"Mute clip"}).changed(){let id=c.id.clone();self.apply(|p|{p.clips.iter_mut().find(|x|x.id==id).unwrap().mute=mute;Ok(())});}
                if c.lane>0 && ui.button("Match loudness · -16 LUFS").on_hover_text("Measure this source range. Target -16 LUFS with a -1 dBTP ceiling. Gain up to +48 dB; digital silence is refused.").clicked(){self.normalize();}
                let linked=c.link.is_some();
                let link_button=ui.button(if linked {"Unlink audio / video"} else {"Link matching audio / video"}); trace(ui.ctx(),"unlink",&link_button);
                if link_button.on_hover_text("Linked clips move, split, trim and delete together. Gain and fades remain independent.").clicked(){self.edit_selected(|p,id|if linked {edit::unlink(p,id)} else {edit::link(p,id)});}
                ui.horizontal(|ui| {
                    let earlier=ui.button("Earlier"); trace(ui.ctx(),"earlier",&earlier);
                    if earlier.clicked(){self.edit_selected(|p,id|edit::reorder(p,id,-1));}
                    let later=ui.button("Later");trace(ui.ctx(),"later",&later);if later.clicked(){self.edit_selected(|p,id|edit::reorder(p,id,1));}
                });
                ui.small("Edges: trim with the opposite edge fixed. Top corners: fade. Linked sound stays in sync.");
            });
            if locked {
                ui.colored_label(Color32::YELLOW, "Track is locked");
            }
        } else {
            ui.label("Click a clip to select it and position the playhead. Press S to split.");
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
