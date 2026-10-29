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
    /// One playback decode at a time, so a part asked for twice is decoded once.
    playback: Mutex<()>,
    pub notify: Arc<dyn Fn(Value) + Send + Sync>,
}
/// Name of a recording started without a title; Summarise may replace it.
const DEFAULT_TITLE: &str = "Untitled recording";
/// The title above each summary added to a recording's notes. The worker's
/// `notes.py::own_notes` leaves these sections out of the notes it is given.
const SUMMARY_TITLE: &str = "# Summary";
/// Recording refuses to start below this much free space on the data volume.
const MIN_RECORD_FREE_BYTES: u64 = 256 * 1024 * 1024;
/// A worker never runs longer than this, or twice the recording's length
/// when that is more: a two-hour cap ended summaries of long recordings on
/// slow machines half-way.
const WORKER_TIMEOUT: Duration = Duration::from_secs(7200);
/// Accurate transcription since 0.1.25: Parakeet TDT 0.6B v2, run by
/// transcribe.cpp's `transcribe-cli` (docs/TESTING.md has the comparison).
const PARAKEET_MODEL: &str = "parakeet-tdt-0.6b-v2-Q6_K.gguf";

/// Whether a recording must not start. A full volume reports `Some(0)`; a
/// volume that could not be queried reports `None` and never blocks the user.
fn disk_too_full(free: Option<u64>) -> bool {
    free.is_some_and(|bytes| bytes < MIN_RECORD_FREE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each Windows preview runs from its own folder, its app beside
    /// `engines/`. Another copy's saved paths are not used; the running copy's
    /// own files and the user's custom paths are, and survive a save of
    /// everything Settings shows (both Save buttons send that).
    #[test]
    fn saved_tool_paths_follow_the_running_copy() {
        let temp = tempfile::tempdir().unwrap();
        let copy = |name: &str| {
            let app = temp.path().join(name);
            fs::create_dir_all(app.join("engines")).unwrap();
            fs::write(app.join(platform::exe("voxloom")), b"").unwrap();
            for engine in ["whisper-cli", "whisper-cli-generic"] {
                fs::write(app.join("engines").join(platform::exe(engine)), b"").unwrap();
            }
            app
        };
        let app = copy("VoxLoom-new");
        fs::create_dir_all(app.join("tools")).unwrap();
        fs::write(app.join("tools").join(platform::exe("whisper-cli")), b"").unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            app.clone(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let default = service.default_tool("whisper_cli");
        assert!(default.starts_with(&*app.to_string_lossy()), "{default}");
        // What Settings shows and what transcription runs.
        let used = |expected: &str| {
            let shown = service.dispatch("settings", json!({})).unwrap();
            assert_eq!(shown["whisper_cli"], expected);
            let request = service.request("transcribe", None).unwrap();
            assert_eq!(request["whisper_cli"], expected);
            // Saving every setting as shown changes nothing.
            service.dispatch("save_settings", shown).unwrap();
            assert_eq!(service.request("transcribe", None).unwrap()["whisper_cli"], expected);
        };
        used(&default);
        assert_eq!(service.db.lock().unwrap().setting("whisper_cli", "unset"), "unset");
        let path = |p: PathBuf| p.to_string_lossy().to_string();
        let choose = |chosen: &str| {
            service.dispatch("save_settings", json!({"whisper_cli": chosen})).unwrap();
        };
        // The running copy's other build, or a tool put inside it, is the user's choice.
        let other_build = if default.contains("generic") { "whisper-cli" } else { "whisper-cli-generic" };
        for chosen in [
            path(app.join("engines").join(platform::exe(other_build))),
            path(app.join("tools").join(platform::exe("whisper-cli"))),
        ] {
            choose(&chosen);
            used(&chosen);
        }
        // Another copy's engine is not, whether that copy is on disk or deleted.
        let old = copy("VoxLoom-old");
        for stale in [
            path(old.join("engines").join(platform::exe("whisper-cli"))),
            path(temp.path().join("VoxLoom-deleted/engines").join(platform::exe("whisper-cli"))),
        ] {
            service.db.lock().unwrap().set_setting("whisper_cli", &stale).unwrap();
            used(&default);
        }
        // A custom path that is missing for now survives saves and is used once back.
        let custom = temp.path().join("my engines").join(platform::exe("whisper-cli"));
        choose(&path(custom.clone()));
        used(&default);
        fs::create_dir_all(custom.parent().unwrap()).unwrap();
        fs::write(&custom, b"").unwrap();
        used(&path(custom));
        // Choosing the default again stores nothing.
        choose(&default);
        assert_eq!(service.db.lock().unwrap().setting("whisper_cli", "unset"), "");
        used(&default);
    }

    #[test]
    fn quitting_asks_first_only_while_something_runs() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        assert!(!service.busy());
        service.setup_active.store(true, Ordering::SeqCst);
        assert!(service.busy());
        service.setup_active.store(false, Ordering::SeqCst);
        *service.running.lock().unwrap() = Some(("summarize".into(), 0));
        assert!(service.busy());
        *service.running.lock().unwrap() = None;
        assert!(!service.busy());
        service.shutdown();
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_python_check_leaves_the_library_free() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        // Stands in for a Python that antivirus or a cold disk keeps starting.
        let slow = temp.path().join("slow-python");
        fs::write(&slow, "#!/bin/sh\nsleep 2\nexit 1\n").unwrap();
        fs::set_permissions(&slow, fs::Permissions::from_mode(0o700)).unwrap();
        service
            .db
            .lock()
            .unwrap()
            .set_setting("python", slow.to_str().unwrap())
            .unwrap();
        let checking = {
            let service = service.clone();
            std::thread::spawn(move || service.dispatch("runtime_status", Value::Null))
        };
        std::thread::sleep(Duration::from_millis(300));
        // What the quit question reads must answer meanwhile (issue #17).
        let started = Instant::now();
        service.dispatch("jobs", Value::Null).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert!(!checking.join().unwrap().unwrap()["inference"].as_bool().unwrap());
        service.shutdown();
    }

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
        let chosen = |s: &Service| {
            let request = s.request("transcribe", None).unwrap();
            assert!(!request["transcribe_cli"].as_str().unwrap().is_empty());
            let model = request["models"]["whisper"].as_str().unwrap().to_string();
            (model.rsplit(['/', '\\']).next().unwrap().to_string(), request["asr_engine"].as_str().unwrap().to_string())
        };
        assert_eq!(chosen(&service), ("ggml-base.en.bin".into(), "whisper".into()));
        // The Whisper model Accurate used until 0.1.25 is no longer chosen.
        fs::write(models.join("ggml-small.en-q8_0.bin"), b"small").unwrap();
        assert_eq!(chosen(&service), ("ggml-base.en.bin".into(), "whisper".into()));
        fs::write(models.join(PARAKEET_MODEL), b"parakeet").unwrap();
        assert_eq!(chosen(&service), (PARAKEET_MODEL.into(), "parakeet".into()));
        service
            .dispatch("save_settings", json!({"transcription_profile":"standard"}))
            .unwrap();
        assert_eq!(chosen(&service), ("ggml-base.en.bin".into(), "whisper".into()));
        // Setup installs only the chosen model: Accurate alone still
        // transcribes when Standard is chosen later and not downloaded.
        fs::remove_file(models.join("ggml-base.en.bin")).unwrap();
        assert_eq!(chosen(&service), (PARAKEET_MODEL.into(), "parakeet".into()));
        service.shutdown();
    }

    #[test]
    fn capture_segments_play_from_a_decoded_copy_that_goes_with_the_recording() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let (first, second) = (db::id(), db::id());
        let folder = temp.path().join("data").join("recordings").join(&first);
        fs::create_dir_all(&folder).unwrap();
        // Capture's own segment muxing: later files' frames keep their place in the stream.
        assert!(process::hidden("ffmpeg")
            .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-ar", "16000", "-ac", "1",
                   "-c:a", "flac", "-f", "segment", "-segment_time", "1", "-reset_timestamps", "1"])
            .arg(folder.join("s0000-mic-%06d.flac"))
            .status()
            .unwrap()
            .success());
        fs::write(folder.join("original.flac"), b"imported").unwrap();
        let tracks = {
            let db = service.db.lock().unwrap();
            db.create(&first, "Fixture", 3000, "ready").unwrap();
            db.create(&second, "Other", 3000, "ready").unwrap();
            db.add_track(&first, &folder.join("s0000-mic-000001.flac"), "mic", 1000).unwrap();
            db.add_track(&second, &folder.join("original.flac"), "imported", 0).unwrap();
            [db.tracks(&first).unwrap()[0].id.clone(), db.tracks(&second).unwrap()[0].id.clone()]
        };
        let play = |id: &str| PathBuf::from(service.dispatch("playback", json!({"id":id})).unwrap()["path"].as_str().unwrap());
        let copy = play(&tracks[0]);
        assert_eq!(copy, temp.path().join("cache").join("playback").join(&first).join(format!("{}.wav", tracks[0])));
        let wav = fs::read(&copy).unwrap();
        assert!(wav.starts_with(b"RIFF") && wav.len() > 44 + 16000 * 2 * 9 / 10, "{}", wav.len());
        let made = fs::metadata(&copy).unwrap().modified().unwrap();
        assert_eq!(play(&tracks[0]), copy);
        assert_eq!(fs::metadata(&copy).unwrap().modified().unwrap(), made, "an unchanged segment is decoded once");
        // An import is one ordinary file and plays as it is; only the open recording's copies stay.
        assert_eq!(play(&tracks[1]), folder.join("original.flac"));
        assert!(copy.exists());
        service.db.lock().unwrap().add_track(&second, &folder.join("s0000-mic-000002.flac"), "mic", 2000).unwrap();
        let other = service.db.lock().unwrap().tracks(&second).unwrap().into_iter().find(|t| t.source == "mic").unwrap().id;
        assert!(play(&other).exists());
        assert!(!copy.exists() && !temp.path().join("cache").join("playback").join(&first).exists());
        service.dispatch("delete", json!({"id":second})).unwrap();
        assert!(!temp.path().join("cache").join("playback").join(&second).exists());
        assert!(service.dispatch("playback", json!({"id":"gone"})).is_err());
        service.shutdown();
    }

    #[test]
    fn a_part_asked_for_twice_at_once_is_decoded_once_for_both() {
        // Choosing a part also prepares the next one, so a quick jump asks for it again mid-decode.
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let meeting = db::id();
        let folder = temp.path().join("data").join("recordings").join(&meeting);
        fs::create_dir_all(&folder).unwrap();
        assert!(process::hidden("ffmpeg")
            .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=12", "-ar", "16000", "-ac", "1",
                   "-c:a", "flac", "-f", "segment", "-segment_time", "6", "-reset_timestamps", "1"])
            .arg(folder.join("s0000-mic-%06d.flac"))
            .status()
            .unwrap()
            .success());
        let track = {
            let db = service.db.lock().unwrap();
            db.create(&meeting, "Fixture", 12000, "ready").unwrap();
            db.add_track(&meeting, &folder.join("s0000-mic-000001.flac"), "mic", 6000).unwrap();
            db.tracks(&meeting).unwrap()[0].id.clone()
        };
        let start = Arc::new(std::sync::Barrier::new(8));
        let callers: Vec<_> = (0..8)
            .map(|_| {
                let (service, start, track) = (service.clone(), start.clone(), track.clone());
                std::thread::spawn(move || {
                    start.wait();
                    service.dispatch("playback", json!({"id":track}))
                })
            })
            .collect();
        let copy = temp.path().join("cache").join("playback").join(&meeting).join(format!("{track}.wav"));
        for caller in callers {
            assert_eq!(PathBuf::from(caller.join().unwrap().unwrap()["path"].as_str().unwrap()), copy);
        }
        // About six seconds of 16 kHz mono PCM (segments end on a FLAC frame), whatever its place in the recording.
        let wav = fs::read(&copy).unwrap();
        let data = wav.windows(4).position(|w| w == b"data").unwrap();
        let bytes = u32::from_le_bytes(wav[data + 4..data + 8].try_into().unwrap()) as usize;
        assert_eq!(bytes, wav.len() - data - 8);
        assert!((5.8..6.2).contains(&(bytes as f64 / 32000.)), "{bytes} bytes");
        assert_eq!(fs::read_dir(copy.parent().unwrap()).unwrap().count(), 1, "no partial file is left");
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
    fn a_summary_is_added_below_the_notes_and_a_save_keeps_it() {
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
            db.create("blank", "Blank", 1000, "ready").unwrap();
            db.create("noted", "Noted", 1000, "ready").unwrap();
            db.conn.execute("UPDATE meetings SET notes='- worried about school\n- call mum ' WHERE id='noted'", []).unwrap();
        }
        let notes = |id: &str| -> String {
            service.db.lock().unwrap().conn.query_row("SELECT notes FROM meetings WHERE id=?", [id], |r| r.get(0)).unwrap()
        };
        // The summary request carries the person's notes for the worker.
        assert_eq!(service.request("summarize", Some("noted")).unwrap()["notes"], "- worried about school\n- call mum ");
        let summary = |text: &str| json!({"type":"summary","text":text,"title":""});
        service.ingest("blank", &summary("## Overview\nA first talk.\n")).unwrap();
        assert_eq!(notes("blank"), "# Summary\n\n## Overview\nA first talk.");
        // Nothing the person wrote is trimmed or replaced; a second summary goes below the first.
        service.ingest("noted", &summary("## Overview\nSchool worries.")).unwrap();
        service.ingest("noted", &summary("## Overview\nAgain.")).unwrap();
        assert_eq!(notes("noted"), "- worried about school\n- call mum \n\n# Summary\n\n## Overview\nSchool worries.\n\n# Summary\n\n## Overview\nAgain.");
        // An empty reply adds no bare title.
        assert!(service.ingest("noted", &summary(" \n")).is_err());
        assert!(notes("noted").ends_with("Again."));
        // A page that loaded the notes before a summary arrived saves its edit and keeps the summary.
        // That summary had no line before it; it still starts on a line of its own.
        service.dispatch("update", json!({"id":"blank","notes":"My own line.","notes_base":""})).unwrap();
        assert_eq!(notes("blank"), "My own line.\n\n# Summary\n\n## Overview\nA first talk.");
        let base = notes("blank");
        service.ingest("blank", &summary("## Overview\nLater.")).unwrap();
        let saved = service.dispatch("update", json!({"id":"blank","notes":"Edited.","notes_base":base})).unwrap();
        assert_eq!(saved["notes"], "Edited.\n\n# Summary\n\n## Overview\nLater.");
        // Without a newer summary the edit is saved as it is, deletions included.
        let base = notes("blank");
        service.dispatch("update", json!({"id":"blank","notes":"","notes_base":base})).unwrap();
        assert_eq!(notes("blank"), "");
        service.shutdown();
    }

    #[test]
    fn only_this_repositorys_page_for_a_plain_version_opens() {
        assert_eq!(
            release_page("0.1.21").unwrap(),
            "https://github.com/chencyberlab/VoxLoom/releases/tag/v0.1.21"
        );
        for bad in ["", "0.1", "0.1.21.1", "v0.1.21", "0.1.x", "0.1.21/../../evil", "0..1", "0.1.21 --new-window"] {
            assert!(release_page(bad).is_err(), "{bad}");
        }
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
        let request = service.request("transcribe", Some("meeting")).unwrap();
        assert_eq!(request["asr_progress"]["system:s0000"]["covered_ms"], 28000);
        assert_eq!(request["tracks"].as_array().unwrap().len(), 1);
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
                .request("transcribe", Some("meeting"))
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

    fn transcriptions(service: &Service, meeting: &str, status: Option<&str>) -> i64 {
        service.db.lock().unwrap().conn.query_row(
            "SELECT count(*) FROM jobs WHERE meeting_id=? AND kind='transcribe' AND (?2 IS NULL OR status=?2)",
            params![meeting, status],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn nothing_is_transcribed_while_recording_and_stop_queues_one_pass() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        fs::write(temp.path().join("data/models/ggml-base.en.bin"), b"base").unwrap();
        let folder = temp.path().join("data/recordings/meeting");
        fs::create_dir_all(&folder).unwrap();
        for name in ["s0000-mic-000000.flac", "s0000-mic-000001.flac"] {
            fs::write(folder.join(name), b"").unwrap();
        }
        service.db.lock().unwrap().create("meeting", DEFAULT_TITLE, 0, "recording").unwrap();
        *service.capture.lock().unwrap() =
            Some(Capture::new("meeting".into(), folder.clone(), Some("mic".into()), None));
        // An earlier recording still waiting for its transcription.
        {
            let db = service.db.lock().unwrap();
            db.create("earlier", DEFAULT_TITLE, 5000, "ready").unwrap();
            db.add_track("earlier", &temp.path().join("earlier.flac"), "mic", 0).unwrap();
            db.enqueue("earlier", "transcribe").unwrap();
        }
        // The scheduler ticks every 250 ms: it registers the closed segment
        // and starts nothing, neither for this recording nor the earlier one.
        let jobs = || -> i64 {
            service.db.lock().unwrap().conn.query_row("SELECT count(*) FROM jobs WHERE meeting_id='meeting'", [], |r| r.get(0)).unwrap()
        };
        std::thread::sleep(Duration::from_millis(1200));
        assert_eq!(service.db.lock().unwrap().tracks("meeting").unwrap().len(), 1);
        assert_eq!(jobs(), 0);
        assert_eq!(transcriptions(&service, "earlier", Some("pending")), 1);
        service.finish_capture(false).unwrap();
        assert!(service.capture.lock().unwrap().as_ref().unwrap().paused);
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(jobs(), 0);
        assert_eq!(transcriptions(&service, "earlier", Some("pending")), 1);
        // Holding the inference lock keeps the scheduler from running the jobs.
        let _busy = service.inference.lock().unwrap();
        service.finish_capture(true).unwrap();
        assert!(service.capture.lock().unwrap().is_none());
        assert_eq!((jobs(), transcriptions(&service, "meeting", Some("pending"))), (1, 1));
        let request = service.request("transcribe", Some("meeting")).unwrap();
        assert_eq!(request["tracks"].as_array().unwrap().len(), 2);
        assert_eq!(request["asr_progress"], json!({}));
        drop(_busy);
        service.shutdown();
    }

    #[test]
    fn transcribe_starts_over_and_retry_resumes() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let _busy = service.inference.lock().unwrap();
        {
            let db = service.db.lock().unwrap();
            db.create("meeting", "Fixture", 60000, "ready").unwrap();
            db.add_track("meeting", &temp.path().join("s0000-mic-000000.flac"), "mic", 0).unwrap();
            db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source,edited) VALUES('meeting',0,1000,'Model words.','mic',0),('meeting',2000,3000,'My correction.','mic',1)",[]).unwrap();
            db.conn.execute("INSERT INTO asr_progress(meeting_id,stream,covered_ms,overlap_text) VALUES('meeting','mic:s0000',28000,'')", []).unwrap();
        }
        let progress = |service: &Arc<Service>| service.request("transcribe", Some("meeting")).unwrap()["asr_progress"].clone();
        let texts = |service: &Arc<Service>| service.db.lock().unwrap().segments("meeting").unwrap().iter().map(|s| s.text.clone()).collect::<Vec<_>>();
        // Retry (no restart) keeps what the failed pass wrote and resumes.
        let job = service.dispatch("enqueue", json!({"id":"meeting","kind":"transcribe"})).unwrap();
        assert_eq!(progress(&service)["mic:s0000"]["covered_ms"], 28000);
        assert_eq!(texts(&service), ["Model words.", "My correction."]);
        service.dispatch("cancel", json!({"id":job})).unwrap();
        // Transcribe cancelled before it started: the transcript is untouched.
        let job = service.dispatch("enqueue", json!({"id":"meeting","kind":"transcribe","restart":true})).unwrap();
        assert_eq!(progress(&service)["mic:s0000"]["covered_ms"], 28000);
        assert_eq!(texts(&service), ["Model words.", "My correction."]);
        service.dispatch("cancel", json!({"id":job})).unwrap();
        // Transcribe that starts clears the rows and checkpoint first, keeping only the edits.
        // (The worker cannot start here, so the job then fails; Retry would resume from the clear.)
        let job = service.dispatch("enqueue", json!({"id":"meeting","kind":"transcribe","restart":true})).unwrap();
        drop(_busy);
        let status = || -> String {
            service.db.lock().unwrap().conn.query_row("SELECT status FROM jobs WHERE id=?", [job.as_str().unwrap()], |r| r.get(0)).unwrap()
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while status() == "pending" || status() == "running" {
            assert!(Instant::now() < deadline, "the job did not run");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(progress(&service), json!({}));
        assert_eq!(texts(&service), ["My correction."]);
        service.shutdown();
    }

    #[test]
    fn a_full_disk_refuses_recording_but_an_unknown_one_does_not() {
        assert!(disk_too_full(Some(0)));
        assert!(disk_too_full(Some(MIN_RECORD_FREE_BYTES - 1)));
        assert!(!disk_too_full(Some(MIN_RECORD_FREE_BYTES)));
        assert!(!disk_too_full(None));
        // The data folder of a running test is on a real, non-full volume.
        assert!(!disk_too_full(platform::free_disk_bytes(&std::env::temp_dir())));
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
            for id in ["half", "clean", "empty"] {
                fs::create_dir_all(temp.path().join("data/recordings").join(id)).unwrap();
            }
            // Cut off before its first file: nothing to transcribe or to call whole.
            db.create("empty", DEFAULT_TITLE, 0, "interrupted").unwrap();
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
            // Stopped recordings whose pass began but did not finish. "stranded"
            // lost its failed live pass in the 0.1.22 upgrade; "cancelled" was
            // stopped by the person, who can transcribe it when they choose.
            for id in ["stranded", "cancelled"] {
                db.create(id, DEFAULT_TITLE, 60000, "ready").unwrap();
                db.add_track(id, &temp.path().join(format!("{id}-0.flac")), "mic", 0).unwrap();
                db.add_track(id, &temp.path().join(format!("{id}-1.flac")), "mic", 30000).unwrap();
                db.conn.execute(&format!("UPDATE audio_tracks SET transcribed=1 WHERE meeting_id='{id}' AND offset_ms=0"), []).unwrap();
                db.conn.execute(&format!(r#"INSERT INTO asr_progress(meeting_id,stream,covered_ms,overlap_text) VALUES('{id}','mic:s0000',28000,'{{"pending":{{"start_ms":27000,"end_ms":28000,"text":"and so we"}}}}')"#), []).unwrap();
            }
            let job = db.enqueue("cancelled", "transcribe").unwrap();
            db.job_status(&job, "cancelled", 0., None).unwrap();
        }
        service.shutdown();
        drop(service);
        let service = open();
        let tracks = service.db.lock().unwrap().tracks("lost").unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].offset_ms, 30000);
        // Queued at start-up; the scheduler may already have tried it, so any
        // status counts. A new library with nothing to recover queues nothing.
        assert_eq!(transcriptions(&service, "lost", None), 1);
        assert_eq!(transcriptions(&service, "half", None), 1);
        assert_eq!(transcriptions(&service, "clean", None), 0);
        assert_eq!(transcriptions(&service, "done", None), 0);
        // Already whole (as 0.1.23 left it): listed as ready, with no new pass.
        // Still owed work, or no audio at all, stays interrupted.
        let status = |id: &str| -> String {
            service.db.lock().unwrap().conn.query_row("SELECT status FROM meetings WHERE id=?", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(status("clean"), "ready");
        assert_eq!(status("half"), "interrupted");
        assert_eq!(status("empty"), "interrupted");
        assert_eq!(transcriptions(&service, "empty", None), 0);
        assert_eq!(transcriptions(&service, "stranded", None), 1);
        assert_eq!(transcriptions(&service, "cancelled", None), 1);
        // It resumes: the checkpoint and its unfinished sentence are still there.
        assert_eq!(service.request("transcribe", Some("stranded")).unwrap()["asr_progress"]["mic:s0000"]["covered_ms"], 28000);
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

    /// A recording cut short by a crash is "interrupted" until its recovery
    /// transcription completes; then the library lists it like any other.
    /// A stand-in worker reports each file's length and exits well, so the
    /// whole scheduler path runs: start, ingest, completion.
    #[test]
    fn an_interrupted_recording_is_ready_once_its_transcription_completes() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("worker")).unwrap();
        fs::write(
            temp.path().join("worker/engine.py"),
            r#"import json, sys
args = sys.argv
with open(args[args.index("--request") + 1], encoding="utf-8") as f:
    request = json.load(f)
with open(args[args.index("--response") + 1], "w", encoding="utf-8") as out:
    for track in request["tracks"]:
        out.write(json.dumps({"type": "track_duration", "track_id": track["id"], "duration_ms": 12000}) + "\n")
"#,
        )
        .unwrap();
        let service = Service::open(
            temp.path().join("data"),
            temp.path().join("cache"),
            temp.path().into(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let python = service.python();
        if !Path::new(&python).is_file() && !platform::on_path(&python) {
            eprintln!("SKIPPED: no Python to run the stand-in worker");
            service.shutdown();
            return;
        }
        fs::write(temp.path().join("data/models/ggml-base.en.bin"), b"base").unwrap();
        let _busy = service.inference.lock().unwrap();
        let job = {
            let db = service.db.lock().unwrap();
            db.create("lost", DEFAULT_TITLE, 0, "interrupted").unwrap();
            db.add_track("lost", &temp.path().join("s0000-mic-000000.flac"), "mic", 0).unwrap();
            db.add_track("lost", &temp.path().join("s0000-mic-000001.flac"), "mic", 30000).unwrap();
            // A recording whose transcription was cancelled is not touched.
            db.create("kept", DEFAULT_TITLE, 0, "interrupted").unwrap();
            db.add_track("kept", &temp.path().join("kept.flac"), "mic", 0).unwrap();
            let cancelled = db.enqueue("kept", "transcribe").unwrap();
            db.job_status(&cancelled, "cancelled", 0., None).unwrap();
            db.enqueue("lost", "transcribe").unwrap()
        };
        drop(_busy);
        let status = |sql: &str, id: &str| -> String {
            service.db.lock().unwrap().conn.query_row(sql, [id], |r| r.get(0)).unwrap()
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while ["pending", "running"].contains(&status("SELECT status FROM jobs WHERE id=?", &job).as_str()) {
            assert!(Instant::now() < deadline, "the transcription did not run");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            status("SELECT status||' '||COALESCE(error,'') FROM jobs WHERE id=?", &job).trim(),
            "completed"
        );
        let meeting = |id: &str| status("SELECT status||' '||duration_ms FROM meetings WHERE id=?", id);
        assert_eq!(meeting("lost"), "ready 42000");
        assert_eq!(meeting("kept"), "interrupted 0");
        assert_eq!(transcriptions(&service, "lost", Some("completed")), 1);
        service.shutdown();
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(|| format!("Missing {key}"))
}
/// The release page of a version the update check reported. Only this
/// repository's page for a plain version number ever reaches the browser.
fn release_page(version: &str) -> Result<String> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return Err("Unknown version".into());
    }
    Ok(format!("https://github.com/chencyberlab/VoxLoom/releases/tag/v{version}"))
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
            if path.is_dir() && (name.starts_with("request-") || name.starts_with("job-") || name == "playback") {
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
            playback: Mutex::new(()),
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
            // The recording never reached Stop, so it was never transcribed;
            // or a pass ended with a sentence it had not written yet. Either
            // way its words reach the transcript only through this pass.
            let owed: i64 = db
                .conn
                .query_row(
                    "SELECT (SELECT count(*) FROM audio_tracks WHERE meeting_id=?1 AND transcribed=0)+(SELECT count(*) FROM asr_progress WHERE meeting_id=?1 AND overlap_text LIKE '%\"pending\"%')",
                    [&meeting.id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if owed > 0 && self.transcription_model(&db).is_file() {
                let _ = db.enqueue(&meeting.id, "transcribe");
            } else if owed == 0 && !db.tracks(&meeting.id)?.is_empty() {
                // Every file read and every sentence written, by a pass that
                // never set the status back (0.1.23) or by one whose host
                // stopped just after it completed: the recording is whole.
                db.conn
                    .execute("UPDATE meetings SET status='ready' WHERE id=? AND status='interrupted'", [&meeting.id])
                    .map_err(err)?;
            }
        }
        // A finished recording whose pass began but never completed, with no
        // transcription left to finish it: the 0.1.22 upgrade removed failed
        // live passes, the only way back to those words. A queued, failed or
        // cancelled transcription stays the person's to retry, and a recording
        // never transcribed (no checkpoint) waits for Transcribe as before.
        let stranded = db
            .conn
            .prepare("SELECT id FROM meetings m WHERE status<>'interrupted' AND EXISTS(SELECT 1 FROM asr_progress WHERE meeting_id=m.id)
             AND (EXISTS(SELECT 1 FROM audio_tracks WHERE meeting_id=m.id AND transcribed=0) OR EXISTS(SELECT 1 FROM asr_progress WHERE meeting_id=m.id AND overlap_text LIKE '%\"pending\"%'))
             AND NOT EXISTS(SELECT 1 FROM jobs WHERE meeting_id=m.id AND kind='transcribe' AND status IN ('pending','running','failed','cancelled'))")
            .map_err(err)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)?;
        if self.transcription_model(&db).is_file() {
            for id in stranded {
                let _ = db.enqueue(&id, "transcribe");
            }
        }
        Ok(())
    }
    /// The selected transcription model. Setup installs only the chosen one,
    /// so either works alone: while the chosen model is not downloaded the
    /// other is used if it is there, and a recording is always transcribed.
    fn transcription_model(&self, db: &Db) -> PathBuf {
        let models =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        let (standard, accurate) = (models.join("ggml-base.en.bin"), models.join(PARAKEET_MODEL));
        let (chosen, other) = if db.setting("transcription_profile", "standard") == "accurate" {
            (accurate, standard)
        } else {
            (standard, accurate)
        };
        if !chosen.is_file() && other.is_file() {
            other
        } else {
            chosen
        }
    }
    fn python(&self) -> String {
        self.tool(&self.db.lock().unwrap(), "python")
    }
    /// The interpreter for setup and Word export, which need only the standard
    /// library. Linux uses the system python3, which also creates the LiteRT
    /// venv; the Windows package bundles one interpreter for everything.
    /// Takes the locked library: callers already hold it.
    /// Asks GitHub for the latest published release, only because the user
    /// clicked Check for updates. The worker sends nothing but the version.
    fn check_updates(&self) -> Result<Value> {
        let python = self.helper_python(&self.db.lock().unwrap());
        let output = process::python(python)
            .arg(self.resources.join("worker/updates.py"))
            .arg("--current")
            .arg(env!("CARGO_PKG_VERSION"))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "The update check could not start.".to_string())?;
        let found: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "The update check failed. Try again later.".to_string())?;
        if let Some(error) = found["error"].as_str() {
            return Err(error.into());
        }
        Ok(json!({"current":env!("CARGO_PKG_VERSION"),"latest":found["latest"],"newer":found["newer"] == true}))
    }
    fn helper_python(&self, db: &Db) -> String {
        if cfg!(windows) {
            self.tool(db, "python")
        } else {
            "python3".into()
        }
    }
    /// The engine or interpreter to run: the one saved in Settings, unless it
    /// is gone or is another copy's bundled tool. The Windows preview runs
    /// from wherever it was unzipped, and saving any setting used to store
    /// that copy's paths; after a newer copy replaced it, setup failed with
    /// "The system cannot find the path specified".
    fn tool(&self, db: &Db, tool: &str) -> String {
        let saved = db.setting(tool, "");
        let path = Path::new(&saved);
        // A bare command name is looked up on PATH when it runs.
        let bare = path.components().count() == 1;
        if saved.is_empty() || self.other_copy(path) || (!bare && !path.is_file()) {
            self.default_tool(tool)
        } else {
            saved
        }
    }
    /// Whether `path` is in the `engines` or `python` folder beside the app
    /// of a VoxLoom copy other than the running one. The running copy's own
    /// files, such as its generic engine build, stay the user's choice.
    fn other_copy(&self, path: &Path) -> bool {
        let Some(folder) = path.parent() else {
            return false;
        };
        let bundled = folder
            .file_name()
            .is_some_and(|name| ["engines", "python"].iter().any(|b| name.eq_ignore_ascii_case(b)));
        folder.parent().is_some_and(|copy| {
            bundled
                && copy.join(platform::exe("voxloom")).is_file()
                && fs::canonicalize(copy).ok() != fs::canonicalize(&self.resources).ok()
        })
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
            "transcribe_cli" => (
                Path::new(".runtimes/transcribe-build/bin").join(platform::exe("transcribe-cli")),
                Path::new("runtimes/transcribe/bin").join(platform::exe("transcribe-cli")),
                "transcribe-cli",
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
        let db = self.db.lock().unwrap();
        let models =
            PathBuf::from(db.setting("models_dir", &self.data.join("models").to_string_lossy()));
        let transcription = self.transcription_model(&db);
        let asr_engine = if transcription.ends_with(PARAKEET_MODEL) { "parakeet" } else { "whisper" };
        let mut v = json!({"kind":kind,"cache_dir":self.cache,"models":{"whisper":transcription,"embedding":models.join("embeddinggemma-2-740m.litertlm"),"summary":""},"threads":db.setting("threads","2").parse::<u32>().unwrap_or(2).clamp(1,8),"language":db.setting("language","en"),"asr_engine":asr_engine,"whisper_cli":self.tool(&db,"whisper_cli"),"transcribe_cli":self.tool(&db,"transcribe_cli"),"llama_cli":self.tool(&db,"llama_cli")});
        if kind == "summarize" {
            v["summary_style"] = json!(db.setting("summary_style", "counselling"));
            v["summary_prompt"] = json!(db.setting("summary_prompt", ""));
            if let Some(m) = meeting {
                // The person's own notes steer the summary; the worker keeps
                // them out of every command line, like the transcript.
                v["notes"] = json!(db
                    .conn
                    .query_row("SELECT notes FROM meetings WHERE id=?", [m], |r| r.get::<_, String>(0))
                    .unwrap_or_default());
            }
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
            // A pass resumes where the last one committed; starting over
            // clears this first (the "enqueue" action with "restart").
            if kind == "transcribe" {
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
                let db = self.db.lock().unwrap();
                for (path, source, offset) in tracks {
                    let _ = db.add_track(&c.meeting, &path, &source, offset);
                }
            }
        }
        // Nothing runs while a recording is open, paused included: the
        // laptop stays quiet (its fan is in the microphone) and the call
        // keeps the processor. Each recording is transcribed after Stop.
        if recording {
            return;
        }
        let next: Option<(String, String, String)> = {
            let db = self.db.lock().unwrap();
            db.conn.query_row("SELECT id,meeting_id,kind FROM jobs WHERE status='pending' ORDER BY CASE kind WHEN 'transcribe' THEN 0 WHEN 'index' THEN 1 ELSE 2 END,created_at,rowid LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().ok().flatten()
        };
        let Some((job, meeting, kind)) = next else {
            return;
        };
        let Ok(_guard) = self.inference.try_lock() else {
            return;
        };
        {
            // Starting over happens with the start, under the same lock, so
            // the request below reads the cleared checkpoint.
            let mut db = self.db.lock().unwrap();
            let _ = db.job_status(&job, "running", 0., None);
            if kind == "transcribe" {
                let _ = db.start_over(&job, &meeting);
            }
        }
        (self.notify)(json!({"type":"jobs"}));
        let result = self.request(&kind, Some(&meeting)).and_then(|req| {
            let result = self.run_worker(&job, &req, Some((&meeting, &kind)));
            if result.is_ok() && kind == "transcribe" {
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
        let recording = self.capture.lock().unwrap().is_some();
        let db = self.db.lock().unwrap();
        let status: String = db
            .conn
            .query_row("SELECT status FROM jobs WHERE id=?", [&job], |r| r.get(0))
            .unwrap_or_default();
        if status != "cancelled" {
            match result {
                Ok(_) => {
                    let _ = db.job_status(&job, "completed", 1., None);
                    let remaining: i64 = if kind == "transcribe" {
                        db.conn.query_row("SELECT count(*) FROM audio_tracks WHERE meeting_id=? AND transcribed=0",[&meeting],|r|r.get(0)).unwrap_or(0)
                    } else {
                        0
                    };
                    // Audio registered after this pass read the recording
                    // (start-up recovery) still needs its own pass.
                    if remaining > 0 {
                        let _ = db.enqueue(&meeting, "transcribe");
                    } else if kind == "transcribe" {
                        // A recording cut short by a crash is whole again:
                        // its files are registered, its length restored from
                        // them (`ingest`, track_duration) and its words
                        // written. Without this it stayed "interrupted" in
                        // the library for good.
                        let _ = db.conn.execute(
                            "UPDATE meetings SET status='ready' WHERE id=? AND status='interrupted'",
                            [&meeting],
                        );
                    }
                    if !recording
                        && remaining == 0
                        && kind == "transcribe"
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
                    // Stopped for a new recording or Quit: a transcription
                    // resumes from its last checkpoint when it runs again.
                    if recording || self.quit.load(Ordering::SeqCst) {
                        let _ = db.job_status(
                            &job,
                            "pending",
                            0.,
                            Some("Waiting until the recording ends / the app starts again"),
                        );
                    } else {
                        let _ = db.job_status(&job, "failed", 0., Some(&e));
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
        if capture_guard.is_some() {
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
                // Added below the notes, never in their place: what the
                // person wrote, and earlier summaries they may have edited,
                // stay exactly as they are. Nothing is trimmed, so a page
                // saving older notes can tell what was added (`update`).
                let text = v["text"].as_str().unwrap_or("").trim();
                if text.is_empty() {
                    return Err("The summary came back empty. Try Summarise again.".into());
                }
                let block = format!("{SUMMARY_TITLE}\n\n{text}");
                db.conn
                    .execute(
                        "UPDATE meetings SET notes=notes||CASE WHEN notes='' THEN '' ELSE char(10)||char(10) END||? WHERE id=?",
                        params![block, meeting],
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
            "open_microphone_settings" => audio::open_microphone_settings().map(|_| Value::Null),
            "check_updates" => self.check_updates(),
            "open_release" => {
                platform::open_in_browser(&release_page(text(&p, "version")?)?)
                    .map(|_| Value::Null)
                    .map_err(|e| format!("{e} The download is at github.com/chencyberlab/VoxLoom/releases."))
            }
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
                    // The page saves the notes it was editing, which it
                    // loaded as `notes_base`. A summary finished since then
                    // was added after that text: keep it below the edit.
                    let mut notes = n.to_string();
                    if let Some(base) = p["notes_base"].as_str() {
                        let current: String = db
                            .conn
                            .query_row("SELECT notes FROM meetings WHERE id=?", [id], |r| r.get(0))
                            .map_err(err)?;
                        if current != base && current.starts_with(base) {
                            // Added to empty notes, a summary has no blank line
                            // before its title: give it one below the edit.
                            let added = &current[base.len()..];
                            if !added.starts_with('\n') && !notes.trim().is_empty() {
                                let tail = notes.len() - notes.trim_end_matches('\n').len();
                                notes.push_str(&"\n\n"[tail.min(2)..]);
                            }
                            notes.push_str(added);
                        }
                    }
                    db.conn
                        .execute("UPDATE meetings SET notes=? WHERE id=?", params![notes, id])
                        .map_err(err)?;
                    return Ok(json!({"notes":notes}));
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
            "enqueue" => {
                let db = self.db.lock().unwrap();
                let (id, kind) = (text(&p, "id")?, text(&p, "kind")?);
                let job = db.enqueue(id, kind)?;
                // Transcribe on a recording's page starts over, but only when
                // its job starts (`Db::start_over`): cancelled before then, it
                // leaves the transcript as it was. Retry resumes instead.
                if kind == "transcribe" && p["restart"] == true {
                    db.conn
                        .execute("INSERT OR IGNORE INTO restart_jobs(job_id) VALUES(?)", [&job])
                        .map_err(err)?;
                }
                Ok(json!(job))
            }
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
                    let path = self.tool(&db, tool);
                    Path::new(&path).is_file() || platform::on_path(path)
                };
                let (whisper, transcribe, llama) =
                    (available("whisper_cli"), available("transcribe_cli"), available("llama_cli"));
                let python = self.tool(&db, "python");
                // Python can take seconds to start (antivirus, a cold disk) or hang: never
                // hold the library meanwhile, or nothing else (the quit question
                // included) can read it (issue #17).
                drop(db);
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
                    json!({"inference":inference,"whisper":whisper,"transcribe":transcribe,"llama":llama,"python":python,"storage":self.data,"disk_free_bytes":platform::free_disk_bytes(&self.data)}),
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
                    json!({"threads":db.setting("threads","2"),"language":db.setting("language","en"),"whisper_cli":self.tool(&db,"whisper_cli"),"transcribe_cli":self.tool(&db,"transcribe_cli"),"llama_cli":self.tool(&db,"llama_cli"),"python":self.tool(&db,"python"),"models_dir":db.setting("models_dir",&self.data.join("models").to_string_lossy()),"summary_profile":summary_profile(&db),"summary_model":db.setting("summary_model",""),"summary_style":db.setting("summary_style","counselling"),"summary_prompt":db.setting("summary_prompt",""),"transcription_profile":db.setting("transcription_profile","standard"),"appearance":db.setting("appearance","system"),"storage":self.data,"low_memory":true}),
                )
            }
            "save_settings" => {
                let db = self.db.lock().unwrap();
                for k in [
                    "threads",
                    "language",
                    "whisper_cli",
                    "transcribe_cli",
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
                            "transcribe_cli",
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
                        // Settings sends back every path it showed. A path the
                        // user did not edit stays as stored (a custom one that
                        // is missing for now survives), and choosing the default
                        // stores nothing, so the default follows the app.
                        if ["whisper_cli", "transcribe_cli", "llama_cli", "python"].contains(&k) {
                            if v == self.tool(&db, k) {
                                continue;
                            }
                            if v == self.default_tool(k) {
                                db.set_setting(k, "")?;
                                continue;
                            }
                        }
                        db.set_setting(k, v)?;
                    }
                }
                Ok(Value::Null)
            }
            "diagnostics" => {
                let tools: &[&str] = if cfg!(windows) {
                    &["ffmpeg", "ffprobe", "whisper-cli", "transcribe-cli", "llama-cli", "python"]
                } else {
                    &[
                        "ffmpeg",
                        "ffprobe",
                        "pactl",
                        "whisper-cli",
                        "transcribe-cli",
                        "llama-cli",
                        "python3",
                    ]
                };
                let tools = tools
                    .iter()
                    .map(|name| json!({"name":name,"available":platform::on_path(name)}))
                    .collect::<Vec<_>>();
                Ok(
                    json!({"tools":tools,"audio":audio::devices().ok(),"data":self.data,"network":"Only what you click: model setup downloads, and Check for updates asks GitHub for the latest version.","last_metrics":fs::read_to_string(self.cache.join("last-inference-metrics.json")).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
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
            "playback" => self.playback(text(&p, "id")?),
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
        if self.transcription_model(&db).is_file() {
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
            // Only the chosen transcription model is installed. Without a choice
            // (older callers) the saved one stays and its model is installed.
            let transcription = p["transcription_profile"].as_str();
            if transcription.is_some_and(|t| !["standard", "accurate"].contains(&t)) {
                return Err("Choose Standard or Accurate transcription.".into());
            }
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
            // One model at a time, each named: the chosen transcription model,
            // search, then only the chosen summary model. Accurate never pulls
            // in Standard, and E4B never pulls in the default E2B.
            let profile = transcription.map_or_else(
                || self.db.lock().unwrap().setting("transcription_profile", "standard"),
                str::to_string,
            );
            models.as_object_mut().unwrap().remove("core_only");
            for model in [if profile == "accurate" { "parakeet" } else { "whisper" }, "embedding"] {
                models["model"] = json!(model);
                self.provision(model_mode, &models)?;
            }
            if let Some(profile) = transcription {
                self.db
                    .lock()
                    .unwrap()
                    .set_setting("transcription_profile", profile)?;
            }
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
            if !["whisper", "parakeet", "embedding", "summary_e2b", "summary_e4b"].contains(&model) {
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
        // Refuse to begin on a nearly full disk instead of failing minutes in.
        if disk_too_full(platform::free_disk_bytes(&self.data)) {
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
        // Recording outranks every other job, so a worker in progress is
        // stopped; its job waits for the recording to end, and a
        // transcription resumes from its last checkpoint. Setup is left
        // alone: killing pip or a model download half-way can leave a broken
        // venv.
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
        // One pass over the whole recording, after Stop. Pause starts none:
        // the recording usually resumes, and nothing runs until it ends.
        if stop && self.transcription_model(&db).is_file() {
            let _ = db.enqueue(&c.meeting, "transcribe");
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
        // Decoded copies for playback go with the recording (start-up clears any left).
        let _ = fs::remove_dir_all(self.cache.join("playback").join(id));
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
    /// The file the webview plays for one track. A capture segment's FLAC
    /// frames carry their position in the whole recording and its header no
    /// length: WebKitGTK cannot seek in it, and both webviews ran its clock
    /// from that position instead of zero. It is decoded to WAV in the private
    /// cache, reused until the segment changes. Only the open recording's
    /// copies are kept; deleting the recording deletes them too.
    fn playback(&self, id: &str) -> Result<Value> {
        let (meeting, path): (String, String) = self
            .db
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT meeting_id,path FROM audio_tracks WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(err)?
            .ok_or("This audio is no longer in the library")?;
        let source = PathBuf::from(&path);
        let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with('s') && (name.contains("-mic-") || name.contains("-system-")) && name.ends_with(".flac")) {
            return Ok(json!({"path":path}));
        }
        // Choosing a part also prepares the next one, so the same part can be asked for
        // again mid-decode; the second caller waits and gets the finished copy.
        let _decoding = self.playback.lock().unwrap();
        let folder = self.cache.join("playback");
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy() != meeting {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        let target = folder.join(&meeting).join(format!("{id}.wav"));
        let modified = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
        if modified(&target) < modified(&source) || !target.is_file() {
            private_dir(target.parent().unwrap())?;
            let part = target.with_extension("part");
            let status = process::hidden("ffmpeg")
                .args(["-nostdin", "-protocol_whitelist", "file,pipe", "-v", "error", "-y", "-i"])
                .arg(&source)
                .args(["-vn", "-c:a", "pcm_s16le", "-f", "wav"])
                .arg(&part)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(err)?;
            if !status.success() {
                let _ = fs::remove_file(&part);
                return Err("This part of the recording could not be read for playback.".into());
            }
            fs::rename(&part, &target).map_err(err)?;
        }
        Ok(json!({"path":target}))
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
        let tracks = db.tracks(id)?;
        let helper = self.helper_python(&db);
        // Word and audio exports run a program for seconds on a long recording:
        // the library stays free for everything else meanwhile.
        drop(db);
        if format == "summary_docx" || format == "docx" {
            let summary = format == "summary_docx";
            let body = if summary {
                crate::meeting_notes(detail["meeting"]["notes"].as_str().unwrap_or(""))
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
                process::python(helper)
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
            "summary" => crate::meeting_notes(detail["meeting"]["notes"].as_str().unwrap_or("")),
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
    /// Something a quit would interrupt: a recording (paused included), a worker
    /// (a transcript, search, a summary or a question) or setup.
    pub fn busy(&self) -> bool {
        self.capture.lock().unwrap().is_some()
            || self.running.lock().unwrap().is_some()
            || self.setup_active.load(Ordering::SeqCst)
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
