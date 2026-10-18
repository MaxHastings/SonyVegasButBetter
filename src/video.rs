//! Persistent, bounded-memory video decoding. FFmpeg structures stay in video.cpp.
use crate::{media::Runtime, model::*, playback::Frame};
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{
    ffi::{CString, c_char, c_void},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};
unsafe extern "C" {
    fn svbb_video_open(
        api: *const *mut c_void,
        path: *const c_char,
        stream: i32,
        origin: f64,
    ) -> *mut c_void;
    fn svbb_video_close(decoder: *mut c_void);
    fn svbb_video_frame(
        decoder: *mut c_void,
        t: f64,
        pixels: *mut u8,
        w: i32,
        h: i32,
        opaque: *mut c_void,
        cancel: extern "C" fn(*mut c_void) -> i32,
        actual: *mut f64,
    ) -> i32;
}
pub(crate) struct Api {
    pub(crate) functions: Vec<*mut c_void>,
    _libraries: Vec<Library>,
}
impl Api {
    pub(crate) fn load(rt: &Runtime) -> Result<Self> {
        let names = [
            ("avutil", 61),
            ("swresample", 7),
            ("avcodec", 63),
            ("avformat", 63),
            ("swscale", 10),
        ];
        let root = rt
            .ffmpeg
            .parent()
            .context("Runtime directory missing")?
            .join("av");
        let mut libs = vec![];
        for (name, version) in names {
            let file = if cfg!(windows) {
                root.join(format!("{name}-{version}.dll"))
            } else if cfg!(target_os = "macos") {
                format!("lib{name}.{version}.dylib").into()
            } else {
                format!("lib{name}.so.{version}").into()
            };
            // SAFETY: pinned FFmpeg ABI majors; libraries outlive every decoder.
            libs.push(
                unsafe { Library::new(&file) }
                    .with_context(|| format!("Cannot load {}", file.display()))?,
            );
        }
        let symbols = [
            "avformat_open_input",
            "avformat_find_stream_info",
            "avformat_close_input",
            "avcodec_find_decoder",
            "avcodec_alloc_context3",
            "avcodec_parameters_to_context",
            "avcodec_open2",
            "avcodec_free_context",
            "av_packet_alloc",
            "av_packet_free",
            "av_packet_unref",
            "av_read_frame",
            "avcodec_send_packet",
            "avcodec_receive_frame",
            "av_frame_alloc",
            "av_frame_free",
            "av_frame_unref",
            "av_frame_move_ref",
            "av_seek_frame",
            "avcodec_flush_buffers",
            "sws_getCachedContext",
            "sws_scale",
            "sws_freeContext",
            "swr_alloc_set_opts2",
            "swr_init",
            "swr_convert",
            "swr_get_delay",
            "swr_free",
            "av_channel_layout_default",
        ];
        let mut functions = vec![];
        for name in symbols {
            let symbol = CString::new(name)?;
            let pointer = libs
                .iter()
                .find_map(|l| unsafe {
                    l.get::<*mut c_void>(symbol.as_bytes_with_nul())
                        .ok()
                        .map(|s| *s)
                })
                .with_context(|| format!("Missing FFmpeg symbol {name}"))?;
            functions.push(pointer);
        }
        Ok(Self {
            functions,
            _libraries: libs,
        })
    }
}
pub struct Decoder {
    handle: *mut c_void,
    _api: Api,
}
impl Decoder {
    pub fn open(rt: &Runtime, media: &Media) -> Result<Self> {
        let api = Api::load(rt)?;
        let stream = media.video.as_ref().context("Source has no video")?;
        let origin = stream.start_pts as f64 * stream.timebase.num as f64
            / stream.timebase.den as f64
            - seconds(stream.start);
        let path = CString::new(media.path.to_string_lossy().as_bytes())?;
        // SAFETY: API table order/signatures match the compiled C++ shim. It copies
        // the table; path is consumed during open. The handle never crosses threads.
        let handle = unsafe {
            svbb_video_open(
                api.functions.as_ptr(),
                path.as_ptr(),
                stream.index as i32,
                origin,
            )
        };
        ensure!(
            !handle.is_null(),
            "Cannot open video decoder: {}",
            media.name
        );
        Ok(Self { handle, _api: api })
    }
    pub fn frame(&mut self, at: Tick) -> Result<Frame> {
        self.read(at, 0, &AtomicU64::new(0), &AtomicBool::new(false))
    }
    fn read(
        &mut self,
        at: Tick,
        sequence: u64,
        generation: &AtomicU64,
        stop: &AtomicBool,
    ) -> Result<Frame> {
        struct Cancel<'a> {
            generation: &'a AtomicU64,
            stop: &'a AtomicBool,
            expected: u64,
        }
        extern "C" fn cancel(p: *mut c_void) -> i32 {
            // SAFETY: read() keeps Cancel alive through the synchronous C call.
            let c = unsafe { &*(p as *const Cancel<'_>) };
            i32::from(
                c.stop.load(Ordering::Relaxed)
                    || c.generation.load(Ordering::Relaxed) != c.expected,
            )
        }
        let mut cancellation = Cancel {
            generation,
            stop,
            expected: sequence,
        };
        let mut pixels = vec![0; 960 * 540 * 4];
        let mut actual = 0.;
        // SAFETY: decoder is exclusive, output is 960*540*4 writable bytes;
        // cancellation and actual stay alive until this call returns.
        let r = unsafe {
            svbb_video_frame(
                self.handle,
                seconds(at),
                pixels.as_mut_ptr(),
                960,
                540,
                (&mut cancellation as *mut Cancel<'_>).cast(),
                cancel,
                &mut actual,
            )
        };
        ensure!(r >= 0, "Video decoding failed ({r})");
        Ok(Frame {
            pixels,
            width: 960,
            height: 540,
            sequence,
        })
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { svbb_video_close(self.handle) }
    }
}
#[derive(Clone)]
struct Request {
    media: Option<Media>,
    at: Tick,
    opacity: f64,
    sequence: u64,
}
type Mailbox = Arc<(Mutex<Option<Request>>, Condvar)>;
pub(crate) struct VideoWorker {
    mailbox: Mailbox,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    pub error: Arc<Mutex<Option<String>>>,
    last: Option<(String, Tick, u64)>,
}
impl VideoWorker {
    pub fn new(rt: Runtime, output: Arc<Mutex<Option<Frame>>>) -> Self {
        let mailbox: Mailbox = Arc::new((Mutex::new(None), Condvar::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (box_, gen_, stop_, err) = (
            mailbox.clone(),
            generation.clone(),
            stop.clone(),
            error.clone(),
        );
        let thread = thread::spawn(move || {
            let mut decoder: Option<(String, Decoder)> = None;
            let mut frames = 0;
            loop {
                let (lock, wake) = &*box_;
                let mut pending = lock.lock().unwrap();
                while pending.is_none() && !stop_.load(Ordering::Relaxed) {
                    pending = wake.wait(pending).unwrap();
                }
                if stop_.load(Ordering::Relaxed) {
                    break;
                }
                let request = pending.take().unwrap();
                drop(pending);
                let result = (|| -> Result<Frame> {
                    if let Some(m) = &request.media
                        && m.video.as_ref().is_some_and(|v| request.at >= v.start)
                    {
                        if decoder
                            .as_ref()
                            .is_none_or(|(path, _)| path != &m.path.to_string_lossy())
                        {
                            decoder =
                                Some((m.path.to_string_lossy().into(), Decoder::open(&rt, m)?));
                        }
                        let mut frame = decoder.as_mut().unwrap().1.read(
                            request.at,
                            request.sequence,
                            &gen_,
                            &stop_,
                        )?;
                        if request.opacity < 1. {
                            for pixel in frame.pixels.chunks_exact_mut(4) {
                                for v in &mut pixel[..3] {
                                    *v = (*v as f64 * request.opacity.clamp(0., 1.)).round() as u8;
                                }
                            }
                        }
                        Ok(frame)
                    } else {
                        let mut pixels = vec![0; 960 * 540 * 4];
                        for p in pixels.chunks_exact_mut(4) {
                            p[3] = 255;
                        }
                        Ok(Frame {
                            pixels,
                            width: 960,
                            height: 540,
                            sequence: request.sequence,
                        })
                    }
                })();
                if gen_.load(Ordering::Relaxed) == request.sequence {
                    match result {
                        Ok(mut frame) => {
                            frames += 1;
                            frame.sequence = frames;
                            *output.lock().unwrap() = Some(frame);
                        }
                        Err(e) => *err.lock().unwrap() = Some(e.to_string()),
                    }
                }
            }
        });
        Self {
            mailbox,
            generation,
            stop,
            thread: Some(thread),
            error,
            last: None,
        }
    }
    pub fn request(&mut self, media: Option<&Media>, at: Tick, opacity: f64) {
        let key = (
            media
                .map(|m| m.path.to_string_lossy().into())
                .unwrap_or_default(),
            at,
            opacity.to_bits(),
        );
        if self.last.as_ref() == Some(&key) {
            return;
        }
        // Normal forward playback must finish its current frame even when a newer
        // frame is queued. Only scrubs/source changes invalidate in-flight decoding.
        let discontinuity = self
            .last
            .as_ref()
            .is_none_or(|last| last.0 != key.0 || at < last.1 || at - last.1 > HZ / 4);
        self.last = Some(key);
        let sequence = if discontinuity {
            self.generation.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.generation.load(Ordering::Relaxed)
        };
        let (lock, wake) = &*self.mailbox;
        *lock.lock().unwrap() = Some(Request {
            media: media.cloned(),
            at,
            opacity,
            sequence,
        });
        wake.notify_one();
    }
}
impl Drop for VideoWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.mailbox.1.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
