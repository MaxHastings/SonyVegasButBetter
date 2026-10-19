//! Small, local session logs. Logging failure must never stop editing.
use std::{
    fs::{self, File},
    io::{Seek, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

const MAX_BYTES: u64 = 1_048_576;
pub(crate) struct PlaybackLog {
    file: File,
    bytes: u64,
    started: Instant,
    pub path: PathBuf,
}
impl PlaybackLog {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let mut old: Vec<_> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.strip_prefix("playback-")
                    .and_then(|s| s.strip_suffix(".log"))
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
                    && entry.file_type().is_ok_and(|t| t.is_file())
            })
            .collect();
        old.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        for entry in old.iter().take(old.len().saturating_sub(7)) {
            let _ = fs::remove_file(entry.path());
        }
        let path = dir.join(format!("playback-{}.log", uuid::Uuid::new_v4()));
        let file = File::options().write(true).create_new(true).open(&path)?;
        let mut log = Self {
            file,
            bytes: 0,
            started: Instant::now(),
            path,
        };
        log.write(format_args!(
            "Session started, unix {}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        ));
        Ok(log)
    }
    pub fn write(&mut self, message: std::fmt::Arguments<'_>) {
        let mut line = format!(
            "{:.3} SVBB {} {message}\n",
            self.started.elapsed().as_secs_f64(),
            env!("CARGO_PKG_VERSION")
        );
        if line.len() > 8192 {
            let mut end = 8192;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
            line.push('\n');
        }
        if self.bytes + line.len() as u64 > MAX_BYTES {
            if self
                .file
                .set_len(0)
                .and_then(|_| self.file.rewind())
                .is_err()
            {
                return;
            }
            self.bytes = 0;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.bytes += line.len() as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_logs_are_bounded_and_preserve_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let unrelated = dir.path().join("notes.log");
        fs::write(&unrelated, "keep").unwrap();
        for _ in 0..10 {
            let mut log = PlaybackLog::open(dir.path()).unwrap();
            for _ in 0..140 {
                log.write(format_args!("{}", "x".repeat(9000)));
            }
            assert!(fs::metadata(&log.path).unwrap().len() <= MAX_BYTES);
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 9);
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
    }
}
