use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Exact common broadcast frame and audio sample positions. Never accumulate seconds.
pub const HZ: i64 = 282_240_000;
pub const MAX_TIME: i64 = HZ * 60 * 60 * 24;
pub const LANES: usize = 4;
pub type Tick = i64;
pub fn seconds(t: Tick) -> f64 {
    t as f64 / HZ as f64
}
pub fn decimal(t: Tick) -> String {
    format!("{:.12}", seconds(t))
}
pub fn ticks(s: f64) -> Tick {
    (s * HZ as f64).round() as Tick
}
pub fn id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}
impl Rational {
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }
    pub fn to_ticks(self, value: i64) -> Result<Tick> {
        ensure!(self.den > 0 && self.num > 0, "Invalid timebase");
        let n = (value as i128)
            .checked_mul(self.num as i128)
            .and_then(|v| v.checked_mul(HZ as i128))
            .ok_or_else(|| anyhow::anyhow!("Timestamp conversion overflow"))?;
        Ok(i64::try_from(n / self.den as i128)?)
    }
    pub fn frame(self, index: i64) -> Tick {
        (index as i128 * HZ as i128 * self.den as i128 / self.num as i128) as i64
    }
    pub fn align(self, t: Tick) -> Tick {
        let i = (t as i128 * self.num as i128 + HZ as i128 * self.den as i128 / 2)
            / (HZ as i128 * self.den as i128);
        self.frame(i as i64)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Stream {
    pub index: u32,
    pub codec: String,
    pub timebase: Rational,
    pub start_pts: i64,
    pub start: Tick,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Media {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub duration: Tick,
    pub video: Option<Stream>,
    pub audio: Option<Stream>,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub frame_rate: Rational,
    pub file_size: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub id: String,
    pub media: String,
    pub lane: usize,
    pub start: Tick,
    pub source_in: Tick,
    pub duration: Tick,
    pub gain_db: f64,
    pub mute: bool,
    pub fade_in: Tick,
    pub fade_out: Tick,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub crop: Option<CropRect>,
    /// Source-relative motion points survive moves, splits and trims unchanged.
    #[serde(default)]
    pub framing: Vec<FramePoint>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FramePoint {
    pub at: Tick,
    pub rect: CropRect,
}

/// A static source-pixel rectangle on one video event. Audio partners have none.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl CropRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
    pub fn validate(self, width: u32, height: u32) -> Result<()> {
        ensure!(
            self.width >= 2
                && self.height >= 2
                && self.width <= width
                && self.height <= height
                && self.x <= width - self.width
                && self.y <= height - self.height,
            "Crop rectangle exceeds source boundaries"
        );
        ensure!(
            self.x.is_multiple_of(2)
                && self.y.is_multiple_of(2)
                && self.width.is_multiple_of(2)
                && self.height.is_multiple_of(2),
            "Crop coordinates and dimensions must be even pixels"
        );
        Ok(())
    }
}
impl Clip {
    pub fn end(&self) -> Tick {
        self.start + self.duration
    }
    pub fn new(media: &Media, lane: usize, start: Tick) -> Self {
        Self {
            id: id(),
            media: media.id.clone(),
            lane,
            start,
            source_in: 0,
            duration: media.duration,
            gain_db: 0.,
            mute: false,
            fade_in: 0,
            fade_out: 0,
            link: None,
            crop: None,
            framing: vec![],
        }
    }
    pub fn crop_at(&self, source: Tick) -> Option<CropRect> {
        let Some(first) = self.framing.first() else {
            return self.crop;
        };
        if source <= first.at {
            return Some(first.rect);
        }
        for pair in self.framing.windows(2) {
            if source < pair[1].at {
                let mix = |a: u32, b: u32| {
                    let value = a as f64
                        + (b as f64 - a as f64) * (source - pair[0].at) as f64
                            / (pair[1].at - pair[0].at) as f64;
                    (value.floor() as u32) & !1
                };
                let (a, b) = (pair[0].rect, pair[1].rect);
                return Some(CropRect {
                    x: mix(a.x, b.x),
                    y: mix(a.y, b.y),
                    width: mix(a.width, b.width).max(2),
                    height: mix(a.height, b.height).max(2),
                });
            }
        }
        self.framing.last().map(|p| p.rect)
    }
    pub fn clamp_fades(&mut self) {
        self.fade_in = self.fade_in.clamp(0, self.duration);
        self.fade_out = self.fade_out.clamp(0, self.duration);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub name: String,
    pub gain_db: f64,
    pub mute: bool,
    pub locked: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub media: Vec<Media>,
    pub clips: Vec<Clip>,
    pub tracks: [Track; LANES],
    pub peak_protection: bool,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            id: id(),
            name: "Untitled".into(),
            width: 1920,
            height: 1080,
            fps: Rational::new(60, 1),
            media: vec![],
            clips: vec![],
            tracks: [
                "V1 · Video",
                "A1 · Source audio",
                "A2 · Voice",
                "A3 · Music",
            ]
            .map(|name| Track {
                name: name.into(),
                gain_db: 0.,
                mute: false,
                locked: false,
            }),
            peak_protection: true,
        }
    }
}
impl Project {
    pub fn duration(&self) -> Tick {
        self.clips.iter().map(Clip::end).max().unwrap_or(0)
    }
    pub fn media(&self, id: &str) -> Result<&Media> {
        self.media
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| anyhow::anyhow!("Missing media reference: {id}"))
    }
    pub fn lane(&self, lane: usize) -> Vec<&Clip> {
        let mut clips: Vec<_> = self.clips.iter().filter(|c| c.lane == lane).collect();
        clips.sort_by_key(|c| (c.start, c.id.as_str()));
        clips
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.fps.num > 0 && self.fps.num <= 120_000 && self.fps.den > 0 && self.fps.den <= 1001,
            "Invalid project frame rate"
        );
        ensure!(
            (16..=7680).contains(&self.width)
                && (16..=4320).contains(&self.height)
                && self.width.is_multiple_of(2)
                && self.height.is_multiple_of(2),
            "Project dimensions must be even and supported"
        );
        let mut ids = std::collections::HashSet::new();
        for m in &self.media {
            ensure!(ids.insert(m.id.clone()), "Duplicate media ID");
            ensure!(
                m.duration > 0 && m.duration <= MAX_TIME,
                "Media duration must be within 24 hours"
            );
            ensure!(
                m.video.is_some() || m.audio.is_some(),
                "Media has no playable streams"
            );
            for s in [&m.video, &m.audio].into_iter().flatten() {
                ensure!(
                    s.timebase.num > 0 && s.timebase.den > 0,
                    "Invalid media timebase"
                );
            }
        }
        ids.clear();
        for t in &self.tracks {
            ensure!(
                t.gain_db.is_finite() && (-60.0..=60.0).contains(&t.gain_db),
                "Invalid track gain"
            );
        }
        for c in &self.clips {
            ensure!(ids.insert(c.id.clone()), "Duplicate clip ID");
            ensure!(
                c.lane < LANES && c.start >= 0 && c.duration > 0 && c.source_in >= 0,
                "Invalid clip range"
            );
            ensure!(
                c.start <= MAX_TIME - c.duration,
                "Timeline exceeds 24 hours"
            );
            let m = self.media(&c.media)?;
            ensure!(
                c.source_in <= m.duration - c.duration,
                "Trim exceeds source boundaries"
            );
            ensure!(
                c.lane != 0 || m.video.is_some(),
                "Audio-only media belongs on an audio lane"
            );
            ensure!(c.lane == 0 || m.audio.is_some(), "Media has no audio");
            if let Some(crop) = c.crop {
                ensure!(c.lane == 0, "Only video clips can have a crop");
                crop.validate(m.width, m.height)?;
            }
            ensure!(
                c.framing.len() <= 256,
                "A clip supports up to 256 framing points"
            );
            let mut previous = -1;
            for point in &c.framing {
                ensure!(
                    c.lane == 0 && point.at > previous && point.at <= m.duration,
                    "Invalid framing point time or lane"
                );
                point.rect.validate(m.width, m.height)?;
                previous = point.at;
            }
            ensure!(
                c.gain_db.is_finite() && (-60.0..=60.0).contains(&c.gain_db),
                "Invalid clip gain"
            );
            ensure!(
                c.fade_in >= 0
                    && c.fade_out >= 0
                    && c.fade_in <= c.duration
                    && c.fade_out <= c.duration,
                "Invalid fades"
            );
        }
        for group in self
            .clips
            .iter()
            .filter_map(|c| c.link.as_ref())
            .collect::<std::collections::HashSet<_>>()
        {
            let members: Vec<_> = self
                .clips
                .iter()
                .filter(|c| c.link.as_ref() == Some(group))
                .collect();
            ensure!(
                members.len() == 2,
                "A link must contain one video and one audio clip"
            );
            let (a, b) = (members[0], members[1]);
            ensure!(
                (a.lane == 0) != (b.lane == 0)
                    && a.media == b.media
                    && a.start == b.start
                    && a.source_in == b.source_in
                    && a.duration == b.duration,
                "Linked video and audio must stay synchronized"
            );
        }
        for lane in 0..LANES {
            for pair in self.lane(lane).windows(2) {
                if pair[0].end() > pair[1].start {
                    bail!(
                        "Clips overlap on lane {}. Move one clip or use the other audio lane.",
                        lane + 1
                    );
                }
            }
        }
        Ok(())
    }
    pub fn missing(&self) -> Vec<&Media> {
        self.media.iter().filter(|m| !m.path.is_file()).collect()
    }
}

pub fn amplitude(db: f64) -> f64 {
    10_f64.powf(db / 20.)
}
pub fn clip_amplitude(c: &Clip, local: Tick) -> f64 {
    if c.mute || local < 0 || local >= c.duration {
        return 0.;
    }
    let edge = ticks(0.003).min(c.duration / 2);
    amplitude(c.gain_db)
        * (local as f64 / c.fade_in.max(edge) as f64).clamp(0., 1.)
        * ((c.duration - local) as f64 / c.fade_out.max(edge) as f64).clamp(0., 1.)
}
pub fn match_loudness(integrated: f64, true_peak: f64) -> Result<f64> {
    ensure!(
        integrated.is_finite() && true_peak.is_finite() && integrated >= -70.,
        "Recording is silent or below -70 LUFS; automatic boost refused. Use manual gain after checking the noise."
    );
    let gain = (-16. - integrated).min(-1. - true_peak).clamp(-60., 48.);
    ensure!(
        true_peak + gain <= -1. + 1e-6,
        "True peak is outside the supported automatic gain range"
    );
    Ok(gain)
}
