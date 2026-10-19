//! Linux capture through PulseAudio/PipeWire. No virtual sinks or routing
//! changes. A single FFmpeg process owns both inputs.
use crate::{err, process, Result};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    path::Path,
    process::{Child, Stdio},
};

pub const START_FAILED: &str = "Capture failed: device disconnected, access denied, or FFmpeg Pulse input unavailable. Refresh sources and retry.";

pub fn devices() -> Result<Value> {
    let out = process::hidden("pactl")
        .args(["--format=json", "list", "sources"])
        .output()
        .map_err(|_| "pactl is missing. Install pulseaudio-utils".to_string())?;
    if !out.status.success() {
        return Err(
            "Cannot connect to PipeWire/PulseAudio. Check your desktop audio session.".into(),
        );
    }
    let data: Value = serde_json::from_slice(&out.stdout).map_err(err)?;
    let default = process::hidden("pactl")
        .arg("get-default-source")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let sink = process::hidden("pactl")
        .arg("get-default-sink")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let sources=data.as_array().ok_or("Unexpected pactl source format")?.iter().map(|s|{
        let name=s["name"].as_str().unwrap_or("");let monitor=name.ends_with(".monitor")||s["monitor_of_sink"].as_str().map(|v|v!="n/a").unwrap_or(false);
        json!({"name":name,"label":s["description"],"monitor":monitor,"state":s["state"],"default":if monitor {name==format!("{sink}.monitor")}else{name==default}})
    }).collect::<Vec<_>>();
    Ok(json!(sources))
}

/// FFmpeg arguments for one capture session: each source becomes its own
/// 16 kHz mono FLAC segments, `sNNNN-<mic|system>-NNNNNN.flac`.
pub fn ffmpeg_args(
    folder: &Path,
    session: usize,
    mic: Option<&str>,
    system: Option<&str>,
) -> Result<Vec<OsString>> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "info", "-y"]
        .iter()
        .map(OsString::from)
        .collect();
    let sources = [mic, system].into_iter().flatten().collect::<Vec<_>>();
    if sources.is_empty() {
        return Err("Choose a microphone or system monitor".into());
    }
    for source in &sources {
        args.extend(
            ["-thread_queue_size", "512", "-f", "pulse", "-i", source]
                .iter()
                .map(OsString::from),
        );
    }
    for (i, source) in sources.iter().enumerate() {
        let kind = if mic == Some(*source) { "mic" } else { "system" };
        let filter=format!("aresample=async=1:first_pts=0,astats=metadata=1:reset=1,ametadata@{kind}=print:key=lavfi.astats.Overall.RMS_level");
        args.extend(
            [
                "-map",
                &format!("{i}:a"),
                "-af",
                &filter,
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "flac",
                "-f",
                "segment",
                "-segment_time",
                "30",
                // Write each FLAC frame as it is encoded, so live transcription
                // can read the file still being recorded seconds behind capture.
                "-segment_format_options",
                "flush_packets=1",
                "-reset_timestamps",
                "1",
            ]
            .iter()
            .map(OsString::from),
        );
        args.push(
            folder
                .join(format!("s{session:04}-{kind}-%06d.flac"))
                .into_os_string(),
        );
    }
    Ok(args)
}

/// Starts FFmpeg writing the session's segments. Its stderr carries the
/// per-source levels; "q" on stdin ends it cleanly.
pub fn spawn(
    folder: &Path,
    session: usize,
    mic: Option<&str>,
    system: Option<&str>,
) -> Result<Child> {
    process::spawn(
        process::command("ffmpeg")
            .args(ffmpeg_args(folder, session, mic, system)?)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )
    .map_err(|_| {
            "FFmpeg capture could not start. Install FFmpeg with PulseAudio input support."
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture command line is what live transcription depends on
    /// (segment names, flush_packets, levels). It must not drift.
    #[test]
    fn capture_arguments_are_unchanged() {
        let folder = Path::new("/data/recordings/id");
        let output = |kind: &str| {
            vec![
                "-map".to_string(),
                if kind == "mic" { "0:a" } else { "1:a" }.to_string(),
                "-af".into(),
                format!("aresample=async=1:first_pts=0,astats=metadata=1:reset=1,ametadata@{kind}=print:key=lavfi.astats.Overall.RMS_level"),
                "-ac".into(), "1".into(), "-ar".into(), "16000".into(), "-c:a".into(), "flac".into(),
                "-f".into(), "segment".into(), "-segment_time".into(), "30".into(),
                "-segment_format_options".into(), "flush_packets=1".into(), "-reset_timestamps".into(), "1".into(),
                format!("/data/recordings/id/s0003-{kind}-%06d.flac"),
            ]
        };
        let mut expected: Vec<String> = ["-hide_banner", "-loglevel", "info", "-y"]
            .map(String::from)
            .to_vec();
        for source in ["alsa_input.usb", "alsa_output.pci.monitor"] {
            expected.extend(["-thread_queue_size", "512", "-f", "pulse", "-i", source].map(String::from));
        }
        expected.extend(output("mic"));
        expected.extend(output("system"));
        let actual = ffmpeg_args(folder, 3, Some("alsa_input.usb"), Some("alsa_output.pci.monitor"))
            .unwrap()
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        let system_only = ffmpeg_args(folder, 0, None, Some("alsa_output.pci.monitor")).unwrap();
        assert_eq!(system_only[11], "0:a");
        assert!(system_only.last().unwrap().to_str().unwrap().ends_with("s0000-system-%06d.flac"));
        assert!(ffmpeg_args(folder, 0, None, None).is_err());
    }
}
