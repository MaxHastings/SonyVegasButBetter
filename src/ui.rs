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

mod crop;
mod dialogs;
#[cfg(feature = "gui-test")]
mod harness;
mod panels;
mod timeline;
const RENDER_PRESETS: [&str; 6] = [
    "Match project",
    "1080p30",
    "1080p60",
    "720p30",
    "720p60",
    "480p30",
];
fn trace(ctx: &egui::Context, name: impl AsRef<str>, response: &egui::Response) {
    #[cfg(feature = "gui-test")]
    ctx.data_mut(|d| d.insert_temp(Id::new(("gui-probe", name.as_ref())), response.rect));
    #[cfg(not(feature = "gui-test"))]
    let _ = (ctx, name, response);
}
enum Event {
    Imported(Result<Media>),
    Wave(String, Result<Vec<f32>>),
    WaveChunk(String, usize, Vec<f32>),
    Thumbnail(String, Vec<u8>),
    ImportDone,
    Preview(u64, Result<PreviewPlan>),
    PreviewProgress(u64, String, f32),
    Saved(u64, PathBuf, Project, Result<()>),
    Opened(PathBuf, Result<Project>),
    Loudness(String, Tick, Tick, Result<(f64, f64)>),
    Progress(f32),
    Rendered(PathBuf, Result<()>),
    Encoding(render::Encoder),
    RenderStage(String),
    Autosaved(Result<()>),
}
#[derive(Clone)]
struct MediaDrag(String);
#[derive(Clone)]
struct Drag {
    id: String,
    edge: i8,
    start: Tick,
    duration: Tick,
    lane: usize,
    origin: Pos2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    FitTimeline,
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
    let mut result: Vec<Binding> = [
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
        (Modifiers::NONE, Key::F, FitTimeline),
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
    .collect();
    // egui ignores extra Shift/Alt while matching; modified shortcuts must win.
    result.sort_by_key(|b| {
        std::cmp::Reverse(u8::from(b.shortcut.modifiers.shift) + u8::from(b.shortcut.modifiers.alt))
    });
    result
}
pub fn run(path: Option<PathBuf>) -> Result<()> {
    let icon = image::load_from_memory(include_bytes!("../assets/svbb.png"))?.to_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SonyVegasButBetter")
            .with_icon(Arc::new(icon))
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
    recovered_unsaved: bool,
    rt: Option<Runtime>,
    player: Option<Player>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    selected: Option<String>,
    media_selected: Option<String>,
    scroll_to_media: bool,
    scroll_to_clip: Option<String>,
    audio_lane: usize,
    playhead: Tick,
    zoom: f32,
    timeline_offset: f32,
    timeline_width: f32,
    timeline_pending_offset: Option<f32>,
    timeline_previous_view: Option<(f32, f32)>,
    snapping: bool,
    waveform_auto: bool,
    asset_cancel: Arc<AtomicBool>,
    source_preview: bool,
    crop_session: Option<crop::CropSession>,
    play_when_ready: bool,
    drag: Option<Drag>,
    wave: HashMap<String, Vec<f32>>,
    wave_peak: HashMap<String, f32>,
    thumb: HashMap<String, egui::TextureHandle>,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: Option<String>,
    importing: bool,
    opening: bool,
    preview_revision: Option<u64>,
    preview_failed_revision: Option<u64>,
    preview_job: Option<(u64, Arc<AtomicBool>, std::thread::JoinHandle<()>)>,
    last_edit: Instant,
    saving: bool,
    last_autosave: Instant,
    autosaving: bool,
    recovery: PathBuf,
    recovery_available: bool,
    recovery_source: Option<PathBuf>,
    render_dialog: bool,
    render_preset: usize,
    render_limit: bool,
    render_limit_mb: f64,
    render_auto_format: bool,
    render_stage: String,
    quality: usize,
    render_path: String,
    render_job: Option<Arc<AtomicBool>>,
    render_progress: f32,
    rendered: Option<PathBuf>,
    encoder: render::Encoder,
    encoder_used: Option<render::Encoder>,
    render_started: Option<Instant>,
    render_duration: Tick,
    close_pending: bool,
    pending_open: Option<Option<PathBuf>>,
    confirm_unsaved: bool,
    allow_close: bool,
    help: bool,
    project_settings: bool,
    shortcuts: Vec<Binding>,
    goto: Option<String>,
    capture_path: Option<PathBuf>,
    capture_requested: bool,
    #[cfg(feature = "gui-test")]
    harness: Option<harness::Harness>,
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
        let recovery_dir = recovery.parent().unwrap();
        let mut recoveries: Vec<_> = std::fs::read_dir(recovery_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("recovery"))
                    && p.extension().is_some_and(|e| e == "svbb")
            })
            .collect();
        recoveries.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
        let recovery_source = recoveries.pop();
        let recovery = recovery_dir.join(format!("recovery-{}.svbb", id()));
        let recovery_available = recovery_source.is_some();
        let p = Project::default();
        let mut app = Self {
            saved: p.clone(),
            p,
            history: History::default(),
            path: None,
            revision: 0,
            dirty: false,
            recovered_unsaved: false,
            rt,
            player,
            tx,
            rx,
            selected: None,
            media_selected: None,
            scroll_to_media: false,
            scroll_to_clip: None,
            audio_lane: 2,
            playhead: 0,
            zoom: 50.,
            timeline_offset: 0.,
            timeline_width: 1000.,
            timeline_pending_offset: None,
            timeline_previous_view: None,
            snapping: true,
            waveform_auto: true,
            asset_cancel: Arc::new(AtomicBool::new(false)),
            source_preview: false,
            crop_session: None,
            play_when_ready: false,
            drag: None,
            wave: HashMap::new(),
            wave_peak: HashMap::new(),
            thumb: HashMap::new(),
            texture: None,
            status: "Ready — drop recordings here or import media (Ctrl+I)".into(),
            error,
            importing: false,
            opening: false,
            preview_revision: None,
            preview_failed_revision: None,
            preview_job: None,
            last_edit: Instant::now(),
            saving: false,
            last_autosave: Instant::now(),
            autosaving: false,
            recovery,
            recovery_available,
            recovery_source,
            render_dialog: false,
            render_preset: 0,
            render_limit: false,
            render_limit_mb: 10.,
            render_auto_format: true,
            render_stage: String::new(),
            quality: 1,
            render_path: String::new(),
            render_job: None,
            render_progress: 0.,
            rendered: None,
            encoder: render::Encoder::Auto,
            encoder_used: None,
            render_started: None,
            render_duration: 0,
            close_pending: false,
            pending_open: None,
            confirm_unsaved: false,
            allow_close: false,
            help: false,
            project_settings: false,
            shortcuts: bindings(),
            goto: None,
            capture_path: std::env::var_os("SVBB_UI_SNAPSHOT").map(PathBuf::from),
            capture_requested: false,
            #[cfg(feature = "gui-test")]
            harness: None,
        };
        if let Some(path) = path {
            app.open(path);
        }
        #[cfg(feature = "gui-test")]
        if let Some(stage) = std::env::var_os("SVBB_GUI_TEST") {
            let harness = harness::Harness::new(stage.to_string_lossy().as_ref());
            app.recovery_available = false;
            app.recovery = harness.root.join("gui-recovery.svbb");
            app.path = Some(harness.root.join(match stage.to_string_lossy().as_ref() {
                "qol" => "gui-qol.svbb",
                "crop-size" => "gui-crop-size.svbb",
                "placement" => "gui-placement.svbb",
                "seek-resize" => "gui-seek-resize.svbb",
                _ => "gui-session.svbb",
            }));
            if stage == "crash-read"
                && let Ok(bytes) = std::fs::read(harness.root.join("gui-report-crash-write.json"))
                && let Ok(report) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                app.recovery_source = report["recovery"].as_str().map(PathBuf::from);
                app.recovery_available = app.recovery_source.is_some();
            }
            app.harness = Some(harness);
        }
        app
    }
    fn edited(&mut self) {
        self.playhead = self.playhead.clamp(0, self.p.duration());
        self.revision += 1;
        self.dirty = self.recovered_unsaved || self.p != self.saved;
        self.preview_revision = None;
        self.preview_failed_revision = None;
        if self.p.clips.is_empty() {
            self.texture = None;
            if let Some(player) = &self.player {
                player.send(&["stop"]);
            }
        }
        self.last_edit = Instant::now();
        if let Some(player) = &self.player {
            player.pause();
        }
        if let Some((_, cancel, _)) = &self.preview_job {
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
        if self.opening {
            self.status = "Wait for the project to finish opening before importing".into();
            return;
        }
        let Some(rt) = self.rt.clone() else {
            return;
        };
        if self.importing {
            self.status = "Import already running; wait for the current files to finish".into();
            return;
        }
        self.importing = true;
        let tx = self.tx.clone();
        let cancel = self.asset_cancel.clone();
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
            for m in &all {
                if m.video.is_some()
                    && let Ok(data) = rt.thumbnail(m)
                {
                    let _ = tx.send(Event::Thumbnail(m.id.clone(), data));
                }
            }
            all.sort_by_key(|m| m.duration);
            for m in all {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let result = rt.waveform_progress(&m, &cancel, |offset, peaks| {
                    let _ = tx.send(Event::WaveChunk(m.id.clone(), offset, peaks));
                });
                let _ = tx.send(Event::Wave(m.id.clone(), result));
            }
        });
        self.status = "Inspecting and decoding imported files…".into();
    }
    fn index_all(&mut self) {
        if let Some(rt) = self.rt.clone() {
            let mut media = self.p.media.clone();
            media.sort_by_key(|m| m.duration);
            let cancel = self.asset_cancel.clone();
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
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let result = rt.waveform_progress(&m, &cancel, |offset, peaks| {
                        let _ = tx.send(Event::WaveChunk(m.id.clone(), offset, peaks));
                    });
                    let _ = tx.send(Event::Wave(m.id.clone(), result));
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
        if selected
            .as_ref()
            .is_some_and(|id| self.p.clips.iter().any(|c| &c.id == id))
        {
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
                let _ = tx.send(Event::Saved(rev, path, p, r));
            });
        } else {
            self.close_pending = false;
            self.pending_open = None;
        }
    }
    fn open(&mut self, path: PathBuf) {
        if self.opening || self.importing || self.saving {
            self.status = "Wait for the current project operation to finish".into();
            return;
        }
        self.opening = true;
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
        if self.opening || self.importing || self.saving {
            self.status = "Wait for the current project operation to finish".into();
            return;
        }
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
        self.crop_session = None;
        if let Some(player) = &self.player {
            player.send(&["stop"]);
        }
        self.asset_cancel.store(true, Ordering::Relaxed);
        self.asset_cancel = Arc::new(AtomicBool::new(false));
        self.source_preview = false;
        self.play_when_ready = false;
        if let Some(player) = &self.player {
            player.pause();
            player.send(&["stop"]);
        }
        self.texture = None;
        self.p = p;
        self.recovered_unsaved = false;
        self.saved = self.p.clone();
        self.path = path;
        self.history = History::default();
        self.selected = None;
        self.media_selected = None;
        self.playhead = 0;
        self.timeline_offset = 0.;
        self.timeline_pending_offset = Some(0.);
        self.timeline_previous_view = None;
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
            let worker = std::thread::spawn(move || {
                let result =
                    render::prepare_preview_progress(&rt, &p, &worker_cancel, |name, n| {
                        let _ = tx.send(Event::PreviewProgress(rev, name.into(), n));
                    });
                let _ = tx.send(Event::Preview(rev, result));
            });
            self.preview_job = Some((rev, cancel, worker));
        }
    }
    fn seek(&mut self, t: Tick) {
        self.refresh_crop_preview();
        self.source_preview = false;
        self.playhead = self
            .p
            .fps
            .align(t.clamp(0, self.p.duration()))
            .min(self.p.duration());
        if let Some(player) = &self.player {
            player.pause();
        }
        if self.preview_revision == Some(self.revision)
            && let Some(player) = &self.player
        {
            player.seek(self.playhead);
        }
    }
    fn show_trim_source(&mut self, media: &str, source: Tick, crop: Option<CropRect>) {
        if let (Some(player), Ok(m)) = (&self.player, self.p.media(media))
            && m.video.is_some()
        {
            player.show_source_cropped(
                m.clone(),
                source.clamp(0, (m.duration - self.p.fps.frame(1)).max(0)),
                crop,
            );
            self.source_preview = true;
        }
    }
    fn play(&mut self) {
        if self.crop_session.is_some() {
            return;
        }
        if self.p.duration() == 0 {
            self.play_when_ready = false;
            self.status = "Add media to the timeline to play".into();
            return;
        }
        if self.source_preview {
            self.seek(self.playhead);
        }
        if self.preview_revision == Some(self.revision) {
            if let Some(player) = &self.player {
                if self.playhead >= self.p.duration() {
                    self.playhead = 0;
                    player.seek(0);
                }
                player.toggle();
            }
        } else {
            self.preview_failed_revision = None;
            self.play_when_ready = true;
            self.status = "Opening source playback…".into();
            self.begin_preview();
        }
    }
    fn normalize(&mut self) {
        let Some(mut c) = self
            .selected
            .as_ref()
            .and_then(|id| self.p.clips.iter().find(|c| &c.id == id))
            .cloned()
        else {
            return;
        };
        if c.lane == 0 {
            if let Some(audio) = self
                .p
                .clips
                .iter()
                .find(|audio| audio.lane > 0 && c.link.is_some() && audio.link == c.link)
            {
                c = audio.clone();
            } else {
                self.status = "Select an audio clip to match loudness".into();
                return;
            }
        }
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
    fn render_settings(&self) -> Result<RenderSettings> {
        let mut settings = RenderSettings::project(&self.p);
        match self.render_preset {
            3..=5 => {
                (settings.width, settings.height) = if self.render_preset == 5 {
                    (854, 480)
                } else {
                    (1280, 720)
                };
                settings.fps = Rational::new(if self.render_preset == 4 { 60 } else { 30 }, 1);
            }
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
        settings.encoder = self.encoder;
        if self.render_limit {
            anyhow::ensure!(
                self.render_limit_mb.is_finite() && (0.1..=5000.).contains(&self.render_limit_mb),
                "Enter a size limit between 0.1 and 5,000 MB"
            );
            settings.max_bytes = Some((self.render_limit_mb * 1_000_000.).floor() as u64);
            render::size_budget(settings.max_bytes.unwrap(), self.p.duration())?;
            if self.render_auto_format {
                render::fit_size_format(&mut settings, self.p.duration())?;
            }
        }
        Ok(settings)
    }
    fn start_render(&mut self) {
        if self.crop_session.is_some() {
            self.error = Some("Apply or cancel the crop before rendering".into());
            return;
        }
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
        let settings = match self.render_settings() {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        if let Err(e) = render::plan(&self.p, &settings) {
            self.error = Some(e.to_string());
            return;
        }
        self.render_progress = 0.;
        self.render_started = Some(Instant::now());
        self.encoder_used = None;
        self.render_stage = "Starting export".into();
        self.render_duration = self.p.duration();
        self.rendered = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.render_job = Some(cancel.clone());
        let p = self.p.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = render::export_report(
                &rt,
                &p,
                &settings,
                &path,
                &cancel,
                |n| {
                    let _ = tx.send(Event::Progress(n));
                },
                |notice| {
                    let event = match notice {
                        render::RenderNotice::Encoder(encoder) => Event::Encoding(encoder),
                        render::RenderNotice::Stage(stage) => Event::RenderStage(stage.into()),
                    };
                    let _ = tx.send(event);
                },
            );
            let _ = tx.send(Event::Rendered(path, result));
        });
    }
    fn process(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::PreviewProgress(rev, name, n) => {
                    if rev == self.revision {
                        self.status = format!("Opening source playback: {name} — {:.0}%", n * 100.);
                    }
                }
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
                Event::WaveChunk(id, offset, peaks) => {
                    if self.p.media.iter().any(|m| m.id == id) {
                        let peak = peaks.iter().copied().fold(0., f32::max);
                        self.wave_peak
                            .entry(id.clone())
                            .and_modify(|v| *v = v.max(peak))
                            .or_insert(peak);
                        let wave = self.wave.entry(id).or_default();
                        if wave.len() == offset {
                            wave.extend(peaks);
                        }
                    }
                }
                Event::Wave(id, result) => match result {
                    Ok(w) => {
                        if self.p.media.iter().any(|m| m.id == id) {
                            self.wave_peak
                                .insert(id.clone(), w.iter().copied().fold(0., f32::max));
                            self.wave.insert(id, w);
                        }
                    }
                    Err(e) => {
                        if e.to_string() != "Cancelled" {
                            self.status = format!("Waveform unavailable: {e}");
                        }
                    }
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
                                    player.load(plan, self.playhead, self.play_when_ready);
                                    self.play_when_ready = false;
                                }
                                self.preview_revision = Some(rev);
                                self.refresh_crop_preview();
                                self.status = "Preview ready".into();
                            }
                            Err(e) => {
                                self.status =
                                    "Preview preparation failed. Play retries the job.".into();
                                if e.to_string() != "Cancelled" {
                                    self.error = Some(format!("Preview: {e}"));
                                }
                                self.preview_failed_revision = Some(rev);
                            }
                        }
                    }
                }
                Event::Saved(_rev, path, snapshot, result) => {
                    self.saving = false;
                    match result {
                        Ok(()) => {
                            self.path = Some(path);
                            self.saved = snapshot;
                            self.recovered_unsaved = false;
                            self.dirty = self.p != self.saved;
                            if !self.dirty {
                                let _ = std::fs::remove_file(&self.recovery);
                                self.recovery_available = false;
                                if let Some(old) = self.recovery_source.take() {
                                    let _ = std::fs::remove_file(old);
                                }
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
                Event::Opened(path, result) => {
                    self.opening = false;
                    match result {
                        Ok(p) => {
                            let recovered = self.recovery_source.as_ref() == Some(&path);
                            self.reset(p, if recovered { None } else { Some(path) });
                            if recovered {
                                self.recovered_unsaved = true;
                                self.dirty = true;
                            }
                            self.status = "Project opened".into();
                        }
                        Err(e) => self.error = Some(format!("Open: {e}")),
                    }
                }
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
                Event::Progress(n) => {
                    if n < self.render_progress {
                        self.render_started = Some(Instant::now());
                    }
                    self.render_progress = n;
                }
                Event::Encoding(encoder) => self.encoder_used = Some(encoder),
                Event::RenderStage(stage) => self.render_stage = stage,
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
                    if !self.dirty {
                        let _ = std::fs::remove_file(&self.recovery);
                    }
                    if let Err(e) = result {
                        self.error = Some(format!("Recovery save failed: {e}"));
                    }
                }
            }
        }
        if self.preview_revision != Some(self.revision)
            && self.preview_failed_revision != Some(self.revision)
            && self.preview_job.is_none()
            && self.drag.is_none()
            && self.last_edit.elapsed() > Duration::from_millis(40)
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
        self.update_crop_preview();
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
                if self.selected.as_ref().is_none_or(|id| {
                    !self
                        .p
                        .clips
                        .iter()
                        .any(|c| &c.id == id && at > c.start && at < c.end())
                }) {
                    self.selected = self
                        .p
                        .lane(0)
                        .into_iter()
                        .find(|c| at > c.start && at < c.end())
                        .map(|c| c.id.clone());
                }
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
            Action::Left => {
                if let Some(player) = &self.player {
                    player.pause();
                }
                self.seek(self.playhead - self.p.fps.frame(1));
            }
            Action::Right => {
                if let Some(player) = &self.player {
                    player.pause();
                }
                self.seek(self.playhead + self.p.fps.frame(1));
            }
            Action::Home => self.seek(0),
            Action::End => self.seek(self.p.duration()),
            Action::ZoomIn => self.zoom_timeline(self.zoom * 1.4, None),
            Action::ZoomOut => self.zoom_timeline(self.zoom / 1.4, None),
            Action::FitTimeline => self.fit_timeline(),
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
                    self.scroll_to_media = true;
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
                    self.scroll_to_clip = self.selected.clone();
                    self.seek(start);
                }
            }
            Action::GoTo => self.goto = Some(format!("{:.3}", seconds(self.playhead))),
            Action::MatchVoice => self.normalize(),
            Action::Render => self.render_dialog = true,
        }
    }
}

impl eframe::App for Editor {
    #[cfg(feature = "gui-test")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(mut harness) = self.harness.take() {
            harness.input(self, ctx, raw);
            self.harness = Some(harness);
        }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(path) = self.capture_path.clone() {
            let captured = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(frame) = captured {
                let rgba: Vec<_> = frame.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(image) =
                    image::RgbaImage::from_raw(frame.width() as u32, frame.height() as u32, rgba)
                    && let Err(e) = image.save(path)
                {
                    self.error = Some(format!("Diagnostic image: {e}"));
                }
                self.capture_path = None;
            } else if !self.capture_requested && self.last_edit.elapsed() > Duration::from_secs(2) {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
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
        if !ctx.wants_keyboard_input()
            && !self.confirm_unsaved
            && !self.render_dialog
            && self.crop_session.is_none()
        {
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
            .default_height(410.)
            .min_height(330.)
            .max_height((ctx.screen_rect().height() - 240.).max(330.))
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
        self.asset_cancel.store(true, Ordering::Relaxed);
        if let Some((_, cancel, worker)) = self.preview_job.take() {
            cancel.store(true, Ordering::Relaxed);
            let _ = worker.join();
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

#[cfg(test)]
mod tests {
    use super::*;
    fn dispatch(key: Key, modifiers: Modifiers) -> Vec<Action> {
        let ctx = egui::Context::default();
        let mut actions = vec![];
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                modifiers,
                ..Default::default()
            },
            |ctx| {
                for b in bindings() {
                    if ctx.input_mut(|i| i.consume_shortcut(&b.shortcut)) {
                        actions.push(b.action);
                    }
                }
            },
        );
        actions
    }
    #[test]
    fn specific_shortcuts_win() {
        assert_eq!(
            dispatch(
                Key::Delete,
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                }
            ),
            [Action::Ripple]
        );
        assert_eq!(
            dispatch(
                Key::S,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                }
            ),
            [Action::SaveAs]
        );
        assert_eq!(
            dispatch(
                Key::Z,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                }
            ),
            [Action::Redo]
        );
    }
    #[test]
    fn ordinary_shortcuts_remain_distinct() {
        assert_eq!(dispatch(Key::Delete, Modifiers::NONE), [Action::Delete]);
        assert_eq!(dispatch(Key::S, Modifiers::NONE), [Action::Split]);
        assert_eq!(dispatch(Key::S, Modifiers::COMMAND), [Action::Save]);
        assert_eq!(
            dispatch(
                Key::ArrowDown,
                Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                }
            ),
            [Action::MediaNext]
        );
    }
}
