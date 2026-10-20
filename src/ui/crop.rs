use super::*;

pub(super) struct CropSession {
    pub clip: String,
    draft: CropRect,
    aspect: bool,
    result: bool,
    drag: Option<CropDrag>,
    refresh: bool,
    at: Tick,
}
impl CropSession {
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}
struct CropDrag {
    origin: Pos2,
    before: CropRect,
    corner: Option<usize>,
    moving: bool,
}

fn pixel_rect(c: CropRect) -> Rect {
    Rect::from_min_size(
        Pos2::new(c.x as f32, c.y as f32),
        Vec2::new(c.width as f32, c.height as f32),
    )
}
fn bounded_rect(
    mut a: Pos2,
    mut b: Pos2,
    width: u32,
    height: u32,
    aspect: Option<f32>,
) -> CropRect {
    let bounds = Rect::from_min_max(
        Pos2::ZERO,
        Pos2::new((width & !1) as f32, (height & !1) as f32),
    );
    a = bounds.clamp(a);
    b = bounds.clamp(b);
    if let Some(ratio) = aspect {
        let delta = b - a;
        let w = delta.x.abs().min(delta.y.abs() * ratio);
        let h = w / ratio;
        b = a + Vec2::new(w * delta.x.signum(), h * delta.y.signum());
    }
    let r = Rect::from_two_pos(a, b);
    let x = (r.min.x as u32 & !1).min((width & !1).saturating_sub(2));
    let y = (r.min.y as u32 & !1).min((height & !1).saturating_sub(2));
    CropRect {
        x,
        y,
        width: ((r.width() as u32) & !1).max(2).min((width & !1) - x),
        height: ((r.height() as u32) & !1).max(2).min((height & !1) - y),
    }
}

impl Editor {
    pub(super) fn refresh_crop_preview(&mut self) {
        if let Some(session) = &mut self.crop_session {
            session.refresh = true;
        }
    }
    pub(super) fn begin_crop(&mut self) {
        let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.p.clips.iter().find(|c| &c.id == id))
            .cloned()
        else {
            return;
        };
        if c.lane != 0 || self.p.tracks[0].locked {
            return;
        }
        let Ok(m) = self.p.media(&c.media) else {
            return;
        };
        if m.width < 2 || m.height < 2 {
            return;
        }
        if let Some(player) = &self.player {
            player.pause();
        }
        if self.render_job.is_none() {
            self.render_dialog = false;
        }
        self.playhead = self
            .playhead
            .clamp(c.start, c.end() - self.p.fps.frame(1).min(c.duration));
        self.crop_session = Some(CropSession {
            clip: c.id.clone(),
            draft: c
                .crop_at(c.source_in + self.playhead - c.start)
                .unwrap_or(CropRect::full(m.width & !1, m.height & !1)),
            aspect: c
                .crop_at(c.source_in + self.playhead - c.start)
                .is_none_or(|r| {
                    (r.width as f64 / r.height as f64 - self.p.width as f64 / self.p.height as f64)
                        .abs()
                        < 0.01
                }),
            result: false,
            drag: None,
            refresh: true,
            at: -1,
        });
    }
    fn finish_crop(&mut self) {
        let Some(_session) = self.crop_session.take() else {
            return;
        };
        self.seek(self.playhead);
    }
    pub(super) fn update_crop_preview(&mut self) {
        let Some(session) = &mut self.crop_session else {
            return;
        };
        let Some(c) = self.p.clips.iter().find(|c| c.id == session.clip) else {
            self.crop_session = None;
            return;
        };
        if self.selected.as_ref() != Some(&session.clip)
            || self.p.tracks[c.lane].locked
            || self.playhead < c.start
            || self.playhead >= c.end()
        {
            self.crop_session = None;
            self.seek(self.playhead);
            return;
        }
        let at = c.source_in
            + (self.playhead - c.start).clamp(0, c.duration - self.p.fps.frame(1).min(c.duration));
        if session.drag.is_none() {
            session.draft = c.crop_at(at).unwrap_or_else(|| {
                self.p
                    .media(&c.media)
                    .map(|m| CropRect::full(m.width & !1, m.height & !1))
                    .unwrap_or(session.draft)
            });
        }
        if (session.refresh || session.at != at)
            && let (Some(player), Ok(m)) = (&self.player, self.p.media(&c.media))
        {
            player.show_source_cropped(m.clone(), at, session.result.then_some(session.draft));
            self.source_preview = true;
            session.at = at;
            session.refresh = false;
        }
    }
    pub(super) fn crop_toolbar(&mut self, ui: &mut egui::Ui) {
        let Some(mut session) = self.crop_session.take() else {
            return;
        };
        let Some(c) = self.p.clips.iter().find(|c| c.id == session.clip).cloned() else {
            return;
        };
        let Ok(m) = self.p.media(&c.media) else {
            return;
        };
        let full = CropRect::full(m.width & !1, m.height & !1);
        let at = c.source_in
            + (self.playhead - c.start).clamp(0, c.duration - self.p.fps.frame(1).min(c.duration));
        let mut close = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("FRAME");
            let free = ui.selectable_value(&mut session.aspect,false,"Free");
            trace(ui.ctx(),"crop-free",&free);
            let aspect = ui.selectable_value(&mut session.aspect,true,"Project aspect");
            trace(ui.ctx(),"crop-aspect",&aspect);
            let view = ui.selectable_label(!session.result,"Adjust frame").on_hover_text("Show the full source and drag the frame. Alt+drag draws a new rectangle anywhere.");
            trace(ui.ctx(),"crop-preview-result",&view);
            if view.clicked() { session.result = !session.result; session.refresh = true; }
            let mut animated = !c.framing.is_empty();
            let animate = ui.checkbox(&mut animated,"Animate").on_hover_text("Off: one frame for the whole clip. On: seek elsewhere and adjust to add a motion point. Turning off holds the current view.");
            trace(ui.ctx(),"frame-animate",&animate);
            if animate.changed() {
                self.apply(|p|edit::animate_frame(p,&session.clip,at,animated));
                session.refresh = true;
            }
            let reset = ui.button("Reset").on_hover_text("Remove framing and motion from this clip. Undo restores it.");
            trace(ui.ctx(),"crop-reset",&reset);
            if reset.clicked() { self.apply(|p|edit::crop(p,&session.clip,None)); session.draft=full; session.refresh=true; }
            let done = ui.button("×").on_hover_text("Close framing controls; changes are already saved to the edit [Esc]");
            trace(ui.ctx(),"frame-close",&done);
            close = done.clicked();
        });
        if !c.framing.is_empty() {
            egui::ScrollArea::horizontal()
                .id_salt("frame-points")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.small("Motion points");
                        let points: Vec<_> = c
                            .framing
                            .iter()
                            .filter(|p| p.at >= c.source_in && p.at < c.source_in + c.duration)
                            .collect();
                        for (i, point) in points.iter().enumerate() {
                            let b = ui.selectable_label(
                                point.at == at,
                                format!(
                                    "{} · {}",
                                    i + 1,
                                    timecode(point.at - c.source_in, self.p.fps)
                                ),
                            );
                            trace(ui.ctx(), format!("frame-point-{i}"), &b);
                            if b.clicked() {
                                self.seek_exact(c.start + point.at - c.source_in);
                                session.refresh = true;
                            }
                        }
                        let remove = ui.add_enabled(
                            c.framing.iter().any(|p| p.at == at),
                            egui::Button::new("Remove point"),
                        );
                        trace(ui.ctx(), "frame-remove", &remove);
                        if remove.clicked() {
                            self.apply(|p| edit::remove_frame(p, &session.clip, at));
                            session.refresh = true;
                        }
                    });
                });
        }
        ui.small(if c.framing.is_empty() { "Drag the preview to frame · release saves · Ctrl+Z undoes · Alt+drag draws a new frame" } else { "Seek, then adjust to add a point · motion is linear between points · drag a corner to zoom" });
        if !ui.ctx().wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::Escape)) {
            if let Some(drag) = session.drag.take() {
                session.draft = drag.before;
                session.result = true;
                session.refresh = true;
            } else {
                close = true;
            }
        }
        self.crop_session = Some(session);
        if close {
            self.finish_crop();
        }
    }
    fn commit_frame(&mut self) {
        let Some(mut session) = self.crop_session.take() else {
            return;
        };
        if let Some(c) = self.p.clips.iter().find(|c| c.id == session.clip) {
            let at = c.source_in
                + (self.playhead - c.start)
                    .clamp(0, c.duration - self.p.fps.frame(1).min(c.duration));
            let rect = session.draft;
            self.apply(|p| edit::frame_at(p, &session.clip, at, rect));
        }
        session.drag = None;
        session.result = true;
        session.refresh = true;
        self.crop_session = Some(session);
    }
    pub(super) fn crop_overlay(
        &mut self,
        ui: &mut egui::Ui,
        target: Rect,
        response: &egui::Response,
    ) {
        // Pressing directly in the preview enters framing; no Apply step.
        let pressed = response.hovered() && ui.input(|i| i.pointer.primary_pressed());
        if pressed && self.crop_session.is_none() {
            self.begin_crop();
        }
        if self.crop_session.is_none() {
            let probe = ui.interact(target, Id::new("crop-source-probe"), Sense::hover());
            trace(ui.ctx(), "crop-source", &probe);
            return;
        }
        let session = self.crop_session.as_mut().unwrap();
        if pressed && session.result {
            session.result = false;
            session.refresh = true;
        }
        let Some(m) = self
            .p
            .clips
            .iter()
            .find(|c| c.id == session.clip)
            .and_then(|c| self.p.media(&c.media).ok())
        else {
            return;
        };
        let mut source = target;
        let ratio = m.width as f32 / m.height as f32;
        if source.width() / source.height() > ratio {
            source = Rect::from_center_size(
                source.center(),
                Vec2::new(source.height() * ratio, source.height()),
            );
        } else {
            source = Rect::from_center_size(
                source.center(),
                Vec2::new(source.width(), source.width() / ratio),
            );
        }
        let probe = ui.interact(source, Id::new("crop-source-probe"), Sense::hover());
        trace(ui.ctx(), "crop-source", &probe);
        if session.result {
            return;
        }
        let to_screen = |p: Pos2| {
            source.min
                + Vec2::new(
                    p.x / m.width as f32 * source.width(),
                    p.y / m.height as f32 * source.height(),
                )
        };
        let to_pixel = |p: Pos2| {
            Pos2::new(
                (p.x - source.min.x) / source.width() * m.width as f32,
                (p.y - source.min.y) / source.height() * m.height as f32,
            )
        };
        let r = pixel_rect(session.draft);
        let crop = Rect::from_min_max(to_screen(r.min), to_screen(r.max));
        let corners = [
            crop.left_top(),
            crop.right_top(),
            crop.right_bottom(),
            crop.left_bottom(),
        ];
        if response.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && source.contains(origin)
        {
            let corner = (!ui.input(|i| i.modifiers.alt))
                .then(|| corners.iter().position(|p| p.distance(origin) < 10.))
                .flatten();
            let full = CropRect::full(m.width & !1, m.height & !1);
            session.drag = Some(CropDrag {
                origin: to_pixel(origin),
                before: session.draft,
                corner,
                moving: corner.is_none()
                    && crop.contains(origin)
                    && session.draft != full
                    && !ui.input(|i| i.modifiers.alt),
            });
        }
        if response.dragged()
            && let (Some(drag), Some(pos)) = (&session.drag, response.interact_pointer_pos())
        {
            let pos = to_pixel(pos);
            let before = pixel_rect(drag.before);
            session.draft = if drag.moving {
                let delta = pos - drag.origin;
                CropRect {
                    x: ((before.min.x + delta.x).clamp(0., (m.width - drag.before.width) as f32)
                        as u32)
                        & !1,
                    y: ((before.min.y + delta.y).clamp(0., (m.height - drag.before.height) as f32)
                        as u32)
                        & !1,
                    ..drag.before
                }
            } else {
                let anchor = match drag.corner {
                    Some(0) => before.right_bottom(),
                    Some(1) => before.left_bottom(),
                    Some(2) => before.left_top(),
                    Some(3) => before.right_top(),
                    _ => drag.origin,
                };
                bounded_rect(
                    anchor,
                    pos,
                    m.width,
                    m.height,
                    session
                        .aspect
                        .then_some(self.p.width as f32 / self.p.height as f32),
                )
            };
            session.refresh = true;
        }
        let commit = response.drag_stopped() && session.drag.is_some();
        let r = pixel_rect(session.draft);
        let crop = Rect::from_min_max(to_screen(r.min), to_screen(r.max));
        let shade = Color32::from_black_alpha(160);
        for region in [
            Rect::from_min_max(source.min, Pos2::new(source.max.x, crop.min.y)),
            Rect::from_min_max(Pos2::new(source.min.x, crop.max.y), source.max),
            Rect::from_min_max(Pos2::new(source.min.x, crop.min.y), crop.left_bottom()),
            Rect::from_min_max(crop.right_top(), Pos2::new(source.max.x, crop.max.y)),
        ] {
            ui.painter().rect_filled(region, 0., shade);
        }
        ui.painter().rect_stroke(
            crop,
            0.,
            Stroke::new(2., Color32::YELLOW),
            egui::StrokeKind::Inside,
        );
        for p in [
            crop.left_top(),
            crop.right_top(),
            crop.right_bottom(),
            crop.left_bottom(),
        ] {
            ui.painter().rect_filled(
                Rect::from_center_size(p, Vec2::splat(7.)),
                0.,
                Color32::YELLOW,
            );
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if commit {
            self.commit_frame();
        }
    }
}
