use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};
use svbb::{
    media::{Runtime, check, command},
    model::*,
    render::{self, RenderNotice, RenderSettings},
};

#[test]
fn budgets_include_audio_and_container_in_decimal_megabytes() {
    for duration in [HZ / 30, HZ, HZ * 30, HZ * 60, HZ * 300] {
        let limit = 10_000_000;
        let b = render::size_budget(limit, duration).unwrap();
        let stream_bytes = (b.video_bps + b.audio_bps) as u128 * duration as u128 / HZ as u128 / 8;
        assert!(stream_bytes + 16_384 < limit as u128);
    }
    assert_eq!(
        render::size_budget(10_000_000, HZ * 30).unwrap().audio_bps,
        128_000
    );
    assert_eq!(
        render::size_budget(10_000_000, HZ * 120).unwrap().audio_bps,
        96_000
    );
    assert!(
        render::size_budget(1_000_000, HZ * 60)
            .unwrap_err()
            .to_string()
            .contains("too small")
    );
    for duration in [0, -1, MAX_TIME + 1] {
        assert!(render::size_budget(10_000_000, duration).is_err());
    }
    assert!(render::size_budget(0, HZ).is_err());
    assert!(render::size_budget(u64::MAX, MAX_TIME).is_ok());
}

#[test]
fn automatic_format_preserves_aspect_and_uses_the_budget() {
    for (width, height) in [(1920, 1080), (1080, 1920), (640, 480)] {
        let p = Project {
            width,
            height,
            ..Default::default()
        };
        let mut s = RenderSettings::project(&p);
        s.max_bytes = Some(10_000_000);
        render::fit_size_format(&mut s, HZ * 60).unwrap();
        assert!(s.width <= width && s.height <= height);
        assert!(s.width.is_multiple_of(2) && s.height.is_multiple_of(2));
        assert!((s.width as f64 / s.height as f64 - width as f64 / height as f64).abs() < 0.01);
        assert_eq!(s.fps, Rational::new(30, 1));
    }
    let mut s = RenderSettings::project(&Project::default());
    s.max_bytes = Some(100_000_000);
    render::fit_size_format(&mut s, HZ * 5).unwrap();
    assert_eq!(
        (s.width, s.height, s.fps),
        (1920, 1080, Rational::new(60, 1))
    );
}

fn fixture() -> (tempfile::TempDir, Runtime, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = Runtime::discover().unwrap();
    rt.cache = dir.path().join("cache");
    fs::create_dir(&rt.cache).unwrap();
    let source = dir.path().join("motion.mp4");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=960x540:r=30:d=60",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=60",
            "-c:v",
            "libx264",
            "-threads",
            "4",
            "-preset",
            "ultrafast",
            "-crf",
            "18",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Generate moving size-limit fixture").unwrap();
    let m = rt.probe(&source).unwrap();
    let mut p = Project {
        width: 960,
        height: 540,
        fps: Rational::new(30, 1),
        ..Default::default()
    };
    let v = Clip::new(&m, 0, 0);
    let mut a = Clip::new(&m, 1, 0);
    a.gain_db = -6.;
    a.fade_in = HZ;
    a.fade_out = HZ;
    p.clips = vec![v, a];
    p.media.push(m);
    (dir, rt, p)
}

#[test]
#[ignore = "requires the pinned FFmpeg runtime; generates and encodes real motion video"]
fn real_exports_fit_limits_preserve_audio_and_cancel_safely() {
    let (dir, rt, mut p) = fixture();
    let mut measurements = vec![];
    for (duration, limit) in [(30, 10_000_000), (60, 10_000_000), (6, 150_000)] {
        for c in &mut p.clips {
            c.duration = HZ * duration;
            c.clamp_fades();
        }
        let mut s = RenderSettings::project(&p);
        s.max_bytes = Some(limit);
        render::fit_size_format(&mut s, p.duration()).unwrap();
        let dest = dir.path().join(format!("{duration}-{limit}.mp4"));
        let mut stages = vec![];
        let mut progress = vec![];
        render::export_report(
            &rt,
            &p,
            &s,
            &dest,
            &AtomicBool::new(false),
            |v| progress.push(v),
            |event| {
                if let RenderNotice::Stage(stage) = event {
                    stages.push(stage)
                }
            },
        )
        .unwrap();
        let bytes = fs::metadata(&dest).unwrap().len();
        assert!(
            bytes <= limit,
            "Hard file limit exceeded: {bytes} > {limit}"
        );
        assert!(
            stages.iter().any(|v| v.contains("Pass 1"))
                && stages.iter().any(|v| v.contains("Pass 2"))
        );
        assert_eq!(progress.last(), Some(&1.));
        assert_eq!(
            fs::read_dir(&rt.cache).unwrap().count(),
            0,
            "Pass logs must be removed after success"
        );
        let o = command(&rt.ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&dest)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        check(&o, "Decode the complete limited export").unwrap();
        // Gain must stay audible and near the expected 0.125-amplitude sine,
        // attenuated by -6 dB. Budget changes audio bitrate, never mix volume.
        let pcm = command(&rt.ffmpeg)
            .args(["-v", "error", "-ss", "2", "-i"])
            .arg(&dest)
            .args(["-t", "1", "-vn", "-ac", "1", "-f", "f32le", "-"])
            .output()
            .unwrap();
        check(&pcm, "Measure exported mix").unwrap();
        let samples: Vec<f64> = pcm
            .stdout
            .chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().unwrap()) as f64)
            .collect();
        let rms = (samples.iter().map(|v| v * v).sum::<f64>() / samples.len() as f64).sqrt();
        // Stereo conversion uses equal-power pan (-3 dB per channel).
        assert!((0.025..0.05).contains(&rms), "Mix gain changed: RMS {rms}");
        let mut too_small = s.clone();
        too_small.max_bytes = Some(bytes - 1);
        assert!(render::validate(&rt, &dest, p.duration(), &too_small).is_err());
        measurements.push(serde_json::json!({"duration_s":duration,"limit_bytes":limit,"actual_bytes":bytes,"width":s.width,"height":s.height,"audio_bps":render::size_budget(limit,p.duration()).unwrap().audio_bps,"mix_rms":rms}));
    }
    let mut s = RenderSettings::project(&p);
    s.max_bytes = Some(150_000);
    render::fit_size_format(&mut s, p.duration()).unwrap();
    let dest = dir.path().join("preserved.mp4");
    fs::write(&dest, b"previous render").unwrap();
    let cancel = AtomicBool::new(false);
    let result = render::export(&rt, &p, &s, &dest, &cancel, |n| {
        if n >= 0.4 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&dest).unwrap(), b"previous render");
    assert_eq!(
        fs::read_dir(&rt.cache).unwrap().count(),
        0,
        "Cancelled jobs must remove pass logs"
    );
    assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".svbb-")
    }));
    let report =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-output/size-limit-report.json");
    fs::create_dir_all(report.parent().unwrap()).unwrap();
    fs::write(report,serde_json::to_vec_pretty(&serde_json::json!({"exports":measurements,"cancel_preserves_destination":true,"temporary_files_cleaned":true})).unwrap()).unwrap();
}
