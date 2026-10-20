//! libmpv client/stream ABI boundary. Native FFmpeg workers supply video and
//! seekable float audio; libmpv owns buffering, the device clock and output.
use crate::{media::Runtime, model::*, render::PreviewPlan};
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::ptr;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[repr(C)]
struct Event {
    kind: i32,
    error: i32,
    reply: u64,
    data: *mut c_void,
}
#[repr(C)]
struct Log {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: i32,
}
#[repr(C)]
union NodeValue {
    string: *mut c_char,
    list: *mut NodeList,
    number: f64,
    integer: i64,
}
#[repr(C)]
struct Node {
    value: NodeValue,
    format: i32,
}
#[repr(C)]
struct NodeList {
    count: i32,
    values: *mut Node,
    keys: *mut *mut c_char,
}
type Handle = *mut c_void;
struct Api {
    _library: Library,
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> i32,
    option: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> i32,
    command: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> i32,
    get: unsafe extern "C" fn(Handle, *const c_char, i32, *mut c_void) -> i32,
    event: unsafe extern "C" fn(Handle, f64) -> *const Event,
    log: unsafe extern "C" fn(Handle, *const c_char) -> i32,
    destroy: unsafe extern "C" fn(Handle),
    free_node: unsafe extern "C" fn(*mut Node),
    add_stream: unsafe extern "C" fn(
        Handle,
        *const c_char,
        *mut c_void,
        extern "C" fn(*mut c_void, *mut c_char, *mut crate::audio::StreamInfo) -> i32,
    ) -> i32,
}
impl Api {
    fn load(rt: &Runtime) -> Result<Self> {
        // SAFETY: All signatures match pinned client.h/render.h. Library stays loaded
        // through every call and is released only after both workers have joined.
        unsafe {
            let library = Library::new(&rt.mpv).context("Cannot load libmpv")?;
            Ok(Self {
                create: *library.get(b"mpv_create\0")?,
                initialize: *library.get(b"mpv_initialize\0")?,
                option: *library.get(b"mpv_set_option_string\0")?,
                command: *library.get(b"mpv_command_async\0")?,
                get: *library.get(b"mpv_get_property\0")?,
                event: *library.get(b"mpv_wait_event\0")?,
                log: *library.get(b"mpv_request_log_messages\0")?,
                destroy: *library.get(b"mpv_terminate_destroy\0")?,
                free_node: *library.get(b"mpv_free_node_contents\0")?,
                add_stream: *library.get(b"mpv_stream_cb_add_ro\0")?,
                _library: library,
            })
        }
    }
    fn send(&self, h: Handle, args: &[String]) -> Result<()> {
        let strings = args
            .iter()
            .map(|s| CString::new(s.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: mpv_command_async copies all strings before returning.
        ensure!(
            unsafe { (self.command)(h, 0, pointers.as_ptr()) } >= 0,
            "Playback command rejected: {}",
            args.join(" ")
        );
        Ok(())
    }
    fn double(&self, h: Handle, name: &CStr) -> Option<f64> {
        let mut n = 0.;
        let r = unsafe { (self.get)(h, name.as_ptr(), 5, (&mut n as *mut f64).cast()) };
        if r >= 0 { Some(n) } else { None }
    }
    fn peaks(&self, h: Handle) -> [f64; 2] {
        let mut node = Node {
            value: NodeValue { integer: 0 },
            format: 0,
        };
        let mut peaks = [-120.; 2];
        // SAFETY: libmpv owns the returned node tree until free_node. Inspect only
        // tagged map/string variants, copy numbers, then free exactly once.
        unsafe {
            if (self.get)(
                h,
                c"af-metadata/meter".as_ptr(),
                6,
                (&mut node as *mut Node).cast(),
            ) >= 0
            {
                if node.format == 8 && !node.value.list.is_null() {
                    let list = &*node.value.list;
                    if list.count >= 0
                        && list.count <= 1024
                        && !list.values.is_null()
                        && !list.keys.is_null()
                    {
                        for i in 0..list.count as usize {
                            let key = *list.keys.add(i);
                            let v = &*list.values.add(i);
                            if !key.is_null() && v.format == 1 && !v.value.string.is_null() {
                                let key = CStr::from_ptr(key).to_bytes();
                                let channel = if key == b"lavfi.astats.1.Peak_level" {
                                    Some(0)
                                } else if key == b"lavfi.astats.2.Peak_level" {
                                    Some(1)
                                } else {
                                    None
                                };
                                if let Some(channel) = channel {
                                    peaks[channel] = CStr::from_ptr(v.value.string)
                                        .to_string_lossy()
                                        .parse::<f64>()
                                        .unwrap_or(-120.)
                                        .clamp(-120., 12.);
                                }
                            }
                        }
                    }
                }
                (self.free_node)(&mut node);
            }
        }
        peaks
    }
}
#[derive(Clone, Default)]
pub struct Snapshot {
    requested_at: Option<Instant>,
    pub position: Tick,
    pub paused: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub peak_db: [f64; 2],
    pub frames: u64,
    pub request_id: u64,
    pub video_position: Tick,
    pub first_frame_ms: Option<f64>,
    pub presented_ms: Option<f64>,
    pub audio_ready_ms: Option<f64>,
    pub cache_hits: u64,
    pub cache_bytes: usize,
    pub decoder_opens: u64,
}
impl Snapshot {
    pub fn waiting_for_frame(&self) -> bool {
        self.request_id > 0
            && self.first_frame_ms.is_none()
            && self
                .requested_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
    }
}
#[derive(Clone)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub sequence: u64,
    pub request_id: u64,
    pub position: Tick,
    pub source_position: Tick,
    pub(crate) requested_at: Instant,
}
#[derive(Clone, Copy)]
pub(crate) struct PreviewRequest {
    pub id: u64,
    pub at: Tick,
    pub started: Instant,
}
enum Control {
    Command(Vec<String>),
    Seek(PreviewRequest),
    Source(Media, PreviewRequest, Option<CropRect>),
    Load(PreviewPlan, PreviewRequest, bool),
    Stop,
}
pub struct Player {
    latest: Arc<AtomicU64>,
    visual: crate::video::VideoSender,
    visual_project: Mutex<Option<Arc<Project>>>,
    log_path: Option<std::path::PathBuf>,
    tx: mpsc::Sender<Control>,
    pub state: Arc<Mutex<Snapshot>>,
    pub frame: Arc<Mutex<Option<Frame>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
fn request_video(
    video: &crate::video::VideoSender,
    p: &Project,
    desired: Tick,
    request: PreviewRequest,
) {
    let at = desired.clamp(0, (p.duration() - p.fps.frame(1)).max(0));
    let index = (at as i128 * p.fps.num as i128 / (HZ as i128 * p.fps.den as i128)) as i64;
    let at = p.fps.frame(index);
    if let Some(c) = p
        .clips
        .iter()
        .find(|c| c.lane == 0 && at >= c.start && at < c.end())
    {
        let local = at - c.start;
        let opacity = if c.mute || p.tracks[0].mute {
            0.
        } else {
            (if c.fade_in > 0 {
                seconds(local) / seconds(c.fade_in)
            } else {
                1.
            })
            .min(1.)
                * (if c.fade_out > 0 {
                    seconds(c.duration - local) / seconds(c.fade_out)
                } else {
                    1.
                })
                .min(1.)
        };
        video.request_cropped(
            p.media(&c.media).ok(),
            c.source_in + local,
            opacity,
            c.crop_at(c.source_in + local),
            request,
            at,
        );
    } else {
        video.request_cropped(None, 0, 1., None, request, at);
    }
}
impl Player {
    pub fn new(rt: &Runtime) -> Result<Self> {
        Self::with_options(rt, vec![])
    }
    /// A real-time null audio device for deterministic headless adapter tests.
    /// Desktop playback always uses new(), with the actual system audio device.
    pub fn headless(rt: &Runtime) -> Result<Self> {
        Self::with_options(rt, vec![("ao".into(), "null".into())])
    }
    pub fn diagnostic(rt: &Runtime, log: &std::path::Path) -> Result<Self> {
        Self::with_options(rt, vec![("log-file".into(), log.to_string_lossy().into())])
    }
    /// Integration tests capture the actual preview audio adapter independently
    /// of the offline exporter, to detect divergence in shared mix semantics.
    pub fn capture(rt: &Runtime, path: &std::path::Path) -> Result<Self> {
        Self::with_options(
            rt,
            vec![
                ("ao".into(), "pcm".into()),
                ("ao-pcm-file".into(), path.to_string_lossy().into()),
                ("audio-samplerate".into(), "48000".into()),
                ("audio-format".into(), "float".into()),
                (
                    "log-file".into(),
                    path.with_extension("log").to_string_lossy().into(),
                ),
            ],
        )
    }
    fn with_options(rt: &Runtime, extra: Vec<(String, String)>) -> Result<Self> {
        let mut log = rt
            .cache
            .parent()
            .and_then(|dir| crate::playback_log::PlaybackLog::open(&dir.join("logs")).ok());
        let log_path = log.as_ref().map(|log| log.path.clone());
        let api = Arc::new(Api::load(rt)?);
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(Snapshot {
            paused: true,
            peak_db: [-120.; 2],
            ..Default::default()
        }));
        let frame = Arc::new(Mutex::new(None));
        let latest = Arc::new(AtomicU64::new(0));
        let worker_latest = latest.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let st = state.clone();
        let fr = frame.clone();
        let stopping = stop.clone();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        let runtime = rt.clone();
        let worker = thread::spawn(move || {
            // Handle is used by one command thread; render context has a separate
            // owner thread. libmpv explicitly permits this concurrent separation.
            let h = unsafe { (api.create)() };
            if h.is_null() {
                let _ = init_tx.send(Err("mpv_create failed".to_string()));
                return;
            }
            let init = (|| -> Result<()> {
                for (key, value) in [
                    ("vo", "null"),
                    ("config", "no"),
                    ("terminal", "no"),
                    ("input-default-bindings", "no"),
                    ("osc", "no"),
                    ("osd-level", "0"),
                    ("keep-open", "yes"),
                    ("pause", "yes"),
                    ("audio-display", "no"),
                    ("ytdl", "no"),
                    ("load-scripts", "no"),
                    ("audio-file-auto", "no"),
                    ("sub-auto", "no"),
                    ("vd-lavc-threads", "4"),
                    ("demuxer-readahead-secs", "1"),
                    ("demuxer-max-bytes", "2MiB"),
                    ("demuxer-max-back-bytes", "1MiB"),
                    ("hr-seek-framedrop", "no"),
                    (
                        "af",
                        "@meter:astats=metadata=1:reset=1:measure_overall=none",
                    ),
                ] {
                    let k = CString::new(key)?;
                    let v = CString::new(value)?;
                    ensure!(
                        unsafe { (api.option)(h, k.as_ptr(), v.as_ptr()) } >= 0,
                        "mpv option failed: {key}"
                    );
                }
                for (key, value) in extra {
                    let k = CString::new(key)?;
                    let v = CString::new(value)?;
                    ensure!(
                        unsafe { (api.option)(h, k.as_ptr(), v.as_ptr()) } >= 0,
                        "mpv capture option failed"
                    );
                }
                ensure!(
                    unsafe { (api.initialize)(h) } >= 0,
                    "mpv initialization failed"
                );
                unsafe {
                    (api.log)(h, c"warn".as_ptr());
                }
                Ok(())
            })();
            if let Err(e) = init {
                let _ = init_tx.send(Err(e.to_string()));
                unsafe {
                    (api.destroy)(h);
                }
                return;
            }
            let mut factory = Box::new(crate::audio::Factory {
                rt: runtime.clone(),
                project: Mutex::new(Project::default()),
                error: Arc::new(Mutex::new(None)),
            });
            let registration = unsafe {
                (api.add_stream)(
                    h,
                    c"svbb".as_ptr(),
                    (&mut *factory as *mut crate::audio::Factory).cast(),
                    crate::audio::open,
                )
            };
            if registration < 0 {
                let _ = init_tx.send(Err("Cannot register native audio stream".into()));
                unsafe {
                    (api.destroy)(h);
                }
                return;
            }
            let video = crate::video::VideoWorker::new(
                runtime,
                fr.clone(),
                worker_latest.clone(),
                st.clone(),
            );
            let _ = init_tx.send(Ok(video.controller.clone()));
            let mut project: Option<Project> = None;
            let mut source_override: Option<(Media, Tick, Option<CropRect>)> = None;
            let mut desired = 0;
            let mut anchor: Option<(Tick, Instant)> = None;
            let mut last_audio = None;
            let mut pending_seek = None;
            let mut should_play = false;
            let mut awaiting_seek: Option<bool> = None;
            let mut audio_loading = false;
            let mut audio_started = false;
            let mut audio_reload_pending = false;
            let mut active = PreviewRequest {
                id: 0,
                at: 0,
                started: Instant::now(),
            };
            let mut logged_frame = 0;
            let mut logged_audio = 0;
            let mut logged_presented = 0;
            while !stopping.load(Ordering::Relaxed) {
                let mut seek = None;
                let controls: Vec<_> = rx.try_iter().collect();
                let last_load = controls
                    .iter()
                    .rposition(|c| matches!(c, Control::Load(..)));
                for (index, cmd) in controls.into_iter().enumerate() {
                    if last_load.is_some_and(|last| index < last) {
                        continue;
                    }
                    let result = match cmd {
                        Control::Seek(request) => {
                            if request.id != worker_latest.load(Ordering::Relaxed) {
                                continue;
                            }
                            active = request;
                            let at = request
                                .at
                                .clamp(0, project.as_ref().map_or(0, Project::duration));
                            if let Some(log) = &mut log {
                                log.write(format_args!("Seek to {}s", seconds(at)));
                            }
                            anchor = None;
                            last_audio = None;
                            source_override = None;
                            desired = at;
                            seek = Some(at);
                            if let Some(p) = &project {
                                request_video(&video.controller, p, desired, active);
                            }
                            Ok(())
                        }
                        Control::Source(media, request, crop) => {
                            if request.id != worker_latest.load(Ordering::Relaxed) {
                                continue;
                            }
                            active = request;
                            let at = request.at;
                            should_play = false;
                            video.controller.request_cropped(
                                Some(&media),
                                at,
                                1.,
                                crop,
                                active,
                                at,
                            );
                            source_override = Some((media, at, crop));
                            let _ = api.send(h, &["set".into(), "pause".into(), "yes".into()]);
                            Ok(())
                        }
                        Control::Command(args) => {
                            if args.first().is_some_and(|s| s == "stop") {
                                project = None;
                                source_override = None;
                                should_play = false;
                                desired = 0;
                                anchor = None;
                                last_audio = None;
                                awaiting_seek = None;
                                pending_seek = None;
                                audio_reload_pending = false;
                                seek = None;
                                // The client already invalidated this transport. A
                                // later source request may now be queued; do not
                                // label the stop frame with that newer identity.
                                active.id = 0;
                                video
                                    .controller
                                    .request_cropped(None, 0, 1., None, active, 0);
                                st.lock().unwrap().loaded = false;
                            }
                            if args.first().is_some_and(|s| s == "cycle")
                                && args.get(1).is_some_and(|s| s == "pause")
                            {
                                // Toggle intent, never the delayed device snapshot. A play
                                // request during reload/seek must survive until restart.
                                should_play = !should_play;
                            }
                            if args.first().is_some_and(|s| s == "set")
                                && args.get(1).is_some_and(|s| s == "pause")
                                && args.get(2).is_some_and(|s| s == "yes")
                            {
                                should_play = false;
                            }
                            if args.first().is_some_and(|s| s == "cycle")
                                && args.get(1).is_some_and(|s| s == "pause")
                                && (seek.is_some()
                                    || awaiting_seek.is_some()
                                    || pending_seek.is_some())
                            {
                                Ok(())
                            } else if args.first().is_some_and(|s| s == "cycle")
                                && args.get(1).is_some_and(|s| s == "pause")
                            {
                                api.send(
                                    h,
                                    &[
                                        "set".into(),
                                        "pause".into(),
                                        if should_play { "no" } else { "yes" }.into(),
                                    ],
                                )
                            } else {
                                api.send(h, &args)
                            }
                        }
                        Control::Stop => {
                            stopping.store(true, Ordering::Relaxed);
                            Ok(())
                        }
                        Control::Load(plan, request, play) => {
                            active = request;
                            let at = request.at.clamp(0, plan.project.duration());
                            // A seek queued for the previous edit must not overwrite
                            // this load's bounded position after the batch is drained.
                            seek = None;
                            if let Some(log) = &mut log {
                                log.write(format_args!(
                                    "Load {} events, duration {}s, position {}s, play={play}",
                                    plan.project.clips.len(),
                                    seconds(plan.project.duration()),
                                    seconds(at)
                                ));
                            }
                            awaiting_seek = None;
                            anchor = None;
                            last_audio = None;
                            if let Ok(mut s) = st.lock() {
                                s.loaded = false;
                                s.error = None;
                            }
                            project = Some(plan.project);
                            source_override = None;
                            desired = at;
                            request_video(
                                &video.controller,
                                project.as_ref().unwrap(),
                                desired,
                                active,
                            );
                            pending_seek = Some(at);
                            should_play = play;
                            audio_reload_pending = true;
                            Ok(())
                        }
                    };
                    if let Err(e) = result
                        && let Ok(mut s) = st.lock()
                    {
                        if let Some(log) = &mut log {
                            log.write(format_args!("Command failed: {e}"));
                        }
                        s.error = Some(e.to_string());
                    }
                }
                // Only one audio load may be in flight. New edits replace the
                // pending snapshot while video already follows the latest edit.
                // FILE_LOADED has no request ID, so serializing this handshake
                // prevents an older completion from seeking/resuming a new mix.
                if audio_reload_pending
                    && !audio_loading
                    && let Some(p) = &project
                {
                    *factory.project.lock().unwrap() = p.clone();
                    let filter = format!(
                        "{},@meter:astats=metadata=1:reset=1:measure_overall=none",
                        crate::render::master(p.peak_protection)
                    );
                    let _ = api.send(h, &["stop".into()]);
                    let _ = api.send(h, &["set".into(), "pause".into(), "yes".into()]);
                    let _ = api.send(h, &["set".into(), "lavfi-complex".into(), String::new()]);
                    let _ = api.send(h, &["set".into(), "af".into(), filter]);
                    match api.send(
                        h,
                        &[
                            "loadfile".into(),
                            "svbb://timeline.wav".into(),
                            "replace".into(),
                        ],
                    ) {
                        Ok(()) => {
                            audio_loading = true;
                            audio_started = false;
                        }
                        Err(e) => st.lock().unwrap().error = Some(e.to_string()),
                    }
                    audio_reload_pending = false;
                }
                if let Some(at) = seek {
                    if pending_seek.is_some() {
                        pending_seek = Some(at);
                    } else {
                        let _ = api.send(h, &["set".into(), "pause".into(), "yes".into()]);
                        awaiting_seek = Some(false);
                        let _ = api.send(h, &["seek".into(), decimal(at), "absolute+exact".into()]);
                    }
                }
                loop {
                    let e = unsafe { &*(api.event)(h, 0.) };
                    if e.kind == 0 {
                        break;
                    }
                    if e.kind == 6 && audio_loading {
                        audio_started = true;
                    }
                    if e.kind == 7 && audio_loading && audio_started {
                        audio_loading = false;
                    }
                    if e.kind == 8 {
                        // MPV_EVENT_FILE_LOADED
                        audio_loading = false;
                        if audio_reload_pending || project.is_none() {
                            continue;
                        }
                        let at = pending_seek.take().unwrap_or(0);
                        if at > 0 {
                            awaiting_seek = Some(false);
                            let _ =
                                api.send(h, &["seek".into(), decimal(at), "absolute+exact".into()]);
                        }
                        if should_play && at == 0 {
                            let _ = api.send(h, &["set".into(), "pause".into(), "no".into()]);
                        }
                        if let Ok(mut s) = st.lock() {
                            s.loaded = true;
                        }
                    }
                    if e.kind == 20 && awaiting_seek.is_some() {
                        awaiting_seek = Some(true);
                    }
                    if e.kind == 21 && awaiting_seek == Some(true) {
                        awaiting_seek = None;
                        if should_play {
                            let _ = api.send(h, &["set".into(), "pause".into(), "no".into()]);
                        }
                    }
                    if e.kind == 2 && !e.data.is_null() {
                        let entry = unsafe { &*(e.data as *const Log) };
                        if entry.log_level <= 30 && !entry.text.is_null() {
                            let msg = unsafe { CStr::from_ptr(entry.text) }.to_string_lossy();
                            if let Some(diagnostic) = &mut log {
                                diagnostic.write(format_args!("mpv: {}", msg.trim()));
                            }
                            if entry.log_level <= 20
                                && let Ok(mut s) = st.lock()
                            {
                                s.error = Some(msg.trim().into());
                            }
                        }
                    }
                    if e.error < 0
                        && let Ok(mut s) = st.lock()
                    {
                        if let Some(log) = &mut log {
                            log.write(format_args!(
                                "Playback error {} (event {})",
                                e.error, e.kind
                            ));
                        }
                        s.error = Some(format!("Playback error {} (event {})", e.error, e.kind));
                    }
                }
                let pos = api.double(h, c"time-pos");
                let peaks = api.peaks(h);
                let mut paused = 1i32;
                unsafe {
                    (api.get)(h, c"pause".as_ptr(), 3, (&mut paused as *mut i32).cast());
                }
                let mut eof = 0i32;
                unsafe {
                    (api.get)(h, c"eof-reached".as_ptr(), 3, (&mut eof as *mut i32).cast());
                }
                if eof != 0 && should_play && pending_seek.is_none() && awaiting_seek.is_none() {
                    if let Some(p) = &project {
                        desired = p.duration();
                    }
                    should_play = false;
                }
                if let Ok(mut s) = st.lock()
                    && active.id == worker_latest.load(Ordering::Relaxed)
                {
                    if eof == 0
                        && paused == 0
                        && pending_seek.is_none()
                        && awaiting_seek.is_none()
                        && let Some(pos) = pos
                    {
                        let audio = ticks(pos);
                        if last_audio != Some(audio) {
                            anchor = Some((audio, Instant::now()));
                            last_audio = Some(audio);
                        }
                        desired = anchor
                            .map(|(at, time)| at + ticks(time.elapsed().as_secs_f64()))
                            .unwrap_or(audio);
                    } else if paused != 0 {
                        anchor = None;
                        last_audio = None;
                    }
                    s.position = desired;
                    s.paused = paused != 0;
                    s.peak_db = if paused == 0 { peaks } else { [-120.; 2] };
                    if source_override.is_none()
                        && s.loaded
                        && pending_seek.is_none()
                        && awaiting_seek.is_none()
                    {
                        s.audio_ready_ms
                            .get_or_insert_with(|| active.started.elapsed().as_secs_f64() * 1000.);
                    }
                }
                // File I/O stays outside the state/output locks used by the UI
                // and video worker. Audio/UI readiness may arrive after the frame.
                let metrics = st.lock().unwrap().clone();
                if metrics.request_id != logged_frame
                    && let Some(ms) = metrics.first_frame_ms
                {
                    if let Some(log) = &mut log {
                        log.write(format_args!("Preview request {} first frame {:.1}ms; cache hits {}, bytes {}, decoder opens {}", metrics.request_id, ms, metrics.cache_hits, metrics.cache_bytes, metrics.decoder_opens));
                    }
                    logged_frame = metrics.request_id;
                }
                if metrics.request_id != logged_audio
                    && let Some(ms) = metrics.audio_ready_ms
                {
                    if let Some(log) = &mut log {
                        log.write(format_args!(
                            "Preview request {} audio ready {:.1}ms",
                            metrics.request_id, ms
                        ));
                    }
                    logged_audio = metrics.request_id;
                }
                if metrics.request_id != logged_presented
                    && let Some(ms) = metrics.presented_ms
                {
                    if let Some(log) = &mut log {
                        log.write(format_args!(
                            "Preview request {} UI texture ready {:.1}ms",
                            metrics.request_id, ms
                        ));
                    }
                    logged_presented = metrics.request_id;
                }
                if let Some((media, at, crop)) = &source_override {
                    video
                        .controller
                        .request_cropped(Some(media), *at, 1., *crop, active, *at);
                } else if let Some(p) = &project {
                    request_video(&video.controller, p, desired, active);
                }
                if let Some(error) = factory.error.lock().unwrap().take() {
                    if let Some(log) = &mut log {
                        log.write(format_args!("Audio: {error}"));
                    }
                    st.lock().unwrap().error = Some(error);
                }
                if let Some((id, error)) = video.error.lock().unwrap().take()
                    && id == worker_latest.load(Ordering::Relaxed)
                    && let Some(log) = &mut log
                {
                    log.write(format_args!("Video: {error}"));
                }
                if let Some(frame) = fr.lock().unwrap().as_ref() {
                    st.lock().unwrap().frames = frame.sequence;
                }
                thread::sleep(Duration::from_millis(8));
            }
            // Keep the command core alive until the render context has been freed.
            drop(video);
            unsafe {
                (api.destroy)(h);
            }
        });
        let result = init_rx
            .recv_timeout(Duration::from_secs(15))
            .context("Video initialization timed out")?;
        let visual = match result {
            Ok(visual) => visual,
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                let _ = worker.join();
                anyhow::bail!(e);
            }
        };
        Ok(Self {
            latest,
            visual,
            visual_project: Mutex::new(None),
            log_path,
            tx,
            state,
            frame,
            stop,
            worker: Some(worker),
        })
    }
    fn request(&self, at: Tick) -> PreviewRequest {
        // Serialize invalidation with publication: a completed old decode cannot
        // slip back into the output between clearing it and issuing a new seek.
        let mut output = self.frame.lock().unwrap();
        let request = PreviewRequest {
            id: self.latest.fetch_add(1, Ordering::Relaxed) + 1,
            at,
            started: Instant::now(),
        };
        *output = None;
        let mut state = self.state.lock().unwrap();
        state.request_id = request.id;
        state.position = at;
        state.paused = true;
        state.first_frame_ms = None;
        state.presented_ms = None;
        state.audio_ready_ms = None;
        state.error = None;
        state.requested_at = Some(request.started);
        request
    }
    pub fn load(&self, plan: PreviewPlan, at: Tick, play: bool) -> u64 {
        let request = self.request(at.clamp(0, plan.project.duration()));
        self.state.lock().unwrap().loaded = false;
        request_video(&self.visual, &plan.project, request.at, request);
        *self.visual_project.lock().unwrap() = Some(Arc::new(plan.project.clone()));
        let _ = self.tx.send(Control::Load(plan, request, play));
        request.id
    }
    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.log_path.as_deref()
    }
    pub fn send(&self, args: &[&str]) {
        if args.first() == Some(&"stop") {
            self.request(0);
            let mut state = self.state.lock().unwrap();
            state.requested_at = None;
            state.loaded = false;
            drop(state);
            *self.visual_project.lock().unwrap() = None;
        }
        let _ = self.tx.send(Control::Command(
            args.iter().map(|s| s.to_string()).collect(),
        ));
    }
    pub fn seek(&self, at: Tick) -> u64 {
        let request = self.request(at);
        if let Some(project) = self.visual_project.lock().unwrap().as_ref() {
            request_video(&self.visual, project, at, request);
        }
        let _ = self.tx.send(Control::Seek(request));
        request.id
    }
    pub fn show_source(&self, media: Media, at: Tick) {
        self.show_source_cropped(media, at, None);
    }
    pub fn show_source_cropped(&self, media: Media, at: Tick, crop: Option<CropRect>) {
        let request = self.request(at);
        self.visual
            .request_cropped(Some(&media), at, 1., crop, request, at);
        let _ = self.tx.send(Control::Source(media, request, crop));
    }
    pub fn frame_presented(&self, frame: &Frame) {
        let mut state = self.state.lock().unwrap();
        if state.request_id == frame.request_id {
            state
                .presented_ms
                .get_or_insert_with(|| frame.requested_at.elapsed().as_secs_f64() * 1000.);
        }
    }
    pub fn pause(&self) {
        self.send(&["set", "pause", "yes"]);
    }
    pub fn toggle(&self) {
        self.send(&["cycle", "pause"]);
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn take_error(&self) -> Option<String> {
        self.state.lock().ok().and_then(|mut s| s.error.take())
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Control::Stop);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
