//! Repeatable adapter benchmark, using the ten-minute acceptance recording.
//! Timings include the real FFmpeg/libmpv workers; OS disk cache is not flushed.
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use svbb::{media::Runtime, model::*, playback::Player, render, video::Decoder};

fn fingerprint(pixels: &[u8]) -> u64 {
    let mut hash = DefaultHasher::new();
    for chunk in pixels.chunks(1024) {
        chunk.hash(&mut hash);
    }
    hash.finish()
}
fn wait_frame(player: &Player, previous: u64, expected: Option<u64>, request_id: u64) -> f64 {
    let start = Instant::now();
    loop {
        assert!(player.take_error().is_none(), "Preview reported a failure");
        if let Some(frame) = player.frame.lock().unwrap().as_ref()
            && frame.request_id == request_id
            && frame.sequence > previous
            && expected.is_none_or(|hash| fingerprint(&frame.pixels) == hash)
        {
            return start.elapsed().as_secs_f64() * 1000.;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "No correct preview within ten seconds"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires the pinned runtime and scripts/make_acceptance_media.py fixtures"]
fn long_recording_preview_workflow() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let profile = tempfile::tempdir().unwrap();
    let mut rt = Runtime::discover().unwrap();
    rt.cache = profile.path().join("cache");
    std::fs::create_dir_all(&rt.cache).unwrap();
    let probe_started = Instant::now();
    let media = rt
        .probe(&root.join("test-output/acceptance-media/gameplay-1.mp4"))
        .unwrap();
    let probe_ms = probe_started.elapsed().as_secs_f64() * 1000.;
    assert!(media.duration >= HZ * 600);
    let p = Project {
        clips: vec![Clip::new(&media, 0, 0), Clip::new(&media, 1, 0)],
        media: vec![media.clone()],
        ..Default::default()
    };
    let player = Player::headless(&rt).unwrap();
    let started = Instant::now();
    let first = player.load(
        render::prepare_preview(&rt, &p, &AtomicBool::new(false)).unwrap(),
        0,
        false,
    );
    wait_frame(&player, 0, None, first);
    let first_ms = started.elapsed().as_secs_f64() * 1000.;
    let mut reference = Decoder::open(&rt, &media).unwrap();
    let positions = [120.2, 240.4, 480.6, 30.8, 120.2, 240.4, 480.6, 30.8];
    let mut seeks = vec![];
    for time in positions {
        let expected = fingerprint(&reference.frame(ticks(time)).unwrap().pixels);
        let previous = player.snapshot().frames;
        let id = player.seek(ticks(time));
        let ms = wait_frame(&player, previous, Some(expected), id);
        seeks.push(serde_json::json!({"seconds":time,"correct_frame_ms":ms}));
    }
    let target = ticks(387.65);
    let expected = fingerprint(&reference.frame(target).unwrap().pixels);
    let previous = player.snapshot().frames;
    let burst_start = Instant::now();
    for n in 0..150 {
        player.seek(ticks((n as f64 * 71.33) % 590.));
    }
    let id = player.seek(target);
    wait_frame(&player, previous, Some(expected), id);
    let burst_ms = burst_start.elapsed().as_secs_f64() * 1000.;
    player.toggle();
    let playing = Instant::now();
    while player.snapshot().paused || player.snapshot().position < target + HZ / 5 {
        assert!(player.take_error().is_none());
        assert!(
            playing.elapsed() < Duration::from_secs(5),
            "Play did not resume after scrubbing"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let play_ms = playing.elapsed().as_secs_f64() * 1000.;
    player.pause();
    let metrics = player.snapshot();
    assert!(metrics.cache_hits >= 4);
    assert!(metrics.cache_bytes <= 32 * 1024 * 1024);
    let report = serde_json::json!({
        "version":env!("CARGO_PKG_VERSION"), "fixture":"ten-minute 1080p acceptance pattern; six-second source repeated; OS cache not flushed",
        "source_bytes":media.file_size, "source_seconds":seconds(media.duration), "probe_ms":probe_ms,
        "first_correct_frame_ms":first_ms,"seeks":seeks,"burst_151_requests_to_correct_frame_ms":burst_ms,
        "play_to_200ms_advance_ms":play_ms,"passed":true
        ,"cache_hits":metrics.cache_hits,"cache_bytes":metrics.cache_bytes,"decoder_opens":metrics.decoder_opens
    });
    let path = std::env::var_os("SVBB_BENCH_REPORT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("test-output/preview-benchmark.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{report}");
}
