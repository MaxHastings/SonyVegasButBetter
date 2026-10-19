//! Persistent, bounded-memory video decoding. FFmpeg structures stay in video.cpp.
use crate::{
    media::Runtime,
    model::*,
    playback::{Frame, PreviewRequest, Snapshot},
};
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{
    collections::VecDeque,
    ffi::{CString, c_char, c_void},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Instant,
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
        crop_x: i32,
        crop_y: i32,
        crop_w: i32,
        crop_h: i32,
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
            "av_frame_apply_cropping",
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
        self.cropped_frame(at, None)
    }
    pub fn cropped_frame(&mut self, at: Tick, crop: Option<CropRect>) -> Result<Frame> {
        self.read(
            at,
            crop,
            0,
            &AtomicU64::new(0),
            &AtomicBool::new(false),
            None,
        )
    }
    fn read(
        &mut self,
        at: Tick,
        crop: Option<CropRect>,
        sequence: u64,
        generation: &AtomicU64,
        stop: &AtomicBool,
        interaction: Option<(&AtomicU64, u64)>,
    ) -> Result<Frame> {
        struct Cancel<'a> {
            generation: &'a AtomicU64,
            stop: &'a AtomicBool,
            expected: u64,
            interaction: Option<(&'a AtomicU64, u64)>,
        }
        extern "C" fn cancel(p: *mut c_void) -> i32 {
            // SAFETY: read() keeps Cancel alive through the synchronous C call.
            let c = unsafe { &*(p as *const Cancel<'_>) };
            i32::from(
                c.stop.load(Ordering::Relaxed)
                    || c.generation.load(Ordering::Relaxed) != c.expected
                    || c.interaction.is_some_and(|(latest, expected)| {
                        latest.load(Ordering::Relaxed) != expected
                    }),
            )
        }
        let mut cancellation = Cancel {
            generation,
            stop,
            expected: sequence,
            interaction,
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
                crop.map_or(0, |c| c.x as i32),
                crop.map_or(0, |c| c.y as i32),
                crop.map_or(0, |c| c.width as i32),
                crop.map_or(0, |c| c.height as i32),
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
            request_id: 0,
            position: at,
            source_position: ticks(actual),
            requested_at: Instant::now(),
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
    crop: Option<CropRect>,
    sequence: u64,
    interaction: PreviewRequest,
    position: Tick,
}
#[derive(Clone, PartialEq)]
struct FrameKey {
    source: String,
    at: Tick,
    crop: Option<CropRect>,
}
#[derive(PartialEq)]
struct RenderKey {
    frame: FrameKey,
    opacity: u64,
    interaction: u64,
    position: Tick,
}
const CACHE_BYTES: usize = 32 * 1024 * 1024;
#[derive(Default)]
struct FrameCache {
    entries: VecDeque<(FrameKey, Frame)>,
    bytes: usize,
}
impl FrameCache {
    fn get(&mut self, key: &FrameKey) -> Option<Frame> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(index).unwrap();
        let frame = entry.1.clone();
        self.entries.push_back(entry);
        Some(frame)
    }
    fn insert(&mut self, key: FrameKey, frame: &Frame) {
        if frame.pixels.len() > CACHE_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|(k, _)| k == &key) {
            self.bytes -= self.entries.remove(index).unwrap().1.pixels.len();
        }
        while self.bytes + frame.pixels.len() > CACHE_BYTES {
            self.bytes -= self.entries.pop_front().unwrap().1.pixels.len();
        }
        self.bytes += frame.pixels.len();
        self.entries.push_back((key, frame.clone()));
    }
}
type Mailbox = Arc<(Mutex<Option<Request>>, Condvar)>;
#[derive(Clone)]
pub(crate) struct VideoSender {
    mailbox: Mailbox,
    generation: Arc<AtomicU64>,
    latest: Arc<AtomicU64>,
    last: Arc<Mutex<Option<RenderKey>>>,
}
pub(crate) struct VideoWorker {
    pub controller: VideoSender,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    pub error: Arc<Mutex<Option<(u64, String)>>>,
}
impl VideoWorker {
    pub fn new(
        rt: Runtime,
        output: Arc<Mutex<Option<Frame>>>,
        latest: Arc<AtomicU64>,
        state: Arc<Mutex<Snapshot>>,
    ) -> Self {
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
        let worker_latest = latest.clone();
        let thread = thread::spawn(move || {
            let latest = worker_latest;
            let mut decoders: VecDeque<(String, Decoder)> = VecDeque::new();
            let mut cache = FrameCache::default();
            let mut frames = 0;
            let mut hits = 0;
            let mut opens = 0;
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
                if latest.load(Ordering::Relaxed) != request.interaction.id {
                    continue;
                }
                let result = (|| -> Result<Frame> {
                    if let Some(m) = &request.media
                        && request.opacity > 0.
                        && m.video.as_ref().is_some_and(|v| request.at >= v.start)
                    {
                        let source = source_key(m);
                        let key = FrameKey {
                            source: source.clone(),
                            at: request.at,
                            crop: request.crop,
                        };
                        let mut frame = if let Some(frame) = cache.get(&key) {
                            hits += 1;
                            frame
                        } else {
                            if let Some(index) = decoders.iter().position(|(k, _)| k == &source) {
                                let decoder = decoders.remove(index).unwrap();
                                decoders.push_back(decoder);
                            } else {
                                if decoders.len() >= 3 {
                                    decoders.pop_front();
                                }
                                decoders.push_back((source, Decoder::open(&rt, m)?));
                                opens += 1;
                            }
                            let frame = decoders.back_mut().unwrap().1.read(
                                request.at,
                                request.crop,
                                request.sequence,
                                &gen_,
                                &stop_,
                                Some((&latest, request.interaction.id)),
                            )?;
                            cache.insert(key, &frame);
                            frame
                        };
                        if request.opacity < 1. {
                            for pixel in frame.pixels.chunks_exact_mut(4) {
                                for value in &mut pixel[..3] {
                                    *value = (*value as f64 * request.opacity.clamp(0., 1.)).round()
                                        as u8;
                                }
                            }
                        }
                        Ok(frame)
                    } else {
                        let mut pixels = vec![0; 960 * 540 * 4];
                        for pixel in pixels.chunks_exact_mut(4) {
                            pixel[3] = 255;
                        }
                        Ok(Frame {
                            pixels,
                            width: 960,
                            height: 540,
                            sequence: 0,
                            request_id: 0,
                            position: 0,
                            source_position: 0,
                            requested_at: Instant::now(),
                        })
                    }
                })();
                // Same lock order as Player::request. Invalidation and publication
                // are atomic with respect to each other, including a decode that
                // finishes just as the user clicks elsewhere.
                let mut output = output.lock().unwrap();
                if latest.load(Ordering::Relaxed) != request.interaction.id
                    || gen_.load(Ordering::Relaxed) != request.sequence
                {
                    continue;
                }
                match result {
                    Ok(mut frame) => {
                        frames += 1;
                        frame.sequence = frames;
                        frame.request_id = request.interaction.id;
                        frame.position = request.position;
                        frame.requested_at = request.interaction.started;
                        let mut state = state.lock().unwrap();
                        if state.request_id == request.interaction.id {
                            state.video_position = request.position;
                            state.frames = frames;
                            state.first_frame_ms.get_or_insert_with(|| {
                                request.interaction.started.elapsed().as_secs_f64() * 1000.
                            });
                            state.cache_hits = hits;
                            state.cache_bytes = cache.bytes;
                            state.decoder_opens = opens;
                        }
                        *output = Some(frame);
                    }
                    Err(e) => {
                        let message = e.to_string();
                        let mut state = state.lock().unwrap();
                        if state.request_id == request.interaction.id {
                            state.error = Some(message.clone());
                        }
                        *err.lock().unwrap() = Some((request.interaction.id, message));
                    }
                }
            }
        });
        Self {
            controller: VideoSender {
                mailbox,
                generation,
                latest,
                last: Arc::new(Mutex::new(None)),
            },
            stop,
            thread: Some(thread),
            error,
        }
    }
}
impl VideoSender {
    pub fn request_cropped(
        &self,
        media: Option<&Media>,
        at: Tick,
        opacity: f64,
        crop: Option<CropRect>,
        interaction: PreviewRequest,
        position: Tick,
    ) {
        let mut last = self.last.lock().unwrap();
        if self.latest.load(Ordering::Relaxed) != interaction.id {
            return;
        }
        let key = RenderKey {
            frame: FrameKey {
                source: media.map(source_key).unwrap_or_default(),
                at,
                crop,
            },
            opacity: opacity.to_bits(),
            interaction: interaction.id,
            position,
        };
        if last.as_ref() == Some(&key) {
            return;
        }
        // Continuous playback may finish a nearby frame. Every new user
        // interaction cancels the prior one, even for a one-frame backward step.
        let discontinuity = last.as_ref().is_none_or(|last| {
            last.interaction != interaction.id
                || last.frame.source != key.frame.source
                || last.frame.crop != crop
                || (at - last.frame.at).abs() > HZ / 4
        });
        *last = Some(key);
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
            crop,
            sequence,
            interaction,
            position,
        });
        wake.notify_one();
    }
}
fn source_key(media: &Media) -> String {
    format!(
        "{}|{}|{}|{}",
        media.id,
        media.path.display(),
        media.file_size,
        media.video.as_ref().map_or(0, |v| v.index)
    )
}
impl Drop for VideoWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.controller.mailbox.1.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_cache_is_bounded_and_never_mutates_unfaded_source_pixels() {
        let mut cache = FrameCache::default();
        let frame = Frame {
            pixels: vec![7; 960 * 540 * 4],
            width: 960,
            height: 540,
            sequence: 0,
            request_id: 0,
            position: 0,
            source_position: 0,
            requested_at: Instant::now(),
        };
        for at in 0..20 {
            cache.insert(
                FrameKey {
                    source: "source".into(),
                    at,
                    crop: None,
                },
                &frame,
            );
            assert!(cache.bytes <= CACHE_BYTES);
        }
        assert_eq!(cache.entries.len(), 16);
        assert!(
            cache
                .get(&FrameKey {
                    source: "source".into(),
                    at: 0,
                    crop: None
                })
                .is_none()
        );
        let key = FrameKey {
            source: "source".into(),
            at: 19,
            crop: None,
        };
        let mut faded = cache.get(&key).unwrap();
        faded.pixels[0] = 0;
        assert_eq!(cache.get(&key).unwrap().pixels[0], 7);
        assert!(
            cache
                .get(&FrameKey {
                    crop: Some(CropRect {
                        x: 0,
                        y: 0,
                        width: 2,
                        height: 2
                    }),
                    ..key
                })
                .is_none()
        );
    }
}
