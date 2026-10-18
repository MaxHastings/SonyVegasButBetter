use super::*;

impl Editor {
    pub(super) fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("TIMELINE");
            ui.checkbox(&mut self.waveform_auto, "Auto waveform scale").on_hover_text("Display only: makes quiet detail visible. Does not change the mix. Disable to show signal amplitude.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(timecode(self.playhead, self.p.fps));
            });
        });
        ui.separator();
        let lane_h = ((ui.available_height() - 44.) / LANES as f32).clamp(64., 160.);
        let header_w = 196.;
        let ruler_h = 26.;
        let available = ui.available_width();
        ui.horizontal_top(|ui| {
            let (headers, _) = ui.allocate_exact_size(
                Vec2::new(header_w, ruler_h + LANES as f32 * lane_h),
                Sense::hover(),
            );
            for lane in 0..LANES {
                let t = self.p.tracks[lane].clone();
                let rect = Rect::from_min_size(
                    headers.min + Vec2::new(0., ruler_h + lane as f32 * lane_h),
                    Vec2::new(header_w - 8., lane_h),
                );
                let mut controls = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(Vec2::new(0., 4.)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                controls.strong(&t.name);
                controls.horizontal(|ui| {
                    let (mut mute, mut locked) = (t.mute, t.locked);
                    if ui
                        .checkbox(&mut mute, if lane == 0 { "Hide" } else { "Mute" })
                        .changed()
                    {
                        self.apply(|p| {
                            p.tracks[lane].mute = mute;
                            Ok(())
                        });
                    }
                    if ui.checkbox(&mut locked, "Lock").changed() {
                        self.apply(|p| {
                            p.tracks[lane].locked = locked;
                            Ok(())
                        });
                    }
                });
                if lane > 0 {
                    let mut gain = t.gain_db;
                    let response =
                        controls.add(egui::Slider::new(&mut gain, -60.0..=60.0).suffix(" dB"));
                    trace(ui.ctx(), format!("track-gain-{lane}"), &response);
                    if response.changed() {
                        self.apply(|p| {
                            p.tracks[lane].gain_db = gain;
                            Ok(())
                        });
                    }
                }
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    Stroke::new(1., Color32::from_gray(60)),
                );
            }
            let canvas_width = (seconds(self.p.duration() + HZ * 10) as f32 * self.zoom)
                .max(available - header_w - 22.);
            egui::ScrollArea::both()
                .id_salt("timeline-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (canvas, canvas_response) = ui.allocate_exact_size(
                        Vec2::new(canvas_width, ruler_h + LANES as f32 * lane_h),
                        Sense::hover(),
                    );
                    let painter = ui.painter_at(canvas);
                    painter.rect_filled(canvas, 0., Color32::from_rgb(24, 26, 29));
                    let ruler = Rect::from_min_size(canvas.min, Vec2::new(canvas.width(), ruler_h));
                    let response = ui.interact(ruler, Id::new("ruler"), Sense::click_and_drag());
                    trace(ui.ctx(), "ruler", &response);
                    trace(ui.ctx(), "canvas", &canvas_response);
                    if (response.clicked() || response.dragged())
                        && response.interact_pointer_pos().is_some()
                    {
                        if let Some(player) = &self.player {
                            player.pause();
                        }
                        let x = response.interact_pointer_pos().unwrap().x;
                        self.seek(ticks(((x - canvas.left()) / self.zoom) as f64));
                    }
                    let step = if self.zoom >= 80. {
                        1.
                    } else if self.zoom >= 15. {
                        5.
                    } else if self.zoom >= 2. {
                        30.
                    } else if self.zoom >= 0.4 {
                        120.
                    } else {
                        600.
                    };
                    let visible = ui.clip_rect().intersect(canvas);
                    let left = (((visible.left() - canvas.left()) / self.zoom / step).floor()
                        as i64)
                        .max(0);
                    let right = (((visible.right() - canvas.left()) / self.zoom / step).ceil()
                        as i64)
                        .max(left);
                    for n in left..=right {
                        let x = canvas.left() + n as f32 * step * self.zoom;
                        painter.line_segment(
                            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
                            Stroke::new(1., Color32::from_gray(43)),
                        );
                        painter.text(
                            Pos2::new(x + 4., canvas.top() + 5.),
                            Align2::LEFT_TOP,
                            format_time(n as f64 * step as f64),
                            FontId::monospace(11.),
                            Color32::from_gray(150),
                        );
                    }
                    for lane in 0..LANES {
                        let rect = Rect::from_min_size(
                            canvas.min + Vec2::new(0., ruler_h + lane as f32 * lane_h),
                            Vec2::new(canvas.width(), lane_h),
                        );
                        painter.line_segment(
                            [rect.left_top(), rect.right_top()],
                            Stroke::new(1., Color32::from_gray(65)),
                        );
                        let response = ui.interact(rect, Id::new(("lane", lane)), Sense::click());
                        trace(ui.ctx(), format!("lane:{lane}"), &response);
                        if response.clicked()
                            && let Some(pos) = response.interact_pointer_pos()
                        {
                            if let Some(player) = &self.player {
                                player.pause();
                            }
                            self.seek(ticks(((pos.x - canvas.left()) / self.zoom) as f64));
                            self.selected = None;
                        }
                    }
                    let clips = self.p.clips.clone();
                    let mut context_action = None;
                    let mut drag_commit = None;
                    for c in &clips {
                        let (mut fade_in, mut fade_out) = (c.fade_in, c.fade_out);
                        if let Some(d) = self
                            .drag
                            .as_ref()
                            .filter(|d| d.id == c.id && d.edge.abs() == 2)
                        {
                            let delta = ticks(
                                (ui.ctx().pointer_latest_pos().unwrap_or(d.origin).x - d.origin.x)
                                    as f64
                                    / self.zoom as f64,
                            );
                            if d.edge < 0 {
                                fade_in = (fade_in + delta).clamp(0, c.duration);
                            } else {
                                fade_out = (fade_out - delta).clamp(0, c.duration);
                            }
                        }
                        let x = canvas.left() + seconds(c.start) as f32 * self.zoom;
                        let w = (seconds(c.duration) as f32 * self.zoom).max(2.);
                        let rect = Rect::from_min_size(
                            Pos2::new(x, canvas.top() + ruler_h + c.lane as f32 * lane_h + 5.),
                            Vec2::new(w, lane_h - 10.),
                        );
                        if self.scroll_to_clip.as_ref() == Some(&c.id) {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                            self.scroll_to_clip = None;
                        }
                        if !rect.intersects(visible) {
                            continue;
                        }
                        let selected = self.selected.as_ref() == Some(&c.id)
                            || c.link.as_ref().is_some_and(|group| {
                                self.p.clips.iter().any(|x| {
                                    self.selected.as_ref() == Some(&x.id)
                                        && x.link.as_ref() == Some(group)
                                })
                            });
                        let muted = c.mute || self.p.tracks[c.lane].mute;
                        let color = if muted {
                            Color32::from_gray(64)
                        } else {
                            [
                                Color32::from_rgb(56, 83, 106),
                                Color32::from_rgb(62, 98, 85),
                                Color32::from_rgb(72, 88, 108),
                                Color32::from_rgb(101, 81, 59),
                            ][c.lane]
                        };
                        painter.rect_filled(rect, 1., color);
                        painter.rect_stroke(
                            rect,
                            1.,
                            Stroke::new(
                                if selected { 2. } else { 1. },
                                if selected {
                                    Color32::from_rgb(157, 208, 240)
                                } else {
                                    Color32::from_gray(106)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        let clip_painter = ui.painter_at(rect.intersect(visible));
                        let m = self.p.media(&c.media).ok();
                        if let Some(m) = m {
                            clip_painter.text(
                                rect.min + Vec2::new(7., 14.),
                                Align2::LEFT_TOP,
                                format!("{}{}", if c.link.is_some() { "↔ " } else { "" }, m.name),
                                FontId::proportional(12.),
                                Color32::from_gray(225),
                            );
                            if c.lane > 0
                                && let Some(wave) = self.wave.get(&c.media)
                            {
                                let center = rect.top() + rect.height() * 0.65;
                                let half = rect.height() * 0.22;
                                let begin = rect.left().max(visible.left()).ceil() as i32;
                                let end = rect.right().min(visible.right()).floor() as i32;
                                let amp =
                                    amplitude(c.gain_db + self.p.tracks[c.lane].gain_db) as f32;
                                for px in (begin..end).step_by(2) {
                                    let local = (px as f32 - rect.left()) / self.zoom;
                                    let time = seconds(c.source_in) + local as f64;
                                    let i = (time * 100.) as usize;
                                    let next =
                                        ((time + 2. / self.zoom as f64) * 100.).ceil() as usize;
                                    let peak = wave
                                        .get(i.min(wave.len())..next.min(wave.len()))
                                        .unwrap_or(&[])
                                        .iter()
                                        .copied()
                                        .fold(0., f32::max);
                                    let peak = if self.waveform_auto {
                                        (peak
                                            / self
                                                .wave_peak
                                                .get(&c.media)
                                                .copied()
                                                .unwrap_or(1.)
                                                .max(1e-8))
                                        .clamp(0., 1.)
                                        .sqrt()
                                    } else {
                                        peak * amp
                                    };
                                    let env = (if fade_in > 0 {
                                        (local as f64 / seconds(fade_in)).min(1.)
                                    } else {
                                        1.
                                    }) * (if fade_out > 0 {
                                        ((seconds(c.duration) - local as f64) / seconds(fade_out))
                                            .min(1.)
                                    } else {
                                        1.
                                    });
                                    let h = (peak * env as f32).clamp(0., 1.) * half;
                                    clip_painter.line_segment(
                                        [
                                            Pos2::new(px as f32, center - h),
                                            Pos2::new(px as f32, center + h),
                                        ],
                                        Stroke::new(
                                            1.,
                                            Color32::from_rgba_unmultiplied(184, 211, 217, 145),
                                        ),
                                    );
                                }
                            }
                        }
                        for (fade, left) in [(fade_in, true), (fade_out, false)] {
                            if fade > 0 {
                                let width = seconds(fade) as f32 * self.zoom;
                                let points = if left {
                                    [
                                        rect.left_bottom(),
                                        Pos2::new(rect.left() + width, rect.top()),
                                    ]
                                } else {
                                    [
                                        Pos2::new(rect.right() - width, rect.top()),
                                        rect.right_bottom(),
                                    ]
                                };
                                clip_painter
                                    .line_segment(points, Stroke::new(1., Color32::from_gray(225)));
                            }
                        }
                        let handle_inset = (rect.width() / 2.).min(5.);
                        let handles = [
                            Pos2::new(
                                (rect.left() + seconds(fade_in) as f32 * self.zoom)
                                    .clamp(rect.left() + handle_inset, rect.right() - handle_inset),
                                rect.top() + 5.,
                            ),
                            Pos2::new(
                                (rect.right() - seconds(fade_out) as f32 * self.zoom)
                                    .clamp(rect.left() + handle_inset, rect.right() - handle_inset),
                                rect.top() + 5.,
                            ),
                        ];
                        if rect.width() >= 14. {
                            for handle in handles {
                                clip_painter.rect_filled(
                                    Rect::from_center_size(handle, Vec2::splat(7.)),
                                    1.,
                                    Color32::from_gray(225),
                                );
                            }
                        }
                        let response =
                            ui.interact(rect, Id::new(("clip", &c.id)), Sense::click_and_drag());
                        trace(ui.ctx(), format!("clip:{}", c.id), &response);
                        if response.clicked() {
                            self.selected = Some(c.id.clone());
                            if let Some(player) = &self.player {
                                player.pause();
                            }
                            if let Some(pos) = response.interact_pointer_pos() {
                                self.seek(ticks(((pos.x - canvas.left()) / self.zoom) as f64));
                            }
                        }
                        if response.double_clicked() {
                            self.selected = Some(c.id.clone());
                            self.seek(c.start);
                        }
                        if response.hovered() {
                            let pos = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
                            let mode = hit_mode(rect, handles, pos);
                            ui.ctx().set_cursor_icon(if mode == 0 {
                                egui::CursorIcon::Grab
                            } else {
                                egui::CursorIcon::ResizeHorizontal
                            });
                            if mode.abs() == 2 {
                                response.clone().on_hover_text(if mode < 0 {
                                    "Drag fade in"
                                } else {
                                    "Drag fade out"
                                });
                            }
                        }
                        if response.drag_started() {
                            self.selected = Some(c.id.clone());
                            if !self.p.tracks[c.lane].locked {
                                let origin = ui
                                    .input(|i| i.pointer.press_origin())
                                    .unwrap_or(rect.center());
                                self.drag = Some(Drag {
                                    id: c.id.clone(),
                                    edge: hit_mode(rect, handles, origin),
                                    start: c.start,
                                    duration: c.duration,
                                    lane: c.lane,
                                    origin,
                                });
                                if let Some(player) = &self.player {
                                    player.pause();
                                }
                            }
                        }
                        if let Some(d) = self.drag.clone().filter(|d| d.id == c.id) {
                            if response.dragged() {
                                let dx = ui.ctx().pointer_latest_pos().unwrap_or(d.origin).x
                                    - d.origin.x;
                                let raw = ticks(dx as f64 / self.zoom as f64);
                                let mut candidate = self.p.clone();
                                if d.edge.abs() == 1 {
                                    let requested = if d.edge < 0 {
                                        d.start + raw
                                    } else {
                                        d.start + d.duration + raw
                                    };
                                    let edge = self.drag_edge(&d, requested);
                                    if edit::trim_edge(&mut candidate, &d.id, d.edge < 0, edge)
                                        .is_ok()
                                    {
                                        let preview =
                                            candidate.clips.iter().find(|x| x.id == d.id).unwrap();
                                        let source = if d.edge < 0 {
                                            preview.source_in
                                        } else {
                                            preview.source_in + preview.duration
                                                - self.p.fps.frame(1)
                                        };
                                        self.show_trim_source(&c.media, source);
                                        self.status = format!(
                                            "{} · source {:.3}s · duration {:.3}s",
                                            if d.edge < 0 { "Trim start" } else { "Trim end" },
                                            seconds(source),
                                            seconds(preview.duration)
                                        );
                                    }
                                } else if d.edge.abs() == 2 {
                                    let value = if d.edge < 0 {
                                        raw + c.fade_in
                                    } else {
                                        c.fade_out - raw
                                    };
                                    let value = value.clamp(0, c.duration);
                                    let preview =
                                        candidate.clips.iter_mut().find(|x| x.id == d.id).unwrap();
                                    if d.edge < 0 {
                                        preview.fade_in = value;
                                    } else {
                                        preview.fade_out = value;
                                    }
                                    self.seek(if d.edge < 0 {
                                        c.start + value
                                    } else {
                                        c.end() - value
                                    });
                                    self.status = format!(
                                        "Fade {} · {:.3}s",
                                        if d.edge < 0 { "in" } else { "out" },
                                        seconds(value)
                                    );
                                }
                                let preview =
                                    candidate.clips.iter().find(|x| x.id == d.id).unwrap();
                                let mut ghost = Rect::from_min_size(
                                    Pos2::new(
                                        canvas.left() + seconds(preview.start) as f32 * self.zoom,
                                        rect.top(),
                                    ),
                                    Vec2::new(
                                        seconds(preview.duration) as f32 * self.zoom,
                                        rect.height(),
                                    ),
                                );
                                if d.edge == 0 {
                                    ghost = ghost.translate(Vec2::new(dx, 0.));
                                }
                                painter.rect_stroke(
                                    ghost,
                                    0.,
                                    Stroke::new(2., Color32::WHITE),
                                    egui::StrokeKind::Inside,
                                );
                                if d.edge.abs() == 2 {
                                    let value = if d.edge < 0 {
                                        preview.fade_in
                                    } else {
                                        preview.fade_out
                                    };
                                    let x = if d.edge < 0 {
                                        rect.left() + seconds(value) as f32 * self.zoom
                                    } else {
                                        rect.right() - seconds(value) as f32 * self.zoom
                                    };
                                    painter.line_segment(
                                        [
                                            if d.edge < 0 {
                                                rect.left_bottom()
                                            } else {
                                                rect.right_bottom()
                                            },
                                            Pos2::new(x, rect.top()),
                                        ],
                                        Stroke::new(2., Color32::WHITE),
                                    );
                                }
                            }
                            if response.drag_stopped() {
                                drag_commit = Some((
                                    d.clone(),
                                    ui.ctx().pointer_latest_pos().unwrap_or(d.origin) - d.origin,
                                    ui.ctx().pointer_latest_pos(),
                                    canvas.left(),
                                    canvas.top(),
                                ));
                            }
                        }
                        response.context_menu(|ui| {
                            self.selected = Some(c.id.clone());
                            for (label, action) in [
                                ("Split at playhead", Action::Split),
                                ("Delete (leave gap)", Action::Delete),
                                ("Ripple delete", Action::Ripple),
                            ] {
                                if ui.button(label).clicked() {
                                    context_action = Some(action);
                                    ui.close_menu();
                                }
                            }
                            if ui
                                .button(if c.link.is_some() {
                                    "Unlink audio / video"
                                } else {
                                    "Link matching audio / video"
                                })
                                .clicked()
                            {
                                let id = c.id.clone();
                                self.apply(|p| {
                                    if c.link.is_some() {
                                        edit::unlink(p, &id)
                                    } else {
                                        edit::link(p, &id)
                                    }
                                });
                                ui.close_menu();
                            }
                            if c.lane > 0 && ui.button("Match voice loudness").clicked() {
                                self.normalize();
                                ui.close_menu();
                            }
                        });
                    }
                    if let Some(a) = context_action {
                        self.action(a);
                    }
                    if let Some((d, delta, pos, _left, top)) = drag_commit {
                        self.drag = None;
                        let raw = ticks(delta.x as f64 / self.zoom as f64);
                        let tolerance = ticks(8. / self.zoom as f64);
                        if d.edge == 0 {
                            let mut at = self.p.fps.align(d.start + raw).max(0);
                            if self.snapping {
                                at = edit::snap(&self.p, at, tolerance, Some(&d.id));
                                if (at - self.playhead).abs() < tolerance {
                                    at = self.playhead;
                                }
                            }
                            let lane = if d.lane == 0 {
                                0
                            } else {
                                pos.map(|p| {
                                    ((p.y - top - ruler_h) / lane_h)
                                        .floor()
                                        .clamp(1., (LANES - 1) as f32)
                                        as usize
                                })
                                .unwrap_or(d.lane)
                            };
                            self.apply(|p| edit::move_clip(p, &d.id, at, lane));
                        } else if d.edge.abs() == 2 {
                            let value = if d.edge < 0 {
                                c_fade(&self.p, &d.id, true) + raw
                            } else {
                                c_fade(&self.p, &d.id, false) - raw
                            };
                            self.apply(|p| {
                                let c = p
                                    .clips
                                    .iter_mut()
                                    .find(|c| c.id == d.id)
                                    .context("Clip removed")?;
                                if d.edge < 0 {
                                    c.fade_in = value.clamp(0, c.duration);
                                } else {
                                    c.fade_out = value.clamp(0, c.duration);
                                }
                                Ok(())
                            });
                        } else {
                            let edge = self.drag_edge(
                                &d,
                                if d.edge < 0 {
                                    d.start + raw
                                } else {
                                    d.start + d.duration + raw
                                },
                            );
                            let mut final_edge = None;
                            self.apply(|p| {
                                final_edge = Some(edit::trim_edge(p, &d.id, d.edge < 0, edge)?);
                                Ok(())
                            });
                            if let Some(at) = final_edge {
                                self.seek(if d.edge < 0 {
                                    at
                                } else {
                                    at - self.p.fps.frame(1)
                                });
                            }
                        }
                        self.source_preview = false;
                        if self.preview_revision == Some(self.revision) {
                            self.seek(self.playhead);
                        }
                    }
                    if ui.rect_contains_pointer(canvas.intersect(ui.clip_rect()))
                        && ui.input(|i| i.pointer.any_released())
                        && let Some(payload) =
                            egui::DragAndDrop::take_payload::<MediaDrag>(ui.ctx())
                        && let Some(pos) = ui.ctx().pointer_latest_pos()
                    {
                        let lane = ((pos.y - canvas.top() - ruler_h) / lane_h)
                            .floor()
                            .clamp(0., (LANES - 1) as f32)
                            as usize;
                        let mut at = self
                            .p
                            .fps
                            .align(ticks(((pos.x - canvas.left()) / self.zoom).max(0.) as f64));
                        if self.snapping {
                            at = edit::snap(&self.p, at, ticks(8. / self.zoom as f64), None);
                        }
                        if lane == 0 {
                            at = self
                                .p
                                .lane(0)
                                .iter()
                                .find(|c| at >= c.start && at < c.end())
                                .map(|c| c.start)
                                .unwrap_or(at);
                        }
                        self.add_media(payload.0.clone(), lane, Some(at));
                    }
                    let x = canvas.left() + seconds(self.playhead) as f32 * self.zoom;
                    painter.line_segment(
                        [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
                        Stroke::new(1.5, Color32::from_rgb(232, 173, 77)),
                    );
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            Pos2::new(x - 5., canvas.top()),
                            Pos2::new(x + 5., canvas.top()),
                            Pos2::new(x, canvas.top() + 8.),
                        ],
                        Color32::from_rgb(232, 173, 77),
                        Stroke::NONE,
                    ));
                    if ui.rect_contains_pointer(canvas.intersect(ui.clip_rect()))
                        && ui.input(|i| i.modifiers.ctrl)
                    {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll != 0. {
                            self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.05, 1000.);
                        }
                    }
                });
        });
    }
    fn drag_edge(&self, d: &Drag, at: Tick) -> Tick {
        let tolerance = ticks(8. / self.zoom as f64);
        let mut edge = self.p.fps.align(at);
        if self.snapping {
            let group = self
                .p
                .clips
                .iter()
                .find(|c| c.id == d.id)
                .and_then(|c| c.link.as_ref());
            if let Some(snap) = self
                .p
                .clips
                .iter()
                .filter(|c| c.id != d.id && (group.is_none() || c.link.as_ref() != group))
                .flat_map(|c| [c.start, c.end()])
                .chain([0, self.playhead])
                .filter(|t| (*t - edge).abs() <= tolerance)
                .min_by_key(|t| (*t - edge).abs())
            {
                edge = snap;
            }
        }
        edge
    }
}

fn c_fade(p: &Project, id: &str, left: bool) -> Tick {
    p.clips
        .iter()
        .find(|c| c.id == id)
        .map(|c| if left { c.fade_in } else { c.fade_out })
        .unwrap_or(0)
}

/// At low zoom, edge/fade hit areas overlap. Choose the nearest target rather
/// than giving the left handle priority and unexpectedly moving the start.
fn hit_mode(rect: Rect, handles: [Pos2; 2], origin: Pos2) -> i8 {
    let left = (origin - handles[0]).length();
    let right = (origin - handles[1]).length();
    if rect.width() >= 14. && left.min(right) <= 10. {
        return if left < right { -2 } else { 2 };
    }
    let left = (origin.x - rect.left()).abs();
    let right = (origin.x - rect.right()).abs();
    if left.min(right) < 7. {
        if left < right { -1 } else { 1 }
    } else {
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_clips_choose_the_correct_edge_and_fade() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(8., 60.));
        assert_eq!(
            hit_mode(
                rect,
                [Pos2::new(1., 5.), Pos2::new(7., 5.)],
                Pos2::new(6., 30.)
            ),
            1
        );
        assert_eq!(
            hit_mode(
                rect,
                [Pos2::new(1., 5.), Pos2::new(7., 5.)],
                Pos2::new(2., 30.)
            ),
            -1
        );
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(16., 60.));
        assert_eq!(
            hit_mode(
                rect,
                [Pos2::new(5., 5.), Pos2::new(11., 5.)],
                Pos2::new(11., 5.)
            ),
            2
        );
        assert_eq!(
            hit_mode(
                rect,
                [Pos2::new(5., 5.), Pos2::new(11., 5.)],
                Pos2::new(5., 5.)
            ),
            -2
        );
    }
}
