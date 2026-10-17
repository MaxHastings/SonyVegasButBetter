use super::*;

impl Editor {
    pub(super) fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("TIMELINE");
            ui.label("V1 + attached sound / A1 / A2");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(timecode(self.playhead, self.p.fps));
            });
        });
        ui.separator();
        let lane_h = ((ui.available_height() - 36.) / 3.).clamp(72., 160.);
        let header_w = 196.;
        let ruler_h = 26.;
        let available = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(header_w, ruler_h + 3. * lane_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add_space(ruler_h);
                    for lane in 0..3 {
                        let t = self.p.tracks[lane].clone();
                        ui.allocate_ui_with_layout(
                            Vec2::new(header_w - 8., lane_h - 5.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.strong(&t.name);
                                ui.horizontal(|ui| {
                                    let (mut mute, mut locked) = (t.mute, t.locked);
                                    if ui.checkbox(&mut mute, "Mute").changed() {
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
                                let mut gain = t.gain_db;
                                let response = ui.add(
                                    egui::Slider::new(&mut gain, -60.0..=12.)
                                        .suffix(" dB")
                                        .clamping(egui::SliderClamping::Never),
                                );
                                trace(ui.ctx(), format!("track-gain-{lane}"), &response);
                                if response.changed() {
                                    self.apply(|p| {
                                        p.tracks[lane].gain_db = gain;
                                        Ok(())
                                    });
                                }
                            },
                        );
                        ui.separator();
                    }
                },
            );
            let canvas_width = (seconds(self.p.duration() + HZ * 10) as f32 * self.zoom)
                .max(available - header_w - 22.);
            egui::ScrollArea::both()
                .id_salt("timeline-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (canvas, canvas_response) = ui.allocate_exact_size(
                        Vec2::new(canvas_width, ruler_h + 3. * lane_h),
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
                    for lane in 0..3 {
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
                        let selected = self.selected.as_ref() == Some(&c.id);
                        let muted = c.mute || self.p.tracks[c.lane].mute;
                        let color = if muted {
                            Color32::from_gray(64)
                        } else {
                            [
                                Color32::from_rgb(56, 83, 106),
                                Color32::from_rgb(62, 98, 85),
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
                                rect.min + Vec2::new(7., 6.),
                                Align2::LEFT_TOP,
                                &m.name,
                                FontId::proportional(12.),
                                Color32::from_gray(225),
                            );
                            if let Some(wave) = self.wave.get(&c.media) {
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
                                        .fold(0., f32::max)
                                        * amp;
                                    let env = (if c.fade_in > 0 {
                                        (local as f64 / seconds(c.fade_in)).min(1.)
                                    } else {
                                        1.
                                    }) * (if c.fade_out > 0 {
                                        ((seconds(c.duration) - local as f64) / seconds(c.fade_out))
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
                        for (fade, left) in [(c.fade_in, true), (c.fade_out, false)] {
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
                        let response =
                            ui.interact(rect, Id::new(("clip", &c.id)), Sense::click_and_drag());
                        trace(ui.ctx(), format!("clip:{}", c.id), &response);
                        if response.clicked() {
                            self.selected = Some(c.id.clone());
                        }
                        if response.double_clicked() {
                            self.selected = Some(c.id.clone());
                            self.seek(c.start);
                        }
                        if response.hovered() {
                            let pos = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
                            ui.ctx().set_cursor_icon(
                                if (pos.x - rect.left()).abs() < 7.
                                    || (pos.x - rect.right()).abs() < 7.
                                {
                                    egui::CursorIcon::ResizeHorizontal
                                } else {
                                    egui::CursorIcon::Grab
                                },
                            );
                        }
                        if response.drag_started() {
                            self.selected = Some(c.id.clone());
                            if !self.p.tracks[c.lane].locked {
                                let origin = ui
                                    .input(|i| i.pointer.press_origin())
                                    .unwrap_or(rect.center());
                                let initial = origin.x;
                                self.drag = Some(Drag {
                                    id: c.id.clone(),
                                    edge: if initial < rect.left() + 7. {
                                        -1
                                    } else if initial > rect.right() - 7. {
                                        1
                                    } else {
                                        0
                                    },
                                    start: c.start,
                                    source_in: c.source_in,
                                    duration: c.duration,
                                    lane: c.lane,
                                    origin,
                                });
                                if let Some(player) = &self.player {
                                    player.pause();
                                }
                            }
                        }
                        if let Some(d) = self.drag.as_ref().filter(|d| d.id == c.id) {
                            if response.dragged() {
                                let dx = ui.ctx().pointer_latest_pos().unwrap_or(d.origin).x
                                    - d.origin.x;
                                let mut ghost = rect;
                                if d.edge == 0 {
                                    ghost = ghost.translate(Vec2::new(dx, 0.));
                                } else if d.edge < 0 {
                                    ghost.min.x += dx;
                                } else {
                                    ghost.max.x += dx;
                                }
                                painter.rect_stroke(
                                    ghost,
                                    0.,
                                    Stroke::new(2., Color32::WHITE),
                                    egui::StrokeKind::Inside,
                                );
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
                            if ui.button("Match voice loudness").clicked() {
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
                            if d.lane == 0 {
                                let target = at;
                                self.apply(|p| {
                                    let moving = p
                                        .clips
                                        .iter()
                                        .find(|c| c.id == d.id)
                                        .context("Clip removed")?
                                        .clone();
                                    anyhow::ensure!(!p.tracks[0].locked, "Track is locked");
                                    if p.lane(0).iter().any(|c| {
                                        c.id != d.id
                                            && target < c.end()
                                            && target + moving.duration > c.start
                                    }) {
                                        let mut ordered: Vec<_> = p
                                            .lane(0)
                                            .into_iter()
                                            .filter(|c| c.id != d.id)
                                            .cloned()
                                            .collect();
                                        let insert = ordered
                                            .iter()
                                            .position(|c| target < c.start + c.duration / 2)
                                            .unwrap_or(ordered.len());
                                        ordered.insert(insert, moving);
                                        let mut cursor = 0;
                                        for c in ordered {
                                            let item =
                                                p.clips.iter_mut().find(|v| v.id == c.id).unwrap();
                                            item.start = cursor;
                                            cursor += item.duration;
                                        }
                                    } else {
                                        edit::move_clip(p, &d.id, target, 0)?;
                                    }
                                    Ok(())
                                });
                            } else {
                                let lane = pos
                                    .map(|p| {
                                        ((p.y - top - ruler_h) / lane_h).floor().clamp(1., 2.)
                                            as usize
                                    })
                                    .unwrap_or(d.lane);
                                self.apply(|p| edit::move_clip(p, &d.id, at, lane));
                            }
                        } else {
                            let mut edge = self.p.fps.align(if d.edge < 0 {
                                d.start + raw
                            } else {
                                d.start + d.duration + raw
                            });
                            if self.snapping {
                                edge = edit::snap(&self.p, edge, tolerance, Some(&d.id));
                            }
                            if d.edge < 0 {
                                let delta = edge - d.start;
                                let source = d.source_in + delta;
                                let duration = d.duration - delta;
                                self.apply(|p| {
                                    if d.lane == 0 {
                                        edit::trim(p, &d.id, source, duration, true)
                                    } else {
                                        edit::trim(p, &d.id, source, duration, false)?;
                                        let c = p.clips.iter_mut().find(|c| c.id == d.id).unwrap();
                                        c.start = edge;
                                        Ok(())
                                    }
                                });
                            } else {
                                self.apply(|p| {
                                    edit::trim(p, &d.id, d.source_in, edge - d.start, d.lane == 0)
                                });
                            }
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
                            .clamp(0., 2.) as usize;
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
}
