use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use voxloom_core::service::Service;

#[test]
fn offline_setup_installs_only_selected_summary_and_saves_successful_choice() {
    // Tiny integrity-checked files exercise real provisioning, not inference.
    // /bin/true explicitly stubs the runtime readiness probe so no dependencies
    // or model weights are downloaded by this orchestration regression test.
    for (profile, core_only, missing_summary, expected) in [
        ("e2b", false, false, Some("summary_e2b")),
        ("e4b", false, false, Some("summary_e4b")),
        ("e2b", true, false, None),
        ("e2b", false, true, None),
        ("unknown", false, false, None),
        // Gemma 3 1B was removed in 0.1.8.
        ("1b", false, false, None),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let resources = temp.path().join("resources");
        let bundle = temp.path().join("offline café folder");
        fs::create_dir_all(resources.join("worker")).unwrap();
        fs::create_dir_all(resources.join("models")).unwrap();
        fs::create_dir_all(bundle.join("models")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../worker/provision.py"),
            resources.join("worker/provision.py"),
        )
        .unwrap();
        let files = [
            ("whisper", "ggml-base.en.bin", false),
            ("embedding", "embeddinggemma-2-740m.litertlm", false),
            ("summary_e2b", "gemma-4-E2B-it-Q4_0.gguf", true),
            ("summary_e4b", "gemma-4-E4B-it-Q4_0.gguf", true),
        ];
        let manifest: Vec<_> = files.iter().map(|(id, filename, optional)| {
            if !missing_summary || *id != "summary_e2b" {
                fs::write(bundle.join("models").join(filename), b"abc").unwrap();
            }
            json!({"id":id,"name":id,"filename":filename,"bytes":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","optional":optional,"manual_only":*id=="summary_e4b"})
        }).collect();
        fs::write(
            resources.join("models/manifest.json"),
            json!({"models":manifest}).to_string(),
        )
        .unwrap();
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let sink = events.clone();
        let data = temp.path().join("data");
        let service = Service::open(
            data.clone(),
            temp.path().join("cache"),
            resources,
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        )
        .unwrap();
        service
            .dispatch(
                "save_settings",
                json!({"python":"/bin/true","summary_profile":"e2b"}),
            )
            .unwrap();
        service.dispatch("setup", json!({"mode":"offline_bundle","path":bundle,"summary_profile":profile,"core_only":core_only})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let done = loop {
            if let Some(event) = events
                .lock()
                .unwrap()
                .iter()
                .find(|event| event["type"] == "setup_done")
                .cloned()
            {
                break event;
            }
            assert!(Instant::now() < deadline, "Setup did not finish");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(
            done["error"].is_null(),
            !missing_summary && !["unknown", "1b"].contains(&profile),
            "{done}"
        );
        for (id, filename, _) in files {
            let installed = if id == "whisper" || id == "embedding" {
                !["unknown", "1b"].contains(&profile)
            } else {
                expected == Some(id)
            };
            assert_eq!(data.join("models").join(filename).exists(), installed, "profile={profile}, core_only={core_only}, missing_summary={missing_summary}, model={id}");
        }
        let settings = service.dispatch("settings", json!({})).unwrap();
        assert_eq!(
            settings["summary_profile"],
            match expected {
                Some("summary_e2b") => "e2b",
                Some("summary_e4b") => "e4b",
                _ => "e2b",
            }
        );
        service.shutdown();
    }
}
