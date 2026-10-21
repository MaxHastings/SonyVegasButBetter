use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Workspace {
    pub media: bool,
    pub properties: bool,
    pub media_width: f32,
    pub properties_width: f32,
    pub timeline_height: f32,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            media: true,
            properties: true,
            media_width: 260.,
            properties_width: 250.,
            timeline_height: 280.,
        }
    }
}
impl Workspace {
    pub fn load(path: &std::path::Path) -> Self {
        let mut value: Self = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        value.media_width = value.media_width.clamp(210., 520.);
        value.properties_width = value.properties_width.clamp(230., 380.);
        value.timeline_height = value.timeline_height.clamp(270., 600.);
        value
    }
    pub fn save(&self, path: &std::path::Path) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

pub(super) fn fit_preview(bounds: Rect) -> Rect {
    let size = Vec2::new(
        bounds.width().min(bounds.height() * 16. / 9.),
        bounds.height().min(bounds.width() * 9. / 16.),
    );
    Rect::from_center_size(bounds.center(), size)
}

impl Editor {
    pub(super) fn toggle_preview_focus(&mut self) {
        if self.drag.is_none() && self.crop_session.as_ref().is_none_or(|s| !s.dragging()) {
            self.preview_focus = !self.preview_focus;
        }
    }
    pub(super) fn reset_workspace(&mut self) {
        if self.drag.is_none() && self.crop_session.as_ref().is_none_or(|s| !s.dragging()) {
            self.workspace = Workspace::default();
            self.preview_focus = false;
            self.workspace_epoch += 1;
        }
    }
}
