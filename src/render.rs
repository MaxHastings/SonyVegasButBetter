use crate::{
    media::{Runtime, check, command, number},
    model::*,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub quality: usize,
    pub protect: bool,
    #[serde(default)]
    pub encoder: Encoder,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoder {
    #[default]
    Auto,
    Software,
    Nvidia,
    Intel,
    Amd,
    Apple,
}
impl Encoder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto · GPU when available",
            Self::Software => "CPU · H.264 software",
            Self::Nvidia => "NVIDIA · NVENC",
            Self::Intel => "Intel · Quick Sync",
            Self::Amd => "AMD · AMF",
            Self::Apple => "Apple · VideoToolbox",
        }
    }
    fn args(self, quality: usize) -> Vec<String> {
        let q = [25, 21, 17][quality.min(2)].to_string();
        let args = match self {
            Self::Nvidia => vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                &q,
                "-b:v",
                "0",
            ],
            Self::Intel => vec![
                "-c:v",
                "h264_qsv",
                "-preset",
                "medium",
                "-global_quality",
                &q,
                "-look_ahead",
                "0",
            ],
            Self::Amd => vec![
                "-c:v", "h264_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", &q, "-qp_p", &q,
                "-qp_b", &q,
            ],
            Self::Apple => vec![
                "-c:v",
                "h264_videotoolbox",
                "-allow_sw",
                "0",
                "-b:v",
                ["6M", "12M", "24M"][quality.min(2)],
            ],
            _ => vec![
                "-c:v", "libx264", "-threads", "4", "-preset", "fast", "-crf", &q,
            ],
        };
        args.into_iter().map(str::to_owned).collect()
    }
}
/// A listed encoder may still fail because a device or compatible driver is absent.
/// Exercise the actual encoder before touching the user's destination.
pub fn select_encoder(rt: &Runtime, requested: Encoder, cancel: &AtomicBool) -> Result<Encoder> {
    if requested == Encoder::Software {
        return Ok(requested);
    }
    let candidates = if requested == Encoder::Auto {
        if cfg!(target_os = "macos") {
            vec![Encoder::Apple]
        } else {
            vec![Encoder::Nvidia, Encoder::Intel, Encoder::Amd]
        }
    } else {
        vec![requested]
    };
    for encoder in candidates {
        let mut args: Vec<String> = ["-f", "lavfi", "-i", "color=black:s=1280x720:r=30:d=0.1"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        args.extend(encoder.args(1));
        args.extend(
            ["-pix_fmt", "yuv420p", "-an", "-f", "null", "-"]
                .into_iter()
                .map(str::to_owned),
        );
        if run_progress(rt, &args, HZ / 10, cancel, |_| {}).is_ok() {
            return Ok(encoder);
        }
        ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
    }
    ensure!(
        requested == Encoder::Auto,
        "{} is unavailable. Update the GPU driver or choose Auto / CPU.",
        requested.label()
    );
    Ok(Encoder::Software)
}
impl RenderSettings {
    pub fn project(p: &Project) -> Self {
        Self {
            width: p.width,
            height: p.height,
            fps: p.fps,
            quality: 1,
            protect: p.peak_protection,
            encoder: Encoder::Auto,
        }
    }
}
#[derive(Debug)]
pub struct Plan {
    pub inputs: Vec<Vec<String>>,
    pub graph: String,
    pub duration: Tick,
}
fn input(path: &Path, start: Tick, duration: Tick) -> Vec<String> {
    vec![
        "-threads".into(),
        "2".into(),
        "-ss".into(),
        decimal(start),
        "-t".into(),
        decimal(duration),
        "-i".into(),
        path.to_string_lossy().into(),
    ]
}
/// Exactly shared by float-PCM preview caches and the offline export planner.
pub fn clip_audio(m: &Media, c: &Clip) -> String {
    let delay = (m.audio.as_ref().map(|s| s.start).unwrap_or(0) - c.source_in).max(0);
    let edge = ticks(0.003).min(c.duration / 2);
    let fi = c.fade_in.max(edge);
    let fo = c.fade_out.max(edge);
    format!(
        "aresample=48000,aformat=sample_fmts=flt:channel_layouts=stereo,asetpts=PTS-STARTPTS,adelay={}S:all=1,apad,atrim=duration={},volume={:.12},afade=t=in:st=0:d={},afade=t=out:st={}:d={}",
        (delay as i128 * 48000 / HZ as i128),
        decimal(c.duration),
        if c.mute { 0. } else { amplitude(c.gain_db) },
        decimal(fi),
        decimal(c.duration - fo),
        decimal(fo)
    )
}
pub fn master(protect: bool) -> &'static str {
    if protect {
        "alimiter=limit=0.98:attack=5:release=50:level=false:latency=true"
    } else {
        "anull"
    }
}
fn video_filter(width: u32, height: u32, fps: Rational, duration: Tick, offset: Tick) -> String {
    format!(
        "fps={}/{}:round=up:start_time={},setpts=PTS-STARTPTS,scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1,pad={}:{}:(ow-iw)/2:(oh-ih)/2,setsar=1,tpad=start_mode=add:start_duration={}:stop_mode=clone:stop_duration={},trim=duration={},setpts=PTS-STARTPTS,format=yuv420p",
        fps.num,
        fps.den,
        decimal(offset),
        width,
        height,
        width,
        height,
        decimal(offset),
        decimal(duration),
        decimal(duration)
    )
}

pub fn plan(p: &Project, s: &RenderSettings) -> Result<Plan> {
    p.validate()?;
    ensure!(p.duration() > 0, "The timeline is empty");
    ensure!(
        p.missing().is_empty(),
        "Relink missing media before rendering"
    );
    ensure!(
        s.width >= 16
            && s.height >= 16
            && s.width.is_multiple_of(2)
            && s.height.is_multiple_of(2)
            && s.fps.num > 0
            && s.fps.den > 0,
        "Invalid render settings"
    );
    let mut inputs = vec![];
    let mut graph = String::new();
    let mut vids = vec![];
    let mut auds = vec![];
    let mut cursor = 0;
    for c in p.lane(0) {
        if c.start > cursor {
            let name = format!("gap{}", vids.len());
            graph += &format!(
                "color=c=black:s={}x{}:r={}/{}:d={}[{}];",
                s.width,
                s.height,
                p.fps.num,
                p.fps.den,
                decimal(c.start - cursor),
                name
            );
            vids.push(name);
        }
        let m = p.media(&c.media)?;
        let n = inputs.len();
        let mut video_input = input(&m.path, c.source_in, c.duration);
        // Input -t counts decoded preroll and can stop before B/VFR frames that
        // cover this range. The trim filter terminates this input precisely.
        video_input.drain(4..6);
        // Keep the frame covering a VFR cut. Accurate CLI seek discards frames
        // whose PTS precedes the cut, even when they are still being displayed.
        // Decode from the preceding keyframe and let fps trim negative PTS.
        video_input.insert(0, "-noaccurate_seek".into());
        inputs.push(video_input);
        let name = format!("v{}", vids.len());
        graph += &format!(
            "[{n}:{}]{}{}{}[{name}];",
            m.video.as_ref().unwrap().index,
            video_filter(
                s.width,
                s.height,
                p.fps,
                c.duration,
                (m.video.as_ref().unwrap().start - c.source_in).max(0)
            ),
            video_fades(c, 0),
            if c.mute || p.tracks[0].mute {
                ",drawbox=color=black:t=fill"
            } else {
                ""
            }
        );
        vids.push(name);
        cursor = c.end();
    }
    if cursor < p.duration() {
        let name = format!("gap{}", vids.len());
        graph += &format!(
            "color=c=black:s={}x{}:r={}/{}:d={}[{}];",
            s.width,
            s.height,
            p.fps.num,
            p.fps.den,
            decimal(p.duration() - cursor),
            name
        );
        vids.push(name);
    }
    graph += &format!(
        "{}concat=n={}:v=1:a=0[vbase];[vbase]fps={}/{}:round=up,trim=duration={}[vout];",
        vids.iter().map(|v| format!("[{v}]")).collect::<String>(),
        vids.len(),
        s.fps.num,
        s.fps.den,
        decimal(p.duration())
    );
    for lane in 1..LANES {
        for c in p.lane(lane) {
            let m = p.media(&c.media)?;
            let Some(a) = &m.audio else {
                continue;
            };
            if c.mute || p.tracks[lane].mute {
                continue;
            }
            let n = inputs.len();
            inputs.push(input(&m.path, c.source_in, c.duration));
            let name = format!("a{}", auds.len());
            graph += &format!(
                "[{n}:{}]{},volume={:.12},adelay={}S:all=1[{name}];",
                a.index,
                clip_audio(m, c),
                amplitude(p.tracks[lane].gain_db),
                c.start as i128 * 48000 / HZ as i128
            );
            auds.push(name);
        }
    }
    if auds.is_empty() {
        graph += &format!(
            "anullsrc=r=48000:cl=stereo,atrim=duration={}[mix];",
            decimal(p.duration())
        );
    } else {
        graph += &format!(
            "{}amix=inputs={}:normalize=0:duration=longest,apad,atrim=duration={}[mix];",
            auds.iter().map(|v| format!("[{v}]")).collect::<String>(),
            auds.len(),
            decimal(p.duration())
        );
    }
    graph += &format!("[mix]{}[aout]", master(s.protect));
    Ok(Plan {
        inputs,
        graph,
        duration: p.duration(),
    })
}
pub fn run_progress(
    rt: &Runtime,
    args: &[String],
    duration: Tick,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f32),
) -> Result<()> {
    let mut child = command(&rt.ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-xerror",
            "-y",
            "-filter_complex_threads",
            "2",
        ])
        .args(args)
        .args(["-progress", "pipe:1", "-nostats"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Cannot start FFmpeg")?;
    let output = child.stdout.take().unwrap();
    let log = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(output)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(n) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<f64>().ok())
            {
                let _ = tx.send(n);
            }
        }
    });
    let logger = std::thread::spawn(move || {
        let mut r = std::io::BufReader::new(log);
        let mut tail = Vec::new();
        let mut b = [0; 8192];
        while let Ok(n) = r.read(&mut b) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&b[..n]);
            if tail.len() > 262144 {
                tail.drain(..131072);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });
    let mut cancelled = false;
    let status = loop {
        for n in rx.try_iter() {
            progress((n / 1e6 / seconds(duration)).clamp(0., 0.995) as f32);
        }
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            let _ = child.kill();
            break child.wait()?;
        }
        if let Some(s) = child.try_wait()? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let _ = reader.join();
    let log = logger.join().unwrap_or_default();
    ensure!(!cancelled, "Cancelled");
    ensure!(status.success(), "FFmpeg failed: {log}");
    Ok(())
}
pub fn export(
    rt: &Runtime,
    p: &Project,
    s: &RenderSettings,
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(f32),
) -> Result<()> {
    export_report(rt, p, s, dest, cancel, progress, |_| {})
}
pub fn export_report(
    rt: &Runtime,
    p: &Project,
    s: &RenderSettings,
    dest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f32),
    mut encoding: impl FnMut(Encoder),
) -> Result<()> {
    let plan = plan(p, s)?;
    let encoder = select_encoder(rt, s.encoder, cancel)?;
    encoding(encoder);
    ensure!(
        !p.media
            .iter()
            .any(|m| crate::media::same_path(&m.path, dest)),
        "Cannot overwrite a source recording"
    );
    let parent = dest.parent().unwrap_or(Path::new("."));
    ensure!(parent.is_dir(), "Output directory does not exist");
    let part = parent.join(format!(".svbb-{}.partial.mp4", id()));
    let result = (|| {
        let graph = rt.cache.join(format!("export-{}.ffgraph", id()));
        fs::write(&graph, &plan.graph)?;
        let mut args = vec![];
        for i in plan.inputs {
            args.extend(i);
        }
        args.extend([
            "-/filter_complex".into(),
            graph.to_string_lossy().into(),
            "-map".into(),
            "[vout]".into(),
            "-map".into(),
            "[aout]".into(),
        ]);
        args.extend(encoder.args(s.quality));
        args.extend([
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
            "-ar".into(),
            "48000".into(),
            "-ac".into(),
            "2".into(),
            "-t".into(),
            decimal(p.duration()),
            "-movflags".into(),
            "+faststart".into(),
            part.to_string_lossy().into(),
        ]);
        let mut r = run_progress(rt, &args, p.duration(), cancel, &mut progress);
        if r.is_err()
            && s.encoder == Encoder::Auto
            && encoder != Encoder::Software
            && !cancel.load(Ordering::Relaxed)
        {
            let start = args.iter().position(|s| s == "-c:v").unwrap();
            let end = args[start..].iter().position(|s| s == "-pix_fmt").unwrap() + start;
            args.splice(start..end, Encoder::Software.args(s.quality));
            encoding(Encoder::Software);
            progress(0.);
            r = run_progress(rt, &args, p.duration(), cancel, &mut progress);
        }
        let _ = fs::remove_file(graph);
        r?;
        ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
        validate(rt, &part, p.duration(), s)?;
        ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part)?
            .sync_all()?;
        fs::rename(&part, dest)?;
        progress(1.);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}
pub fn validate(rt: &Runtime, path: &Path, duration: Tick, s: &RenderSettings) -> Result<()> {
    ensure!(
        path.is_file() && fs::metadata(path)?.len() > 100,
        "No complete render file was produced"
    );
    let out = command(&rt.ffprobe)
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    check(&out, "Validate output")?;
    let j: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = j["streams"].as_array().context("Output has no streams")?;
    let v = streams
        .iter()
        .find(|v| v["codec_type"] == "video")
        .context("Output video is missing")?;
    let a = streams
        .iter()
        .find(|v| v["codec_type"] == "audio")
        .context("Output audio is missing")?;
    ensure!(
        v["codec_name"] == "h264" && a["codec_name"] == "aac",
        "Output codecs do not match H.264/AAC"
    );
    ensure!(
        v["width"] == s.width && v["height"] == s.height && v["pix_fmt"] == "yuv420p",
        "Output dimensions or pixel format mismatch"
    );
    let actual = number(&j["format"]["duration"]).context("Output has no duration")?;
    ensure!(
        (actual - seconds(duration)).abs() < 2. * s.fps.den as f64 / s.fps.num as f64 + 0.05,
        "Output duration mismatch ({actual:.3}s)"
    );
    ensure!(
        a["sample_rate"] == "48000" && a["channels"] == 2,
        "Output audio format mismatch"
    );
    for start in [0., (seconds(duration) - 0.25).max(0.)] {
        let o = command(&rt.ffmpeg)
            .args(["-v", "error", "-nostdin", "-ss", &start.to_string(), "-i"])
            .arg(path)
            .args(["-t", "0.25", "-f", "null", "-"])
            .output()?;
        check(&o, "Decode rendered beginning/end")?;
    }
    Ok(())
}
pub fn audio_cache(rt: &Runtime, m: &Media, c: &Clip, cancel: &AtomicBool) -> Result<PathBuf> {
    let stamp = fs::metadata(&m.path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let filter = clip_audio(m, c);
    let key = format!(
        "{}|{}|{}|{}|{}|{}",
        m.path.display(),
        m.file_size,
        stamp,
        c.source_in,
        c.duration,
        filter
    );
    let hash = format!("{:x}", Sha256::digest(key));
    let path = rt.cache.join(format!("audio-{hash}.wav"));
    if path.is_file() {
        return Ok(path);
    }
    let part = rt.cache.join(format!("audio-{}.part.wav", id()));
    let mut args = input(&m.path, c.source_in, c.duration);
    args.extend([
        "-map".into(),
        format!("0:{}", m.audio.as_ref().context("No audio stream")?.index),
        "-vn".into(),
        "-af".into(),
        filter,
        "-c:a".into(),
        "pcm_f32le".into(),
        "-rf64".into(),
        "auto".into(),
        part.to_string_lossy().into(),
    ]);
    let result = run_progress(rt, &args, c.duration, cancel, |_| {});
    if result.is_ok() {
        fs::rename(&part, &path)?;
    } else {
        let _ = fs::remove_file(&part);
    }
    result?;
    Ok(path)
}
#[derive(Clone, Debug)]
pub struct PreviewPlan {
    pub project: Project,
}
pub fn prepare_preview(rt: &Runtime, p: &Project, cancel: &AtomicBool) -> Result<PreviewPlan> {
    prepare_preview_progress(rt, p, cancel, |_, _| {})
}
pub fn prepare_preview_progress(
    _rt: &Runtime,
    p: &Project,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str, f32),
) -> Result<PreviewPlan> {
    p.validate()?;
    ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
    ensure!(p.duration() > 0, "Timeline is empty");
    ensure!(
        p.missing().is_empty(),
        "Relink missing media before previewing"
    );
    progress("Source playback", 1.);
    Ok(PreviewPlan { project: p.clone() })
}

fn video_fades(c: &Clip, offset: Tick) -> String {
    let mut filters = String::new();
    for (kind, start, duration) in [
        ("in", offset, c.fade_in),
        ("out", offset + c.duration - c.fade_out, c.fade_out),
    ] {
        if duration > 0 {
            filters += &format!(
                ",fade=t={kind}:st={}:d={}:enable='between(t,{},{})'",
                decimal(start),
                decimal(duration),
                decimal(start),
                decimal(start + duration)
            );
        }
    }
    filters
}
