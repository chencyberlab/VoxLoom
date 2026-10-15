CREATE TABLE IF NOT EXISTS asr_progress (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 stream TEXT NOT NULL, covered_ms INTEGER NOT NULL, overlap_text TEXT NOT NULL DEFAULT '',
 PRIMARY KEY(meeting_id,stream)
);
CREATE TABLE IF NOT EXISTS audio_durations (
 track_id TEXT PRIMARY KEY REFERENCES audio_tracks(id) ON DELETE CASCADE,
 duration_ms INTEGER NOT NULL CHECK(duration_ms>0)
);
PRAGMA user_version = 2;
