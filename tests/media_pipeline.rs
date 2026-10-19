//! Run with: cargo test --test media_pipeline -- --ignored --nocapture --test-threads=1
use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use svbb::{
    edit,
    media::{Runtime, check, command},
    model::*,
    persistence,
    playback::Player,
    render::{self, RenderSettings},
};
fn fixture(rt: &Runtime, path: &Path, color: &str, fps: &str, audio: &str, duration: &str) {
    let out = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color={color}:s=320x180:r={fps}:d={duration}"),
            "-f",
            "lavfi",
            "-i",
            audio,
            "-t",
            duration,
            "-c:v",
            "libx264",
            "-g",
            "120",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(path)
        .output()
        .unwrap();
    check(&out, "Create fixture").unwrap();
}
fn pixel(rt: &Runtime, path: &Path, at: f64) -> [f64; 3] {
    let out = command(&rt.ffmpeg)
        .args(["-v", "error", "-ss", &at.to_string(), "-i"])
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
    check(&out, "Decode pixel").unwrap();
    assert!(out.stdout.len() >= 3);
    [
        out.stdout[0] as f64,
        out.stdout[1] as f64,
        out.stdout[2] as f64,
    ]
}
fn samples(rt: &Runtime, path: &Path, start: f64, duration: f64) -> Vec<f32> {
    let out = command(&rt.ffmpeg)
        .args(["-v", "error", "-ss", &start.to_string(), "-i"])
        .arg(path)
        .args([
            "-t",
            &duration.to_string(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "48000",
            "-f",
            "f32le",
            "-",
        ])
        .output()
        .unwrap();
    check(&out, "Decode samples").unwrap();
    out.stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}
#[test]
#[ignore]
fn actual_cut_mix_export_save_reopen_and_preview() {
    let rt = Runtime::discover().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-output");
    std::fs::create_dir_all(&root).unwrap();
    let red = root.join("red.mp4");
    let blue = root.join("blue.mkv");
    let voice = root.join("quiet-voice.wav");
    fixture(
        &rt,
        &red,
        "red",
        "60",
        "sine=frequency=440:sample_rate=48000",
        "4",
    );
    fixture(
        &rt,
        &blue,
        "blue",
        "30000/1001",
        "sine=frequency=880:sample_rate=44100",
        "4",
    );
    let out = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=44100:duration=4",
            "-af",
            "volume=0.1",
            "-ac",
            "1",
        ])
        .arg(&voice)
        .output()
        .unwrap();
    check(&out, "Create voice").unwrap();
    let mut p = Project {
        width: 320,
        height: 180,
        ..Default::default()
    };
    for path in [&red, &blue, &voice] {
        p.media.push(rt.probe(path).unwrap());
    }
    let ids: Vec<_> = p.media.iter().map(|m| m.id.clone()).collect();
    edit::append(&mut p, &ids[0], 0, None).unwrap();
    edit::append(&mut p, &ids[1], 0, None).unwrap();
    let v = edit::append(&mut p, &ids[2], 2, Some(HZ)).unwrap();
    let a = p.lane(0)[0].id.clone();
    edit::split(&mut p, &a, HZ).unwrap();
    let remove = p.lane(0)[1].id.clone();
    edit::split(&mut p, &remove, HZ * 2).unwrap();
    edit::delete(&mut p, &remove, true).unwrap();
    let v = p.clips.iter().find(|c| c.id == v).unwrap().clone();
    let (lufs, tp) = rt.loudness(p.media(&v.media).unwrap(), &v).unwrap();
    let gain = match_loudness(lufs, tp).unwrap();
    assert!(gain > 0.);
    let c = p.clips.iter_mut().find(|c| c.id == v.id).unwrap();
    c.gain_db = gain;
    c.fade_in = HZ / 2;
    c.fade_out = HZ / 2;
    p.tracks[1].gain_db = -12.;
    p.validate().unwrap();
    let path = root.join("integration.svbb");
    persistence::save(&path, &p).unwrap();
    assert_eq!(persistence::load(&path).unwrap(), p);
    let s = RenderSettings::project(&p);
    let cancel = AtomicBool::new(false);
    let output = root.join("integration.mp4");
    let started = Instant::now();
    render::export(&rt, &p, &s, &output, &cancel, |_| {}).unwrap();
    println!(
        "Validated export: {:?}, duration {:.3}s, loudness {lufs:.2} LUFS -> {gain:+.2}dB",
        started.elapsed(),
        seconds(p.duration())
    );
    let r = pixel(&rt, &output, 0.5);
    let b = pixel(&rt, &output, 3.5);
    assert!(r[0] > 180. && r[2] < 30., "red cut: {r:?}");
    assert!(b[2] > 180. && b[0] < 30., "blue cut: {b:?}");
    let audio = samples(&rt, &output, 0., seconds(p.duration()));
    assert!(rms(&audio) > 0.005);
    assert!(audio.iter().all(|s| s.is_finite()));
    assert!(audio.iter().map(|s| s.abs()).fold(0., f32::max) < 1.05);
    let cc = p.clips.iter().find(|c| c.id == v.id).unwrap();
    let cache = render::audio_cache(&rt, p.media(&cc.media).unwrap(), cc, &cancel).unwrap();
    assert!(
        rms(&samples(&rt, &cache, 0.001, 0.02)) < rms(&samples(&rt, &cache, 0.5, 0.1)) * 0.2,
        "fade must attenuate beginning"
    );
    let ready = Instant::now();
    let preview = render::prepare_preview(&rt, &p, &cancel).unwrap();
    println!("Preview preparation {:?}", ready.elapsed());
    let player = Player::headless(&rt).unwrap();
    player.load(preview, 0, false);
    let started = Instant::now();
    while !player.snapshot().loaded && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let st = player.snapshot();
    println!(
        "Player loaded={} frames={} error={:?}",
        st.loaded, st.frames, st.error
    );
    assert!(st.loaded, "preview failed: {:?}", st.error);
    player.seek(HZ / 2);
    std::thread::sleep(Duration::from_millis(700));
    let st = player.snapshot();
    assert!(
        (st.position - HZ / 2).abs() < HZ / 10,
        "seek: {}",
        seconds(st.position)
    );
    assert!(st.frames > 0, "real frames must render");
    if let Some(f) = player.frame.lock().unwrap().as_ref() {
        let i = (f.height / 2 * f.width + f.width / 2) * 4;
        assert!(
            f.pixels[i] > 180 && f.pixels[i + 2] < 30,
            "Preview must show red source frame"
        );
    }
    player.toggle();
    std::thread::sleep(Duration::from_millis(700));
    assert!(player.snapshot().position > HZ / 2);
    assert!(
        player.snapshot().peak_db.into_iter().any(|n| n > -60.),
        "Meters must read actual decoded audio"
    );
    player.pause();
    player.seek(HZ * 4);
    std::thread::sleep(Duration::from_millis(500));
    if let Some(f) = player.frame.lock().unwrap().as_ref() {
        let i = (f.height / 2 * f.width + f.width / 2) * 4;
        assert!(
            f.pixels[i + 2] > 180 && f.pixels[i] < 30,
            "Preview must show blue source frame after cut"
        );
    }
    drop(player);
    let captured = root.join("preview-mix.wav");
    let capture = Player::capture(&rt, &captured).unwrap();
    capture.load(render::prepare_preview(&rt, &p, &cancel).unwrap(), 0, true);
    let start = Instant::now();
    while capture.snapshot().position < p.duration() - HZ / 10
        && start.elapsed() < Duration::from_secs(15)
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!(
        "Preview capture: {:.3}s, {:?} loaded={} paused={} frames={}",
        seconds(capture.snapshot().position),
        capture.snapshot().error,
        capture.snapshot().loaded,
        capture.snapshot().paused,
        capture.snapshot().frames
    );
    drop(capture);
    let captured_all = samples(&rt, &captured, 0., 20.);
    assert!(
        (captured_all.len() as f64 / 48000. - seconds(p.duration())).abs() < 0.02,
        "Preview capture must not duplicate samples while initializing/seeking: {}s",
        captured_all.len() as f64 / 48000.
    );
    for position in [0.5, 1.5, 3.5, 5.5] {
        let preview = samples(&rt, &captured, position, 0.25);
        let exported = samples(&rt, &output, position, 0.25);
        let ratio = rms(&preview) / rms(&exported);
        println!("Mix RMS at {position}s preview/export={ratio:.6}");
        assert!(
            (ratio - 1.).abs() < 0.03,
            "preview/export loudness must match"
        );
    }
    let mut reopened = persistence::load(&path).unwrap();
    let first = reopened.lane(0)[0].clone();
    edit::trim(
        &mut reopened,
        &first.id,
        first.source_in,
        first.duration - HZ / 2,
        true,
    )
    .unwrap();
    persistence::save(&path, &reopened).unwrap();
    let mut cpu_settings = s.clone();
    cpu_settings.encoder = render::Encoder::Software;
    render::export(
        &rt,
        &reopened,
        &cpu_settings,
        &root.join("integration-second.mp4"),
        &cancel,
        |_| {},
    )
    .unwrap();
    let cancelled = AtomicBool::new(true);
    let target = root.join("cancelled.mp4");
    std::fs::write(&target, b"previous output").unwrap();
    assert!(render::export(&rt, &p, &s, &target, &cancelled, |_| {}).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"previous output");
}
