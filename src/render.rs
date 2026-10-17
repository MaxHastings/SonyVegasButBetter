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
}
impl RenderSettings {
    pub fn project(p: &Project) -> Self {
        Self {
            width: p.width,
            height: p.height,
            fps: p.fps,
            quality: 1,
            protect: p.peak_protection,
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
            "[{n}:{}]{}[{name}];",
            m.video.as_ref().unwrap().index,
            video_filter(
                s.width,
                s.height,
                p.fps,
                c.duration,
                (m.video.as_ref().unwrap().start - c.source_in).max(0)
            )
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
    for lane in 0..3 {
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
    mut progress: impl FnMut(f32),
) -> Result<()> {
    let plan = plan(p, s)?;
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
            "-c:v".into(),
            "libx264".into(),
            "-threads".into(),
            "4".into(),
            "-preset".into(),
            "fast".into(),
            "-crf".into(),
            ["25", "21", "17"][s.quality.min(2)].into(),
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
        let r = run_progress(rt, &args, p.duration(), cancel, &mut progress);
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
/// Source-level project-frame cache preserves the frame covering VFR cuts.
/// Later splits, trims, reorders and seeks reuse this file unchanged.
pub fn video_cache(
    rt: &Runtime,
    m: &Media,
    fps: Rational,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f32),
) -> Result<PathBuf> {
    let stamp = fs::metadata(&m.path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let filter = video_filter(
        960,
        540,
        fps,
        m.duration,
        m.video.as_ref().context("No video stream")?.start.max(0),
    );
    let key = format!(
        "preview-video-v1|{}|{}|{}|{}",
        m.path.display(),
        m.file_size,
        stamp,
        filter
    );
    let hash = format!("{:x}", Sha256::digest(key));
    let path = rt.cache.join(format!("video-{hash}.mp4"));
    if path.is_file() {
        return Ok(path);
    }
    let part = rt.cache.join(format!("video-{}.part.mp4", id()));
    let args = vec![
        "-threads".into(),
        "4".into(),
        "-i".into(),
        m.path.to_string_lossy().into(),
        "-map".into(),
        format!("0:{}", m.video.as_ref().unwrap().index),
        "-an".into(),
        "-vf".into(),
        filter,
        "-c:v".into(),
        "libx264".into(),
        "-threads".into(),
        "4".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-crf".into(),
        "18".into(),
        "-bf".into(),
        "0".into(),
        "-g".into(),
        (fps.num / fps.den).max(1).to_string(),
        "-video_track_timescale".into(),
        HZ.to_string(),
        "-movflags".into(),
        "+faststart".into(),
        part.to_string_lossy().into(),
    ];
    let result = run_progress(rt, &args, m.duration, cancel, &mut progress);
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
    pub combined: PathBuf,
    pub graph: String,
}
fn edl_file(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    format!("%{}%{}", s.len(), s)
}
fn entry(path: &Path, start: Tick, duration: Tick) -> String {
    format!(
        "{},{},{}\n",
        edl_file(path),
        decimal(start),
        decimal(duration)
    )
}
fn gap_entries(path: &Path, mut duration: Tick, limit: Tick) -> String {
    let mut s = String::new();
    while duration > 0 {
        let d = duration.min(limit);
        s += &entry(path, 0, d);
        duration -= d;
    }
    s
}
pub fn prepare_preview(rt: &Runtime, p: &Project, cancel: &AtomicBool) -> Result<PreviewPlan> {
    prepare_preview_progress(rt, p, cancel, |_, _| {})
}
pub fn prepare_preview_progress(
    rt: &Runtime,
    p: &Project,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str, f32),
) -> Result<PreviewPlan> {
    p.validate()?;
    ensure!(p.duration() > 0, "Timeline is empty");
    ensure!(
        p.missing().is_empty(),
        "Relink missing media before previewing"
    );
    let silence = rt.cache.join("silence-f32-1h.wav");
    let black = rt
        .cache
        .join(format!("black-frames-{}-{}-10s.mp4", p.fps.num, p.fps.den));
    if !silence.exists() {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = fs::File::create(&silence)?;
        let bytes = 48000u32 * 8 * 3600;
        f.write_all(b"RIFF")?;
        f.write_all(&(bytes + 36).to_le_bytes())?;
        f.write_all(b"WAVEfmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&3u16.to_le_bytes())?;
        f.write_all(&2u16.to_le_bytes())?;
        f.write_all(&48000u32.to_le_bytes())?;
        f.write_all(&384000u32.to_le_bytes())?;
        f.write_all(&8u16.to_le_bytes())?;
        f.write_all(&32u16.to_le_bytes())?;
        f.write_all(b"data")?;
        f.write_all(&bytes.to_le_bytes())?;
        f.seek(SeekFrom::Start(44 + bytes as u64 - 1))?;
        f.write_all(&[0])?;
    }
    if !black.exists() {
        let out = command(&rt.ffmpeg)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("color=black:s=960x540:r={}/{}:d=10", p.fps.num, p.fps.den),
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-bf",
                "0",
                "-g",
                "60",
                "-video_track_timescale",
                "282240000",
            ])
            .arg(&black)
            .output()?;
        check(&out, "Prepare black frames")?;
    }
    let mut video = String::from("# mpv EDL v0\n!delay_open,media_type=video\n!no_chapters\n");
    let mut cursor = 0;
    for c in p.lane(0) {
        if c.start > cursor {
            video += &gap_entries(&black, c.start - cursor, HZ * 10);
        }
        let media = p.media(&c.media)?;
        let cache = video_cache(rt, media, p.fps, cancel, |n| progress(&media.name, n))?;
        video += &entry(&cache, c.source_in, c.duration);
        cursor = c.end();
    }
    if cursor < p.duration() {
        video += &gap_entries(&black, p.duration() - cursor, HZ * 10);
    }
    let mut combined = video;
    let mut graph = String::from("[vid1]null[vo];");
    let mut names = String::new();
    for lane in 0..3 {
        combined += "!new_stream\n!delay_open,media_type=audio\n!no_chapters\n";
        cursor = 0;
        for c in p.lane(lane) {
            if c.start > cursor {
                combined += &gap_entries(&silence, c.start - cursor, HZ * 3600);
            }
            let m = p.media(&c.media)?;
            if m.audio.is_some() && !c.mute && !p.tracks[lane].mute {
                let audio = audio_cache(rt, m, c, cancel)?;
                combined += &entry(&audio, 0, c.duration);
            } else {
                combined += &gap_entries(&silence, c.duration, HZ * 3600);
            }
            cursor = c.end();
        }
        if cursor < p.duration() {
            combined += &gap_entries(&silence, p.duration() - cursor, HZ * 3600);
        }
        graph += &format!(
            "[aid{}]aresample=48000,aformat=sample_fmts=flt:channel_layouts=stereo,volume={:.12}[a{lane}];",
            lane + 1,
            if p.tracks[lane].mute {
                0.
            } else {
                amplitude(p.tracks[lane].gain_db)
            }
        );
        names += &format!("[a{lane}]");
    }
    graph += &format!(
        "{names}amix=inputs=3:normalize=0:duration=longest,{}[ao]",
        master(p.peak_protection)
    );
    let combined_path = rt.cache.join(format!("preview-{}.edl", id()));
    fs::write(&combined_path, combined)?;
    Ok(PreviewPlan {
        combined: combined_path,
        graph,
    })
}
