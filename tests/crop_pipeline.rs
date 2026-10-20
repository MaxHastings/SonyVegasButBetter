use std::{fs, sync::atomic::AtomicBool};
use svbb::{
    media::{Runtime, check, command},
    model::*,
    render::{self, RenderSettings},
    video::Decoder,
};

#[test]
#[ignore = "requires the pinned native FFmpeg runtime"]
fn each_cut_has_its_own_crop_in_preview_and_limited_export() {
    let temp = tempfile::tempdir().unwrap();
    let rt = Runtime::discover().unwrap();
    let source = temp.path().join("two-colors.mp4");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=320x180:r=30:d=2,drawbox=x=160:y=0:w=160:h=180:color=lime:t=fill",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Generate crop reference").unwrap();
    let m = rt.probe(&source).unwrap();
    let left = CropRect {
        x: 0,
        y: 0,
        width: 160,
        height: 180,
    };
    let right = CropRect {
        x: 160,
        y: 0,
        width: 160,
        height: 180,
    };
    let mut decoder = Decoder::open(&rt, &m).unwrap();
    for (crop, channel) in [(left, 0), (right, 1)] {
        let frame = decoder.cropped_frame(HZ / 2, Some(crop)).unwrap();
        let pixel = &frame.pixels[(270 * 960 + 480) * 4..][..3];
        assert!(
            pixel[channel] > 220 && pixel[1 - channel] < 30,
            "Wrong native crop: {pixel:?}"
        );
    }
    let original = decoder.frame(HZ / 2).unwrap();
    assert!(
        original.pixels[(270 * 960 + 100) * 4] > 220,
        "Applying a crop must not mutate the retained source frame"
    );
    assert!(original.pixels[(270 * 960 + 800) * 4 + 1] > 220);
    let mut p = Project {
        width: 320,
        height: 180,
        fps: Rational::new(30, 1),
        ..Default::default()
    };
    let mut c = Clip::new(&m, 0, 0);
    c.duration = HZ;
    c.crop = Some(left);
    let mut second = c.clone();
    second.id = id();
    second.start = HZ;
    second.source_in = HZ;
    second.crop = Some(right);
    p.clips = vec![c, second];
    p.media.push(m);
    let mut s = RenderSettings::project(&p);
    s.max_bytes = Some(100_000);
    let dest = temp.path().join("cropped.mp4");
    render::export(&rt, &p, &s, &dest, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(fs::metadata(&dest).unwrap().len() <= 100_000);
    for (time, channel) in [("0.5", 0), ("1.5", 1)] {
        let o = command(&rt.ffmpeg)
            .args(["-v", "error", "-ss", time, "-i"])
            .arg(&dest)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "crop=2:2:160:90",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap();
        check(&o, "Read export crop pixel").unwrap();
        assert!(
            o.stdout[channel] > 200 && o.stdout[1 - channel] < 40,
            "Wrong exported crop: {:?}",
            &o.stdout[..3]
        );
    }
}

#[test]
#[ignore = "requires the pinned native FFmpeg runtime"]
fn animated_vfr_pan_zoom_and_aspect_match_preview_after_a_source_trim_and_cut() {
    use svbb::edit;
    let temp = tempfile::tempdir().unwrap();
    let rt = Runtime::discover().unwrap();
    let source = temp.path().join("motion-reference.mp4");
    let o = command(&rt.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=320x180:r=30:d=5,format=rgb24,geq=r='X*255/W':g='Y*255/H':b='20+N',setpts='if(lt(N,60),N/(30*TB),(2+(N-60)/15)/TB)'",
            "-fps_mode", "vfr", "-c:v",
            "libx264",
            "-crf",
            "10",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    check(&o, "Generate motion reference").unwrap();
    let m = rt.probe(&source).unwrap();
    let mut p = Project {
        width: 320,
        height: 180,
        fps: Rational::new(30, 1),
        ..Default::default()
    };
    let mut c = Clip::new(&m, 0, 0);
    c.source_in = HZ;
    c.duration = 3 * HZ;
    let id = c.id.clone();
    p.clips.push(c);
    p.media.push(m.clone());
    edit::crop(
        &mut p,
        &id,
        Some(CropRect {
            x: 0,
            y: 0,
            width: 256,
            height: 144,
        }),
    )
    .unwrap();
    edit::animate_frame(&mut p, &id, 0, true).unwrap();
    edit::frame_at(
        &mut p,
        &id,
        3 * HZ,
        CropRect {
            x: 224,
            y: 60,
            width: 96,
            height: 120,
        },
    )
    .unwrap();
    edit::split(&mut p, &id, HZ).unwrap();
    p.validate().unwrap();
    let mut s = RenderSettings::project(&p);
    s.max_bytes = Some(200_000);
    s.encoder = render::Encoder::Software;
    let dest = temp.path().join("animated.mp4");
    render::export(&rt, &p, &s, &dest, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(fs::metadata(&dest).unwrap().len() <= 200_000);
    let output = rt.probe(&dest).unwrap();
    assert!((output.duration - 3 * HZ).abs() < p.fps.frame(2));
    let mut original = Decoder::open(&rt, &m).unwrap();
    let mut encoded = Decoder::open(&rt, &output).unwrap();
    let player = svbb::playback::Player::headless(&rt).unwrap();
    let plan = render::prepare_preview(&rt, &p, &AtomicBool::new(false)).unwrap();
    player.load(plan, 0, false);

    for at in [0, HZ / 2, HZ, HZ + HZ / 2, HZ * 2, HZ * 2 + HZ / 2] {
        let c = p
            .clips
            .iter()
            .find(|c| c.start <= at && c.end() > at)
            .unwrap();
        let source_at = c.source_in + at - c.start;
        let expected = original
            .cropped_frame(source_at, c.crop_at(source_at))
            .unwrap();
        let request = player.seek(at);
        let started = std::time::Instant::now();
        loop {
            assert!(player.take_error().is_none());
            if let Some(frame) = player
                .frame
                .lock()
                .unwrap()
                .as_ref()
                .filter(|f| f.request_id == request)
            {
                assert_eq!(
                    frame.pixels,
                    expected.pixels,
                    "Live timeline preview must evaluate source-time motion at {}",
                    seconds(at)
                );
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "Timed out waiting for animated preview"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let actual = encoded.frame(at).unwrap();
        let mut error = 0_u64;
        let mut count = 0;
        for y in (40..500).step_by(20) {
            for x in (40..920).step_by(20) {
                let i = (y * 960 + x) * 4;
                for channel in 0..3 {
                    error +=
                        expected.pixels[i + channel].abs_diff(actual.pixels[i + channel]) as u64;
                    count += 1;
                }
            }
        }
        assert!(
            (error as f64 / count as f64) < 8.,
            "Preview/export differ at {}: mean error {}",
            seconds(at),
            error as f64 / count as f64
        );
    }
}
