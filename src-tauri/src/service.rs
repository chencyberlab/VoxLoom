use crate::{
    audio::{self, Capture},
    db::{self, Db},
    err,
    platform::{self, private_dir},
    process, search, Result,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub struct Service {
    pub db: Mutex<Db>,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub resources: PathBuf,
    capture: Mutex<Option<Capture>>,
    running: Mutex<Option<(String, u32)>>,
    inference: Mutex<()>,
    quit: AtomicBool,
    setup_active: AtomicBool,
    setup_cancel: AtomicBool,
    live_poll: Mutex<Instant>,
    pub notify: Arc<dyn Fn(Value) + Send + Sync>,
}
/// How often a recording's newest audio is offered to live transcription.
const LIVE_POLL: Duration = Duration::from_millis(2500);
/// Name of a recording started without a title; Summarise may replace it.
const DEFAULT_TITLE: &str = "Untitled recording";
/// Recording refuses to start below this much free space on the data volume.
const MIN_RECORD_FREE_BYTES: u64 = 256 * 1024 * 1024;
/// A worker never runs longer than this, or twice the recording's length
/// when that is more: a two-hour cap ended summaries of long recordings on
/// slow machines half-way.
const WORKER_TIMEOUT: Duration = Duration::from_secs(7200);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accurate_transcription_falls_back_until_downloaded() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let models = temp.path().join("data/models");
        fs::create_dir_all(&models).unwrap();
        fs::write(models.join("ggml-base.en.bin"), b"base").unwrap();
        assert!(service
            .dispatch("save_settings", json!({"transcription_profile":"anything"}))
            .is_err());
        service
            .dispatch("save_settings", json!({"transcription_profile":"accurate"}))
            .unwrap();
        let chosen = |s: &Service| s.request("transcribe", None).unwrap()["models"]["whisper"].clone();
        assert!(chosen(&service).as_str().unwrap().ends_with("ggml-base.en.bin"));
        fs::write(models.join("ggml-small.en-q8_0.bin"), b"small").unwrap();
        assert!(chosen(&service).as_str().unwrap().ends_with("ggml-small.en-q8_0.bin"));
        service
            .dispatch("save_settings", json!({"transcription_profile":"standard"}))
            .unwrap();
        assert!(chosen(&service).as_str().unwrap().ends_with("ggml-base.en.bin"));
        service.shutdown();
    }

    #[test]
    fn summary_title_replaces_only_the_default_name() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        {
            let db = service.db.lock().unwrap();
            db.create("untitled", DEFAULT_TITLE, 1000, "ready").unwrap();
            db.create("named", "Board meeting", 1000, "ready").unwrap();
        }
        let summary = json!({"type":"summary","text":"Overview\nNotes.","title":"Privacy workshop planning"});
        service.ingest("untitled", &summary).unwrap();
        service.ingest("named", &summary).unwrap();
        service.ingest("named", &json!({"type":"summary","text":"Overview\nNotes.","title":""})).unwrap();
        let title = |id: &str| -> String {
            service.db.lock().unwrap().conn.query_row("SELECT title FROM meetings WHERE id=?", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(title("untitled"), "Privacy workshop planning");
        assert_eq!(title("named"), "Board meeting");
        service.shutdown();
    }

    #[test]
    fn summary_profiles_pick_their_pinned_model() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let chosen = |s: &Service| s.request("summarize", None).unwrap()["models"]["summary"].clone();
        assert!(chosen(&service).as_str().unwrap().ends_with("gemma-4-E2B-it-Q4_0.gguf"));
        // Gemma 3 1B was removed: a library that still names it uses E2B.
        service.db.lock().unwrap().set_setting("summary_profile", "1b").unwrap();
        assert!(chosen(&service).as_str().unwrap().ends_with("gemma-4-E2B-it-Q4_0.gguf"));
        assert_eq!(service.dispatch("settings", json!({})).unwrap()["summary_profile"], "e2b");
        assert!(service.dispatch("save_settings", json!({"summary_profile":"1b"})).is_err());
        for (profile, filename) in [
            ("e2b", "gemma-4-E2B-it-Q4_0.gguf"),
            ("e4b", "gemma-4-E4B-it-Q4_0.gguf"),
        ] {
            service
                .dispatch("save_settings", json!({"summary_profile":profile}))
                .unwrap();
            assert!(chosen(&service).as_str().unwrap().ends_with(filename), "{profile}");
        }
        assert!(service
            .dispatch("save_settings", json!({"summary_profile":"e8b"}))
            .is_err());
        let summary = |s: &Arc<Service>| s.request("summarize", None).unwrap();
        assert_eq!(summary(&service)["summary_style"], "counselling");
        assert!(service.request("transcribe", None).unwrap()["summary_prompt"].is_null());
        service
            .dispatch("save_settings", json!({"summary_style":"custom","summary_prompt":"List the client's goals."}))
            .unwrap();
        assert_eq!(summary(&service)["summary_style"], "custom");
        assert_eq!(summary(&service)["summary_prompt"], "List the client's goals.");
        assert!(service.dispatch("save_settings", json!({"summary_style":"poetry"})).is_err());
        assert!(service.dispatch("save_settings", json!({"summary_prompt":"x".repeat(2001)})).is_err());
        service.shutdown();
    }

    #[test]
    fn word_export_runs_the_helper_interpreter() {
        // Word export holds the library lock while it starts Python; on
        // Windows the interpreter comes from settings, so it must not lock again.
        let temp = tempfile::tempdir().unwrap();
        let resources = temp.path().join("resources");
        fs::create_dir_all(resources.join("worker")).unwrap();
        for name in ["export_docx.py", "notes.py"] {
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../worker").join(name),
                resources.join("worker").join(name),
            )
            .unwrap();
        }
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            resources,
            Arc::new(|_| {}),
        )
        .unwrap();
        {
            let db = service.db.lock().unwrap();
            db.create("meeting", "Café notes", 1000, "ready").unwrap();
            db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('meeting',0,1000,'Zoë approved the budget — on Friday.','mic')", []).unwrap();
        }
        let target = temp.path().join("exports").join("Café notes.docx");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        service
            .dispatch("export", json!({"id":"meeting","path":target,"format":"docx"}))
            .unwrap();
        assert!(fs::read(&target).unwrap().starts_with(b"PK"));
        service.shutdown();
    }

    #[test]
    fn appearance_is_system_light_or_dark() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        assert_eq!(service.dispatch("settings", json!({})).unwrap()["appearance"], "system");
        for value in ["light", "dark", "system"] {
            service.dispatch("save_settings", json!({"appearance": value})).unwrap();
            assert_eq!(service.dispatch("settings", json!({})).unwrap()["appearance"], value);
        }
        assert!(service.dispatch("save_settings", json!({"appearance": "grove"})).is_err());
        assert_eq!(service.dispatch("settings", json!({})).unwrap()["appearance"], "system");
        service.shutdown();
    }

    #[test]
    fn removing_models_frees_only_pinned_files_when_idle() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let service = Service::open(
            temp.path().join("data"),
            cache.clone(),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let models = temp.path().join("data/models");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(models.join("ggml-base.en.bin"), b"base").unwrap();
        fs::write(models.join("gemma-4-E4B-it-Q4_0.gguf.part"), b"half").unwrap();
        fs::write(models.join("embeddinggemma-2-740m.litertlm"), b"embed").unwrap();
        fs::write(models.join("my-own.gguf"), b"custom").unwrap();
        fs::write(models.join("gemma-3-1b-it-qat-Q4_0.gguf"), b"old 1b").unwrap();
        let compiled = cache.join("embeddinggemma-2-740m.litertlm_1_5.text_encoder.xnnpack_cache");
        fs::write(&compiled, b"compiled").unwrap();
        fs::write(cache.join("last-inference-metrics.json"), b"{}").unwrap();
        let on_disk = |s: &Arc<Service>, id: &str| {
            s.dispatch("models", json!({})).unwrap()["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == id)
                .unwrap()["disk_bytes"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(on_disk(&service, "summary_e4b"), 4);
        // A retired model is listed only while its file is on disk.
        assert_eq!(on_disk(&service, "summary_1b"), 6);
        let remove = |s: &Arc<Service>, ids: Value| s.dispatch("remove_models", json!({"ids":ids}));
        assert!(remove(&service, json!([])).is_err());
        assert!(remove(&service, json!(["whisper", "my-own"])).is_err());
        assert!(models.join("ggml-base.en.bin").exists());
        {
            let _busy = service.inference.lock().unwrap();
            assert!(remove(&service, json!(["whisper"])).is_err());
        }
        // Running, not pending, so the scheduler thread leaves it alone.
        service.db.lock().unwrap().conn.execute("INSERT INTO jobs(id,meeting_id,kind,status,created_at) VALUES('job',NULL,'summarize','running',0)", []).unwrap();
        assert!(remove(&service, json!(["whisper"])).is_err());
        service
            .db
            .lock()
            .unwrap()
            .conn
            .execute("UPDATE jobs SET status='cancelled'", [])
            .unwrap();
        let freed = remove(&service, json!(["whisper", "summary_e4b", "embedding", "summary_1b"])).unwrap();
        assert_eq!(freed["freed_bytes"], 4 + 4 + 5 + 8 + 6);
        assert!(!models.join("gemma-3-1b-it-qat-Q4_0.gguf").exists());
        assert!(!service.dispatch("models", json!({})).unwrap()["models"].as_array().unwrap().iter().any(|m| m["id"] == "summary_1b"));
        assert!(!models.join("ggml-base.en.bin").exists());
        assert!(!models.join("gemma-4-E4B-it-Q4_0.gguf.part").exists());
        assert!(!models.join("embeddinggemma-2-740m.litertlm").exists());
        assert!(!compiled.exists());
        assert!(models.join("my-own.gguf").exists());
        assert!(cache.join("last-inference-metrics.json").exists());
        assert_eq!(on_disk(&service, "summary_e4b"), 0);
        service.shutdown();
    }

    #[test]
    fn asr_batches_checkpoint_atomically_resume_and_preserve_edits() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        {
            let db = service.db.lock().unwrap();
            db.create("meeting", "Fixture", 60000, "ready").unwrap();
            db.add_track(
                "meeting",
                &temp.path().join("s0000-system-000000.flac"),
                "system",
                0,
            )
            .unwrap();
            db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source,edited) VALUES('meeting',1000,2000,'My correction','system',1)",[]).unwrap();
        }
        let good = json!({"type":"asr_batch","stream":"system:s0000","covered_ms":28000,"overlap_text":"","segments":[{"start_ms":1000,"end_ms":2000,"text":"Model replacement","source":"system"},{"start_ms":3000,"end_ms":4000,"text":"A complete sentence.","source":"system"},{"start_ms":3000,"end_ms":4500,"text":"Another sentence with an imprecise timestamp.","source":"system"}]});
        service.ingest("meeting", &good).unwrap();
        service.ingest("meeting", &good).unwrap();
        let mut invalid = good.clone();
        invalid["covered_ms"] = json!(50000);
        invalid["segments"] = json!([{"start_ms":5000,"end_ms":6000,"text":"Must roll back","source":"system"},{"start_ms":9000,"end_ms":8000,"text":"Invalid range","source":"system"}]);
        assert!(service.ingest("meeting", &invalid).is_err());
        let request = service.request("transcribe_live", Some("meeting")).unwrap();
        assert_eq!(request["asr_progress"]["system:s0000"]["covered_ms"], 28000);
        assert_eq!(request["finalize"], true);
        assert_eq!(request["context_tracks"].as_array().unwrap().len(), 1);
        assert_eq!(request["segments"].as_array().unwrap().len(), 3);
        let segments = request["segments"].to_string();
        assert!(segments.contains("My correction"));
        assert!(!segments.contains("Model replacement") && !segments.contains("Must roll back"));
        service.shutdown();
        drop(service);
        let reopened = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        assert_eq!(
            reopened
                .request("transcribe_live", Some("meeting"))
                .unwrap()["asr_progress"]["system:s0000"]["covered_ms"],
            28000
        );
        {
            let db = reopened.db.lock().unwrap();
            db.conn
                .execute("DELETE FROM meetings WHERE id='meeting'", [])
                .unwrap();
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM asr_progress", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        reopened.shutdown();
    }

    fn live_jobs(service: &Service, meeting: &str, status: Option<&str>) -> i64 {
        service.db.lock().unwrap().conn.query_row(
            "SELECT count(*) FROM jobs WHERE meeting_id=? AND kind='transcribe_live' AND (?2 IS NULL OR status=?2)",
            params![meeting, status],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn live_request_registers_segments_closed_since_the_last_tick() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        // Stop the scheduler so only request() can register the segment.
        service.quit.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(400));
        let folder = temp.path().join("data/recordings/meeting");
        fs::create_dir_all(&folder).unwrap();
        for name in ["s0000-mic-000000.flac", "s0000-mic-000001.flac"] {
            fs::write(folder.join(name), b"").unwrap();
        }
        service.db.lock().unwrap().create("meeting", DEFAULT_TITLE, 0, "recording").unwrap();
        *service.capture.lock().unwrap() =
            Some(Capture::new("meeting".into(), folder.clone(), Some("mic".into()), None));
        let request = service.request("transcribe_live", Some("meeting")).unwrap();
        assert_eq!(request["finalize"], false);
        let paths = |key: &str| {
            request[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["path"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths("context_tracks").len(), 1);
        assert!(paths("context_tracks")[0].ends_with("s0000-mic-000000.flac"));
        assert_eq!(paths("tracks"), paths("context_tracks"));
        assert_eq!(request["context_tracks"][0]["offset_ms"], 0);
        // A second request finds the segment registered, not registered twice.
        service.request("transcribe_live", Some("meeting")).unwrap();
        assert_eq!(service.db.lock().unwrap().tracks("meeting").unwrap().len(), 1);
        *service.capture.lock().unwrap() = None;
    }

    #[test]
    fn pause_and_stop_both_queue_the_final_live_pass() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        // Holding the inference lock keeps the scheduler from running the job.
        let _busy = service.inference.lock().unwrap();
        fs::write(temp.path().join("data/models/ggml-base.en.bin"), b"base").unwrap();
        let folder = temp.path().join("data/recordings/meeting");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("s0000-system-000000.flac"), b"").unwrap();
        service.db.lock().unwrap().create("meeting", DEFAULT_TITLE, 0, "recording").unwrap();
        *service.capture.lock().unwrap() =
            Some(Capture::new("meeting".into(), folder, None, Some("monitor".into())));
        service.finish_capture(false).unwrap();
        assert!(service.capture.lock().unwrap().as_ref().unwrap().paused);
        assert_eq!(service.db.lock().unwrap().tracks("meeting").unwrap().len(), 1);
        assert_eq!(live_jobs(&service, "meeting", Some("pending")), 1);
        // The pass after Pause finalises: nothing is read as a growing file.
        let request = service.request("transcribe_live", Some("meeting")).unwrap();
        assert_eq!(request["finalize"], true);
        assert!(request["growing"].as_array().unwrap().is_empty());
        service.finish_capture(true).unwrap();
        assert!(service.capture.lock().unwrap().is_none());
        assert_eq!(live_jobs(&service, "meeting", None), 1);
        drop(_busy);
        service.shutdown();
    }

    #[test]
    fn interrupted_recordings_resume_transcription_and_regain_their_length() {
        let temp = tempfile::tempdir().unwrap();
        let open = || {
            Service::open(
                temp.path().join("data"),
                temp.path().join("cache"),
                temp.path().into(),
                Arc::new(|_| {}),
            )
            .unwrap()
        };
        let service = open();
        fs::write(temp.path().join("data/models/ggml-base.en.bin"), b"base").unwrap();
        let folder = temp.path().join("data/recordings/lost");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("session-0000.json"), br#"{"offset_ms":0}"#).unwrap();
        for name in ["s0000-mic-000000.flac", "s0000-mic-000001.flac"] {
            fs::write(folder.join(name), b"").unwrap();
        }
        {
            let db = service.db.lock().unwrap();
            // The host died mid-recording: the row still says "recording".
            db.create("lost", DEFAULT_TITLE, 0, "recording").unwrap();
            db.create("done", DEFAULT_TITLE, 5000, "ready").unwrap();
            db.add_track("done", &temp.path().join("s0000-mic-000000.flac"), "mic", 0).unwrap();
            for id in ["half", "clean"] {
                fs::create_dir_all(temp.path().join("data/recordings").join(id)).unwrap();
            }
            // Every file was read, but the last pass carried an unfinished sentence.
            db.create("half", DEFAULT_TITLE, 0, "interrupted").unwrap();
            db.add_track("half", &temp.path().join("half.flac"), "mic", 0).unwrap();
            db.conn.execute("UPDATE audio_tracks SET transcribed=1 WHERE meeting_id='half'", []).unwrap();
            db.conn.execute(r#"INSERT INTO asr_progress(meeting_id,stream,covered_ms,overlap_text) VALUES('half','mic:s0000',28000,'{"floor_db":-50.0,"pending":{"start_ms":27000,"end_ms":28000,"text":"and so we"}}')"#, []).unwrap();
            // Fully transcribed and flushed: nothing is owed.
            db.create("clean", DEFAULT_TITLE, 0, "interrupted").unwrap();
            db.add_track("clean", &temp.path().join("clean.flac"), "mic", 0).unwrap();
            db.conn.execute("UPDATE audio_tracks SET transcribed=1 WHERE meeting_id='clean'", []).unwrap();
            db.conn.execute(r#"INSERT INTO asr_progress(meeting_id,stream,covered_ms,overlap_text) VALUES('clean','mic:s0000',28000,'{"floor_db":-50.0}')"#, []).unwrap();
        }
        service.shutdown();
        drop(service);
        let service = open();
        let tracks = service.db.lock().unwrap().tracks("lost").unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].offset_ms, 30000);
        // Queued at start-up; the scheduler may already have tried it, so any
        // status counts. A new library with nothing to recover queues nothing.
        assert_eq!(live_jobs(&service, "lost", None), 1);
        assert_eq!(live_jobs(&service, "half", None), 1);
        assert_eq!(live_jobs(&service, "clean", None), 0);
        assert_eq!(live_jobs(&service, "done", None), 0);
        let duration = |id: &str| -> i64 {
            service.db.lock().unwrap().conn.query_row("SELECT duration_ms FROM meetings WHERE id=?", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(duration("lost"), 0);
        service.ingest("lost", &json!({"type":"track_duration","track_id":tracks[1].id,"duration_ms":12000})).unwrap();
        service.ingest("lost", &json!({"type":"track_duration","track_id":tracks[0].id,"duration_ms":30000})).unwrap();
        assert_eq!(duration("lost"), 42000);
        let done = service.db.lock().unwrap().tracks("done").unwrap();
        service.ingest("done", &json!({"type":"track_duration","track_id":done[0].id,"duration_ms":90000})).unwrap();
        assert_eq!(duration("done"), 5000);
        service.shutdown();
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(|| format!("Missing {key}"))
}
/// The chosen summary model. Gemma 3 1B was removed in 0.1.8; libraries
/// that still name it use E2B, the new default.
fn summary_profile(db: &Db) -> String {
    match db.setting("summary_profile", "e2b").as_str() {
        p @ ("e4b" | "custom") => p.to_string(),
        _ => "e2b".to_string(),
    }
}
/// Where provision.py keeps a download until its SHA-256 is verified.
fn part_path(model: &Path) -> PathBuf {
    let mut name = model.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}
struct ScratchDir(PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Service {
    pub fn open(
        data: PathBuf,
        cache: PathBuf,
        resources: PathBuf,
        notify: Arc<dyn Fn(Value) + Send + Sync>,
    ) -> Result<Arc<Self>> {
        for p in [
            &data,
            &cache,
            &data.join("recordings"),
            &data.join("models"),
        ] {
            private_dir(p)?;
        }
        for entry in fs::read_dir(data.join("recordings")).map_err(err)?.flatten() {
            if entry.file_name().to_string_lossy().starts_with(".deleted-") {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        // Interrupted jobs can leave private request/chunk scratch files. They
        // are disposable and never serve as a second recording archive.
        for entry in fs::read_dir(&cache).map_err(err)? {
            let path = entry.map_err(err)?.path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() && (name.starts_with("request-") || name.starts_with("job-")) {
                fs::remove_dir_all(path).map_err(err)?;
            }
        }
        let db = Db::open(&data.join("library.db"))?;
        platform::private_file(&data.join("library.db"))?;
        let s = Arc::new(Self {
            db: Mutex::new(db),
            data,
            cache,
            resources,
            capture: Mutex::new(None),
            running: Mutex::new(None),
            inference: Mutex::new(()),
            quit: AtomicBool::new(false),
            setup_active: AtomicBool::new(false),
            setup_cancel: AtomicBool::new(false),
            live_poll: Mutex::new(Instant::now()),
            notify,
        });
        s.recover_tracks()?;
        let weak = Arc::downgrade(&s);
        std::thread::spawn(move || loop {
            let Some(s) = weak.upgrade() else {
                break;
            };
            if s.quit.load(Ordering::SeqCst) {
                break;
            }
            s.tick();
            drop(s);
            std::thread::sleep(Duration::from_millis(250));
        });
        Ok(s)
    }
    /// Registers the segment files of recordings that ended in a crash or
    /// power loss and queues the transcription they were still owed. Their
    /// audio is complete on disk; only the host stopped.
    fn recover_tracks(&self) -> Result<()> {
        let db = self.db.lock().unwrap();
        for meeting in db.meetings()?.iter().filter(|m| m.status == "interrupted") {
            let dir = self.data.join("recordings").join(&meeting.id);
            let existing = db.tracks(&meeting.id)?;
            if !dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(dir).map_err(err)? {
                let path = entry.map_err(err)?.path();
                let name = path.file_name().unwrap().to_string_lossy();
                if path.extension().and_then(|s| s.to_str()) != Some("flac")
                    || existing.iter().any(|t| t.path == path.to_string_lossy())
                {
                    continue;
                }
                let index = name
                    .trim_end_matches(".flac")
                    .rsplit('-')
                    .next()
                    .and_then(|x| x.parse::<i64>().ok())
                    .unwrap_or(0);
                db.add_track(
                    &meeting.id,
                    &path,
                    if name.contains("-mic-") {
                        "mic"
                    } else {
                        "system"
                    },
                    {
                        let session = name
                            .split('-')
                            .next()
                            .unwrap_or("s0000")
                            .trim_start_matches('s');
                        let offset = fs::read_to_string(
                            path.parent()
                                .unwrap()
                                .join(format!("session-{session}.json")),
                        )
                        .ok()
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                        .and_then(|v| v["offset_ms"].as_i64())
                        .unwrap_or(0);
                        offset + index * 30000
                    },
                )?;
            }
            // Audio that no live pass has read yet (including the file that
            // was still being written), or a sentence left unfinished by the
            // last pass, still needs a final pass or its words never reach
            // the transcript.
            let owed: i64 = db
                .conn
                .query_row(
                    "SELECT (SELECT count(*) FROM audio_tracks WHERE meeting_id=?1 AND transcribed=0)+(SELECT count(*) FROM asr_progress WHERE meeting_id=?1 AND overlap_text LIKE '%\"pending\"%')",
                    [&meeting.id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if owed > 0 && self.whisper_model(&db).is_file() {
                let _ = db.enqueue(&meeting.id, "transcribe_live");
            }
        }
        Ok(())
    }
    /// The selected transcription model. Accurate falls back to the standard
    /// model until it is downloaded, so live transcription never stops.
    fn whisper_model(&self, db: &Db) -> PathBuf {
        let models =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        let accurate = models.join("ggml-small.en-q8_0.bin");
        if db.setting("transcription_profile", "standard") == "accurate" && accurate.is_file() {
            accurate
        } else {
            models.join("ggml-base.en.bin")
        }
    }
    fn python(&self) -> String {
        self.db
            .lock()
            .unwrap()
            .setting("python", &self.default_tool("python"))
    }
    /// The interpreter for setup and Word export, which need only the standard
    /// library. Linux uses the system python3, which also creates the LiteRT
    /// venv; the Windows package bundles one interpreter for everything.
    /// Takes the locked library: callers already hold it.
    fn helper_python(&self, db: &Db) -> String {
        if cfg!(windows) {
            db.setting("python", &self.default_tool("python"))
        } else {
            "python3".into()
        }
    }
    fn default_tool(&self, tool: &str) -> String {
        let (dev, installed, fallback) = match tool {
            "python" => (
                platform::venv_python(Path::new(".venv")),
                platform::venv_python(Path::new("runtimes/inference")),
                if cfg!(windows) { "python" } else { "python3" },
            ),
            "whisper_cli" => (
                Path::new(".runtimes/whisper-build/bin").join(platform::exe("whisper-cli")),
                Path::new("runtimes/whisper/bin").join(platform::exe("whisper-cli")),
                "whisper-cli",
            ),
            _ => (
                Path::new(".runtimes/llama-build/bin").join(platform::exe("llama-completion")),
                Path::new("runtimes/llama/bin").join(platform::exe("llama-completion")),
                "llama-completion",
            ),
        };
        let optimized = platform::optimized_cpu();
        let engines = self.resources.join("engines");
        let bundled = match tool {
            // The Windows package ships its own interpreter with LiteRT installed.
            "python" if cfg!(windows) => self.resources.join("python").join("python.exe"),
            "python" => engines.join(fallback),
            _ if !optimized => engines.join(platform::exe(&format!("{fallback}-generic"))),
            _ => engines.join(platform::exe(fallback)),
        };
        if tool != "python" && !optimized {
            let generic = self
                .resources
                .join(".runtimes/generic")
                .join(platform::exe(fallback));
            if generic.is_file() {
                return generic.to_string_lossy().to_string();
            }
        }
        // A Windows checkout stages the bundled runtime under src-tauri/ for debug builds.
        let staged = if cfg!(windows) {
            Some(self.resources.join("src-tauri").join(bundled.strip_prefix(&self.resources).unwrap_or(&bundled)))
        } else {
            None
        };
        for path in [Some(bundled), staged, Some(self.resources.join(dev)), Some(self.data.join(installed))].into_iter().flatten() {
            if path.is_file() {
                return path.to_string_lossy().to_string();
            }
        }
        fallback.into()
    }
    fn request(&self, kind: &str, meeting: Option<&str>) -> Result<Value> {
        // The capture and the library are read under the same locks. A
        // segment that closed since `tick` registered the last one would
        // otherwise be neither a registered track nor the growing file, and
        // the live pass would see a gap in the stream and lose its words.
        let mut capture = self.capture.lock().unwrap();
        let db = self.db.lock().unwrap();
        let (finalize, growing) = match capture.as_mut() {
            Some(c) if Some(c.meeting.as_str()) == meeting && !c.paused => {
                for (path, source, offset) in c.closed_tracks().unwrap_or_default() {
                    let _ = db.add_track(&c.meeting, &path, &source, offset);
                }
                (false, c.open_tracks().unwrap_or_default())
            }
            _ => (true, vec![]),
        };
        drop(capture);
        let models =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        let mut v = json!({"kind":kind,"cache_dir":self.cache,"models":{"whisper":self.whisper_model(&db),"embedding":models.join("embeddinggemma-2-740m.litertlm"),"summary":""},"threads":db.setting("threads","2").parse::<u32>().unwrap_or(2).clamp(1,8),"language":db.setting("language","en"),"whisper_cli":db.setting("whisper_cli",&self.default_tool("whisper_cli")),"llama_cli":db.setting("llama_cli",&self.default_tool("llama_cli"))});
        if kind == "summarize" {
            v["summary_style"] = json!(db.setting("summary_style", "counselling"));
            v["summary_prompt"] = json!(db.setting("summary_prompt", ""));
        }
        v["models"]["summary"] = json!(match summary_profile(&db).as_str() {
            "e4b" => models
                .join("gemma-4-E4B-it-Q4_0.gguf")
                .to_string_lossy()
                .to_string(),
            "custom" => db.setting("summary_model", ""),
            _ => models
                .join("gemma-4-E2B-it-Q4_0.gguf")
                .to_string_lossy()
                .to_string(),
        });
        if let Some(m) = meeting {
            let mut tracks = serde_json::to_value(db.tracks(m)?).map_err(err)?;
            for track in tracks.as_array_mut().unwrap() {
                let duration: Option<i64> = db
                    .conn
                    .query_row(
                        "SELECT duration_ms FROM audio_durations WHERE track_id=?",
                        [track["id"].as_str().unwrap()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(err)?;
                if let Some(duration) = duration {
                    track["duration_ms"] = json!(duration);
                }
            }
            if kind == "transcribe_live" {
                v["context_tracks"] = tracks.clone();
                v["finalize"] = json!(finalize);
                v["growing"] = json!(growing
                    .iter()
                    .map(|(path, source, offset)| json!({"path":path,"source":source,"offset_ms":offset}))
                    .collect::<Vec<_>>());
                let mut progress = serde_json::Map::new();
                let mut query = db
                    .conn
                    .prepare("SELECT stream,covered_ms,overlap_text FROM asr_progress WHERE meeting_id=?")
                    .map_err(err)?;
                let rows = query
                    .query_map([m], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(err)?;
                for row in rows {
                    let (stream, covered, overlap_text) = row.map_err(err)?;
                    progress.insert(
                        stream,
                        json!({"covered_ms":covered,"overlap_text":overlap_text}),
                    );
                }
                v["asr_progress"] = json!(progress);
                tracks.as_array_mut().unwrap().retain(|t| {
                    db.conn
                        .query_row(
                            "SELECT transcribed FROM audio_tracks WHERE id=?",
                            [t["id"].as_str().unwrap()],
                            |r| r.get::<_, i64>(0),
                        )
                        .unwrap_or(0)
                        == 0
                });
            }
            v["tracks"] = json!(tracks);
            v["segments"] = json!(db.segments(m)?);
            if kind == "index" {
                db.conn.execute("DELETE FROM embedding_items WHERE meeting_id=? AND (revision<>? OR dimensions<>768 OR chunk_version<>1 OR backend<>'litert-lm-cpu')",params![m,search::REVISION]).map_err(err)?;
                let mut stmt=db.conn.prepare("SELECT modality,track_id,start_ms,end_ms FROM embedding_items WHERE meeting_id=?").map_err(err)?;
                let rows = stmt
                    .query_map([m], |r| {
                        Ok(json!([
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?
                        ]))
                    })
                    .map_err(err)?;
                v["indexed"] = json!(rows
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(err)?);
            }
        }
        Ok(v)
    }
    fn tick(&self) {
        let mut recording = false;
        {
            let mut capture = self.capture.lock().unwrap();
            if let Some(c) = capture.as_mut() {
                recording = true;
                let tracks = c.closed_tracks().unwrap_or_default();
                let mut poll = self.live_poll.lock().unwrap();
                // Live passes read the file still being written, so words can
                // appear seconds after a pause instead of when a chunk closes.
                if !tracks.is_empty() || (!c.paused && poll.elapsed() >= LIVE_POLL) {
                    *poll = Instant::now();
                    let db = self.db.lock().unwrap();
                    for (path, source, offset) in tracks {
                        let _ = db.add_track(&c.meeting, &path, &source, offset);
                    }
                    if self.whisper_model(&db).is_file() {
                        let _ = db.enqueue(&c.meeting, "transcribe_live");
                    }
                }
            }
        }
        let next: Option<(String, String, String)> = {
            let db = self.db.lock().unwrap();
            db.conn.query_row("SELECT id,meeting_id,kind FROM jobs WHERE status='pending' AND (?=0 OR kind='transcribe_live') ORDER BY CASE kind WHEN 'transcribe_live' THEN 0 WHEN 'transcribe' THEN 1 WHEN 'index' THEN 2 ELSE 3 END,created_at,rowid LIMIT 1",[recording as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().ok().flatten()
        };
        let Some((job, meeting, kind)) = next else {
            return;
        };
        let Ok(_guard) = self.inference.try_lock() else {
            return;
        };
        let _ = self
            .db
            .lock()
            .unwrap()
            .job_status(&job, "running", 0., None);
        (self.notify)(json!({"type":"jobs"}));
        let mut provisional = false;
        let result = self.request(&kind, Some(&meeting)).and_then(|req| {
            provisional = req["finalize"] == false;
            let result = self.run_worker(&job, &req, Some((&meeting, &kind)));
            if result.is_ok() && (kind == "transcribe" || kind == "transcribe_live") {
                let db = self.db.lock().unwrap();
                for t in req["tracks"].as_array().unwrap() {
                    let _ = db.conn.execute(
                        "UPDATE audio_tracks SET transcribed=1 WHERE id=?",
                        [t["id"].as_str().unwrap_or("")],
                    );
                }
            }
            result
        });
        let (recording, capturing_meeting) = {
            let capture = self.capture.lock().unwrap();
            let active = capture.as_ref().is_some_and(|c| c.meeting == meeting && !c.paused);
            (capture.is_some(), active)
        };
        let db = self.db.lock().unwrap();
        let status: String = db
            .conn
            .query_row("SELECT status FROM jobs WHERE id=?", [&job], |r| r.get(0))
            .unwrap_or_default();
        if status != "cancelled" {
            match result {
                Ok(_) => {
                    if kind == "transcribe_live" {
                        // Frequent live passes carry no state of their own:
                        // progress lives in asr_progress. Keep the job list small.
                        let _ = db.conn.execute("DELETE FROM jobs WHERE id=?", [&job]);
                    } else {
                        let _ = db.job_status(&job, "completed", 1., None);
                    }
                    let remaining: i64 = if kind == "transcribe_live" {
                        db.conn.query_row("SELECT count(*) FROM audio_tracks WHERE meeting_id=? AND transcribed=0",[&meeting],|r|r.get(0)).unwrap_or(0)
                    } else {
                        0
                    };
                    // A pass that started before Stop/Pause may have left the
                    // tail waiting for a pause: run a final pass to flush it.
                    if remaining > 0 || (provisional && !capturing_meeting) {
                        let _ = db.enqueue(&meeting, "transcribe_live");
                    }
                    if !recording
                        && remaining == 0
                        && (kind == "transcribe" || kind == "transcribe_live")
                        && PathBuf::from(
                            db.setting("models_dir", &self.data.join("models").to_string_lossy()),
                        )
                        .join("embeddinggemma-2-740m.litertlm")
                        .is_file()
                    {
                        let _ = db.enqueue(&meeting, "index");
                    }
                }
                Err(e) => {
                    if (recording && kind != "transcribe_live") || self.quit.load(Ordering::SeqCst)
                    {
                        let _ = db.job_status(
                            &job,
                            "pending",
                            0.,
                            Some("Deferred while recording / closing"),
                        );
                    } else {
                        let _ = db.job_status(&job, "failed", 0., Some(&e));
                        if kind == "transcribe_live" {
                            // Live passes repeat every few seconds: keep one
                            // failure visible and back off before retrying.
                            let _ = db.conn.execute("DELETE FROM jobs WHERE meeting_id=? AND kind='transcribe_live' AND status='failed' AND id<>?", params![meeting, job]);
                            *self.live_poll.lock().unwrap() =
                                Instant::now() + Duration::from_secs(30);
                        }
                    }
                }
            }
        }
        (self.notify)(json!({"type":"jobs"}));
    }
    fn run_worker(
        &self,
        job: &str,
        request: &Value,
        ingest: Option<(&str, &str)>,
    ) -> Result<Vec<Value>> {
        if self.quit.load(Ordering::SeqCst) {
            return Err("Application is closing".into());
        }
        let folder = self.cache.join(format!("request-{}", db::id()));
        private_dir(&folder)?;
        let _scratch = ScratchDir(folder.clone());
        let input = folder.join("input.json");
        let output = folder.join("output.jsonl");
        fs::write(&input, serde_json::to_vec(request).map_err(err)?).map_err(err)?;
        let capture_guard = self.capture.lock().unwrap();
        if capture_guard.is_some() && request["kind"] != "transcribe_live" {
            return Err("Inference deferred while recording".into());
        }
        let mut child = process::spawn(
            process::python(self.python())
                .arg(self.resources.join("worker/engine.py"))
                .arg("--request")
                .arg(&input)
                .arg("--response")
                .arg(&output)
                .env("HF_HUB_OFFLINE", "1")
                .env("HF_HUB_DISABLE_TELEMETRY", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .map_err(|_| {
                "Cannot launch Python worker. Configure its local executable in Settings."
                    .to_string()
            })?;
        let pid = child.id();
        *self.running.lock().unwrap() = Some((job.into(), pid));
        drop(capture_guard);
        let start = Instant::now();
        let timeout = ingest
            .and_then(|(meeting, _)| {
                self.db
                    .lock()
                    .unwrap()
                    .conn
                    .query_row(
                        "SELECT duration_ms FROM meetings WHERE id=?",
                        [meeting],
                        |r| r.get::<_, i64>(0),
                    )
                    .ok()
            })
            .map(|ms| Duration::from_millis(ms.max(0) as u64 * 2))
            .unwrap_or(Duration::ZERO)
            .max(WORKER_TIMEOUT);
        let mut reader: Option<BufReader<File>> = None;
        let mut pending = String::new();
        let mut answers = vec![];
        let mut failure = None;
        let mut peak_tree = 0;
        if let Some((meeting, "transcribe")) = ingest {
            let db = self.db.lock().unwrap();
            db.conn
                .execute(
                    "DELETE FROM transcript_segments WHERE meeting_id=? AND edited=0",
                    [meeting],
                )
                .map_err(err)?;
            db.conn
                .execute("DELETE FROM embedding_items WHERE meeting_id=?", [meeting])
                .map_err(err)?;
            db.conn
                .execute("DELETE FROM asr_progress WHERE meeting_id=?", [meeting])
                .map_err(err)?;
        }
        let mut exited = false;
        let processing = (|| -> Result<()> {
            loop {
                peak_tree = peak_tree.max(crate::metrics::tree_rss_kib(std::process::id()));
                if reader.is_none() && output.exists() {
                    reader = Some(BufReader::new(File::open(&output).map_err(err)?));
                }
                if let Some(r) = reader.as_mut() {
                    loop {
                        let mut line = String::new();
                        let n = r.read_line(&mut line).map_err(err)?;
                        if n == 0 {
                            break;
                        }
                        pending.push_str(&line);
                        if !pending.ends_with('\n') {
                            break;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(&pending) {
                            match v["type"].as_str().unwrap_or("") {
                                "progress" => {
                                    let _=self.db.lock().unwrap().conn.execute("UPDATE jobs SET progress=? WHERE id=? AND status='running'",params![v["progress"].as_f64().unwrap_or(0.),job]);
                                    (self.notify)(
                                        json!({"type":"jobs","job":job,"progress":v["progress"]}),
                                    );
                                }
                                "error" => {
                                    failure = Some(
                                        v["message"]
                                            .as_str()
                                            .unwrap_or("Local inference failed")
                                            .to_string(),
                                    )
                                }
                                "segment" | "asr_batch" | "track_duration" | "embedding"
                                | "summary" => {
                                    if let Some((meeting, _)) = ingest {
                                        self.ingest(meeting, &v)?;
                                        (self.notify)(
                                            json!({"type":"detail","meeting_id":meeting}),
                                        );
                                    }
                                }
                                "metrics" => {
                                    let mut v = v.clone();
                                    v["host_tree_peak_rss_kib"] = json!(peak_tree);
                                    v["kind"] = request["kind"].clone();
                                    let _ = fs::write(
                                        self.cache.join("last-inference-metrics.json"),
                                        serde_json::to_vec_pretty(&v).unwrap(),
                                    );
                                    let name = format!(
                                        "metrics-{}.json",
                                        request["kind"].as_str().unwrap_or("unknown")
                                    );
                                    let _ = fs::write(
                                        self.cache.join(name),
                                        serde_json::to_vec_pretty(&v).unwrap(),
                                    );
                                }
                                _ => answers.push(v),
                            }
                        }
                        pending.clear();
                    }
                }
                if exited {
                    break;
                }
                if child.try_wait().map_err(err)?.is_some() {
                    exited = true;
                    continue;
                }
                if self.quit.load(Ordering::SeqCst) || start.elapsed() > timeout {
                    process::kill_tree(pid);
                    let _ = child.wait();
                    failure = Some("Inference stopped or timed out".into());
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Ok(())
        })();
        // Also reap a worker's descendants if a runtime exits unexpectedly.
        process::kill_tree(pid);
        let status = child.wait().map_err(err);
        *self.running.lock().unwrap() = None;
        let _ = fs::remove_dir_all(&folder);
        processing?;
        let status = status?;
        if let Some(e) = failure {
            return Err(e);
        }
        if !status.success() {
            return Err("Local worker stopped. Check installed runtime/model compatibility, or retry the job.".into());
        }
        Ok(answers)
    }
    fn ingest(&self, meeting: &str, v: &Value) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        match v["type"].as_str().unwrap_or("") {
            "track_duration" => {
                db.conn.execute("INSERT OR REPLACE INTO audio_durations(track_id,duration_ms) SELECT id,? FROM audio_tracks WHERE id=? AND meeting_id=?", params![v["duration_ms"].as_i64(),v["track_id"].as_str(),meeting]).map_err(err)?;
                // A recording cut short by a crash never reached Stop, so its
                // length is whatever the last Pause wrote, or 0. The decoded
                // segments say how long it really was.
                db.conn.execute("UPDATE meetings SET duration_ms=MAX(duration_ms,COALESCE((SELECT offset_ms+? FROM audio_tracks WHERE id=? AND meeting_id=?),0)) WHERE id=? AND status='interrupted'", params![v["duration_ms"].as_i64(),v["track_id"].as_str(),meeting,meeting]).map_err(err)?;
            }
            "asr_batch" => {
                let tx = db.conn.transaction().map_err(err)?;
                for segment in v["segments"]
                    .as_array()
                    .ok_or("Missing transcription batch")?
                {
                    tx.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) SELECT ?,?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM transcript_segments WHERE meeting_id=? AND source=? AND ((start_ms=? AND text=?) OR (edited=1 AND start_ms<? AND end_ms>?)))", params![meeting,segment["start_ms"].as_i64(),segment["end_ms"].as_i64(),segment["text"].as_str(),segment["source"].as_str(),meeting,segment["source"].as_str(),segment["start_ms"].as_i64(),segment["text"].as_str(),segment["end_ms"].as_i64(),segment["start_ms"].as_i64()]).map_err(err)?;
                }
                tx.execute("INSERT INTO asr_progress(meeting_id,stream,covered_ms,overlap_text) VALUES(?,?,?,?) ON CONFLICT(meeting_id,stream) DO UPDATE SET covered_ms=excluded.covered_ms,overlap_text=excluded.overlap_text WHERE excluded.covered_ms>=covered_ms",params![meeting,v["stream"].as_str(),v["covered_ms"].as_i64(),v["overlap_text"].as_str().unwrap_or("")]).map_err(err)?;
                tx.commit().map_err(err)?;
            }
            "segment" => {
                db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) SELECT ?,?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM transcript_segments WHERE meeting_id=? AND source=? AND start_ms=?)",params![meeting,v["start_ms"].as_i64(),v["end_ms"].as_i64(),v["text"].as_str(),v["source"].as_str(),meeting,v["source"].as_str(),v["start_ms"].as_i64()]).map_err(err)?;
            }
            "embedding" => {
                let vector: Vec<f32> = serde_json::from_value(v["vector"].clone()).map_err(err)?;
                let vector = search::normalized(&vector)?;
                let item = format!(
                    "{meeting}:{}:{}:{}:{}",
                    v["track_id"], v["modality"], v["start_ms"], v["end_ms"]
                );
                let tx = db.conn.transaction().map_err(err)?;
                tx.execute("INSERT INTO model_registry(id,revision,path,dimensions) VALUES(?,?,?,768) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,path=excluded.path,dimensions=excluded.dimensions",params![search::MODEL_ID,search::REVISION,"embeddinggemma-2-740m.litertlm"]).map_err(err)?;
                tx.execute("INSERT OR REPLACE INTO embedding_items(id,meeting_id,track_id,modality,start_ms,end_ms,text,model_id,revision,backend,dimensions,chunk_version) VALUES(?,?,?,?,?,?,?,?,?,'litert-lm-cpu',768,1)",params![item,meeting,v["track_id"].as_str(),v["modality"].as_str(),v["start_ms"].as_i64(),v["end_ms"].as_i64(),v["text"].as_str(),search::MODEL_ID,search::REVISION]).map_err(err)?;
                tx.execute(
                    "INSERT OR REPLACE INTO embedding_vectors(item_id,vector) VALUES(?,?)",
                    params![item, search::bytes(&vector)],
                )
                .map_err(err)?;
                tx.commit().map_err(err)?;
            }
            "summary" => {
                db.conn
                    .execute(
                        "UPDATE meetings SET summary=? WHERE id=?",
                        params![v["text"].as_str(), meeting],
                    )
                    .map_err(err)?;
                // The suggested title only replaces the default name, never
                // a name the user typed or an imported file's name.
                if let Some(title) = v["title"]
                    .as_str()
                    .map(str::trim)
                    .filter(|t| !t.is_empty() && t.chars().count() <= 300)
                {
                    db.conn
                        .execute(
                            "UPDATE meetings SET title=? WHERE id=? AND title=?",
                            params![title, meeting, DEFAULT_TITLE],
                        )
                        .map_err(err)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn dispatch(self: &Arc<Self>, action: &str, p: Value) -> Result<Value> {
        match action {
            "library" => Ok(json!(self.db.lock().unwrap().meetings()?)),
            "detail" => self.db.lock().unwrap().detail(text(&p, "id")?),
            "jobs" => self.db.lock().unwrap().jobs(),
            "devices" => audio::devices(),
            "status" => Ok(self
                .capture
                .lock()
                .unwrap()
                .as_mut()
                .map(|c| c.status())
                .unwrap_or(Value::Null)),
            "import" => self.import(Path::new(text(&p, "path")?)),
            "update" => {
                let db = self.db.lock().unwrap();
                let id = text(&p, "id")?;
                if let Some(t) = p["title"].as_str() {
                    if t.trim().is_empty() || t.chars().count() > 300 {
                        return Err("Title must contain 1–300 characters".into());
                    }
                    db.conn
                        .execute(
                            "UPDATE meetings SET title=? WHERE id=?",
                            params![t.trim(), id],
                        )
                        .map_err(err)?;
                }
                if let Some(n) = p["notes"].as_str() {
                    db.conn
                        .execute("UPDATE meetings SET notes=? WHERE id=?", params![n, id])
                        .map_err(err)?;
                }
                Ok(Value::Null)
            }
            "edit_segment" => {
                let db = self.db.lock().unwrap();
                let m = text(&p, "meeting_id")?;
                let correction = text(&p, "text")?.trim();
                if correction.is_empty() {
                    return Err("A correction cannot be empty".into());
                }
                db.conn.execute("UPDATE transcript_segments SET text=?,edited=1 WHERE id=? AND meeting_id=?",params![correction,p["id"].as_i64(),m]).map_err(err)?;
                db.conn
                    .execute("DELETE FROM embedding_items WHERE meeting_id=?", [m])
                    .map_err(err)?;
                Ok(Value::Null)
            }
            "enqueue" => Ok(json!(self
                .db
                .lock()
                .unwrap()
                .enqueue(text(&p, "id")?, text(&p, "kind")?)?)),
            "rebuild_index" => {
                let db = self.db.lock().unwrap();
                let id = text(&p, "id")?;
                let job = db.enqueue(id, "index")?;
                db.conn
                    .execute("DELETE FROM embedding_items WHERE meeting_id=?", [id])
                    .map_err(err)?;
                Ok(json!(job))
            }
            "cancel" => {
                let id = text(&p, "id")?;
                let running = self.running.lock().unwrap();
                self.db
                    .lock()
                    .unwrap()
                    .job_status(id, "cancelled", 0., None)?;
                if let Some((job, pid)) = running.as_ref() {
                    if job == id {
                        process::kill_tree(*pid);
                    }
                }
                Ok(Value::Null)
            }
            "search" => self.search(&p),
            "answer" => {
                let _guard = self
                    .inference
                    .try_lock()
                    .map_err(|_| "Inference is busy. Wait or cancel the current job.")?;
                if self.capture.lock().unwrap().is_some() {
                    return Err("Q&A is deferred while recording".into());
                }
                let mut req = self.request("answer", None)?;
                req["query"] = p["query"].clone();
                req["evidence"] = p["evidence"].clone();
                Ok(json!(self.run_worker("interactive", &req, None)?))
            }
            "models" => self.models(),
            "remove_models" => self.remove_models(&p),
            "runtime_status" => {
                let db = self.db.lock().unwrap();
                let available = |tool: &str| {
                    let path = db.setting(tool, &self.default_tool(tool));
                    Path::new(&path).is_file() || platform::on_path(path)
                };
                let python = db.setting("python", &self.default_tool("python"));
                let inference = process::python(&python)
                    .args([
                        "-I",
                        "-c",
                        "import importlib.metadata as m; assert m.version('litert-lm') == '0.18.0'",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                Ok(
                    json!({"inference":inference,"whisper":available("whisper_cli"),"llama":available("llama_cli"),"python":python,"storage":self.data,"disk_free_bytes":platform::free_disk_bytes(&self.data)}),
                )
            }
            "setup" => {
                let s = self.clone();
                let mode = text(&p, "mode")?.to_string();
                if ![
                    "check",
                    "download",
                    "import",
                    "runtime",
                    "recommended",
                    "offline_bundle",
                ]
                .contains(&mode.as_str())
                {
                    return Err("Invalid setup action".into());
                }
                let guard = self
                    .inference
                    .try_lock()
                    .map_err(|_| "Inference/setup already running")?;
                if self.setup_active.swap(true, Ordering::SeqCst) {
                    return Err("Setup is already running".into());
                }
                self.setup_cancel.store(false, Ordering::SeqCst);
                drop(guard);
                std::thread::spawn(move || {
                    let _guard = s.inference.lock().unwrap();
                    let result = s.provision(&mode, &p);
                    s.setup_active.store(false, Ordering::SeqCst);
                    (s.notify)(json!({"type":"setup_done","error":result.err()}));
                });
                Ok(Value::Null)
            }
            "cancel_setup" => {
                self.setup_cancel.store(true, Ordering::SeqCst);
                if let Some((job, pid)) = self.running.lock().unwrap().as_ref() {
                    if job == "setup" {
                        process::kill_tree(*pid);
                    }
                }
                Ok(Value::Null)
            }
            "settings" => {
                let db = self.db.lock().unwrap();
                Ok(
                    json!({"threads":db.setting("threads","2"),"language":db.setting("language","en"),"whisper_cli":db.setting("whisper_cli",&self.default_tool("whisper_cli")),"llama_cli":db.setting("llama_cli",&self.default_tool("llama_cli")),"python":db.setting("python",&self.default_tool("python")),"models_dir":db.setting("models_dir",&self.data.join("models").to_string_lossy()),"summary_profile":summary_profile(&db),"summary_model":db.setting("summary_model",""),"summary_style":db.setting("summary_style","counselling"),"summary_prompt":db.setting("summary_prompt",""),"transcription_profile":db.setting("transcription_profile","standard"),"appearance":db.setting("appearance","system"),"storage":self.data,"low_memory":true}),
                )
            }
            "save_settings" => {
                let db = self.db.lock().unwrap();
                for k in [
                    "threads",
                    "language",
                    "whisper_cli",
                    "llama_cli",
                    "python",
                    "models_dir",
                    "summary_model",
                    "summary_profile",
                    "summary_style",
                    "summary_prompt",
                    "transcription_profile",
                    "appearance",
                ] {
                    if let Some(v) = p[k].as_str() {
                        if k == "threads" && !(1..=8).contains(&v.parse::<u32>().unwrap_or(0)) {
                            return Err("CPU threads must be 1–8".into());
                        }
                        if k == "summary_profile" && !["e2b", "e4b", "custom"].contains(&v) {
                            return Err("Unknown summary profile".into());
                        }
                        if k == "transcription_profile" && !["standard", "accurate"].contains(&v) {
                            return Err("Unknown transcription profile".into());
                        }
                        if k == "appearance" && !["system", "light", "dark"].contains(&v) {
                            return Err("Unknown appearance".into());
                        }
                        if k == "summary_style"
                            && !["counselling", "service", "meeting", "custom"].contains(&v)
                        {
                            return Err("Unknown summary style".into());
                        }
                        if k == "summary_prompt" && v.chars().count() > 2000 {
                            return Err("Custom summary instructions must be 2,000 characters or fewer".into());
                        }
                        if [
                            "whisper_cli",
                            "llama_cli",
                            "python",
                            "models_dir",
                            "summary_model",
                        ]
                        .contains(&k)
                            && (v.contains('\0')
                                || v.starts_with("http:")
                                || v.starts_with("https:"))
                        {
                            return Err("Use only local executable and model paths".into());
                        }
                        db.set_setting(k, v)?;
                    }
                }
                Ok(Value::Null)
            }
            "diagnostics" => {
                let tools: &[&str] = if cfg!(windows) {
                    &["ffmpeg", "ffprobe", "whisper-cli", "llama-cli", "python"]
                } else {
                    &[
                        "ffmpeg",
                        "ffprobe",
                        "pactl",
                        "whisper-cli",
                        "llama-cli",
                        "python3",
                    ]
                };
                let tools = tools
                    .iter()
                    .map(|name| json!({"name":name,"available":platform::on_path(name)}))
                    .collect::<Vec<_>>();
                Ok(
                    json!({"tools":tools,"audio":audio::devices().ok(),"data":self.data,"network":"No runtime networking. Only explicit model setup can download.","last_metrics":fs::read_to_string(self.cache.join("last-inference-metrics.json")).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
                )
            }
            "record" => self.record(p),
            "pause" => {
                self.finish_capture(false)?;
                Ok(Value::Null)
            }
            "resume" => {
                let mut c = self.capture.lock().unwrap();
                let c = c.as_mut().ok_or("No paused recording")?;
                if !c.paused {
                    return Err("Recording is already active".into());
                }
                c.start()?;
                Ok(Value::Null)
            }
            "stop" => {
                self.finish_capture(true)?;
                Ok(Value::Null)
            }
            "delete" => self.delete(text(&p, "id")?),
            "export" => self.export(&p),
            _ => Err("Unknown application command".into()),
        }
    }
    fn import(&self, path: &Path) -> Result<Value> {
        let path = path
            .canonicalize()
            .map_err(|_| "Selected media no longer exists")?;
        if !path.is_file() {
            return Err("Choose a local audio or video file".into());
        }
        let ext = path
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ![
            "wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "mp4", "mkv", "webm", "mov",
        ]
        .contains(&ext.as_str())
        {
            return Err("Unsupported media extension".into());
        }
        let duration = audio::duration(&path)?;
        let id = db::id();
        let folder = self.data.join("recordings").join(&id);
        private_dir(&folder)?;
        // Video is decoded to FLAC without storing picture frames or blobs.
        let target = folder.join("original.flac");
        let status = process::command("ffmpeg")
            .args([
                "-nostdin",
                "-protocol_whitelist",
                "file,pipe",
                "-v",
                "error",
                "-y",
                "-i",
            ])
            .arg(&path)
            .args(["-vn", "-c:a", "flac"])
            .arg(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(err)?;
        if !status.success() {
            let _ = fs::remove_dir_all(&folder);
            return Err("FFmpeg could not decode this media. No recording was added.".into());
        }
        let db = self.db.lock().unwrap();
        db.create(
            &id,
            &path.file_stem().unwrap().to_string_lossy(),
            duration,
            "ready",
        )?;
        db.add_track(&id, &target, "imported", 0)?;
        let models =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        if self.whisper_model(&db).is_file() {
            let _ = db.enqueue(&id, "transcribe");
        } else if models.join("embeddinggemma-2-740m.litertlm").is_file() {
            let _ = db.enqueue(&id, "index");
        }
        Ok(json!({"id":id}))
    }
    fn models(&self) -> Result<Value> {
        let mut manifest: Value =
            serde_json::from_str(include_str!("../../models/manifest.json")).map_err(err)?;
        let db = self.db.lock().unwrap();
        let directory =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        let size = |p: &Path| p.metadata().map(|m| m.len()).unwrap_or(0);
        for model in manifest["models"].as_array_mut().unwrap() {
            let path = directory.join(model["filename"].as_str().unwrap());
            model["present"] = json!(path
                .metadata()
                .map(|m| m.len() == model["bytes"].as_u64().unwrap())
                .unwrap_or(false));
            // Unfinished downloads count too: they are what users look for
            // when a cancelled multi-GB download has filled the disk.
            model["disk_bytes"] = json!(size(&path) + size(&part_path(&path)));
            model["path"] = json!(path);
        }
        // Models earlier versions offered are listed only while their files
        // are still on disk, so Settings can remove them.
        for mut model in manifest["retired"].as_array().cloned().unwrap_or_default() {
            let path = directory.join(model["filename"].as_str().unwrap());
            let bytes = size(&path) + size(&part_path(&path));
            if bytes > 0 {
                model["retired"] = json!(true);
                model["present"] = json!(false);
                model["disk_bytes"] = json!(bytes);
                model["path"] = json!(path);
                manifest["models"].as_array_mut().unwrap().push(model);
            }
        }
        Ok(manifest)
    }
    /// Frees disk space by deleting pinned model files, unfinished downloads
    /// and the compiled caches LiteRT keeps for them. A custom GGUF is the
    /// user's own file and is never touched. Recordings, transcripts and
    /// settings stay, and setup can download the models again.
    fn remove_models(&self, p: &Value) -> Result<Value> {
        let ids: Vec<&str> = p["ids"]
            .as_array()
            .map(|ids| ids.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let manifest = self.models()?;
        let chosen: Vec<&Value> = manifest["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| ids.contains(&m["id"].as_str().unwrap_or("")))
            .collect();
        if ids.is_empty() || chosen.len() != ids.len() {
            return Err("Unknown model".into());
        }
        let _guard = self
            .inference
            .try_lock()
            .map_err(|_| "Wait for the current job or setup to finish, then try again.")?;
        if self.setup_active.load(Ordering::SeqCst) {
            return Err("Wait for setup to finish, then try again.".into());
        }
        // Held until the files are gone, so a recording cannot start while
        // the model that transcribes it is being removed.
        let capture = self.capture.lock().unwrap();
        if capture.is_some() {
            return Err("Stop recording before removing models.".into());
        }
        let queued: i64 = self
            .db
            .lock()
            .unwrap()
            .conn
            .query_row(
                "SELECT count(*) FROM jobs WHERE status IN ('pending','running')",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if queued > 0 {
            return Err("Wait for queued work to finish, or cancel it, before removing models.".into());
        }
        let remove = |path: &Path| -> Result<u64> {
            match fs::symlink_metadata(path) {
                Ok(m) if !m.is_dir() => {
                    fs::remove_file(path).map_err(err)?;
                    Ok(m.len())
                }
                _ => Ok(0),
            }
        };
        let caches: Vec<PathBuf> = fs::read_dir(&self.cache)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        let mut freed = 0;
        for model in chosen {
            let path = PathBuf::from(model["path"].as_str().unwrap());
            freed += remove(&path)? + remove(&part_path(&path))?;
            let prefix = format!("{}_", model["filename"].as_str().unwrap());
            for cache in &caches {
                let name = cache.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with(&prefix) && name.ends_with(".xnnpack_cache") {
                    freed += remove(cache)?;
                }
            }
        }
        Ok(json!({"freed_bytes":freed}))
    }
    fn provision(&self, mode: &str, p: &Value) -> Result<()> {
        if self.quit.load(Ordering::SeqCst) || self.setup_cancel.load(Ordering::SeqCst) {
            return Err("Setup cancelled. You can continue later.".into());
        }
        if mode == "recommended" || mode == "offline_bundle" {
            let summary_profile = p["summary_profile"].as_str().unwrap_or("e2b");
            let summary_model = match summary_profile {
                "e2b" => "summary_e2b",
                "e4b" => "summary_e4b",
                _ => return Err("Choose a supported summary model: E2B or E4B.".into()),
            };
            let python = self.python();
            let ready = process::python(&python)
                .args([
                    "-I",
                    "-c",
                    "import importlib.metadata as m; assert m.version('litert-lm') == '0.18.0'",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ready {
                if cfg!(windows) {
                    return Err("VoxLoom's search engine is missing or damaged. Reinstall VoxLoom, then run setup again.".into());
                }
                let mut runtime = json!({});
                if mode == "offline_bundle" {
                    let wheels = Path::new(text(p, "path")?).join("wheels");
                    if !wheels.is_dir() {
                        return Err("These setup files are missing search support. Connect once and choose Download & set up, or choose a complete offline setup folder.".into());
                    }
                    runtime["wheelhouse"] = json!(wheels);
                }
                self.provision("runtime", &runtime)?;
            }
            let mut models = p.clone();
            if mode == "offline_bundle" {
                let folder = Path::new(text(p, "path")?).join("models");
                if folder.is_dir() {
                    models["path"] = json!(folder);
                }
            }
            let model_mode = if mode == "recommended" {
                "download"
            } else {
                "import"
            };
            // Install core tools independently, then only the explicitly chosen
            // summary model. E4B must never pull in the default E2B as well.
            models.as_object_mut().unwrap().remove("model");
            models["core_only"] = json!(true);
            self.provision(model_mode, &models)?;
            if p["core_only"] != true {
                models["model"] = json!(summary_model);
                self.provision(model_mode, &models)?;
                self.db
                    .lock()
                    .unwrap()
                    .set_setting("summary_profile", summary_profile)?;
            }
            return Ok(());
        }
        let directory = self
            .db
            .lock()
            .unwrap()
            .setting("models_dir", &self.data.join("models").to_string_lossy());
        let mut cmd = process::python(self.helper_python(&self.db.lock().unwrap()));
        if mode == "runtime" {
            cmd.arg(self.resources.join("worker/setup_runtime.py"))
                .arg("--install")
                .arg("--directory")
                .arg(self.data.join("runtimes/inference"));
            if let Some(wheelhouse) = p["wheelhouse"].as_str() {
                cmd.arg("--wheelhouse").arg(wheelhouse);
            }
        } else {
            cmd.arg(self.resources.join("worker/provision.py"))
                .arg("--directory")
                .arg(directory);
        }
        match mode {
            "check" => {
                cmd.arg("--check");
            }
            "download" => {
                cmd.arg("--download");
            }
            "import" => {
                cmd.arg("--offline-import").arg(text(p, "path")?);
            }
            "runtime" => {}
            _ => return Err("Invalid setup mode".into()),
        }
        if mode != "runtime" && p["core_only"] == true {
            cmd.arg("--core-only");
        }
        if let Some(model) = p["model"].as_str() {
            if !["whisper", "whisper_accurate", "embedding", "summary_e2b", "summary_e4b"].contains(&model) {
                return Err("Unknown model".into());
            }
            cmd.arg("--model").arg(model);
        }
        let mut child = process::spawn(cmd.stdout(Stdio::piped()).stderr(Stdio::null())).map_err(err)?;
        *self.running.lock().unwrap() = Some(("setup".into(), child.id()));
        if self.setup_cancel.load(Ordering::SeqCst) || self.quit.load(Ordering::SeqCst) {
            process::kill_tree(child.id());
        }
        for line in BufReader::new(child.stdout.take().unwrap())
            .lines()
            .map_while(std::result::Result::ok)
        {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                (self.notify)(json!({"type":"setup_progress","value":value}));
            }
        }
        let success = child.wait().map_err(err)?.success();
        process::finished(child.id());
        *self.running.lock().unwrap() = None;
        if !success {
            if mode == "runtime" {
                return Err("Local search runtime setup did not finish. Check the connection and disk space, then retry or import offline wheels.".into());
            }
            return Err("Model setup did not finish. Review status, then retry or import a file matching the manifest.".into());
        }
        if mode == "runtime" {
            self.db.lock().unwrap().set_setting(
                "python",
                &platform::venv_python(&self.data.join("runtimes/inference")).to_string_lossy(),
            )?;
        }
        Ok(())
    }
    fn search(&self, p: &Value) -> Result<Value> {
        let query = text(p, "query")?.trim();
        if query.is_empty() {
            return Ok(json!([]));
        }
        if query.len() > 2000 {
            return Err("Search query is too long".into());
        }
        let mode = p["mode"].as_str().unwrap_or("exact");
        let filter = &p["filter"];
        let exact = if mode != "semantic" {
            search::exact(&self.db.lock().unwrap(), query, filter)?
        } else {
            vec![]
        };
        if mode == "exact" {
            return Ok(json!(search::dedup(exact, 40)));
        }
        if self.capture.lock().unwrap().is_some() {
            return Err("Semantic search waits while recording. Exact search is available.".into());
        }
        let _guard = self
            .inference
            .try_lock()
            .map_err(|_| "Inference is busy. Exact search is available while jobs finish.")?;
        let mut request = self.request("query", None)?;
        request["query"] = json!(query);
        let results = self.run_worker("interactive", &request, None)?;
        let vector = results
            .iter()
            .find(|v| v["type"] == "query")
            .ok_or("Embedding runtime returned no query vector")?;
        let v: Vec<f32> = serde_json::from_value(vector["vector"].clone()).map_err(err)?;
        let semantic = search::semantic(&self.db.lock().unwrap(), &v, filter)?;
        Ok(json!(if mode == "combined" {
            search::combined(exact, semantic)
        } else {
            semantic
        }))
    }
    fn record(&self, p: Value) -> Result<Value> {
        let mut capture = self.capture.lock().unwrap();
        if capture.is_some() {
            return Err("A recording is already active".into());
        }
        let devices = audio::devices()?;
        let mic = p["mic"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let system = p["system"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        for (source, monitor) in [(&mic, false), (&system, true)] {
            if let Some(name) = source {
                if !devices
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["name"] == *name && d["monitor"] == monitor)
                {
                    return Err("Selected audio source is unavailable. Refresh devices.".into());
                }
            }
        }
        // FLAC segments and the WAL grow for as long as the user records.
        // Refuse to begin on a nearly full disk instead of failing minutes
        // in; 0 means the free space could not be read, not that it is 0.
        let free = platform::free_disk_bytes(&self.data);
        if free > 0 && free < MIN_RECORD_FREE_BYTES {
            return Err("Not enough free disk space to record (less than 256 MB left)".into());
        }
        let id = db::id();
        let folder = self.data.join("recordings").join(&id);
        private_dir(&folder)?;
        let mut c = Capture::new(id.clone(), folder, mic, system);
        c.start()?;
        self.db.lock().unwrap().create(
            &id,
            p["title"]
                .as_str()
                .filter(|x| !x.trim().is_empty())
                .unwrap_or(DEFAULT_TITLE),
            0,
            "recording",
        )?;
        *capture = Some(c);
        // Recording outranks indexing, summaries and questions, so a worker
        // in progress is stopped; its job is re-queued or fails cleanly.
        // Setup is left alone: killing pip or a model download half-way can
        // leave a broken venv, and live passes simply wait for it to finish.
        if let Some((job, pid)) = self.running.lock().unwrap().as_ref() {
            if job != "setup" {
                process::kill_tree(*pid);
            }
        }
        Ok(json!({"id":id}))
    }
    fn finish_capture(&self, stop: bool) -> Result<()> {
        let mut guard = self.capture.lock().unwrap();
        let c = guard.as_mut().ok_or("No active recording")?;
        let tracks = c.pause()?;
        let db = self.db.lock().unwrap();
        for (path, source, offset) in tracks {
            db.add_track(&c.meeting, &path, &source, offset)?;
        }
        db.conn
            .execute(
                "UPDATE meetings SET duration_ms=?,status=? WHERE id=?",
                params![
                    c.elapsed(),
                    if stop { "ready" } else { "recording" },
                    c.meeting
                ],
            )
            .map_err(err)?;
        // Pause closes the segment files like Stop does, and an unfinished
        // sentence is only flushed by a final pass. Without this a pause
        // taken while no pass was running left the last words unseen until
        // the recording resumed.
        if self.whisper_model(&db).is_file() {
            let _ = db.enqueue(&c.meeting, "transcribe_live");
        }
        if stop {
            *guard = None;
        }
        (self.notify)(json!({"type":"capture"}));
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<Value> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid recording ID")?;
        if self
            .capture
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.meeting == id)
            .unwrap_or(false)
        {
            return Err("Stop this recording before deleting".into());
        }
        let db = self.db.lock().unwrap();
        let active: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM jobs WHERE meeting_id=? AND status IN ('pending','running')",
                [id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if active > 0 {
            return Err("Cancel pending/running jobs before deleting".into());
        }
        let folder = self.data.join("recordings").join(id);
        let trash = self.data.join("recordings").join(format!(".deleted-{id}"));
        if folder.exists() {
            fs::rename(&folder, &trash).map_err(|e| {
                if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied {
                    "This recording is still open, for example playing. Stop playback, then delete it again.".to_string()
                } else {
                    err(e)
                }
            })?;
        }
        if let Err(e) = db.conn.execute("DELETE FROM meetings WHERE id=?", [id]) {
            let _ = fs::rename(&trash, &folder);
            return Err(err(e));
        }
        if trash.exists() {
            // Windows can keep a just-closed file locked for a moment; the
            // recording is already gone from the library, and the next start
            // removes what is left.
            if let Err(e) = fs::remove_dir_all(&trash) {
                if !cfg!(windows) {
                    return Err(err(e));
                }
            }
        }
        Ok(Value::Null)
    }
    fn export(&self, p: &Value) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let id = text(p, "id")?;
        let target = PathBuf::from(text(p, "path")?);
        let resolved = if target.exists() {
            target.canonicalize().map_err(err)?
        } else {
            target
                .parent()
                .ok_or("Export needs a parent directory")?
                .canonicalize()
                .map_err(err)?
                .join(target.file_name().ok_or("Export needs a filename")?)
        };
        if resolved.starts_with(self.data.canonicalize().map_err(err)?)
            || resolved.starts_with(self.cache.canonicalize().map_err(err)?)
        {
            return Err(
                "Choose an export location outside the application's managed data/cache folders"
                    .into(),
            );
        }
        let format = text(p, "format")?;
        let detail = db.detail(id)?;
        let segments = db.segments(id)?;
        if format == "summary_docx" || format == "docx" {
            let summary = format == "summary_docx";
            let body = if summary {
                crate::meeting_notes(detail["meeting"]["summary"].as_str().unwrap_or(""))
            } else {
                segments
                    .iter()
                    .map(|s| {
                        format!(
                            "[{}] {}: {}\n\n",
                            crate::timestamp(s.start_ms),
                            s.source,
                            s.text
                        )
                    })
                    .collect::<String>()
            };
            let mut child = process::spawn(
                process::python(self.helper_python(&db))
                    .arg(self.resources.join("worker/export_docx.py"))
                    .arg(&target)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
            )
            .map_err(|_| {
                    "Word export could not start. Check that Python 3 is installed.".to_string()
                })?;
            let payload = serde_json::to_vec(
                &json!({"title":detail["meeting"]["title"],"text":body,"summary":summary}),
            )
            .map_err(err)?;
            let written = child
                .stdin
                .take()
                .ok_or("Word export input is unavailable")?
                .write_all(&payload);
            let status = child.wait().map_err(err)?;
            process::finished(child.id());
            if written.is_err() || !status.success() {
                return Err(
                    "The Word document could not be saved. Check the folder and available space."
                        .into(),
                );
            }
            platform::private_file(&target)?;
            return Ok(Value::Null);
        }
        let content = match format {
            "json" => serde_json::to_string_pretty(&detail).map_err(err)?,
            "srt" => segments
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    format!(
                        "{}\n{},{:03} --> {},{:03}\n{}\n\n",
                        i + 1,
                        crate::timestamp(s.start_ms),
                        s.start_ms % 1000,
                        crate::timestamp(s.end_ms),
                        s.end_ms % 1000,
                        s.text
                    )
                })
                .collect(),
            "summary" => crate::meeting_notes(detail["meeting"]["summary"].as_str().unwrap_or("")),
            "txt" | "md" => {
                let body = segments
                    .iter()
                    .map(|s| {
                        format!(
                            "[{}] {}: {}\n",
                            crate::timestamp(s.start_ms),
                            s.source,
                            s.text
                        )
                    })
                    .collect::<String>();
                if format == "md" {
                    format!(
                        "# {}\n\n{}\n\n## Notes\n\n{}",
                        detail["meeting"]["title"].as_str().unwrap_or("Recording"),
                        body,
                        detail["meeting"]["notes"].as_str().unwrap_or("")
                    )
                } else {
                    body
                }
            }
            "audio" => {
                let tracks = db.tracks(id)?;
                let track = tracks
                    .iter()
                    .find(|t| Some(t.id.as_str()) == p["track_id"].as_str())
                    .ok_or("Select an audio track")?;
                let mut source_tracks = tracks
                    .iter()
                    .filter(|t| t.source == track.source)
                    .cloned()
                    .collect::<Vec<_>>();
                source_tracks.sort_by_key(|t| t.offset_ms);
                audio::export_audio(&source_tracks, &target)?;
                platform::private_file(&target)?;
                return Ok(Value::Null);
            }
            _ => return Err("Unsupported export format".into()),
        };
        fs::write(&target, content).map_err(err)?;
        platform::private_file(&target)?;
        Ok(Value::Null)
    }
    pub fn shutdown(&self) {
        if self.quit.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let running = self.running.lock().unwrap();
            if let Some((_, pid)) = running.as_ref() {
                process::kill_tree(*pid);
            }
        }
        if self.capture.lock().unwrap().is_some() {
            let _ = self.finish_capture(true);
        }
        // Give the supervisor time to wait/reap the killed worker before exit.
        for _ in 0..40 {
            if self.running.lock().unwrap().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
