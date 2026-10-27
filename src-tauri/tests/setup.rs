// Linux setup installs LiteRT into a venv with the system python3, stubbed here
// with /bin/true. The Windows package bundles its runtime and needs its own test.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use voxloom_core::service::Service;

#[test]
fn offline_setup_installs_only_selected_models_and_saves_successful_choices() {
    // Tiny integrity-checked files exercise real provisioning, not inference.
    // /bin/true explicitly stubs the runtime readiness probe so no dependencies
    // or model weights are downloaded by this orchestration regression test.
    // Transcription: no choice keeps the saved one; only the chosen model is
    // installed (Accurate without Standard), and a choice is saved only once
    // its model is installed.
    for (profile, core_only, missing, transcription, before) in [
        ("e2b", false, None, None, "standard"),
        ("e4b", false, None, None, "accurate"),
        ("e2b", true, None, None, "standard"),
        ("e2b", false, Some("summary_e2b"), None, "standard"),
        ("unknown", false, None, None, "standard"),
        // Gemma 3 1B was removed in 0.1.8.
        ("1b", false, None, None, "standard"),
        ("e2b", true, None, Some("accurate"), "standard"),
        ("e4b", false, None, Some("accurate"), "standard"),
        ("e2b", false, None, Some("standard"), "accurate"),
        ("e2b", false, Some("parakeet"), Some("accurate"), "standard"),
        // Accurate needs no Standard file; Standard alone still needs its own.
        ("e2b", false, Some("whisper"), Some("accurate"), "standard"),
        ("e2b", false, Some("whisper"), None, "standard"),
        ("e2b", false, None, Some("best"), "standard"),
    ] {
        let valid = ["e2b", "e4b"].contains(&profile)
            && transcription.is_none_or(|t| ["standard", "accurate"].contains(&t));
        let accurate = transcription.unwrap_or(before) == "accurate";
        let transcribed = valid && missing != Some(if accurate { "parakeet" } else { "whisper" });
        let summary = format!("summary_{profile}");
        let summarised = transcribed && !core_only && missing != Some(summary.as_str());
        let expected = summarised.then_some(summary.as_str());
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
            ("parakeet", "parakeet-tdt-0.6b-v2-Q6_K.gguf", true),
        ];
        let manifest: Vec<_> = files.iter().map(|(id, filename, optional)| {
            if missing != Some(*id) {
                fs::write(bundle.join("models").join(filename), b"abc").unwrap();
            }
            json!({"id":id,"name":id,"filename":filename,"bytes":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","optional":optional,"manual_only":(["summary_e4b","parakeet"].contains(id))})
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
                json!({"python":"/bin/true","summary_profile":"e2b","transcription_profile":before}),
            )
            .unwrap();
        let mut request = json!({"mode":"offline_bundle","path":bundle,"summary_profile":profile,"core_only":core_only});
        if let Some(choice) = transcription {
            request["transcription_profile"] = json!(choice);
        }
        service.dispatch("setup", request).unwrap();
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
            transcribed && (core_only || summarised),
            "{done}"
        );
        for (id, filename, _) in files {
            let installed = match id {
                "whisper" => transcribed && !accurate,
                "embedding" => transcribed,
                "parakeet" => transcribed && accurate,
                _ => expected == Some(id),
            };
            assert_eq!(data.join("models").join(filename).exists(), installed, "profile={profile}, core_only={core_only}, missing={missing:?}, transcription={transcription:?}, model={id}");
        }
        let settings = service.dispatch("settings", json!({})).unwrap();
        assert_eq!(settings["summary_profile"], if summarised { profile } else { "e2b" });
        assert_eq!(
            settings["transcription_profile"],
            if transcribed { transcription.unwrap_or(before) } else { before },
            "profile={profile}, transcription={transcription:?}"
        );
        service.shutdown();
    }
}
