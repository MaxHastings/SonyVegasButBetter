# SVBB project format

An `.svbb` file is an ordinary SQLite database, independent of the UI and media
implementation. Integers store ticks at 282,240,000 Hz. UTF-8 paths and stable
UUID strings identify sources/entities. `PRAGMA user_version` is the schema version.

Current version: **2**.

| Table | Authoritative fields |
|---|---|
| project | id, name, width, height, fps_num, fps_den, peak_protection |
| media | id, path, metadata JSON |
| tracks | lane (0..2), name, gain_db, mute, locked |
| clips | id, media FK, lane FK, start, source_in, duration, gain_db, mute, fade_in, fade_out |

Media metadata JSON contains the plain Rust Media representation: duration,
dimensions, file size, sample rate/channels, rational frame rate, optional first
video/audio Stream objects (index, codec name, rational timebase, original
start_pts, start offset relative to the container). Its path field is redundant;
the table's path column is authoritative when loading. Source paths within the
project directory are stored relatively; other paths remain absolute.

No thumbnails, waveforms, PCM caches, UI objects or native decoder structures are
stored here. A future implementation can understand edits using these tables,
the timebase and the processing semantics documented in ARCHITECTURE.md.

## Compatibility and migration

Version 1 had the same tables without clip fade columns. Migration 1→2 adds
`fade_in` and `fade_out`, default zero, in a transaction. Version 0 is initialized
only while creating a fresh save; opening an arbitrary version-0 database fails.
Newer unknown versions are rejected without mutation. Before migrating an older
project, the loader preserves a `.vN.backup` file. Migrations are explicit code;
fixtures/schema-v1.sql and automated tests pin the compatibility behavior.

Any future semantic change must migrate stored values or preserve the old
interpretation. Adding a UI feature alone must not reinterpret existing edits.

## Failure-safe saves and recovery

Save creates a new sibling SQLite database, commits all tables in one transaction,
closes and syncs it, copies the previous saved file to `.bak`, then atomically
renames the new file over the destination. Failed writes do not partially update
the old document. Temporary files are removed after handled errors. Sudden power
loss durability ultimately depends on the OS/filesystem; no power-loss claim is
made from normal-save testing.

Recovery saves every 15 seconds while dirty to an individual session file and is separate from manual project
files. Startup offers recovery; Save As turns recovered work into a normal project.
Recovery never automatically overwrites the last good manual save. A backup is
an ordinary project and can be opened directly. Missing source files do not prevent
loading/editing a project, but preview/export require relinking.
