use std::{path::Path, sync::atomic::AtomicBool};
use svbb::{
    media::{Runtime, check, command},
    model::*,
    render::{self, RenderSettings},
};
fn rgb(rt: &Runtime, path: &Path, t: f64) -> [u8; 3] {
    let o = command(&rt.ffmpeg)
        .args(["-v", "error", "-ss", &t.to_string(), "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=1:1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .output()
        .unwrap();
    check(&o, "Read pixel").unwrap();
    o.stdout[..3].try_into().unwrap()
}
#[test]
#[ignore]
fn vfr_non_keyframe_cut_holds_the_covering_frame() {
    let rt = Runtime::discover().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-output");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("vfr-colors.mkv");
    let vf = "format=gbrp,geq=r='if(eq(N,0),255,0)':g='if(eq(N,1),255,0)':b='if(eq(N,2),255,0)',settb=1/1000,setpts='if(eq(N,0),0,if(eq(N,1),200,700))'";
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=black:s=128x72:r=10:d=0.3",
            "-vf",
            vf,
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-g",
            "120",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Create VFR colors").unwrap();
    let mut p = Project {
        width: 128,
        height: 72,
        ..Default::default()
    };
    let media = rt.probe(&source).unwrap();
    let mut clip = Clip::new(&media, 0, 0);
    clip.source_in = HZ / 2;
    clip.duration = HZ / 4;
    p.media.push(media);
    p.clips.push(clip);
    let out = root.join("vfr-cut.mp4");
    render::export(
        &rt,
        &p,
        &RenderSettings::project(&p),
        &out,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let pixel = rgb(&rt, &out, 0.02);
    assert!(
        pixel[1] > 180 && pixel[2] < 30,
        "At source 0.52s the green frame (PTS .2) still covers the cut: {pixel:?}"
    );
    let pixel = rgb(&rt, &out, 0.22);
    assert!(
        pixel[2] > 180,
        "At source .72 the blue frame must show: {pixel:?}"
    );
    let player = svbb::playback::Player::diagnostic(&rt, &root.join("vfr-preview.log")).unwrap();
    player.load(
        render::prepare_preview(&rt, &p, &AtomicBool::new(false)).unwrap(),
        0,
        false,
    );
    std::thread::sleep(std::time::Duration::from_millis(350));
    for (time, channel) in [(HZ / 60, 1), (HZ * 13 / 60, 2)] {
        player.seek(time);
        std::thread::sleep(std::time::Duration::from_millis(250));
        let frames = player.frame.lock().unwrap();
        let frame = frames.as_ref().expect("VFR preview must render");
        let at = (frame.height / 2 * frame.width + frame.width / 2) * 4;
        assert!(
            frame.pixels[at + channel] > 180,
            "Preview covering frame differs at {}: {:?}, pos {}, error {:?}",
            seconds(time),
            &frame.pixels[at..at + 3],
            seconds(player.snapshot().position),
            player.snapshot().error
        );
    }
}
#[test]
#[ignore]
fn offsets_mono_resampling_odd_dimensions_and_codec_imports() {
    let rt = Runtime::discover().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("offset.mkv");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=321x181:rate=60:duration=2",
            "-itsoffset",
            "0.25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1.75",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Offset/odd fixture").unwrap();
    let media = rt.probe(&source).unwrap();
    assert_eq!((media.width, media.height), (321, 181));
    assert!((media.audio.as_ref().unwrap().start - HZ / 4).abs() < HZ / 100);
    let mut p = Project {
        width: 320,
        height: 180,
        ..Default::default()
    };
    let mut clip = Clip::new(&media, 0, 0);
    clip.duration = HZ * 2;
    let audio = Clip::new(&media, 1, 0);
    p.media.push(media);
    p.clips.push(clip);
    p.clips.push(audio);
    let out = dir.path().join("offset.mp4");
    render::export(
        &rt,
        &p,
        &RenderSettings::project(&p),
        &out,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let o = command(&rt.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&out)
        .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "f32le", "-"])
        .output()
        .unwrap();
    check(&o, "Decode offset audio").unwrap();
    let samples: Vec<_> = o
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let peak = |start: usize, end: usize| {
        samples[start..end]
            .iter()
            .map(|s| s.abs())
            .fold(0., f32::max)
    };
    assert!(
        peak(2000, 9000) < 0.001,
        "Delayed audio must remain silent before .25s"
    );
    assert!(
        peak(16000, 24000) > 0.02,
        "Delayed audio must begin after its original PTS offset"
    );
    let capture_path = dir.path().join("offset-preview.wav");
    let capture = svbb::playback::Player::capture(&rt, &capture_path).unwrap();
    capture.load(
        render::prepare_preview(&rt, &p, &AtomicBool::new(false)).unwrap(),
        0,
        true,
    );
    let started = std::time::Instant::now();
    while capture.snapshot().position < p.duration()
        && started.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        capture.snapshot().error.is_none(),
        "{:?}",
        capture.snapshot().error
    );
    drop(capture);
    let output = command(&rt.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(capture_path)
        .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "f32le", "-"])
        .output()
        .unwrap();
    check(&output, "Decode offset preview").unwrap();
    let decoded: Vec<_> = output
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert!(
        decoded.len() > 90000,
        "Native preview must not truncate mixed-rate source"
    );
    assert!(
        decoded[2000..9000].iter().all(|s| s.abs() < 0.001),
        "Native audio must preserve initial silence"
    );
    assert!(
        decoded[16000..24000].iter().any(|s| s.abs() > 0.02),
        "Native audio must retain delayed tone"
    );
    let mp3 = dir.path().join("voice.mp3");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:sample_rate=44100:duration=1",
            "-c:a",
            "libmp3lame",
        ])
        .arg(&mp3)
        .output()
        .unwrap();
    check(&o, "MP3 fixture").unwrap();
    assert!(rt.probe(&mp3).unwrap().audio.is_some());
    let mov = dir.path().join("hevc.mov");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=yellow:s=128x72:r=24:d=0.5",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error:pools=1",
            "-tag:v",
            "hvc1",
        ])
        .arg(&mov)
        .output()
        .unwrap();
    check(&o, "HEVC MOV fixture").unwrap();
    let m = rt.probe(&mov).unwrap();
    assert_eq!(m.video.unwrap().codec, "hevc");
    let silent = dir.path().join("silence.wav");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=mono",
            "-t",
            "1",
        ])
        .arg(&silent)
        .output()
        .unwrap();
    check(&o, "Silence fixture").unwrap();
    let m = rt.probe(&silent).unwrap();
    let c = Clip::new(&m, 1, 0);
    let (i, tp) = rt.loudness(&m, &c).unwrap();
    assert!(match_loudness(i, tp).is_err());
    let invalid = dir.path().join("invalid.mp4");
    std::fs::write(&invalid, b"not media").unwrap();
    assert!(rt.probe(&invalid).is_err());
}
#[test]
#[ignore]
fn very_short_clips_and_independent_output_fps() {
    let rt = Runtime::discover().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("one.mp4");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=128x72:r=60:d=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Short clip fixture").unwrap();
    let m = rt.probe(&source).unwrap();
    let mut p = Project {
        width: 128,
        height: 72,
        ..Default::default()
    };
    let mut a = Clip::new(&m, 0, 0);
    a.duration = p.fps.frame(1);
    let mut b = a.clone();
    b.id = id();
    b.start = a.duration;
    b.source_in = HZ / 2;
    p.media.push(m);
    p.clips = vec![a, b];
    let mut s = RenderSettings::project(&p);
    s.fps = Rational::new(30, 1);
    let output = dir.path().join("short.mp4");
    render::export(&rt, &p, &s, &output, &AtomicBool::new(false), |_| {}).unwrap();
    let o = command(&rt.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "1");
    assert!(rgb(&rt, &output, 0.)[0] > 180);
    p.tracks[0].mute = true;
    let hidden = dir.path().join("hidden.mp4");
    render::export(&rt, &p, &s, &hidden, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(
        rgb(&rt, &hidden, 0.).iter().all(|v| *v < 5),
        "Video mute must export black as in preview"
    );
}
#[test]
#[ignore]
fn preview_crosses_black_gaps_without_stalling_audio() {
    let rt = Runtime::discover().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("red.mp4");
    let audio = dir.path().join("tone.wav");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=128x72:r=60:d=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video)
        .output()
        .unwrap();
    check(&o, "Gap video fixture").unwrap();
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
        ])
        .arg(&audio)
        .output()
        .unwrap();
    check(&o, "Gap audio fixture").unwrap();
    let v = rt.probe(&video).unwrap();
    let a = rt.probe(&audio).unwrap();
    let mut p = Project {
        width: 128,
        height: 72,
        ..Default::default()
    };
    let mut first = Clip::new(&v, 0, 0);
    first.duration = HZ * 3 / 10;
    let mut second = first.clone();
    second.id = id();
    second.start = HZ * 6 / 10;
    let sound = Clip::new(&a, 1, 0);
    p.media = vec![v, a];
    p.clips = vec![first, second, sound];
    let cancel = AtomicBool::new(false);
    let player = svbb::playback::Player::new(&rt).unwrap();
    player.load(render::prepare_preview(&rt, &p, &cancel).unwrap(), 0, false);
    std::thread::sleep(std::time::Duration::from_millis(350));
    player.seek(HZ * 45 / 100);
    std::thread::sleep(std::time::Duration::from_millis(250));
    {
        let f = player.frame.lock().unwrap();
        let f = f.as_ref().expect("Gap preview must render");
        let at = (f.height / 2 * f.width + f.width / 2) * 4;
        assert!(
            f.pixels[at..at + 3].iter().all(|v| *v < 5),
            "Primary gap must be black"
        );
    }
    player.seek(0);
    player.toggle();
    std::thread::sleep(std::time::Duration::from_millis(900));
    assert!(
        player.snapshot().position > HZ * 7 / 10,
        "Playback stalled at a video gap"
    );
    assert!(
        player.snapshot().peak_db.iter().any(|v| *v > -60.),
        "Audio must continue over the gap"
    );
    assert!(player.snapshot().error.is_none());
}
