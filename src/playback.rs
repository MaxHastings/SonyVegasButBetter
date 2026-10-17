//! The only native media FFI boundary. libmpv's stable C API owns decoding,
//! timestamp seeking, synchronized audio output and its worker threads.
use crate::{media::Runtime, model::*, render::PreviewPlan};
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[repr(C)]
struct Param {
    kind: i32,
    data: *mut c_void,
}
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
    render_create: unsafe extern "C" fn(*mut Handle, Handle, *mut Param) -> i32,
    render_update: unsafe extern "C" fn(Handle) -> u64,
    render: unsafe extern "C" fn(Handle, *mut Param) -> i32,
    render_free: unsafe extern "C" fn(Handle),
    free_node: unsafe extern "C" fn(*mut Node),
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
                render_create: *library.get(b"mpv_render_context_create\0")?,
                render_update: *library.get(b"mpv_render_context_update\0")?,
                render: *library.get(b"mpv_render_context_render\0")?,
                render_free: *library.get(b"mpv_render_context_free\0")?,
                free_node: *library.get(b"mpv_free_node_contents\0")?,
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
    pub position: Tick,
    pub paused: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub peak_db: [f64; 2],
    pub frames: u64,
}
pub struct Frame {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub sequence: u64,
}
enum Control {
    Command(Vec<String>),
    Load(PreviewPlan, Tick, bool),
    Stop,
}
pub struct Player {
    tx: mpsc::Sender<Control>,
    pub state: Arc<Mutex<Snapshot>>,
    pub frame: Arc<Mutex<Option<Frame>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Player {
    pub fn new(rt: &Runtime) -> Result<Self> {
        Self::with_options(rt, vec![])
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
        let api = Arc::new(Api::load(rt)?);
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(Snapshot {
            paused: true,
            peak_db: [-120.; 2],
            ..Default::default()
        }));
        let frame = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let st = state.clone();
        let fr = frame.clone();
        let stopping = stop.clone();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
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
                    ("vo", "libmpv"),
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
            let render_api = api.clone();
            let render_stop = stopping.clone();
            let render_state = st.clone();
            let handle_bits = h as usize;
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let renderer = thread::spawn(move || {
                let h = handle_bits as Handle;
                let mut ctx = ptr::null_mut();
                let mut params = [
                    Param {
                        kind: 1,
                        data: c"sw".as_ptr() as *mut c_void,
                    },
                    Param {
                        kind: 0,
                        data: ptr::null_mut(),
                    },
                ];
                let r = unsafe { (render_api.render_create)(&mut ctx, h, params.as_mut_ptr()) };
                let _ = ready_tx.send(r);
                if r < 0 {
                    return;
                }
                // u32 storage guarantees the pixel alignment required by rgb0.
                let (w, height) = (960usize, 540usize);
                let mut pixels = vec![0u32; w * height];
                let mut size = [w as i32, height as i32];
                let mut stride = w * 4;
                let mut block = 0i32;
                let mut seq = 0;
                while !render_stop.load(Ordering::Relaxed) {
                    if unsafe { (render_api.render_update)(ctx) } & 1 != 0 {
                        let mut params = [
                            Param {
                                kind: 17,
                                data: size.as_mut_ptr().cast(),
                            },
                            Param {
                                kind: 18,
                                data: c"rgb0".as_ptr() as *mut c_void,
                            },
                            Param {
                                kind: 19,
                                data: (&mut stride as *mut usize).cast(),
                            },
                            Param {
                                kind: 20,
                                data: pixels.as_mut_ptr().cast(),
                            },
                            Param {
                                kind: 12,
                                data: (&mut block as *mut i32).cast(),
                            },
                            Param {
                                kind: 0,
                                data: ptr::null_mut(),
                            },
                        ];
                        let r = unsafe { (render_api.render)(ctx, params.as_mut_ptr()) };
                        if r >= 0 {
                            seq += 1;
                            let mut bytes = Vec::with_capacity(w * height * 4);
                            for pixel in &pixels {
                                let mut p = pixel.to_ne_bytes();
                                p[3] = 255;
                                bytes.extend_from_slice(&p);
                            }
                            if let Ok(mut f) = fr.try_lock() {
                                *f = Some(Frame {
                                    pixels: bytes,
                                    width: w,
                                    height,
                                    sequence: seq,
                                });
                            }
                            if let Ok(mut s) = render_state.try_lock() {
                                s.frames = seq;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(3));
                }
                unsafe {
                    (render_api.render_free)(ctx);
                }
            });
            let ready = ready_rx.recv().unwrap_or(-1);
            let _ = init_tx.send(if ready >= 0 {
                Ok(())
            } else {
                Err("Cannot initialize video renderer".into())
            });
            let mut pending_seek = None;
            let mut should_play = false;
            while !stopping.load(Ordering::Relaxed) {
                for cmd in rx.try_iter() {
                    let result = match cmd {
                        Control::Command(args) => api.send(h, &args),
                        Control::Stop => {
                            stopping.store(true, Ordering::Relaxed);
                            Ok(())
                        }
                        Control::Load(plan, at, play) => {
                            if let Ok(mut s) = st.lock() {
                                s.loaded = false;
                                s.error = None;
                            }
                            pending_seek = Some(at);
                            should_play = play;
                            let _ = api.send(h, &["stop".into()]);
                            let _ = api.send(h, &["set".into(), "pause".into(), "yes".into()]);
                            let _ =
                                api.send(h, &["set".into(), "lavfi-complex".into(), plan.graph]);
                            api.send(
                                h,
                                &[
                                    "loadfile".into(),
                                    plan.combined.to_string_lossy().into(),
                                    "replace".into(),
                                ],
                            )
                        }
                    };
                    if let Err(e) = result
                        && let Ok(mut s) = st.lock()
                    {
                        s.error = Some(e.to_string());
                    }
                }
                loop {
                    let e = unsafe { &*(api.event)(h, 0.) };
                    if e.kind == 0 {
                        break;
                    }
                    if e.kind == 8 {
                        // MPV_EVENT_FILE_LOADED
                        if let Some(at) = pending_seek.take() {
                            let _ =
                                api.send(h, &["seek".into(), decimal(at), "absolute+exact".into()]);
                        }
                        if should_play {
                            let _ = api.send(h, &["set".into(), "pause".into(), "no".into()]);
                        }
                        if let Ok(mut s) = st.lock() {
                            s.loaded = true;
                        }
                    }
                    if e.kind == 2 && !e.data.is_null() {
                        let log = unsafe { &*(e.data as *const Log) };
                        if log.log_level <= 20 && !log.text.is_null() {
                            let msg = unsafe { CStr::from_ptr(log.text) }.to_string_lossy();
                            if let Ok(mut s) = st.lock() {
                                s.error = Some(msg.trim().into());
                            }
                        }
                    }
                    if e.error < 0
                        && let Ok(mut s) = st.lock()
                    {
                        s.error = Some(format!("Playback error {} (event {})", e.error, e.kind));
                    }
                }
                let pos = api.double(h, c"time-pos");
                let peaks = api.peaks(h);
                let mut paused = 1i32;
                unsafe {
                    (api.get)(h, c"pause".as_ptr(), 3, (&mut paused as *mut i32).cast());
                }
                if let Ok(mut s) = st.lock() {
                    if let Some(pos) = pos {
                        s.position = ticks(pos);
                    }
                    s.paused = paused != 0;
                    s.peak_db = if paused == 0 { peaks } else { [-120.; 2] };
                }
                thread::sleep(Duration::from_millis(8));
            }
            // Keep the command core alive until the render context has been freed.
            let _ = renderer.join();
            unsafe {
                (api.destroy)(h);
            }
        });
        let result = init_rx
            .recv_timeout(Duration::from_secs(15))
            .context("Video initialization timed out")?;
        if let Err(e) = result {
            stop.store(true, Ordering::Relaxed);
            let _ = worker.join();
            anyhow::bail!(e);
        }
        Ok(Self {
            tx,
            state,
            frame,
            stop,
            worker: Some(worker),
        })
    }
    pub fn load(&self, plan: PreviewPlan, at: Tick, play: bool) {
        let _ = self.tx.send(Control::Load(plan, at, play));
    }
    pub fn send(&self, args: &[&str]) {
        let _ = self.tx.send(Control::Command(
            args.iter().map(|s| s.to_string()).collect(),
        ));
    }
    pub fn seek(&self, at: Tick) {
        self.send(&["seek", &decimal(at), "absolute+exact"]);
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
