PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS meetings (
 id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL,
 duration_ms INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'ready',
 notes TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS audio_tracks (
 id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 path TEXT NOT NULL, source TEXT NOT NULL, offset_ms INTEGER NOT NULL DEFAULT 0,
 transcribed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS transcript_segments (
 id INTEGER PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL, source TEXT NOT NULL,
 edited INTEGER NOT NULL DEFAULT 0, CHECK(end_ms >= start_ms AND start_ms >= 0)
);
CREATE INDEX IF NOT EXISTS segment_time ON transcript_segments(meeting_id, start_ms);
CREATE VIRTUAL TABLE IF NOT EXISTS transcript_fts USING fts5(text, content='transcript_segments',content_rowid='id');
CREATE TRIGGER IF NOT EXISTS segment_insert AFTER INSERT ON transcript_segments BEGIN
 INSERT INTO transcript_fts(rowid,text) VALUES(new.id,new.text); END;
CREATE TRIGGER IF NOT EXISTS segment_delete AFTER DELETE ON transcript_segments BEGIN
 INSERT INTO transcript_fts(transcript_fts,rowid,text) VALUES('delete',old.id,old.text); END;
CREATE TRIGGER IF NOT EXISTS segment_update AFTER UPDATE OF text ON transcript_segments BEGIN
 INSERT INTO transcript_fts(transcript_fts,rowid,text) VALUES('delete',old.id,old.text);
 INSERT INTO transcript_fts(rowid,text) VALUES(new.id,new.text); END;
CREATE TABLE IF NOT EXISTS embedding_items (
 id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 track_id TEXT REFERENCES audio_tracks(id) ON DELETE CASCADE,
 modality TEXT NOT NULL CHECK(modality IN ('text','audio')), start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
 text TEXT NOT NULL DEFAULT '', model_id TEXT NOT NULL, revision TEXT NOT NULL, backend TEXT NOT NULL,
 dimensions INTEGER NOT NULL, chunk_version INTEGER NOT NULL,
 UNIQUE(meeting_id,track_id,modality,start_ms,end_ms,revision,chunk_version)
);
CREATE TABLE IF NOT EXISTS embedding_vectors (
 item_id TEXT PRIMARY KEY REFERENCES embedding_items(id) ON DELETE CASCADE, vector BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS jobs (
 id TEXT PRIMARY KEY, meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE, kind TEXT NOT NULL,
 status TEXT NOT NULL, progress REAL NOT NULL DEFAULT 0, error TEXT, created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS model_registry (id TEXT PRIMARY KEY, revision TEXT NOT NULL, path TEXT NOT NULL, dimensions INTEGER);
PRAGMA user_version = 1;
