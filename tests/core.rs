use svbb::{
    edit::{self, History},
    model::*,
    persistence,
};
fn project() -> Project {
    let mut p = Project::default();
    p.media.push(Media {
        id: "media".into(),
        path: "recording.mp4".into(),
        name: "recording".into(),
        duration: HZ * 100,
        video: Some(Stream {
            index: 0,
            codec: "h264".into(),
            timebase: Rational::new(1, 90000),
            start_pts: 0,
            start: 0,
        }),
        audio: Some(Stream {
            index: 1,
            codec: "aac".into(),
            timebase: Rational::new(1, 48000),
            start_pts: 0,
            start: 0,
        }),
        width: 1920,
        height: 1080,
        sample_rate: 48000,
        channels: 2,
        frame_rate: Rational::new(60, 1),
        file_size: 0,
    });
    p
}
fn add(p: &mut Project, lane: usize, start: i64, duration: i64) -> String {
    let mut c = Clip::new(&p.media[0], lane, start * HZ);
    c.duration = duration * HZ;
    let id = c.id.clone();
    p.clips.push(c);
    id
}
#[test]
fn split_trim_undo_atomic() {
    let mut p = project();
    let id = add(&mut p, 0, 0, 10);
    let original = p.clone();
    let mut h = History::default();
    h.apply(&mut p, |p| {
        edit::split(p, &id, HZ * 4)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(p.clips.len(), 2);
    assert_eq!(p.lane(0)[1].source_in, HZ * 4);
    h.undo(&mut p);
    assert_eq!(p, original);
    h.redo(&mut p);
    assert_eq!(p.clips.len(), 2);
    let before = p.clone();
    assert!(
        h.apply(&mut p, |p| edit::trim(p, &id, -1, HZ * 200, false))
            .is_err()
    );
    assert_eq!(p, before);
    h.apply(&mut p, |p| edit::trim(p, &id, HZ, HZ * 2, true))
        .unwrap();
    assert_eq!(p.lane(0)[1].start, HZ * 2);
}
#[test]
fn insert_and_append() {
    let mut p = project();
    p.media[0].duration = HZ * 2;
    let mut h = History::default();
    h.apply(&mut p, |p| {
        edit::append(p, "media", 0, None)?;
        edit::append(p, "media", 0, None)?;
        Ok(())
    })
    .unwrap();
    h.apply(&mut p, |p| {
        edit::append(p, "media", 0, Some(HZ * 2))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        p.lane(0).iter().map(|c| c.start).collect::<Vec<_>>(),
        [0, HZ * 2, HZ * 4]
    );
}
#[test]
fn ripple_spanning_and_locked() {
    let mut p = project();
    add(&mut p, 0, 0, 10);
    add(&mut p, 1, 1, 8);
    let lock = add(&mut p, 2, 2, 7);
    p.tracks[2].locked = true;
    edit::ripple_region(&mut p, HZ * 3, HZ * 6).unwrap();
    p.validate().unwrap();
    assert_eq!(p.duration(), HZ * 9);
    let a = p.lane(1);
    assert_eq!(
        (
            a[0].start,
            a[0].duration,
            a[1].start,
            a[1].source_in,
            a[1].duration
        ),
        (HZ, HZ * 2, HZ * 3, HZ * 5, HZ * 3)
    );
    assert_eq!(p.clips.iter().find(|c| c.id == lock).unwrap().start, HZ * 2);
}
#[test]
fn ripple_intersection_cases() {
    let mut p = project();
    add(&mut p, 0, 0, 2);
    add(&mut p, 0, 2, 2);
    add(&mut p, 0, 4, 2);
    add(&mut p, 0, 6, 2);
    edit::ripple_region(&mut p, HZ, HZ * 5).unwrap();
    p.validate().unwrap();
    let lane = p.lane(0);
    assert_eq!(lane.len(), 3);
    assert_eq!(
        lane.iter().map(|c| c.start).collect::<Vec<_>>(),
        [0, HZ, HZ * 2]
    );
    assert_eq!(lane[1].source_in, HZ);
}
#[test]
fn reorder_move_lock_and_snap() {
    let mut p = project();
    let a = add(&mut p, 0, 0, 2);
    let b = add(&mut p, 0, 2, 3);
    edit::reorder(&mut p, &b, -1).unwrap();
    p.validate().unwrap();
    assert_eq!(p.lane(0)[0].id, b);
    assert_eq!(p.lane(0)[1].id, a);
    assert_eq!(edit::snap(&p, HZ * 3 + 100, 200, None), HZ * 3);
    p.tracks[0].locked = true;
    assert!(edit::move_clip(&mut p, &a, HZ * 10, 0).is_err());
    assert!(edit::delete(&mut p, &a, false).is_err());
}
#[test]
fn close_gaps_preserves_relationships() {
    let mut p = project();
    add(&mut p, 0, 2, 2);
    add(&mut p, 0, 7, 3);
    add(&mut p, 1, 8, 1);
    edit::close_gaps(&mut p).unwrap();
    p.validate().unwrap();
    assert_eq!(
        p.lane(0).iter().map(|c| c.start).collect::<Vec<_>>(),
        [0, HZ * 2]
    );
    assert_eq!(p.lane(1)[0].start, HZ * 3);
}
#[test]
fn exact_time() {
    for fps in [
        Rational::new(24, 1),
        Rational::new(30000, 1001),
        Rational::new(60000, 1001),
        Rational::new(60, 1),
    ] {
        for f in [0, 1, 100, 216000] {
            assert_eq!(fps.align(fps.frame(f)), fps.frame(f));
        }
    }
    assert_eq!(
        Rational::new(1, 48000).to_ticks(48000 * 3600).unwrap(),
        HZ * 3600
    );
    assert_eq!(Rational::new(1, 44100).to_ticks(44100).unwrap(), HZ);
    assert!(Rational::new(1, 0).to_ticks(1).is_err());
}
#[test]
fn gain_matching() {
    assert!((amplitude(-6.) - 0.501187).abs() < 1e-6);
    assert_eq!(match_loudness(-30., -10.).unwrap(), 9.);
    assert_eq!(match_loudness(-45., -30.).unwrap(), 18.);
    assert!(match_loudness(-60., -40.).is_err());
    assert!(match_loudness(f64::NEG_INFINITY, -99.).is_err());
}
#[test]
fn persistence_equivalence_and_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.svbb");
    let mut p = project();
    p.media[0].path = tmp.path().join("recording.mp4");
    let a = add(&mut p, 0, 0, 10);
    p.clips[0].fade_in = HZ;
    p.tracks[1].locked = true;
    p.clips[0].gain_db = -12.;
    persistence::save(&path, &p).unwrap();
    assert_eq!(persistence::load(&path).unwrap(), p);
    let before = p.clone();
    edit::split(&mut p, &a, HZ * 4).unwrap();
    persistence::save(&path, &p).unwrap();
    assert_eq!(persistence::load(&path).unwrap(), p);
    assert_eq!(
        persistence::load(&persistence::backup_path(&path)).unwrap(),
        before
    );
    let mut invalid = p.clone();
    invalid.clips[0].duration = -1;
    assert!(persistence::save(&path, &invalid).is_err());
    assert_eq!(persistence::load(&path).unwrap(), p);
}
#[test]
fn schema_migration_fixture() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("v1.svbb");
    let mut c = rusqlite::Connection::open(&file).unwrap();
    c.execute_batch(include_str!("../fixtures/schema-v1.sql"))
        .unwrap();
    persistence::migrate(&mut c).unwrap();
    let version: i32 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let count: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('clips') WHERE name IN ('fade_in','fade_out')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    persistence::migrate(&mut c).unwrap();
    c.pragma_update(None, "user_version", 999).unwrap();
    assert!(persistence::migrate(&mut c).is_err());
}
#[test]
fn populated_old_project_migrates_without_losing_edits() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("old.svbb");
    let mut p = project();
    p.media[0].path = temp.path().join("source.mp4");
    add(&mut p, 0, 0, 7);
    add(&mut p, 1, 2, 3);
    p.tracks[1].gain_db = -3.;
    persistence::save(&file, &p).unwrap();
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute_batch("ALTER TABLE clips DROP COLUMN fade_in; ALTER TABLE clips DROP COLUMN fade_out; PRAGMA user_version=1;").unwrap();
    drop(conn);
    assert_eq!(persistence::load(&file).unwrap(), p);
    assert!(file.with_extension("v1.backup").is_file());
}
#[test]
fn source_overwrite_and_timestamp_overflow_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.mp4");
    std::fs::write(&source, b"recording").unwrap();
    let mut p = project();
    p.media[0].path = source.clone();
    assert!(persistence::save(&source, &p).is_err());
    assert_eq!(std::fs::read(source).unwrap(), b"recording");
    assert!(Rational::new(i64::MAX, 1).to_ticks(i64::MAX).is_err());
    assert!(match_loudness(10., 100.).is_err());
}
#[test]
fn export_plan_obeys_model() {
    let mut p = project();
    add(&mut p, 0, 0, 4);
    add(&mut p, 1, 1, 2);
    p.clips[1].gain_db = 6.;
    p.clips[1].fade_in = HZ / 2;
    // Planner refuses missing assets instead of producing an unrelated substitute.
    assert!(svbb::render::plan(&p, &svbb::render::RenderSettings::project(&p)).is_err());
}
proptest::proptest! {
    #[test]fn repeated_splits_preserve_total(frames in proptest::collection::vec(1i64..599,0..60)) {
        let mut p=project();add(&mut p,0,0,10);let original=p.clone();let mut h=History::default();let mut successes=0;
        for frame in frames {let at=p.fps.frame(frame);let target=p.clips.iter().find(|c|c.start<at&&c.end()>at).map(|c|c.id.clone());if let Some(id)=target{h.apply(&mut p,|p|{edit::split(p,&id,at)?;Ok(())}).unwrap();successes+=1;}}
        p.validate().unwrap();proptest::prop_assert_eq!(p.clips.iter().map(|c|c.duration).sum::<i64>(),HZ*10);for _ in 0..successes{h.undo(&mut p);}proptest::prop_assert_eq!(p,original);
    }
}
