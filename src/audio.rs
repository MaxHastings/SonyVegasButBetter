//! libmpv's custom stream API reads a virtual RF64 mix from its demux/cache
//! thread. Nothing decodes, reads disk, or takes these locks in the device callback.
use crate::{media::Runtime, model::*, video::Api};
use anyhow::{Context, Result, ensure};
use std::{
    cell::UnsafeCell,
    collections::HashMap,
    ffi::{CString, c_char, c_void},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
unsafe extern "C" {
    fn svbb_video_open(
        api: *const *mut c_void,
        path: *const c_char,
        stream: i32,
        origin: f64,
    ) -> *mut c_void;
    fn svbb_video_close(handle: *mut c_void);
    fn svbb_audio_read(handle: *mut c_void, start: i64, count: i32, out: *mut f32) -> i32;
}
struct AudioDecoder {
    handle: *mut c_void,
    _api: Api,
}
impl AudioDecoder {
    fn open(rt: &Runtime, m: &Media) -> Result<Self> {
        let api = Api::load(rt)?;
        let s = m.audio.as_ref().context("No audio stream")?;
        let origin =
            s.start_pts as f64 * s.timebase.num as f64 / s.timebase.den as f64 - seconds(s.start);
        let path = CString::new(m.path.to_string_lossy().as_bytes())?;
        // SAFETY: ABI table and ownership are identical to the video boundary;
        // audio uses a separate handle exclusively from the demux thread.
        let handle = unsafe {
            svbb_video_open(
                api.functions.as_ptr(),
                path.as_ptr(),
                s.index as i32,
                origin,
            )
        };
        ensure!(!handle.is_null(), "Cannot decode audio: {}", m.name);
        Ok(Self { handle, _api: api })
    }
    fn read(&mut self, start: i64, count: usize) -> Result<Vec<f32>> {
        let mut out = vec![0.; count * 2];
        // SAFETY: exclusive decoder; output has count stereo frames.
        let r = unsafe { svbb_audio_read(self.handle, start, count as i32, out.as_mut_ptr()) };
        ensure!(r >= 0, "Audio decoding failed ({r})");
        Ok(out)
    }
}
impl Drop for AudioDecoder {
    fn drop(&mut self) {
        unsafe { svbb_video_close(self.handle) }
    }
}
pub(crate) struct Factory {
    pub rt: Runtime,
    pub project: Mutex<Project>,
    pub error: std::sync::Arc<Mutex<Option<String>>>,
}
#[repr(C)]
pub(crate) struct StreamInfo {
    cookie: *mut c_void,
    read: Option<extern "C" fn(*mut c_void, *mut c_char, u64) -> i64>,
    seek: Option<extern "C" fn(*mut c_void, i64) -> i64>,
    size: Option<extern "C" fn(*mut c_void) -> i64>,
    close: Option<extern "C" fn(*mut c_void)>,
    cancel: Option<extern "C" fn(*mut c_void)>,
}
struct Stream {
    rt: Runtime,
    p: Project,
    header: Vec<u8>,
    position: i64,
    length: i64,
    decoders: HashMap<String, (u64, AudioDecoder)>,
    reads: u64,
    cancelled: std::sync::Arc<AtomicBool>,
    error: std::sync::Arc<Mutex<Option<String>>>,
}
fn sample(t: Tick) -> i64 {
    (t as i128 * 48000 / HZ as i128) as i64
}
impl Stream {
    fn new(factory: &Factory) -> Self {
        let p = factory.project.lock().unwrap().clone();
        let frames = sample(p.duration());
        let bytes = frames as u64 * 8;
        let mut header = Vec::new();
        header.extend(b"RF64");
        header.extend(u32::MAX.to_le_bytes());
        header.extend(b"WAVEds64");
        header.extend(28u32.to_le_bytes());
        header.extend((bytes + 72).to_le_bytes());
        header.extend(bytes.to_le_bytes());
        header.extend((frames as u64).to_le_bytes());
        header.extend(0u32.to_le_bytes());
        header.extend(b"fmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(3u16.to_le_bytes());
        header.extend(2u16.to_le_bytes());
        header.extend(48000u32.to_le_bytes());
        header.extend(384000u32.to_le_bytes());
        header.extend(8u16.to_le_bytes());
        header.extend(32u16.to_le_bytes());
        header.extend(b"data");
        header.extend(u32::MAX.to_le_bytes());
        Self {
            rt: factory.rt.clone(),
            p,
            header,
            position: 0,
            length: 80 + bytes as i64,
            decoders: HashMap::new(),
            reads: 0,
            cancelled: std::sync::Arc::new(AtomicBool::new(false)),
            error: factory.error.clone(),
        }
    }
    fn mix(&mut self, start: i64, count: usize) -> Result<Vec<f32>> {
        self.reads += 1;
        let mut mixed = vec![0f32; count * 2];
        for c in &self.p.clips {
            if c.lane == 0 || c.mute || self.p.tracks[c.lane].mute {
                continue;
            }
            let begin = sample(c.start).max(start);
            let end = sample(c.end()).min(start + count as i64);
            if begin >= end {
                continue;
            }
            if !self.decoders.contains_key(&c.id) {
                if self.decoders.len() >= 8 {
                    let oldest = self
                        .decoders
                        .iter()
                        .min_by_key(|(_, v)| v.0)
                        .map(|(id, _)| id.clone())
                        .unwrap();
                    self.decoders.remove(&oldest);
                }
                self.decoders.insert(
                    c.id.clone(),
                    (
                        self.reads,
                        AudioDecoder::open(&self.rt, self.p.media(&c.media)?)?,
                    ),
                );
            }
            let (last, decoder) = self.decoders.get_mut(&c.id).unwrap();
            *last = self.reads;
            let source = sample(c.source_in) + begin - sample(c.start);
            let n = (end - begin) as usize;
            let decoded = decoder.read(source, n)?;
            let track = amplitude(self.p.tracks[c.lane].gain_db);
            for frame in 0..n {
                let local = (begin - sample(c.start) + frame as i64) as i128 * HZ as i128 / 48000;
                let gain = clip_amplitude(c, local as Tick) * track;
                let offset = (begin - start) as usize * 2 + frame * 2;
                mixed[offset] += decoded[frame * 2] * gain as f32;
                mixed[offset + 1] += decoded[frame * 2 + 1] * gain as f32;
            }
        }
        Ok(mixed)
    }
    fn bytes(&mut self, limit: usize) -> Result<Vec<u8>> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Ok(vec![]);
        }
        let n = limit
            .min((self.length - self.position).max(0) as usize)
            .min(32768);
        let mut out = Vec::with_capacity(n);
        if self.position < 80 {
            let end = (self.position as usize + n).min(80);
            out.extend_from_slice(&self.header[self.position as usize..end]);
            self.position = end as i64;
        }
        if out.len() < n {
            let offset = self.position - 80;
            let skip = (offset % 8) as usize;
            let frames = (n - out.len() + skip).div_ceil(8);
            let samples = self.mix(offset / 8, frames)?;
            let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            let take = n - out.len();
            out.extend_from_slice(&bytes[skip..skip + take]);
            self.position += take as i64;
        }
        Ok(out)
    }
}
struct Cookie {
    data: UnsafeCell<Stream>,
    cancelled: std::sync::Arc<AtomicBool>,
}
// libmpv guarantees that read/seek/size/close for a cookie are serialized;
// cancel alone may run concurrently and touches only the atomic flag.
extern "C" fn read(cookie: *mut c_void, buf: *mut c_char, n: u64) -> i64 {
    // SAFETY: registered cookie remains allocated until close; mpv provides n writable bytes.
    let cookie = unsafe { &*(cookie as *const Cookie) };
    let stream = unsafe { &mut *cookie.data.get() };
    match stream.bytes(n.min(32768) as usize) {
        Ok(bytes) => {
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.cast(), bytes.len());
            }
            bytes.len() as i64
        }
        Err(e) => {
            *stream.error.lock().unwrap() = Some(e.to_string());
            -1
        }
    }
}
extern "C" fn seek(cookie: *mut c_void, offset: i64) -> i64 {
    let cookie = unsafe { &*(cookie as *const Cookie) };
    let s = unsafe { &mut *cookie.data.get() };
    if offset < 0 || offset > s.length {
        return -1;
    }
    s.position = offset;
    offset
}
extern "C" fn size(cookie: *mut c_void) -> i64 {
    let cookie = unsafe { &*(cookie as *const Cookie) };
    unsafe { (*cookie.data.get()).length }
}
extern "C" fn close(cookie: *mut c_void) {
    unsafe {
        drop(Box::from_raw(cookie as *mut Cookie));
    }
}
extern "C" fn cancel(cookie: *mut c_void) {
    let cookie = unsafe { &*(cookie as *const Cookie) };
    cookie.cancelled.store(true, Ordering::Relaxed);
}
pub(crate) extern "C" fn open(user: *mut c_void, _uri: *mut c_char, info: *mut StreamInfo) -> i32 {
    // SAFETY: Factory is boxed for the entire mpv lifetime; each stream owns a
    // project snapshot, decoder handles and cursor independent of other opens.
    let factory = unsafe { &*(user as *const Factory) };
    let data = Stream::new(factory);
    let cancelled = data.cancelled.clone();
    let stream = Box::into_raw(Box::new(Cookie {
        data: UnsafeCell::new(data),
        cancelled,
    }));
    unsafe {
        *info = StreamInfo {
            cookie: stream.cast(),
            read: Some(read),
            seek: Some(seek),
            size: Some(size),
            close: Some(close),
            cancel: Some(cancel),
        };
    }
    0
}
