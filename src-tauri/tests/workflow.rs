use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use voxloom_core::service::Service;

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
    s.dispatch("save_settings",json!({"python":root.join(".venv/bin/python"),"whisper_cli":root.join(".runtimes/whisper-build/bin/whisper-cli"),"llama_cli":root.join(".runtimes/llama-build/bin/llama-completion"),"models_dir":PathBuf::from(std::env::var_os("VOXLOOM_TEST_MODELS").unwrap_or_else(|| root.join(".local-data/models").into_os_string()))})).unwrap();
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
        .dispatch("import", json!({"path":"/tmp/voxloom-budget.wav"}))
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
    assert_eq!(
        s.dispatch("detail", json!({"id":id})).unwrap()["meeting"]["summary"],
        ""
    );
    s.dispatch("enqueue", json!({"id":id,"kind":"summarize"}))
        .unwrap();
    await_jobs(&s);
    measured.insert("summary".into(), read_metrics("summarize"));
    if let Ok(path) = std::env::var("VOXLOOM_TEST_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&measured).unwrap()).unwrap();
    }
    let summary = s.dispatch("detail", json!({"id":id})).unwrap();
    assert!(summary["meeting"]["summary"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("budget"));
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
    let children = voxloom_core::metrics::tree_pids(std::process::id())
        .into_iter()
        .filter(|p| *p != std::process::id())
        .collect::<Vec<_>>();
    assert!(!children.is_empty());
    reopened.shutdown();
    for pid in children {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        assert!(
            status.is_empty()
                || status
                    .lines()
                    .any(|l| l.starts_with("State:") && l.contains('Z')),
            "Worker {pid} stayed resident after shutdown"
        );
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

#[test]
#[ignore = "requires explicit synthetic PipeWire fixture setup"]
fn real_pulse_dual_capture_live_transcription_pause_resume_shutdown() {
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
    let started = Instant::now();
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
    let deadline = Instant::now() + Duration::from_secs(50);
    loop {
        let d = s.dispatch("detail", json!({"id":id})).unwrap();
        if d["segments"].as_array().unwrap().iter().any(|v| {
            v["text"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("budget")
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Live transcript did not arrive: {}",
            s.dispatch("jobs", json!({})).unwrap()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    // Live passes read the file still being written: text must not wait for
    // the first 30-second storage file to close.
    let first_text = started.elapsed();
    eprintln!("first live transcript after {first_text:?}");
    assert!(first_text < Duration::from_secs(30), "live text waited for a closed file");
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
