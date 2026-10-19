use crate::model::*;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[derive(Clone, Debug)]
pub struct Runtime {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub mpv: PathBuf,
    pub cache: PathBuf,
}
pub fn command(path: &Path) -> Command {
    let cmd = Command::new(path);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(0x08000000);
        cmd
    };
    cmd
}
impl Runtime {
    fn asset_cache(&self, m: &Media, kind: &str) -> Result<PathBuf> {
        let modified = fs::metadata(&m.path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let key = format!("{}|{}|{}", m.path.display(), m.file_size, modified);
        Ok(self.cache.join(format!("{kind}-{:x}", Sha256::digest(key))))
    }
    pub fn discover() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let base = exe.parent().unwrap();
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = std::env::var_os("SVBB_RUNTIME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if base.join("runtime").exists() {
                    base.join("runtime")
                } else {
                    workspace.join("runtime")
                }
            });
        let find = |name: &str, variants: &[&str]| -> Result<PathBuf> {
            if let Some(p) = variants.iter().map(|s| root.join(s)).find(|p| p.is_file()) {
                return Ok(p);
            }
            if let Some(paths) = std::env::var_os("PATH")
                && let Some(p) = std::env::split_paths(&paths)
                    .map(|p| p.join(name))
                    .find(|p| p.is_file())
            {
                return Ok(p);
            }
            anyhow::bail!(
                "Missing runtime {name}. See BUILDING.md or place it in {}",
                root.display()
            )
        };
        #[cfg(windows)]
        let (ff, probe, mpv) = (
            find("ffmpeg.exe", &["ffmpeg.exe", "ffmpeg/ffmpeg.exe"])?,
            find("ffprobe.exe", &["ffprobe.exe"])?,
            find("libmpv-2.dll", &["libmpv-2.dll", "mpv/libmpv-2.dll"])?,
        );
        #[cfg(not(windows))]
        let (ff, probe, mpv) = (
            find("ffmpeg", &["ffmpeg"])?,
            find("ffprobe", &["ffprobe"])?,
            std::env::var_os("SVBB_MPV")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(if cfg!(target_os = "macos") {
                        "libmpv.2.dylib"
                    } else {
                        "libmpv.so.2"
                    })
                }),
        );
        let data = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| {
                    PathBuf::from(home).join(if cfg!(target_os = "macos") {
                        "Library/Caches"
                    } else {
                        ".cache"
                    })
                })
            })
            .unwrap_or_else(std::env::temp_dir);
        let cache = data.join("SVBB").join("cache-v1");
        fs::create_dir_all(&cache)?;
        Ok(Self {
            ffmpeg: ff,
            ffprobe: probe,
            mpv,
            cache,
        })
    }
    pub fn probe(&self, path: &Path) -> Result<Media> {
        let path =
            fs::canonicalize(path).with_context(|| format!("Cannot read {}", path.display()))?;
        // Avoid Windows verbatim prefix in file URLs passed to mpv/FFmpeg.
        let path = PathBuf::from(path.to_string_lossy().trim_start_matches("\\\\?\\"));
        let out = command(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-show_format",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()?;
        check(&out, "Inspect media")?;
        let v: Value = serde_json::from_slice(&out.stdout)?;
        let streams = v["streams"].as_array().context("No streams found")?;
        let video = streams
            .iter()
            .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1);
        let audio = streams.iter().find(|s| s["codec_type"] == "audio");
        ensure!(
            !video.is_some_and(|s| matches!(
                s["color_transfer"].as_str(),
                Some("smpte2084" | "arib-std-b67")
            )),
            "HDR video is outside SVBB 0.1's supported SDR workflow"
        );
        ensure!(
            video.is_some() || audio.is_some(),
            "File has no supported video or audio stream"
        );
        let parse_r = |s: &str| -> Rational {
            let mut n = s.split('/');
            Rational::new(
                n.next().unwrap_or("1").parse().unwrap_or(1),
                n.next().unwrap_or("1").parse::<i64>().unwrap_or(1).max(1),
            )
        };
        let stream = |s: &Value| Stream {
            index: s["index"].as_u64().unwrap_or(0) as u32,
            codec: s["codec_name"].as_str().unwrap_or("unknown").into(),
            timebase: parse_r(s["time_base"].as_str().unwrap_or("1/1000000")),
            start_pts: s["start_pts"].as_i64().unwrap_or(0),
            start: ticks(number(&s["start_time"]).unwrap_or(0.)),
        };
        let origin = number(&v["format"]["start_time"]).unwrap_or(0.);
        let duration = number(&v["format"]["duration"])
            .or_else(|| {
                streams
                    .iter()
                    .filter_map(|s| number(&s["duration"]))
                    .reduce(f64::max)
            })
            .context("Cannot determine a finite media duration")?;
        ensure!(
            duration.is_finite() && duration > 0. && duration <= 86400.,
            "Media duration must be finite and within 24 hours"
        );
        let mut vs = video.map(stream);
        let mut aud = audio.map(stream);
        for s in [&mut vs, &mut aud].into_iter().flatten() {
            s.start -= ticks(origin);
        }
        let m = Media {
            id: id(),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            file_size: fs::metadata(&path)?.len(),
            path,
            duration: ticks(duration),
            video: vs,
            audio: aud,
            width: video.and_then(|s| s["width"].as_u64()).unwrap_or(0) as u32,
            height: video.and_then(|s| s["height"].as_u64()).unwrap_or(0) as u32,
            sample_rate: audio
                .and_then(|s| s["sample_rate"].as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            channels: audio.and_then(|s| s["channels"].as_u64()).unwrap_or(0) as u32,
            frame_rate: parse_r(
                video
                    .and_then(|s| s["avg_frame_rate"].as_str())
                    .unwrap_or("0/1"),
            ),
        };
        // Actually decode the first frame/block; codec names alone aren't support claims.
        let mut test = command(&self.ffmpeg);
        test.args(["-v", "error", "-nostdin", "-threads", "1", "-i"])
            .arg(&m.path);
        if let Some(s) = &m.video {
            test.args(["-map", &format!("0:{}", s.index), "-frames:v", "1"]);
        } else {
            test.args(["-t", "0.05"]);
        }
        let out = test.args(["-f", "null", "-"]).output()?;
        check(&out, "Decode media")?;
        Ok(m)
    }
    pub fn waveform(&self, m: &Media) -> Result<Vec<f32>> {
        self.waveform_progress(m, &std::sync::atomic::AtomicBool::new(false), |_, _| {})
    }
    pub fn waveform_progress(
        &self,
        m: &Media,
        cancel: &std::sync::atomic::AtomicBool,
        mut progress: impl FnMut(usize, Vec<f32>),
    ) -> Result<Vec<f32>> {
        if m.audio.is_none() {
            return Ok(vec![]);
        }
        let cache = self.asset_cache(m, "wave-v2")?;
        if let Ok(bytes) = fs::read(&cache)
            && bytes.len().is_multiple_of(4)
        {
            return Ok(bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect());
        }
        let mut child = command(&self.ffmpeg)
            .args(["-v", "error", "-nostdin", "-threads", "1", "-i"])
            .arg(&m.path)
            .args([
                "-map",
                &format!("0:{}", m.audio.as_ref().unwrap().index),
                "-vn",
                "-ac",
                "1",
                "-ar",
                "8000",
                "-f",
                "f32le",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let err = child.stderr.take().unwrap();
        let log = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::BufReader::new(err).read_to_string(&mut s);
            s
        });
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let delay = m.audio.as_ref().map(|s| s.start).unwrap_or(0).max(0);
        let mut peaks = vec![0.; (seconds(delay) * 100.).round() as usize];
        let mut reported = 0;
        let mut last_report = std::time::Instant::now();
        let mut block = [0u8; 320];
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = log.join();
                anyhow::bail!("Cancelled");
            }
            let mut filled = 0;
            while filled < block.len() {
                let n = reader.read(&mut block[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            let peak = block[..filled / 4 * 4]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()).abs())
                .fold(0., f32::max);
            peaks.push(peak);
            if last_report.elapsed() > std::time::Duration::from_millis(250) {
                progress(reported, peaks[reported..].to_vec());
                reported = peaks.len();
                last_report = std::time::Instant::now();
            }
        }
        let status = child.wait()?;
        let log = log.join().unwrap_or_default();
        ensure!(status.success(), "Waveform decoding failed: {log}");
        let bytes: Vec<u8> = peaks.iter().flat_map(|v| v.to_le_bytes()).collect();
        let tmp = cache.with_extension(format!("{}.tmp", id()));
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, cache)?;
        Ok(peaks)
    }
    pub fn thumbnail(&self, m: &Media) -> Result<Vec<u8>> {
        let cache = self.asset_cache(m, "thumbnail-v1")?;
        if let Ok(bytes) = fs::read(&cache) {
            return Ok(bytes);
        }
        let out = command(&self.ffmpeg)
            .args(["-v", "error", "-nostdin", "-ss", "0", "-i"])
            .arg(&m.path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=160:90:force_original_aspect_ratio=decrease,pad=160:90:(ow-iw)/2:(oh-ih)/2",
                "-f",
                "image2pipe",
                "-c:v",
                "png",
                "-",
            ])
            .output()?;
        check(&out, "Create thumbnail")?;
        fs::write(cache, &out.stdout)?;
        Ok(out.stdout)
    }
    pub fn loudness(&self, m: &Media, c: &Clip) -> Result<(f64, f64)> {
        let s = m.audio.as_ref().context("Clip has no audio")?;
        let out = command(&self.ffmpeg)
            .args(["-v", "info", "-nostdin", "-ss", &decimal(c.source_in), "-i"])
            .arg(&m.path)
            .args([
                "-t",
                &decimal(c.duration),
                "-map",
                &format!("0:{}", s.index),
                "-vn",
                "-af",
                "loudnorm=I=-16:TP=-1:LRA=11:print_format=json",
                "-f",
                "null",
                "-",
            ])
            .output()?;
        check(&out, "Measure loudness")?;
        let log = String::from_utf8_lossy(&out.stderr);
        let start = log
            .rfind('{')
            .context("Loudness analyzer returned no measurement")?;
        let end = log[start..]
            .find('}')
            .context("Incomplete loudness measurement")?
            + start;
        let v: Value = serde_json::from_str(&log[start..=end])?;
        Ok((
            number(&v["input_i"]).context("Invalid integrated loudness")?,
            number(&v["input_tp"]).context("Invalid true peak")?,
        ))
    }
}
pub fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}
pub fn check(o: &Output, what: &str) -> Result<()> {
    ensure!(
        o.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    Ok(())
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    a == b || (a.exists() && b.exists() && same_file::is_same_file(a, b).unwrap_or(false))
}
