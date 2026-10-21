// Opt-in real-engine checks. The import workflow runs on Linux and Windows;
// capture is driven by PipeWire tools and runs on Linux only.
//   VOXLOOM_TEST_ROOT      checkout with worker/ and models/ (required)
//   VOXLOOM_TEST_MODELS    models folder (default ROOT/.local-data/models)
//   VOXLOOM_TEST_FIXTURES  folder of scripts/make-fixtures.sh output (default /tmp)
//   VOXLOOM_TEST_PYTHON, VOXLOOM_TEST_WHISPER_CLI, VOXLOOM_TEST_LLAMA_CLI  engines
//                          (default: the Linux checkout layout under ROOT)
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use voxloom_core::service::Service;

fn env_path(name: &str, default: PathBuf) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or(default)
}

/// Processes the test host started, for the shutdown check.
fn children() -> Vec<u32> {
    #[cfg(target_os = "linux")]
    let pids = voxloom_core::metrics::tree_pids(std::process::id());
    #[cfg(windows)]
    let pids = voxloom_core::process::contained_pids().unwrap_or_default();
    pids.into_iter().filter(|p| *p != std::process::id()).collect()
}

fn gone(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        status.is_empty() || status.lines().any(|l| l.starts_with("State:") && l.contains('Z'))
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return true;
        }
        let mut code = 0;
        GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        code != 259 // STILL_ACTIVE
    }
}

fn await_jobs(s: &Arc<Service>) {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let jobs = s.dispatch("jobs", json!({})).unwrap();
        let jobs = jobs.as_array().unwrap();
        if let Some(f) = jobs.iter().find(|v| v["status"] == "failed") {
            panic!("Job failed: {f}");
        }
        if jobs
            .iter()
            .all(|v| v["status"] == "completed" || v["status"] == "cancelled")
        {
            break;
        }
        assert!(Instant::now() < deadline, "Job timed out: {jobs:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "requires provisioned local models and generated speech fixtures"]
fn real_offline_import_transcribe_index_search_restart_export_delete() {
    let Ok(root) = std::env::var("VOXLOOM_TEST_ROOT") else {
        eprintln!("SKIPPED: model files not provisioned; set VOXLOOM_TEST_ROOT to run real engine acceptance");
        return;
    };
    let root = PathBuf::from(root);
    // Windows: the test is its own host, so its Job Object lists the workers.
    voxloom_core::process::contain_children();
    let fixtures = env_path("VOXLOOM_TEST_FIXTURES", PathBuf::from("/tmp"));
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("library café with spaces");
    let cache = temp.path().join("cache");
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let s = Service::open(
        data.clone(),
        cache.clone(),
        root.clone(),
        Arc::new(move |event| event_sink.lock().unwrap().push(event)),
    )
    .unwrap();
    s.dispatch("save_settings",json!({"python":env_path("VOXLOOM_TEST_PYTHON",root.join(".venv/bin/python")),"whisper_cli":env_path("VOXLOOM_TEST_WHISPER_CLI",root.join(".runtimes/whisper-build/bin/whisper-cli")),"llama_cli":env_path("VOXLOOM_TEST_LLAMA_CLI",root.join(".runtimes/llama-build/bin/llama-completion")),"models_dir":env_path("VOXLOOM_TEST_MODELS",root.join(".local-data/models"))})).unwrap();
    assert_eq!(
        s.dispatch("runtime_status", json!({})).unwrap()["inference"],
        true
    );
    s.dispatch("setup", json!({"mode":"recommended","core_only":false}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let done = events
            .lock()
            .unwrap()
            .iter()
            .find(|v| v["type"] == "setup_done")
            .cloned();
        if let Some(done) = done {
            assert!(done["error"].is_null(), "One-click setup failed: {done}");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let imported = s
        .dispatch("import", json!({"path":fixtures.join("voxloom-budget.wav")}))
        .unwrap();
    let id = imported["id"].as_str().unwrap();
    await_jobs(&s);
    let d = s.dispatch("detail", json!({"id":id})).unwrap();
    let mut measured = serde_json::Map::new();
    let read_metrics = |kind: &str| {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(cache.join(format!("metrics-{kind}.json"))).unwrap(),
        )
        .unwrap()
    };
    measured.insert("transcribe".into(), read_metrics("transcribe"));
    assert!(d["segments"].as_array().unwrap().iter().any(|v| v["text"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("budget")));
    measured.insert("index".into(), read_metrics("index"));
    let count: i64 =
        s.db.lock()
            .unwrap()
            .conn
            .query_row("SELECT count(*) FROM embedding_items", [], |r| r.get(0))
            .unwrap();
    s.dispatch("enqueue", json!({"id":id,"kind":"index"}))
        .unwrap();
    await_jobs(&s);
    assert_eq!(
        s.db.lock()
            .unwrap()
            .conn
            .query_row("SELECT count(*) FROM embedding_items", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        count,
        "Resumed indexing duplicated vectors"
    );
    let hits = s
        .dispatch(
            "search",
            json!({"query":"project budget invoice","mode":"semantic","filter":{}}),
        )
        .unwrap();
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["modality"] == "audio"));
    let exact = s
        .dispatch(
            "search",
            json!({"query":"budget","mode":"exact","filter":{}}),
        )
        .unwrap();
    assert!(!exact.as_array().unwrap().is_empty());
    // Nothing is summarised until asked. The person's own note goes to the
    // real model as the NOTES block and must stay exactly as typed.
    assert_eq!(
        s.dispatch("detail", json!({"id":id})).unwrap()["meeting"]["notes"],
        ""
    );
    let typed = "- ask who signs off the budget";
    s.dispatch("update", json!({"id":id,"notes":typed,"notes_base":""}))
        .unwrap();
    s.dispatch("enqueue", json!({"id":id,"kind":"summarize"}))
        .unwrap();
    await_jobs(&s);
    measured.insert("summary".into(), read_metrics("summarize"));
    if let Ok(path) = std::env::var("VOXLOOM_TEST_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&measured).unwrap()).unwrap();
    }
    let notes = s.dispatch("detail", json!({"id":id})).unwrap()["meeting"]["notes"]
        .as_str()
        .unwrap()
        .to_string();
    let added = notes
        .strip_prefix(&format!("{typed}\n\n# Summary\n\n"))
        .unwrap_or_else(|| panic!("The summary did not go below the notes: {notes:?}"));
    assert!(added.to_lowercase().contains("budget"), "{added}");
    let answer=s.dispatch("answer",json!({"query":"What was approved?","evidence":hits.as_array().unwrap().iter().take(2).collect::<Vec<_>>()})).unwrap();
    assert!(answer
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["type"] == "answer"
            && v["text"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("budget")));
    for format in ["srt", "txt", "md", "json", "docx", "summary_docx"] {
        let target = temp.path().join(format!("export.{format}"));
        s.dispatch("export", json!({"id":id,"format":format,"path":target}))
            .unwrap();
        assert!(std::fs::metadata(target).unwrap().len() > 10);
    }
    s.shutdown();
    drop(s);
    let reopened = Service::open(data, cache, root, Arc::new(|_| {})).unwrap();
    let d = reopened.dispatch("detail", json!({"id":id})).unwrap();
    assert!(!d["segments"].as_array().unwrap().is_empty());
    let running_job = reopened
        .dispatch("enqueue", json!({"id":id,"kind":"index"}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while voxloom_core::metrics::tree_rss_kib(std::process::id()) < 200_000 {
        assert!(
            Instant::now() < deadline,
            "Could not observe a real resident model for shutdown test"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let children = children();
    assert!(!children.is_empty());
    reopened.shutdown();
    // Windows releases a killed process a moment after TerminateJobObject.
    let deadline = Instant::now() + Duration::from_secs(5);
    while children.iter().any(|pid| !gone(*pid)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in children {
        assert!(gone(pid), "Worker {pid} stayed resident after shutdown");
    }
    reopened
        .dispatch("cancel", json!({"id":running_job}))
        .unwrap();
    reopened.dispatch("delete", json!({"id":id})).unwrap();
    assert_eq!(reopened.dispatch("library", json!({})).unwrap(), json!([]));
    assert_eq!(
        reopened
            .dispatch(
                "search",
                json!({"query":"budget","mode":"exact","filter":{}})
            )
            .unwrap(),
        json!([])
    );
    reopened.shutdown();
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires explicit synthetic PipeWire fixture setup"]
fn real_pulse_dual_capture_transcribed_after_stop_pause_resume_shutdown() {
    let (Ok(root), Ok(mic), Ok(system)) = (
        std::env::var("VOXLOOM_TEST_ROOT"),
        std::env::var("VOXLOOM_TEST_MIC"),
        std::env::var("VOXLOOM_TEST_MONITOR"),
    ) else {
        eprintln!(
            "SKIPPED: synthetic PipeWire sources not provisioned; run scripts/test-capture.py"
        );
        return;
    };
    let root = PathBuf::from(root);
    let temp = tempfile::tempdir().unwrap();
    let s = Service::open(
        temp.path().join("data"),
        temp.path().join("cache"),
        root.clone(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let whisper = std::env::var_os("VOXLOOM_TEST_WHISPER_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".runtimes/whisper-build/bin/whisper-cli"));
    s.dispatch("save_settings",json!({"python":root.join(".venv/bin/python"),"whisper_cli":whisper,"models_dir":PathBuf::from(std::env::var_os("VOXLOOM_TEST_MODELS").unwrap_or_else(|| root.join(".local-data/models").into_os_string()))})).unwrap();
    let result = s
        .dispatch(
            "record",
            json!({"title":"Synthetic dual-source fixture","mic":mic,"system":system}),
        )
        .unwrap();
    let id = result["id"].as_str().unwrap();
    let mut playback = std::process::Command::new("paplay")
        .args(["--device=voxloom_fixture", "/tmp/voxloom-long.wav"])
        .spawn()
        .unwrap();
    // Nothing is transcribed while recording: no job and no rows until Stop.
    let until = Instant::now() + Duration::from_secs(35);
    while Instant::now() < until && playback.try_wait().unwrap().is_none() {
        let d = s.dispatch("detail", json!({"id":id})).unwrap();
        assert!(d["segments"].as_array().unwrap().is_empty(), "transcribed while recording");
        let jobs = s.dispatch("jobs", json!({})).unwrap();
        assert!(jobs.as_array().unwrap().is_empty(), "a job was queued while recording: {jobs}");
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(!s.dispatch("status", json!({})).unwrap().is_null());
    s.dispatch("pause", json!({})).unwrap();
    let paused = s.dispatch("status", json!({})).unwrap();
    assert_eq!(paused["paused"], true);
    let elapsed = paused["elapsed_ms"].as_i64().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        s.dispatch("status", json!({})).unwrap()["elapsed_ms"],
        elapsed
    );
    s.dispatch("resume", json!({})).unwrap();
    std::thread::sleep(Duration::from_secs(1));
    s.dispatch("stop", json!({})).unwrap();
    await_jobs(&s);
    let d = s.dispatch("detail", json!({"id":id})).unwrap();
    assert!(
        d["segments"].as_array().unwrap().iter().any(|v| v["text"].as_str().unwrap().to_lowercase().contains("budget")),
        "transcript after Stop: {}",
        d["segments"]
    );
    assert_eq!(
        s.db.lock()
            .unwrap()
            .conn
            .query_row(
                "SELECT count(*) FROM audio_tracks WHERE transcribed=0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0,
        "Final recording tail was not transcribed"
    );
    s.dispatch(
        "record",
        json!({"title":"Shutdown while capturing","mic":mic,"system":system}),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    s.shutdown();
    let _ = playback.kill();
    let _ = playback.wait();
    let detail = s.dispatch("detail", json!({"id":id})).unwrap();
    let tracks = detail["tracks"].as_array().unwrap();
    assert!(tracks.iter().any(|t| t["source"] == "mic"));
    assert!(tracks.iter().any(|t| t["source"] == "system"));
    assert!(s.dispatch("status", json!({})).unwrap().is_null());
}
