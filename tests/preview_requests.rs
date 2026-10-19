use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use svbb::{
    media::{Runtime, check, command},
    model::*,
    playback::{Frame, Player},
    render,
};

fn wait(player: &Player, id: u64, channel: usize) -> Frame {
    let started = Instant::now();
    loop {
        assert!(player.take_error().is_none());
        if let Some(frame) = player.frame.lock().unwrap().as_ref() {
            assert_eq!(
                frame.request_id, id,
                "An obsolete frame was published after a newer request"
            );
            let offset = (frame.height / 2 * frame.width + frame.width / 4) * 4;
            if frame.pixels[offset + channel] > 180 {
                return frame.clone();
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "Latest correct frame did not arrive"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires pinned FFmpeg and libmpv runtime"]
fn latest_request_wins_and_cached_sources_crops_and_reloads_stay_correct() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = Runtime::discover().unwrap();
    rt.cache = dir.path().join("cache");
    std::fs::create_dir_all(&rt.cache).unwrap();
    let mut media = vec![];
    for color in ["red", "lime", "blue"] {
        let path = dir.path().join(format!("{color}.mp4"));
        let mut cmd = command(&rt.ffmpeg);
        cmd.args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color={color}:s=128x72:r=30:d=2"),
        ]);
        if color == "red" {
            cmd.args(["-vf", "drawbox=x=64:y=0:w=64:h=72:color=blue:t=fill"]);
        }
        let out = cmd
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&path)
            .output()
            .unwrap();
        check(&out, "Create preview source fixture").unwrap();
        media.push(rt.probe(&path).unwrap());
    }
    let mut project = Project::default();
    project.clips.push(Clip::new(&media[0], 0, 0));
    project.media = media.clone();
    let player = Player::headless(&rt).unwrap();
    let plan = render::prepare_preview(&rt, &project, &AtomicBool::new(false)).unwrap();
    let first = player.load(plan.clone(), HZ / 2, false);
    wait(&player, first, 0);
    for _ in 0..3 {
        for (index, source) in media.iter().enumerate() {
            player.show_source(source.clone(), HZ / 2);
            wait(&player, player.snapshot().request_id, index);
        }
    }
    assert_eq!(
        player.snapshot().decoder_opens,
        3,
        "Switching sources should reuse warm decoders"
    );
    assert!(player.snapshot().cache_hits >= 7);
    player.show_source_cropped(
        media[0].clone(),
        HZ / 2,
        Some(CropRect {
            x: 64,
            y: 0,
            width: 64,
            height: 72,
        }),
    );
    wait(&player, player.snapshot().request_id, 2);
    player.show_source(media[0].clone(), HZ / 2);
    wait(&player, player.snapshot().request_id, 0);

    for index in 0..100 {
        player.show_source(media[index % 3].clone(), HZ / 2);
    }
    player.show_source(media[2].clone(), HZ / 2);
    let id = player.snapshot().request_id;
    let frame = wait(&player, id, 2);
    player.frame_presented(&frame);
    let state = player.snapshot();
    assert!(state.presented_ms.unwrap() >= state.first_frame_ms.unwrap());
    assert!(state.cache_bytes <= 32 * 1024 * 1024);

    // Returning from a source/crop preview restores the timeline source.
    let id = player.seek(HZ / 2);
    wait(&player, id, 0);
    for _ in 0..4 {
        player.load(plan.clone(), HZ, false);
        player.seek(HZ);
        player.show_source(media[1].clone(), HZ / 2);
        let id = player.snapshot().request_id;
        wait(&player, id, 1);
        assert_eq!(player.snapshot().request_id, id);
    }
    let mut missing = media[0].clone();
    missing.id = svbb::model::id();
    missing.path = dir.path().join("missing.mp4");
    player.show_source(missing, HZ / 2);
    let started = Instant::now();
    while player.snapshot().error.is_none() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(2));
    }
    player.show_source(media[2].clone(), HZ / 2);
    wait(&player, player.snapshot().request_id, 2);
}

#[test]
#[ignore = "requires pinned FFmpeg and libmpv runtime"]
fn rapidly_replaced_audio_mixes_resume_at_the_latest_position() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = Runtime::discover().unwrap();
    rt.cache = dir.path().join("cache");
    std::fs::create_dir_all(&rt.cache).unwrap();
    let source = dir.path().join("reloads.wav");
    let out = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=700:sample_rate=48000:duration=8",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&out, "Create reload fixture").unwrap();
    let media = rt.probe(&source).unwrap();
    let mut long = Project::default();
    long.clips.push(Clip::new(&media, 1, 0));
    long.media.push(media);
    let mut short = long.clone();
    short.clips[0].duration = HZ * 4;
    let plan = |p: &Project| render::prepare_preview(&rt, p, &AtomicBool::new(false)).unwrap();
    let player = Player::headless(&rt).unwrap();
    for delay in [0, 2, 5, 15, 0, 2, 5, 15] {
        player.load(plan(&short), HZ, false);
        std::thread::sleep(Duration::from_millis(delay));
        player.load(plan(&long), HZ * 6, false);
        let id = player.seek(HZ * 6);
        player.toggle();
        let started = Instant::now();
        loop {
            let state = player.snapshot();
            assert!(player.take_error().is_none());
            assert_eq!(state.request_id, id);
            if !state.paused
                && state.audio_ready_ms.is_some()
                && state.position > HZ * 6 + HZ / 4
                && state.peak_db.iter().any(|v| *v > -60.)
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "Latest mix failed to resume: position {}, paused {}, ready {:?}",
                seconds(state.position),
                state.paused,
                state.audio_ready_ms
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(player.snapshot().position < HZ * 8);
        player.pause();
    }
}
