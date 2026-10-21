use crate::{err, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub duration_ms: i64,
    pub status: String,
    pub notes: String,
    pub summary: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub meeting_id: String,
    pub path: String,
    pub source: String,
    pub offset_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: i64,
    pub meeting_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub source: String,
}

pub struct Db {
    pub conn: Connection,
}
impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(err)?;
        if version > 4 {
            return Err("This library was created by a newer VoxLoom version".into());
        }
        conn.execute_batch(include_str!("../migrations/001.sql"))
            .map_err(err)?;
        conn.execute_batch(include_str!("../migrations/002.sql"))
            .map_err(err)?;
        conn.execute_batch(include_str!("../migrations/003.sql"))
            .map_err(err)?;
        conn.execute_batch(include_str!("../migrations/004.sql"))
            .map_err(err)?;
        conn.execute_batch("PRAGMA secure_delete=ON;")
            .map_err(err)?;
        conn.execute("UPDATE jobs SET status='pending',error='Resuming after interrupted shutdown' WHERE status='running'",[]).map_err(err)?;
        conn.execute(
            "UPDATE meetings SET status='interrupted' WHERE status='recording'",
            [],
        )
        .map_err(err)?;
        Ok(Self { conn })
    }
    pub fn meetings(&self) -> Result<Vec<Meeting>> {
        let mut q=self.conn.prepare("SELECT id,title,created_at,duration_ms,status,notes,summary FROM meetings ORDER BY created_at DESC").map_err(err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Meeting {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    created_at: r.get(2)?,
                    duration_ms: r.get(3)?,
                    status: r.get(4)?,
                    notes: r.get(5)?,
                    summary: r.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn tracks(&self, meeting: &str) -> Result<Vec<Track>> {
        let mut q=self.conn.prepare("SELECT id,meeting_id,path,source,offset_ms FROM audio_tracks WHERE meeting_id=? ORDER BY offset_ms,id").map_err(err)?;
        let rows = q
            .query_map([meeting], |r| {
                Ok(Track {
                    id: r.get(0)?,
                    meeting_id: r.get(1)?,
                    path: r.get(2)?,
                    source: r.get(3)?,
                    offset_ms: r.get(4)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn segments(&self, meeting: &str) -> Result<Vec<Segment>> {
        let mut q=self.conn.prepare("SELECT id,meeting_id,start_ms,end_ms,text,source FROM transcript_segments WHERE meeting_id=? ORDER BY start_ms,id").map_err(err)?;
        let rows = q
            .query_map([meeting], |r| {
                Ok(Segment {
                    id: r.get(0)?,
                    meeting_id: r.get(1)?,
                    start_ms: r.get(2)?,
                    end_ms: r.get(3)?,
                    text: r.get(4)?,
                    source: r.get(5)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn detail(&self, meeting: &str) -> Result<Value> {
        let m = self
            .meetings()?
            .into_iter()
            .find(|m| m.id == meeting)
            .ok_or("Recording not found")?;
        Ok(json!({"meeting":m,"tracks":self.tracks(meeting)?,"segments":self.segments(meeting)?}))
    }
    pub fn create(&self, meeting: &str, title: &str, duration: i64, status: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO meetings(id,title,created_at,duration_ms,status) VALUES(?,?,?,?,?)",
                params![meeting, title, now(), duration, status],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn add_track(&self, meeting: &str, path: &Path, source: &str, offset: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO audio_tracks(id,meeting_id,path,source,offset_ms) VALUES(?,?,?,?,?)",
                params![id(), meeting, path.to_string_lossy(), source, offset],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn jobs(&self) -> Result<Value> {
        let mut q=self.conn.prepare("SELECT id,meeting_id,kind,status,progress,error FROM jobs ORDER BY created_at DESC,rowid DESC LIMIT 100").map_err(err)?;
        let items=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"meeting_id":r.get::<_,Option<String>>(1)?,"kind":r.get::<_,String>(2)?,"status":r.get::<_,String>(3)?,"progress":r.get::<_,f64>(4)?,"error":r.get::<_,Option<String>>(5)?}))).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
        Ok(json!(items))
    }
    pub fn enqueue(&self, meeting: &str, kind: &str) -> Result<String> {
        if !["transcribe", "index", "summarize"].contains(&kind) {
            return Err("Unknown inference job".into());
        }
        let count: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM jobs WHERE status IN ('pending','running')",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        // The cap keeps the user from piling up summaries and re-indexes. A
        // transcription is how a recording's words reach the transcript, so
        // it is never turned away: at most one is pending per recording.
        if count >= 8 && kind != "transcribe" {
            return Err("Inference queue is full (8 jobs). Wait or cancel a job.".into());
        }
        let dup:i64=self.conn.query_row("SELECT count(*) FROM jobs WHERE meeting_id=? AND kind=? AND status IN ('pending','running')",params![meeting,kind],|r|r.get(0)).map_err(err)?;
        if dup > 0 {
            return Err("This job is already queued".into());
        }
        if self.tracks(meeting)?.is_empty() {
            return Err("Recording has no audio tracks".into());
        }
        let job = id();
        self.conn
            .execute(
                "INSERT INTO jobs(id,meeting_id,kind,status,created_at) VALUES(?,?,?,'pending',?)",
                params![job, meeting, kind, now()],
            )
            .map_err(err)?;
        Ok(job)
    }
    /// Transcribe asked to start over (`restart_jobs`): only now, as its job
    /// starts, do the rows it wrote go, with the search index and the
    /// checkpoint. The user's edited rows stay. A Transcribe cancelled before
    /// it started never gets here, so the transcript stays as it was.
    pub fn start_over(&mut self, job: &str, meeting: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(err)?;
        if tx.execute("DELETE FROM restart_jobs WHERE job_id=?", [job]).map_err(err)? > 0 {
            for sql in [
                "DELETE FROM transcript_segments WHERE meeting_id=? AND edited=0",
                "DELETE FROM embedding_items WHERE meeting_id=?",
                "DELETE FROM asr_progress WHERE meeting_id=?",
                "UPDATE audio_tracks SET transcribed=0 WHERE meeting_id=?",
            ] {
                tx.execute(sql, [meeting]).map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }
    pub fn job_status(
        &self,
        job: &str,
        status: &str,
        progress: f64,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE jobs SET status=?,progress=?,error=? WHERE id=?",
                params![status, progress, error, job],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn setting(&self, key: &str, default: &str) -> String {
        self.conn
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .unwrap_or_else(|_| default.to_string())
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_fts_edit_and_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let db = Db::open(&path).unwrap();
        db.create("a", "Meeting", 10000, "ready").unwrap();
        db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('a',0,1000,'the budget was approved','mic')",[]).unwrap();
        let count = |term: &str| {
            db.conn
                .query_row(
                    "SELECT count(*) FROM transcript_fts WHERE transcript_fts MATCH ?",
                    [term],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(count("budget"), 1);
        db.conn
            .execute(
                "UPDATE transcript_segments SET text='the launch was postponed'",
                [],
            )
            .unwrap();
        assert_eq!(count("budget"), 0);
        assert_eq!(count("launch"), 1);
        db.conn.execute("INSERT INTO jobs(id,meeting_id,kind,status,created_at) VALUES('j','a','index','pending',0)",[]).unwrap();
        db.conn
            .execute("DELETE FROM meetings WHERE id='a'", [])
            .unwrap();
        assert_eq!(count("launch"), 0);
        assert_eq!(db.jobs().unwrap(), json!([]));
        drop(db);
        assert!(Db::open(&path).unwrap().meetings().unwrap().is_empty());
    }

    #[test]
    fn a_queued_live_pass_from_an_older_version_becomes_a_transcription() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let db = Db::open(&path).unwrap();
            for id in ["a", "b", "c"] {
                db.create(id, "Meeting", 10000, "ready").unwrap();
            }
            for (id, meeting, kind, status) in [("1", "a", "transcribe_live", "pending"), ("2", "b", "transcribe_live", "running"),
                                                ("3", "b", "transcribe", "pending"), ("4", "c", "transcribe_live", "failed")] {
                db.conn.execute("INSERT INTO jobs(id,meeting_id,kind,status,created_at) VALUES(?,?,?,?,0)", params![id, meeting, kind, status]).unwrap();
            }
            db.conn.execute_batch("PRAGMA user_version = 2;").unwrap();
        }
        let db = Db::open(&path).unwrap();
        let jobs = db.conn.prepare("SELECT id,kind,status FROM jobs ORDER BY id").unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap()
            .collect::<std::result::Result<Vec<_>, _>>().unwrap();
        // One pending transcription per recording; the old failure is gone.
        assert_eq!(jobs, [("1".into(), "transcribe".into(), "pending".into()), ("3".into(), "transcribe".into(), "pending".into())]);
        assert_eq!(db.conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 4);
        assert!(db.enqueue("a", "transcribe_live").is_err());
    }

    #[test]
    fn summaries_from_an_older_version_move_below_the_notes_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let db = Db::open(&path).unwrap();
            for (id, notes, summary) in [("both", "My notes.", "## Overview\nSummary.\n"), ("only", "", "## Overview\nJust a summary."),
                                         ("none", "Notes only.", ""), ("draft", "Mine.", "<|channel>thought half")] {
                db.create(id, "Meeting", 1000, "ready").unwrap();
                db.conn.execute("UPDATE meetings SET notes=?,summary=? WHERE id=?", params![notes, summary, id]).unwrap();
            }
            db.conn.execute_batch("PRAGMA user_version = 3;").unwrap();
        }
        let read = |db: &Db| -> Vec<(String, String, String)> {
            db.conn.prepare("SELECT id,notes,summary FROM meetings ORDER BY id").unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
                .collect::<std::result::Result<Vec<_>, _>>().unwrap()
        };
        let expected = vec![
            ("both".into(), "My notes.\n\n# Summary\n\n## Overview\nSummary.".into(), "".into()),
            ("draft".into(), "Mine.".into(), "<|channel>thought half".into()),
            ("none".into(), "Notes only.".into(), "".into()),
            ("only".into(), "# Summary\n\n## Overview\nJust a summary.".into(), "".into()),
        ];
        assert_eq!(read(&Db::open(&path).unwrap()), expected);
        // Every open runs the migrations: the second one changes nothing.
        assert_eq!(read(&Db::open(&path).unwrap()), expected);
    }

    #[test]
    fn full_queue_never_turns_away_a_transcription() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("library.db")).unwrap();
        for i in 0..9 {
            let id = format!("m{i}");
            db.create(&id, "Meeting", 10000, "ready").unwrap();
            db.add_track(&id, &dir.path().join(format!("{id}.flac")), "mic", 0).unwrap();
        }
        for i in 0..8 {
            db.enqueue(&format!("m{i}"), "summarize").unwrap();
        }
        assert!(db.enqueue("m8", "summarize").is_err());
        assert!(db.enqueue("m8", "index").is_err());
        db.enqueue("m8", "transcribe").unwrap();
        // Still one per recording: the second is the duplicate, not the cap.
        assert!(db.enqueue("m8", "transcribe").unwrap_err().contains("already queued"));
    }
}
