//! Linux boundary. No virtual sinks or routing changes. A single FFmpeg process
//! owns both inputs; segmented FLAC keeps completed chunks recoverable.
use crate::{db::Track, err, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    // A killed host must not leave a recorder or model resident. All descendants
    // join a dedicated process group; orderly shutdown also kills that group.
    unsafe {
        c.pre_exec(|| {
            libc::setpgid(0, 0);
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    c
}
pub fn kill_group(pid: u32, signal: i32) {
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}
pub fn devices() -> Result<Value> {
    let out = Command::new("pactl")
        .args(["--format=json", "list", "sources"])
        .output()
        .map_err(|_| "pactl is missing. Install pulseaudio-utils".to_string())?;
    if !out.status.success() {
        return Err(
            "Cannot connect to PipeWire/PulseAudio. Check your desktop audio session.".into(),
        );
    }
    let data: Value = serde_json::from_slice(&out.stdout).map_err(err)?;
    let default = Command::new("pactl")
        .arg("get-default-source")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let sink = Command::new("pactl")
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

pub struct Capture {
    pub meeting: String,
    pub folder: PathBuf,
    pub mic: Option<String>,
    pub system: Option<String>,
    child: Option<Child>,
    started: Option<Instant>,
    elapsed_ms: i64,
    session: usize,
    pub levels: Arc<Mutex<[f64; 2]>>,
    pub paused: bool,
    pub registered: Vec<PathBuf>,
}
impl Capture {
    pub fn new(
        meeting: String,
        folder: PathBuf,
        mic: Option<String>,
        system: Option<String>,
    ) -> Self {
        Self {
            meeting,
            folder,
            mic,
            system,
            child: None,
            started: None,
            elapsed_ms: 0,
            session: 0,
            levels: Arc::new(Mutex::new([-90.; 2])),
            paused: false,
            registered: vec![],
        }
    }
    pub fn start(&mut self) -> Result<()> {
        fs::write(
            self.folder
                .join(format!("session-{:04}.json", self.session)),
            serde_json::to_vec(&json!({"offset_ms":self.elapsed_ms})).map_err(err)?,
        )
        .map_err(err)?;
        let mut cmd = command("ffmpeg");
        cmd.args(["-hide_banner", "-loglevel", "info", "-y"]);
        let sources = [self.mic.as_ref(), self.system.as_ref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Err("Choose a microphone or system monitor".into());
        }
        for source in &sources {
            cmd.args(["-thread_queue_size", "512", "-f", "pulse", "-i", source]);
        }
        for (i, source) in sources.iter().enumerate() {
            let kind = if self.mic.as_ref() == Some(source) {
                "mic"
            } else {
                "system"
            };
            let filter=format!("aresample=async=1:first_pts=0,astats=metadata=1:reset=1,ametadata@{kind}=print:key=lavfi.astats.Overall.RMS_level");
            cmd.args([
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
            ]);
            cmd.arg(
                self.folder
                    .join(format!("s{:04}-{kind}-%06d.flac", self.session)),
            );
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| {
                "FFmpeg capture could not start. Install FFmpeg with PulseAudio input support."
                    .to_string()
            })?;
        let stderr = child.stderr.take().unwrap();
        let levels = self.levels.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Some((_, v)) = line.split_once("lavfi.astats.Overall.RMS_level=") {
                    if let Ok(db) = v.parse::<f64>() {
                        if let Ok(mut l) = levels.lock() {
                            let index = if line.contains("ametadata@system") {
                                1
                            } else {
                                0
                            };
                            l[index] = db.clamp(-90., 0.);
                        }
                    }
                }
            }
        });
        std::thread::sleep(Duration::from_millis(180));
        if child.try_wait().map_err(err)?.is_some() {
            return Err("Capture failed: device disconnected, access denied, or FFmpeg Pulse input unavailable. Refresh sources and retry.".into());
        }
        self.child = Some(child);
        self.started = Some(Instant::now());
        self.paused = false;
        Ok(())
    }
    pub fn elapsed(&self) -> i64 {
        self.elapsed_ms
            + self
                .started
                .map(|x| x.elapsed().as_millis() as i64)
                .unwrap_or(0)
    }
    /// (path, source, index) of every file in the active session.
    fn session_files(&self) -> Result<Vec<(PathBuf, String, i64)>> {
        let mut files = vec![];
        for entry in fs::read_dir(&self.folder).map_err(err)? {
            let path = entry.map_err(err)?.path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with(&format!("s{:04}-", self.session)) || !name.ends_with(".flac") {
                continue;
            }
            let index = name
                .trim_end_matches(".flac")
                .rsplit('-')
                .next()
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            files.push((
                path.clone(),
                if name.contains("-mic-") {
                    "mic".to_string()
                } else {
                    "system".to_string()
                },
                index,
            ));
        }
        Ok(files)
    }
    /// The file each source is still writing. It is never registered as a
    /// track; live transcription reads it up to its last complete frame.
    pub fn open_tracks(&self) -> Result<Vec<(PathBuf, String, i64)>> {
        if self.paused || self.child.is_none() {
            return Ok(vec![]);
        }
        let files = self.session_files()?;
        Ok(files
            .iter()
            .filter(|(path, source, index)| {
                !self.registered.contains(path)
                    && !files.iter().any(|(_, s, i)| s == source && i > index)
            })
            .map(|(path, source, index)| {
                (path.clone(), source.clone(), self.elapsed_ms + index * 30000)
            })
            .collect())
    }
    pub fn closed_tracks(&mut self) -> Result<Vec<(PathBuf, String, i64)>> {
        let files = self.session_files()?;
        let mut ready = vec![];
        for (path, source, index) in &files {
            // The newest file for each source is still being written.
            if !self.registered.contains(path)
                && files.iter().any(|(_, s, i)| s == source && i > index)
            {
                ready.push((
                    path.clone(),
                    source.clone(),
                    self.elapsed_ms + index * 30000,
                ));
                self.registered.push(path.clone());
            }
        }
        Ok(ready)
    }
    pub fn status(&mut self) -> Value {
        let failed = self
            .child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
            .is_some();
        json!({"meeting_id":self.meeting,"elapsed_ms":self.elapsed(),"paused":self.paused,"failed":failed,"levels":*self.levels.lock().unwrap()})
    }
    pub fn pause(&mut self) -> Result<Vec<(PathBuf, String, i64)>> {
        let offset = self.elapsed_ms;
        if let Some(mut child) = self.child.take() {
            if let Some(mut input) = child.stdin.take() {
                let _ = input.write_all(b"q\n");
            }
            let deadline = Instant::now() + Duration::from_secs(4);
            while child.try_wait().map_err(err)?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if child.try_wait().map_err(err)?.is_none() {
                kill_group(child.id(), libc::SIGKILL);
                let _ = child.wait();
            }
        }
        self.elapsed_ms = self.elapsed();
        self.started = None;
        self.paused = true;
        *self.levels.lock().unwrap() = [-90.; 2];
        let mut tracks = vec![];
        for entry in fs::read_dir(&self.folder).map_err(err)? {
            let path = entry.map_err(err)?.path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with(&format!("s{:04}-", self.session))
                || !name.ends_with(".flac")
                || self.registered.contains(&path)
            {
                continue;
            }
            let index = name
                .trim_end_matches(".flac")
                .rsplit('-')
                .next()
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let source = if name.contains("-mic-") {
                "mic"
            } else {
                "system"
            };
            tracks.push((path.clone(), source.into(), offset + index * 30000));
            self.registered.push(path);
        }
        self.session += 1;
        Ok(tracks)
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.pause();
    }
}

pub fn duration(path: &Path) -> Result<i64> {
    let out = Command::new("ffprobe")
        .args([
            "-protocol_whitelist",
            "file,pipe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .map_err(|_| "FFprobe is missing. Install FFmpeg.".to_string())?;
    let secs = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| "The selected file has no readable audio duration".to_string())?;
    if !out.status.success() || !secs.is_finite() || secs <= 0. {
        return Err("Invalid audio file".into());
    }
    Ok((secs * 1000.) as i64)
}
pub fn export_audio(tracks: &[Track], target: &Path) -> Result<()> {
    if tracks.is_empty() {
        return Err("Recording has no audio".into());
    }
    if tracks.len() == 1 {
        fs::copy(&tracks[0].path, target).map_err(err)?;
        return Ok(());
    }
    let list = target.with_file_name(format!(".voxloom-export-{}.ffconcat", crate::db::id()));
    let mut contents = String::from("ffconcat version 1.0\n");
    for track in tracks {
        if track.path.contains(['\n', '\r']) {
            return Err(
                "Audio export does not support newline characters in the storage path".into(),
            );
        }
        contents.push_str(&format!("file '{}'\n", track.path.replace('\'', "'\\''")));
    }
    fs::write(&list, contents).map_err(err)?;
    let result = command("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list)
        .args(["-c:a", "flac"])
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = fs::remove_file(list);
    if !result.map_err(err)?.success() {
        return Err("Could not export the complete audio source".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_complete_source_with_space_unicode_and_quote_paths() {
        if Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("SKIPPED: FFmpeg not installed");
            return;
        }
        let dir = tempfile::Builder::new()
            .prefix("voxloom café ' ")
            .tempdir()
            .unwrap();
        let first = dir.path().join("first.flac");
        let second = dir.path().join("second.flac");
        let target = dir.path().join("joined.flac");
        assert!(Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.2",
                "-ar",
                "16000",
                "-ac",
                "1"
            ])
            .arg(&first)
            .status()
            .unwrap()
            .success());
        fs::copy(&first, &second).unwrap();
        let tracks = vec![
            Track {
                id: "1".into(),
                meeting_id: "m".into(),
                path: first.to_string_lossy().to_string(),
                source: "mic".into(),
                offset_ms: 0,
            },
            Track {
                id: "2".into(),
                meeting_id: "m".into(),
                path: second.to_string_lossy().to_string(),
                source: "mic".into(),
                offset_ms: 200,
            },
        ];
        export_audio(&tracks, &target).unwrap();
        let ms = duration(&target).unwrap();
        assert!((380..=420).contains(&ms), "{ms}");
    }
}
