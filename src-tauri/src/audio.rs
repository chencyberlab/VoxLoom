//! Capture and stored audio. Finding devices and starting the capture process
//! are per platform (`pulse` on Linux, `wasapi` on Windows); session files,
//! segment bookkeeping, durations and export are shared. Segmented FLAC keeps
//! completed chunks recoverable.
use crate::{db::Track, err, process, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod pulse;
#[cfg(target_os = "linux")]
use pulse as backend;
#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
use wasapi as backend;

pub use backend::devices;

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
        let mut child = backend::spawn(
            &self.folder,
            self.session,
            self.mic.as_deref(),
            self.system.as_deref(),
        )?;
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
            return Err(backend::START_FAILED.into());
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
                process::kill_tree(child.id());
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
    let out = process::hidden("ffprobe")
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
    let result = process::command("ffmpeg")
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
    use std::process::Command;
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
