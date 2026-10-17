use crate::{
    edit::{self, History},
    media::{Runtime, command},
    model::*,
    persistence,
    playback::Player,
    render::{self, PreviewPlan, RenderSettings},
};
use anyhow::{Context, Result};
use eframe::egui::{
    self, Align2, Color32, FontId, Id, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke,
    Vec2,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

enum Event {
    Imported(Result<Media>),
    Wave(String, Result<Vec<f32>>),
    Thumbnail(String, Vec<u8>),
    ImportDone,
    Preview(u64, Result<PreviewPlan>),
    Saved(u64, PathBuf, Result<()>),
    Opened(PathBuf, Result<Project>),
    Loudness(String, Tick, Tick, Result<(f64, f64)>),
    Progress(f32),
    Rendered(PathBuf, Result<()>),
    Autosaved(Result<()>),
}
#[derive(Clone)]
struct MediaDrag(String);
#[derive(Clone)]
struct Drag {
    id: String,
    edge: i8,
    start: Tick,
    source_in: Tick,
    duration: Tick,
    lane: usize,
}
#[derive(Clone, Copy)]
enum Action {
    Play,
    Split,
    Delete,
    Ripple,
    Undo,
    Redo,
    Save,
    SaveAs,
    Import,
    Open,
    Left,
    Right,
    Home,
    End,
    ZoomIn,
    ZoomOut,
    AppendMedia,
    MediaPrevious,
    MediaNext,
    ClipPrevious,
    ClipNext,
    GoTo,
    MatchVoice,
    Render,
}
struct Binding {
    shortcut: KeyboardShortcut,
    action: Action,
}
fn bindings() -> Vec<Binding> {
    use Action::*;
    let command = Modifiers::COMMAND;
    let shift = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    let cs = Modifiers {
        shift: true,
        ..command
    };
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    [
        (Modifiers::NONE, Key::Space, Play),
        (Modifiers::NONE, Key::S, Split),
        (Modifiers::NONE, Key::Delete, Delete),
        (shift, Key::Delete, Ripple),
        (command, Key::Z, Undo),
        (cs, Key::Z, Redo),
        (command, Key::Y, Redo),
        (command, Key::S, Save),
        (cs, Key::S, SaveAs),
        (command, Key::I, Import),
        (command, Key::O, Open),
        (Modifiers::NONE, Key::ArrowLeft, Left),
        (Modifiers::NONE, Key::ArrowRight, Right),
        (Modifiers::NONE, Key::Home, Home),
        (Modifiers::NONE, Key::End, End),
        (Modifiers::NONE, Key::Plus, ZoomIn),
        (Modifiers::NONE, Key::Minus, ZoomOut),
        (command, Key::Enter, AppendMedia),
        (alt, Key::ArrowUp, MediaPrevious),
        (alt, Key::ArrowDown, MediaNext),
        (command, Key::ArrowLeft, ClipPrevious),
        (command, Key::ArrowRight, ClipNext),
        (command, Key::G, GoTo),
        (command, Key::L, MatchVoice),
        (command, Key::R, Render),
    ]
    .into_iter()
    .map(|(m, k, a)| Binding {
        shortcut: KeyboardShortcut::new(m, k),
        action: a,
    })
    .collect()
}
pub fn run(path: Option<PathBuf>) -> Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SonyVegasButBetter")
            .with_inner_size([1440., 900.])
            .with_min_inner_size([1024., 700.]),
        ..Default::default()
    };
    eframe::run_native(
        "SonyVegasButBetter",
        native,
        Box::new(move |cc| Ok(Box::new(Editor::new(cc, path)))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
struct Editor {
    p: Project,
    history: History,
    path: Option<PathBuf>,
    saved: Project,
    revision: u64,
    dirty: bool,
    rt: Option<Runtime>,
    player: Option<Player>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    selected: Option<String>,
    media_selected: Option<String>,
    audio_lane: usize,
    playhead: Tick,
    zoom: f32,
    snapping: bool,
    drag: Option<Drag>,
    wave: HashMap<String, Vec<f32>>,
    thumb: HashMap<String, egui::TextureHandle>,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: Option<String>,
    importing: bool,
    preview_revision: Option<u64>,
    preview_job: Option<(u64, Arc<AtomicBool>)>,
    last_edit: Instant,
    saving: bool,
    last_autosave: Instant,
    autosaving: bool,
    recovery: PathBuf,
    recovery_available: bool,
    render_dialog: bool,
    render_preset: usize,
    quality: usize,
    render_path: String,
    render_job: Option<Arc<AtomicBool>>,
    render_progress: f32,
    rendered: Option<PathBuf>,
    close_pending: bool,
    pending_open: Option<Option<PathBuf>>,
    confirm_unsaved: bool,
    allow_close: bool,
    help: bool,
    project_settings: bool,
    shortcuts: Vec<Binding>,
    goto: Option<String>,
}
impl Editor {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let ctx = &cc.egui_ctx;
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(31, 33, 36);
        visuals.window_fill = Color32::from_rgb(37, 39, 43);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., Color32::from_gray(62));
        visuals.selection.bg_fill = Color32::from_rgb(53, 95, 128);
        visuals.selection.stroke = Stroke::new(1., Color32::from_rgb(130, 191, 235));
        ctx.set_visuals(visuals);
        let mut style = (*ctx.style()).clone();
        style.spacing.button_padding = Vec2::new(8., 4.);
        style.spacing.item_spacing = Vec2::new(7., 5.);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(2);
        ctx.set_style(style);
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::discover();
        let mut error = None;
        let rt = match runtime {
            Ok(r) => Some(r),
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        };
        let player = rt.as_ref().and_then(|r| match Player::new(r) {
            Ok(p) => Some(p),
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        });
        let recovery = rt
            .as_ref()
            .map(|r| r.cache.parent().unwrap().join("recovery.svbb"))
            .unwrap_or_else(|| std::env::temp_dir().join("svbb-recovery.svbb"));
        let recovery_available = recovery.exists();
        let p = Project::default();
        let mut app = Self {
            saved: p.clone(),
            p,
            history: History::default(),
            path: None,
            revision: 0,
            dirty: false,
            rt,
            player,
            tx,
            rx,
            selected: None,
            media_selected: None,
            audio_lane: 1,
            playhead: 0,
            zoom: 50.,
            snapping: true,
            drag: None,
            wave: HashMap::new(),
            thumb: HashMap::new(),
            texture: None,
            status: "Ready — drop recordings here or import media (Ctrl+I)".into(),
            error,
            importing: false,
            preview_revision: None,
            preview_job: None,
            last_edit: Instant::now(),
            saving: false,
            last_autosave: Instant::now(),
            autosaving: false,
            recovery,
            recovery_available,
            render_dialog: false,
            render_preset: 0,
            quality: 1,
            render_path: String::new(),
            render_job: None,
            render_progress: 0.,
            rendered: None,
            close_pending: false,
            pending_open: None,
            confirm_unsaved: false,
            allow_close: false,
            help: false,
            project_settings: false,
            shortcuts: bindings(),
            goto: None,
        };
        if let Some(path) = path {
            app.open(path);
        }
        app
    }
    fn edited(&mut self) {
        self.revision += 1;
        self.dirty = self.p != self.saved;
        self.preview_revision = None;
        self.last_edit = Instant::now();
        if let Some(player) = &self.player {
            player.pause();
        }
        if let Some((_, cancel)) = &self.preview_job {
            cancel.store(true, Ordering::Relaxed);
        }
        self.status = "Edit applied; preparing preview…".into();
    }
    fn apply(&mut self, f: impl FnOnce(&mut Project) -> Result<()>) {
        match self.history.apply(&mut self.p, f) {
            Ok(true) => self.edited(),
            Ok(false) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn edit_selected(&mut self, f: impl FnOnce(&mut Project, &str) -> Result<()>) {
        if let Some(id) = self.selected.clone() {
            self.apply(|p| f(p, &id));
        } else {
            self.status = "Select a timeline clip first".into();
        }
    }
    fn import_dialog(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Import video and audio")
            .add_filter(
                "Media",
                &[
                    "mp4", "mkv", "mov", "wav", "mp3", "m4a", "flac", "ogg", "webm", "avi",
                ],
            )
            .pick_files()
        {
            self.import(files);
        }
    }
    fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(rt) = self.rt.clone() else {
            return;
        };
        if self.importing {
            self.status = "Import already running; wait for the current files to finish".into();
            return;
        }
        self.importing = true;
        let tx = self.tx.clone();
        let existing: Vec<_> = self.p.media.iter().map(|m| m.path.clone()).collect();
        std::thread::spawn(move || {
            let mut all = vec![];
            for path in paths {
                if existing.contains(&path) {
                    continue;
                }
                let result = rt.probe(&path);
                if let Ok(m) = &result {
                    all.push(m.clone());
                }
                let _ = tx.send(Event::Imported(result));
            }
            let _ = tx.send(Event::ImportDone);
            for m in all {
                if m.video.is_some()
                    && let Ok(data) = rt.thumbnail(&m)
                {
                    let _ = tx.send(Event::Thumbnail(m.id.clone(), data));
                }
                let _ = tx.send(Event::Wave(m.id.clone(), rt.waveform(&m)));
            }
        });
        self.status = "Inspecting and decoding imported files…".into();
    }
    fn index_all(&mut self) {
        if let Some(rt) = self.rt.clone() {
            let media = self.p.media.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                for m in media {
                    if !m.path.is_file() {
                        continue;
                    }
                    if m.video.is_some()
                        && let Ok(data) = rt.thumbnail(&m)
                    {
                        let _ = tx.send(Event::Thumbnail(m.id.clone(), data));
                    }
                    let _ = tx.send(Event::Wave(m.id.clone(), rt.waveform(&m)));
                }
            });
        }
    }
    fn add_media(&mut self, id: String, lane: usize, at: Option<Tick>) {
        let mut selected = None;
        self.apply(|p| {
            selected = Some(edit::append(p, &id, lane, at)?);
            Ok(())
        });
        if selected.is_some() {
            self.selected = selected;
        }
    }
    fn save(&mut self, save_as: bool) {
        if self.saving {
            return;
        }
        let path = if save_as || self.path.is_none() {
            rfd::FileDialog::new()
                .set_title("Save SVBB project")
                .add_filter("SVBB project", &["svbb"])
                .set_file_name(format!("{}.svbb", self.p.name))
                .save_file()
        } else {
            self.path.clone()
        };
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("svbb");
            }
            self.saving = true;
            let p = self.p.clone();
            let rev = self.revision;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let r = persistence::save(&path, &p);
                let _ = tx.send(Event::Saved(rev, path, r));
            });
        }
    }
    fn open(&mut self, path: PathBuf) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = persistence::load(&path);
            let _ = tx.send(Event::Opened(path, result));
        });
        self.status = "Opening project…".into();
    }
    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SVBB project", &["svbb"])
            .pick_file()
        {
            self.request_open(Some(path));
        }
    }
    fn request_open(&mut self, path: Option<PathBuf>) {
        if self.dirty {
            self.pending_open = Some(path);
            self.confirm_unsaved = true;
        } else if let Some(path) = path {
            self.open(path);
        } else {
            self.reset(Project::default(), None);
        }
    }
    fn reset(&mut self, p: Project, path: Option<PathBuf>) {
        if let Some(player) = &self.player {
            player.pause();
            player.send(&["stop"]);
        }
        self.texture = None;
        self.p = p;
        self.saved = self.p.clone();
        self.path = path;
        self.history = History::default();
        self.selected = None;
        self.media_selected = None;
        self.playhead = 0;
        self.wave.clear();
        self.thumb.clear();
        self.edited();
        self.dirty = false;
        self.index_all();
    }
    fn begin_preview(&mut self) {
        if self.p.duration() == 0 || !self.p.missing().is_empty() || self.preview_job.is_some() {
            return;
        }
        if let Some(rt) = self.rt.clone() {
            let p = self.p.clone();
            let rev = self.revision;
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let tx = self.tx.clone();
            self.preview_job = Some((rev, cancel));
            std::thread::spawn(move || {
                let result = render::prepare_preview(&rt, &p, &worker_cancel);
                let _ = tx.send(Event::Preview(rev, result));
            });
        }
    }
    fn seek(&mut self, t: Tick) {
        self.playhead = self.p.fps.align(t.clamp(0, self.p.duration()));
        if self.preview_revision == Some(self.revision)
            && let Some(player) = &self.player
        {
            player.seek(self.playhead);
        }
    }
    fn play(&mut self) {
        if self.preview_revision == Some(self.revision) {
            if let Some(player) = &self.player {
                if self.playhead >= self.p.duration() {
                    self.playhead = 0;
                    player.seek(0);
                }
                player.toggle();
            }
        } else {
            self.status =
                "The changed mix is still preparing. Playback becomes available when it is ready."
                    .into();
            self.begin_preview();
        }
    }
    fn normalize(&mut self) {
        let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.p.clips.iter().find(|c| &c.id == id))
            .cloned()
        else {
            return;
        };
        if self.p.tracks[c.lane].locked {
            self.error = Some("Track is locked".into());
            return;
        }
        if let (Some(rt), Ok(m)) = (self.rt.clone(), self.p.media(&c.media).cloned()) {
            let tx = self.tx.clone();
            self.status = "Measuring selected clip loudness (LUFS / true peak)…".into();
            std::thread::spawn(move || {
                let r = rt.loudness(&m, &c);
                let _ = tx.send(Event::Loudness(c.id, c.source_in, c.duration, r));
            });
        }
    }
    fn start_render(&mut self) {
        let Some(rt) = self.rt.clone() else {
            return;
        };
        if self.render_job.is_some() {
            return;
        }
        let path = PathBuf::from(&self.render_path);
        if self.render_path.is_empty() {
            self.error = Some("Choose an output file".into());
            return;
        }
        let mut settings = RenderSettings::project(&self.p);
        match self.render_preset {
            1 => {
                settings.width = 1920;
                settings.height = 1080;
                settings.fps = Rational::new(30, 1);
            }
            2 => {
                settings.width = 1920;
                settings.height = 1080;
                settings.fps = Rational::new(60, 1);
            }
            _ => {}
        }
        settings.quality = self.quality;
        if let Err(e) = render::plan(&self.p, &settings) {
            self.error = Some(e.to_string());
            return;
        }
        self.render_progress = 0.;
        self.rendered = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.render_job = Some(cancel.clone());
        let p = self.p.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = render::export(&rt, &p, &settings, &path, &cancel, |n| {
                let _ = tx.send(Event::Progress(n));
            });
            let _ = tx.send(Event::Rendered(path, result));
        });
    }
    fn process(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Imported(result) => match result {
                    Ok(m) => {
                        self.media_selected = Some(m.id.clone());
                        let replaced = self.p.media.iter().any(|x| x.id == m.id);
                        if replaced {
                            self.apply(|p| {
                                let old = p.media.iter_mut().find(|x| x.id == m.id).unwrap();
                                *old = m;
                                Ok(())
                            });
                            self.index_all();
                        } else if !self.p.media.iter().any(|x| x.path == m.path) {
                            self.apply(|p| {
                                p.media.push(m);
                                Ok(())
                            });
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                },
                Event::ImportDone => {
                    self.importing = false;
                    self.status =
                        "Media imported. Double-click a file to append it, or drag it onto a lane."
                            .into();
                }
                Event::Wave(id, result) => match result {
                    Ok(w) => {
                        self.wave.insert(id, w);
                    }
                    Err(e) => self.error = Some(format!("Waveform: {e}")),
                },
                Event::Thumbnail(id, data) => {
                    if let Ok(img) = image::load_from_memory(&data) {
                        let rgba = img.to_rgba8();
                        self.thumb.insert(
                            id,
                            ctx.load_texture(
                                "media",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [rgba.width() as usize, rgba.height() as usize],
                                    &rgba,
                                ),
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                    }
                }
                Event::Preview(rev, result) => {
                    self.preview_job = None;
                    if rev == self.revision {
                        match result {
                            Ok(plan) => {
                                if let Some(player) = &self.player {
                                    player.load(plan, self.playhead, false);
                                }
                                self.preview_revision = Some(rev);
                                self.status = "Preview ready".into();
                            }
                            Err(e) => {
                                if e.to_string() != "Cancelled" {
                                    self.error = Some(format!("Preview: {e}"));
                                }
                                self.preview_revision = Some(rev);
                            }
                        }
                    }
                }
                Event::Saved(rev, path, result) => {
                    self.saving = false;
                    match result {
                        Ok(()) => {
                            self.path = Some(path);
                            if rev == self.revision {
                                self.saved = self.p.clone();
                                self.dirty = false;
                                let _ = std::fs::remove_file(&self.recovery);
                                self.recovery_available = false;
                                if self.close_pending {
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                if let Some(next) = self.pending_open.take() {
                                    if let Some(path) = next {
                                        self.open(path);
                                    } else {
                                        self.reset(Project::default(), None);
                                    }
                                }
                            }
                            self.status = "Project saved".into();
                        }
                        Err(e) => {
                            self.close_pending = false;
                            self.pending_open = None;
                            self.error = Some(format!("Save: {e}"));
                        }
                    }
                }
                Event::Opened(path, result) => match result {
                    Ok(p) => {
                        let recovered = path == self.recovery;
                        self.reset(p, if recovered { None } else { Some(path) });
                        if recovered {
                            self.dirty = true;
                        }
                        self.status = "Project opened".into();
                    }
                    Err(e) => self.error = Some(format!("Open: {e}")),
                },
                Event::Loudness(id, source_in, duration, result) => {
                    match result.and_then(|(i, tp)| Ok((i, tp, match_loudness(i, tp)?))) {
                        Ok((i, tp, gain)) => {
                            self.apply(|p| {
                                let c = p
                                    .clips
                                    .iter_mut()
                                    .find(|c| c.id == id)
                                    .context("Clip was removed during measurement")?;
                                anyhow::ensure!(
                                    !p.tracks[c.lane].locked
                                        && c.source_in == source_in
                                        && c.duration == duration,
                                    "Clip changed during measurement; measure again"
                                );
                                c.gain_db = gain;
                                Ok(())
                            });
                            self.status = format!(
                                "Measured {i:.1} LUFS / {tp:.1} dBTP; matched gain {gain:+.1} dB (target −16 LUFS)"
                            );
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                Event::Progress(n) => self.render_progress = n,
                Event::Rendered(path, result) => {
                    self.render_job = None;
                    match result {
                        Ok(()) => {
                            self.rendered = Some(path);
                            self.status = "Render complete — H.264/AAC output validated".into();
                        }
                        Err(e) => self.error = Some(format!("Render: {e}")),
                    }
                }
                Event::Autosaved(result) => {
                    self.autosaving = false;
                    if let Err(e) = result {
                        self.error = Some(format!("Recovery save failed: {e}"));
                    }
                }
            }
        }
        if self.preview_revision != Some(self.revision)
            && self.preview_job.is_none()
            && self.last_edit.elapsed() > Duration::from_millis(300)
        {
            self.begin_preview();
        }
        if self.dirty && !self.autosaving && self.last_autosave.elapsed() > Duration::from_secs(15)
        {
            let p = self.p.clone();
            let path = self.recovery.clone();
            let tx = self.tx.clone();
            self.autosaving = true;
            self.last_autosave = Instant::now();
            std::thread::spawn(move || {
                let _ = tx.send(Event::Autosaved(persistence::save(&path, &p)));
            });
        }
        if let Some(player) = &self.player {
            let snap = player.snapshot();
            if !snap.paused && self.preview_revision == Some(self.revision) {
                self.playhead = snap.position.min(self.p.duration());
            }
            if let Some(error) = player.take_error() {
                self.error = Some(error);
            }
            if let Ok(mut f) = player.frame.try_lock()
                && let Some(frame) = f.take()
            {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width, frame.height],
                    &frame.pixels,
                );
                if let Some(tex) = &mut self.texture {
                    tex.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture =
                        Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
                }
            }
        }
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            self.import(dropped);
        }
    }
    fn action(&mut self, a: Action) {
        match a {
            Action::Play => self.play(),
            Action::Split => {
                let at = self.playhead;
                self.edit_selected(|p, id| {
                    edit::split(p, id, at)?;
                    Ok(())
                });
            }
            Action::Delete => self.edit_selected(|p, id| edit::delete(p, id, false)),
            Action::Ripple => self.edit_selected(|p, id| edit::delete(p, id, true)),
            Action::Undo => {
                if self.history.undo(&mut self.p) {
                    self.edited();
                }
            }
            Action::Redo => {
                if self.history.redo(&mut self.p) {
                    self.edited();
                }
            }
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::Import => self.import_dialog(),
            Action::Open => self.open_dialog(),
            Action::Left => self.seek(self.playhead - self.p.fps.frame(1)),
            Action::Right => self.seek(self.playhead + self.p.fps.frame(1)),
            Action::Home => self.seek(0),
            Action::End => self.seek(self.p.duration()),
            Action::ZoomIn => self.zoom = (self.zoom * 1.4).min(1000.),
            Action::ZoomOut => self.zoom = (self.zoom / 1.4).max(0.05),
            Action::AppendMedia => {
                if let Some(m) = self
                    .media_selected
                    .as_ref()
                    .and_then(|id| self.p.media.iter().find(|m| &m.id == id))
                {
                    self.add_media(
                        m.id.clone(),
                        if m.video.is_some() {
                            0
                        } else {
                            self.audio_lane
                        },
                        None,
                    );
                }
            }
            Action::MediaPrevious | Action::MediaNext => {
                let delta = if matches!(a, Action::MediaNext) {
                    1
                } else {
                    -1
                };
                if !self.p.media.is_empty() {
                    let pos = self
                        .media_selected
                        .as_ref()
                        .and_then(|id| self.p.media.iter().position(|m| &m.id == id))
                        .unwrap_or(0) as i32;
                    self.media_selected = Some(
                        self.p.media
                            [(pos + delta).clamp(0, self.p.media.len() as i32 - 1) as usize]
                            .id
                            .clone(),
                    );
                }
            }
            Action::ClipPrevious | Action::ClipNext => {
                let mut clips: Vec<_> = self.p.clips.iter().collect();
                clips.sort_by_key(|c| (c.lane, c.start));
                if !clips.is_empty() {
                    let pos = self
                        .selected
                        .as_ref()
                        .and_then(|id| clips.iter().position(|c| &c.id == id));
                    let pos = if let Some(pos) = pos {
                        (pos as i32 + if matches!(a, Action::ClipNext) { 1 } else { -1 })
                            .clamp(0, clips.len() as i32 - 1) as usize
                    } else {
                        0
                    };
                    let c = clips[pos];
                    let (id, start) = (c.id.clone(), c.start);
                    self.selected = Some(id);
                    self.seek(start);
                }
            }
            Action::GoTo => self.goto = Some(format!("{:.3}", seconds(self.playhead))),
            Action::MatchVoice => self.normalize(),
            Action::Render => self.render_dialog = true,
        }
    }
    fn top(&mut self, ctx: &egui::Context) {
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
                if ui.button("Fit").clicked() {
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
    fn media_panel(&mut self, ui: &mut egui::Ui) {
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
    fn preview_panel(&mut self, ui: &mut egui::Ui) {
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
        let height = (space.y - 48.).max(80.);
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
    fn inspector(&mut self, ui: &mut egui::Ui) {
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
                for(label,value)in[("Position (s)",&mut start),("Source in (s)",&mut source),("Duration (s)",&mut duration),("Gain (dB)",&mut gain),("Fade in (s)",&mut fi),("Fade out (s)",&mut fo)]{ui.label(label);let response=ui.add(egui::DragValue::new(value).speed(if label=="Gain (dB)"{0.2}else{1./60.}).max_decimals(3));
                    // egui persists keyboard text across frames, but DragValue writes
                    // its parsed value into this frame's local copy on commit.
                    if response.changed(){let id=c.id.clone();let new_value=*value;let label=label.to_string();self.apply(move|p|{let fps=p.fps;let cl=p.clips.iter_mut().find(|x|x.id==id).context("Clip removed")?;match label.as_str(){"Position (s)"=>cl.start=fps.align(ticks(new_value)),"Source in (s)"=>cl.source_in=fps.align(ticks(new_value)),"Duration (s)"=>{cl.duration=fps.align(ticks(new_value));cl.clamp_fades();},"Gain (dB)"=>cl.gain_db=new_value,"Fade in (s)"=>cl.fade_in=ticks(new_value),_=>cl.fade_out=ticks(new_value)}Ok(())});}ui.end_row();}
            });
            if ui.checkbox(&mut mute,"Mute clip").changed(){let id=c.id.clone();self.apply(|p|{p.clips.iter_mut().find(|x|x.id==id).unwrap().mute=mute;Ok(())});}
            if ui.button("Match voice loudness · −16 LUFS").on_hover_text("Measure selected source range. Limit boost to +18 dB and true peak to −1 dBTP; silence below −50 LUFS is refused.").clicked(){self.normalize();}
            ui.horizontal(|ui|{
                if ui.button("Earlier").clicked(){self.edit_selected(|p,id|edit::reorder(p,id,-1));}
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
    fn timeline(&mut self, ui: &mut egui::Ui) {
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
                                if ui
                                    .add(
                                        egui::Slider::new(&mut gain, -60.0..=12.)
                                            .suffix(" dB")
                                            .clamping(egui::SliderClamping::Never),
                                    )
                                    .changed()
                                {
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
                                let initial =
                                    response.interact_pointer_pos().unwrap_or(rect.center()).x
                                        - response.drag_delta().x;
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
                                });
                                if let Some(player) = &self.player {
                                    player.pause();
                                }
                            }
                        }
                        if let Some(d) = self.drag.as_ref().filter(|d| d.id == c.id) {
                            if response.dragged() {
                                let dx = response.drag_delta().x;
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
                                    response.drag_delta(),
                                    response.interact_pointer_pos(),
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
                    if let Some(payload) = canvas_response.dnd_release_payload::<MediaDrag>()
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
                    if canvas_response.hovered() && ui.input(|i| i.modifiers.ctrl) {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll != 0. {
                            self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.05, 1000.);
                        }
                    }
                });
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
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
                        if ui.button("Recover unsaved session").clicked() {
                            self.open(self.recovery.clone());
                            self.recovery_available = false;
                        }
                        if ui.button("Dismiss recovery").clicked() {
                            self.recovery_available = false;
                            let _ = std::fs::remove_file(&self.recovery);
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
                        egui::ComboBox::from_label("Resolution / frame rate")
                            .selected_text(
                                ["Match project", "1080p30", "1080p60"][self.render_preset],
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in
                                    ["Match project", "1080p30", "1080p60"].iter().enumerate()
                                {
                                    ui.selectable_value(&mut self.render_preset, i, *name);
                                }
                            });
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
                        ui.label("Audio: timeline mix · AAC stereo 48 kHz / 192 kbps");
                        ui.label(if self.p.peak_protection {
                            "Peak protection enabled"
                        } else {
                            "Peak protection disabled (change in Clip Properties)"
                        });
                        ui.horizontal(|ui| {
                            ui.label("Destination");
                            ui.text_edit_singleline(&mut self.render_path);
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
                        if ui.button("Render").clicked() {
                            self.start_render();
                        }
                    });
                    if let Some(cancel) = &self.render_job {
                        ui.add(egui::ProgressBar::new(self.render_progress).show_percentage());
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
            for line in ["Space: play / pause · Left / Right: one project frame · Home / End: timeline bounds","S: split selected clip · Delete: delete and leave a gap · Shift+Delete: ripple delete","Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y): undo / redo · Ctrl+S: save · Ctrl+Shift+S: save as","Ctrl+I: import · Ctrl+O: open · + / −: zoom · Ctrl+wheel: zoom over timeline","Double-click media: append. Drag media to a lane: insert. Drag timeline edges: trim.","Video drag onto another clip reorders a gapless primary sequence; independent audio stays at its positions.","Ripple delete removes the selected time range on every unlocked lane. Intersecting audio is trimmed/split, never silently left over the removed time. Lock A1/A2 to protect their positions.","Primary edge trims ripple downstream clips whose start is at or beyond the old end. Audio crossing that boundary stays anchored. Numeric property edits do not ripple.","Audio lanes permit gaps and reject overlaps; use the other lane for overlapping sound.","Match loudness measures the selected source range, targets −16 LUFS, caps boost at +18 dB and predicted true peak at −1 dBTP. Manual gain remains editable.","Splits retain the original outer fades and clear the two new internal fades. All audio edges have a 3 ms safety ramp.","The default master limiter caps sample peaks at 0.98; AAC may have small reconstructed overshoots.","Save creates a .bak of the previous saved version. Recovery saves run every 15 seconds while dirty."]{ui.label(line);ui.add_space(4.);}
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
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.render_job.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.render_dialog = true;
                self.status = "Cancel or finish the active render before closing.".into();
            } else if self.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_pending = true;
                self.confirm_unsaved = true;
            }
        }
        if !ctx.wants_keyboard_input() && !self.confirm_unsaved && !self.render_dialog {
            let actions: Vec<_> = self
                .shortcuts
                .iter()
                .filter_map(|b| {
                    ctx.input_mut(|i| i.consume_shortcut(&b.shortcut))
                        .then_some(b.action)
                })
                .collect();
            for action in actions {
                self.action(action);
            }
        }
        self.top(ctx);
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.small(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(if self.dirty {
                            "Unsaved changes"
                        } else {
                            "Saved"
                        });
                        if !self.p.missing().is_empty() {
                            ui.colored_label(
                                Color32::LIGHT_RED,
                                format!(
                                    "{} missing sources — right-click media to relink",
                                    self.p.missing().len()
                                ),
                            );
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("timeline-panel")
            .resizable(true)
            .default_height(370.)
            .min_height(280.)
            .show(ctx, |ui| self.timeline(ui));
        egui::SidePanel::left("media-panel")
            .resizable(true)
            .default_width(370.)
            .width_range(240.0..=640.)
            .show(ctx, |ui| self.media_panel(ui));
        egui::SidePanel::right("properties-panel")
            .resizable(true)
            .default_width(275.)
            .width_range(240.0..=380.)
            .show(ctx, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.preview_panel(ui));
        self.dialogs(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — SonyVegasButBetter",
            self.p.name,
            if self.dirty { " *" } else { "" }
        )));
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some((_, cancel)) = &self.preview_job {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(cancel) = &self.render_job {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
fn timecode(t: Tick, fps: Rational) -> String {
    let whole = t / HZ;
    let frame = ((t % HZ) as i128 * fps.num as i128 / (HZ as i128 * fps.den as i128)) as i64;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        whole / 3600,
        whole / 60 % 60,
        whole % 60,
        frame
    )
}
fn format_time(s: f64) -> String {
    let n = s as i64;
    format!("{:02}:{:02}", n / 60, n % 60)
}
fn open_external(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = command(std::path::Path::new("explorer.exe"))
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = command(std::path::Path::new("open")).arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = command(std::path::Path::new("xdg-open")).arg(path).spawn();
    }
}
