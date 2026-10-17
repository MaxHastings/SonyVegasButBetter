use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use svbb::{media::Runtime, model::*, playback::Player, render};
fn main() -> anyhow::Result<()> {
    let rt = Runtime::discover()?;
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-output/acceptance-media/gameplay-1.mp4");
    let m = rt.probe(&source)?;
    let mut c = Clip::new(&m, 0, 0);
    c.source_in = HZ * 580;
    c.duration = HZ * 10;
    let mut p = Project::default();
    p.media.push(m);
    p.clips.push(c);
    let prepare = Instant::now();
    let plan = render::prepare_preview(&rt, &p, &AtomicBool::new(false))?;
    let prepare_ms = prepare.elapsed().as_secs_f64() * 1000.;
    let startup = Instant::now();
    let player = Player::new(&rt)?;
    player.load(plan, 0, false);
    while !player.snapshot().loaded && startup.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let startup_ms = startup.elapsed().as_secs_f64() * 1000.;
    let mut seeks = vec![];
    for t in [0.5, 8.25, 2.5, 6.0, 1.0] {
        let seq = player.snapshot().frames;
        let start = Instant::now();
        player.seek(ticks(t));
        while (player.snapshot().frames <= seq
            || (player.snapshot().position - ticks(t)).abs() > HZ / 60)
            && start.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        seeks.push(start.elapsed().as_secs_f64() * 1000.);
    }
    player.seek(0);
    std::thread::sleep(Duration::from_millis(100));
    let frames = player.snapshot().frames;
    let start = Instant::now();
    player.toggle();
    std::thread::sleep(Duration::from_secs(4));
    player.pause();
    let fps = (player.snapshot().frames - frames) as f64 / start.elapsed().as_secs_f64();
    let report = serde_json::json!({"source":"1080p60 synthetic ten-minute recording, ten-second tail clip","preview":"960x540 software render, persistent libmpv; excludes UI texture upload","warm_preview_prepare_ms":prepare_ms,"adapter_initialize_and_load_ms":startup_ms,"seek_ms":seeks,"rendered_fps_over_four_seconds":fps});
    println!("{}", serde_json::to_string_pretty(&report)?);
    std::fs::write(
        "test-output/benchmark.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
