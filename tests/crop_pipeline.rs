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
