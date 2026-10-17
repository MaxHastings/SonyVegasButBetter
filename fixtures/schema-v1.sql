-- Compatibility fixture: the original schema has no fades. Migration 2 supplies zero.
PRAGMA user_version=1;
CREATE TABLE project(id TEXT PRIMARY KEY,name TEXT,width INTEGER,height INTEGER,fps_num INTEGER,fps_den INTEGER,peak_protection INTEGER);
CREATE TABLE media(id TEXT PRIMARY KEY,path TEXT,metadata TEXT);
CREATE TABLE tracks(lane INTEGER PRIMARY KEY,name TEXT,gain_db REAL,mute INTEGER,locked INTEGER);
CREATE TABLE clips(id TEXT PRIMARY KEY,media TEXT,lane INTEGER,start INTEGER,source_in INTEGER,duration INTEGER,gain_db REAL,mute INTEGER);
