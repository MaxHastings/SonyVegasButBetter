use crate::model::*;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: i32 = 3;
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(
        version <= SCHEMA_VERSION,
        "Project was created by a newer SVBB (schema {version})"
    );
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn.transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    if version == 0 {
        tx.execute_batch("CREATE TABLE project(id TEXT PRIMARY KEY, name TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL, fps_num INTEGER NOT NULL, fps_den INTEGER NOT NULL, peak_protection INTEGER NOT NULL);
            CREATE TABLE media(id TEXT PRIMARY KEY, path TEXT NOT NULL, metadata TEXT NOT NULL);
            CREATE TABLE tracks(lane INTEGER PRIMARY KEY, name TEXT NOT NULL, gain_db REAL NOT NULL, mute INTEGER NOT NULL, locked INTEGER NOT NULL);
            CREATE TABLE clips(id TEXT PRIMARY KEY, media TEXT NOT NULL REFERENCES media(id), lane INTEGER NOT NULL REFERENCES tracks(lane), start INTEGER NOT NULL, source_in INTEGER NOT NULL, duration INTEGER NOT NULL, gain_db REAL NOT NULL, mute INTEGER NOT NULL);")?;
    }
    if version < 2 {
        tx.execute_batch("ALTER TABLE clips ADD COLUMN fade_in INTEGER NOT NULL DEFAULT 0; ALTER TABLE clips ADD COLUMN fade_out INTEGER NOT NULL DEFAULT 0;")?;
    }
    if version < 3 {
        tx.execute_batch("ALTER TABLE clips ADD COLUMN link TEXT;
            UPDATE clips SET lane=lane+1 WHERE lane>0;
            UPDATE tracks SET lane=3 WHERE lane=2;
            UPDATE tracks SET lane=2 WHERE lane=1;
            INSERT INTO tracks SELECT 1,'A1 · Source audio',gain_db,mute,locked FROM tracks WHERE lane=0;
            UPDATE clips SET link=id WHERE lane=0 AND media IN (SELECT id FROM media WHERE json_extract(metadata,'$.audio') IS NOT NULL);
            INSERT INTO clips(id,media,lane,start,source_in,duration,gain_db,mute,fade_in,fade_out,link)
                SELECT lower(hex(randomblob(16))),media,1,start,source_in,duration,gain_db,mute,fade_in,fade_out,link FROM clips WHERE lane=0 AND link IS NOT NULL;
            UPDATE clips SET gain_db=0,mute=0,fade_in=0,fade_out=0 WHERE lane=0;
            UPDATE tracks SET name='V1 · Video',gain_db=0,mute=0 WHERE lane=0;")?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}
pub fn save(path: &Path, p: &Project) -> Result<()> {
    p.validate()?;
    ensure!(
        !p.media
            .iter()
            .any(|m| crate::media::same_path(path, &m.path)),
        "Cannot overwrite a source recording with a project"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".svbb-save-{}.tmp", id()));
    let result = (|| -> Result<()> {
        let mut c = Connection::open(&tmp)?;
        c.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut c)?;
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO project VALUES (?,?,?,?,?,?,?)",
            params![
                p.id,
                p.name,
                p.width,
                p.height,
                p.fps.num,
                p.fps.den,
                p.peak_protection
            ],
        )?;
        for m in &p.media {
            let stored = m.path.strip_prefix(parent).unwrap_or(&m.path);
            tx.execute(
                "INSERT INTO media VALUES (?,?,?)",
                params![m.id, stored.to_string_lossy(), serde_json::to_string(m)?],
            )?;
        }
        for (lane, t) in p.tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO tracks VALUES (?,?,?,?,?)",
                params![lane, t.name, t.gain_db, t.mute, t.locked],
            )?;
        }
        for v in &p.clips {
            tx.execute(
                "INSERT INTO clips VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    v.id,
                    v.media,
                    v.lane,
                    v.start,
                    v.source_in,
                    v.duration,
                    v.gain_db,
                    v.mute,
                    v.fade_in,
                    v.fade_out,
                    v.link
                ],
            )?;
        }
        tx.commit()?;
        c.close().map_err(|(_, e)| e)?;
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp)?
            .sync_all()?;
        if path.exists() {
            fs::copy(path, backup_path(path))
                .context("Could not preserve previous saved project")?;
        }
        fs::rename(&tmp, path)
            .context("Could not atomically replace project; last saved project is intact")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", path.display()))
}
pub fn load(path: &Path) -> Result<Project> {
    ensure!(path.is_file(), "Project does not exist: {}", path.display());
    let mut c = Connection::open(path)?;
    let check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(check == "ok", "Project database is damaged: {check}");
    let version: i32 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(version > 0, "This is not an SVBB project");
    if version < SCHEMA_VERSION {
        fs::copy(path, path.with_extension(format!("v{version}.backup")))?;
    }
    migrate(&mut c)?;
    let mut p = c.query_row(
        "SELECT id,name,width,height,fps_num,fps_den,peak_protection FROM project",
        [],
        |r| {
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                fps: Rational::new(r.get(4)?, r.get(5)?),
                peak_protection: r.get(6)?,
                ..Default::default()
            })
        },
    )?;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut stmt = c.prepare("SELECT path,metadata FROM media ORDER BY rowid")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (file, json) = row?;
        let mut m: Media = serde_json::from_str(&json)?;
        m.path = PathBuf::from(file);
        if m.path.is_relative() {
            m.path = parent.join(&m.path);
        }
        p.media.push(m);
    }
    let mut tracks = c.prepare("SELECT lane,name,gain_db,mute,locked FROM tracks")?;
    let mut seen = [false; LANES];
    for row in tracks.query_map([], |r| {
        Ok((
            r.get::<_, usize>(0)?,
            Track {
                name: r.get(1)?,
                gain_db: r.get(2)?,
                mute: r.get(3)?,
                locked: r.get(4)?,
            },
        ))
    })? {
        let (i, t) = row?;
        ensure!(i < LANES, "Invalid track index");
        p.tracks[i] = t;
        seen[i] = true;
    }
    ensure!(seen.into_iter().all(|x| x), "Project is missing tracks");
    let mut clips = c.prepare("SELECT id,media,lane,start,source_in,duration,gain_db,mute,fade_in,fade_out,link FROM clips ORDER BY rowid")?;
    p.clips = clips
        .query_map([], |r| {
            Ok(Clip {
                id: r.get(0)?,
                media: r.get(1)?,
                lane: r.get(2)?,
                start: r.get(3)?,
                source_in: r.get(4)?,
                duration: r.get(5)?,
                gain_db: r.get(6)?,
                mute: r.get(7)?,
                fade_in: r.get(8)?,
                fade_out: r.get(9)?,
                link: r.get(10)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    p.validate()?;
    Ok(p)
}
