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
            clip: c.id,
            draft: c
                .crop
                .unwrap_or(CropRect::full(m.width & !1, m.height & !1)),
            aspect: c.crop.is_none_or(|r| {
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
    fn finish_crop(&mut self, apply: bool) {
        let Some(session) = self.crop_session.take() else {
            return;
        };
        if apply {
            let crop = self
                .p
                .clips
                .iter()
                .find(|c| c.id == session.clip)
                .and_then(|c| self.p.media(&c.media).ok())
                .and_then(|m| {
                    (session.draft != CropRect::full(m.width & !1, m.height & !1))
                        .then_some(session.draft)
                });
            self.apply(|p| edit::crop(p, &session.clip, crop));
        }
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
        if self.selected.as_ref() != Some(&session.clip) || self.p.tracks[c.lane].locked {
            self.crop_session = None;
            self.seek(self.playhead);
            return;
        }
        let at = c.source_in
            + (self.playhead - c.start).clamp(0, c.duration - self.p.fps.frame(1).min(c.duration));
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
        let Some(c) = self.p.clips.iter().find(|c| c.id == session.clip) else {
            return;
        };
        let Ok(m) = self.p.media(&c.media) else {
            return;
        };
        let full = CropRect::full(m.width & !1, m.height & !1);
        let mut apply = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("CROP");
            let free = ui.selectable_value(&mut session.aspect, false, "Free");
            trace(ui.ctx(), "crop-free", &free);
            let aspect = ui.selectable_value(&mut session.aspect, true, "Match project aspect");
            trace(ui.ctx(), "crop-aspect", &aspect);
            if aspect.clicked() {
                let r = pixel_rect(session.draft);
                let ratio = self.p.width as f32 / self.p.height as f32;
                let size = Vec2::new(
                    r.width().min(r.height() * ratio),
                    r.height().min(r.width() / ratio),
                );
                let fit = Rect::from_center_size(r.center(), size);
                session.draft = bounded_rect(fit.min, fit.max, m.width, m.height, None);
                session.refresh = true;
            }
            let preview = ui.selectable_label(session.result, "Preview result");
            trace(ui.ctx(), "crop-preview-result", &preview);
            if preview.clicked() {
                session.result = !session.result;
                session.refresh = true;
            }
            let reset = ui.button("Reset crop");
            trace(ui.ctx(), "crop-reset", &reset);
            if reset.clicked() {
                session.draft = full;
                session.refresh = true;
            }
            let accept = ui.button("Apply");
            trace(ui.ctx(), "crop-apply", &accept);
            apply = accept.clicked();
            let dismiss = ui.button("Cancel");
            trace(ui.ctx(), "crop-cancel", &dismiss);
            cancel = dismiss.clicked();
        });
        ui.small(format!("Draw a box · drag inside to move · drag corners to resize · Enter applies / Esc cancels · {}×{} at {},{}",session.draft.width,session.draft.height,session.draft.x,session.draft.y));
        if !ui.ctx().wants_keyboard_input() {
            apply |= ui.input(|i| i.key_pressed(Key::Enter));
            cancel |= ui.input(|i| i.key_pressed(Key::Escape));
        }
        self.crop_session = Some(session);
        if cancel || apply {
            self.finish_crop(apply && !cancel);
        }
    }
    pub(super) fn crop_overlay(
        &mut self,
        ui: &mut egui::Ui,
        target: Rect,
        response: &egui::Response,
    ) {
        let Some(session) = &mut self.crop_session else {
            return;
        };
        if session.result {
            return;
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
            let corner = corners.iter().position(|p| p.distance(origin) < 10.);
            let full = CropRect::full(m.width & !1, m.height & !1);
            session.drag = Some(CropDrag {
                origin: to_pixel(origin),
                before: session.draft,
                corner,
                moving: corner.is_none() && crop.contains(origin) && session.draft != full,
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
        if response.drag_stopped() {
            session.drag = None;
        }
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
        let probe = ui.interact(source, Id::new("crop-source-probe"), Sense::hover());
        trace(ui.ctx(), "crop-source", &probe);
    }
}
