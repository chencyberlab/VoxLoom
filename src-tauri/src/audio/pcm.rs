//! Capture from raw PCM on FFmpeg's stdin, for platforms where FFmpeg cannot
//! open the devices itself (Windows: WASAPI microphones and loopback). The
//! capture code places each source's packets on one clock (`Timeline`) and
//! writes interleaved 16-bit frames; FFmpeg splits them into the same
//! per-source FLAC segments and level lines as the Linux capture.
use crate::Result;
use std::{collections::VecDeque, ffi::OsString, path::Path};

/// The rate every source is captured at; Windows converts each device to it.
pub const RATE: u32 = 48_000;

/// A packet starting this close to where the previous one ended continues it:
/// device timestamps jitter by a few milliseconds, and placing every packet
/// exactly would cut tiny gaps into speech.
const TOLERANCE: u64 = RATE as u64 / 50;

/// Frames of `RATE` in a span of 100-nanosecond units (WASAPI's clock).
pub fn frames(hundred_ns: u64) -> u64 {
    (hundred_ns as u128 * RATE as u128 / 10_000_000) as u64
}

/// The sources' audio on one clock, frame 0 being the start of the session.
/// Wherever a source heard nothing (loopback while nothing plays, a
/// disconnected microphone) it is silent, so every source's files stay in
/// step with the elapsed time. Captured audio is never dropped: a packet
/// that arrives after its place was written out goes right after it instead.
pub struct Timeline {
    /// The first frame the next `take` returns.
    emitted: u64,
    sources: Vec<Source>,
}

#[derive(Default)]
struct Source {
    /// Samples from `emitted` on.
    samples: VecDeque<i16>,
    /// Where the next packet continues, once one has arrived.
    next: Option<u64>,
}

impl Timeline {
    pub fn new(sources: usize) -> Self {
        Self {
            emitted: 0,
            sources: (0..sources).map(|_| Source::default()).collect(),
        }
    }

    /// Adds a packet whose first frame was captured at frame `at`.
    pub fn push(&mut self, source: usize, at: u64, data: &[i16]) {
        let emitted = self.emitted;
        let s = &mut self.sources[source];
        let start = match s.next {
            // A gap (silence, a glitch, a device that came back) starts where
            // the clock says; anything else continues the previous packet.
            Some(next) if at <= next + TOLERANCE => next,
            _ => at,
        };
        let start = start.max(emitted + s.samples.len() as u64);
        let silence = (start - emitted) as usize - s.samples.len();
        s.samples.extend(std::iter::repeat_n(0, silence));
        s.samples.extend(data);
        s.next = Some(start + data.len() as u64);
    }

    /// Interleaved frames from the end of the last call up to `until`.
    pub fn take(&mut self, until: u64) -> Vec<i16> {
        let count = until.saturating_sub(self.emitted) as usize;
        let mut out = Vec::with_capacity(count * self.sources.len());
        for _ in 0..count {
            for s in &mut self.sources {
                out.push(s.samples.pop_front().unwrap_or(0));
            }
        }
        self.emitted += count as u64;
        out
    }
}

/// Little-endian bytes, as FFmpeg's s16le input reads them.
pub fn bytes(frames: &[i16]) -> Vec<u8> {
    frames.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// FFmpeg arguments for one capture session fed on stdin: 16-bit frames at
/// `RATE`, one channel per source in the order mic, system. Each source
/// becomes its own 16 kHz mono FLAC segments, `sNNNN-<mic|system>-NNNNNN.flac`,
/// with the level lines `audio.rs` reads, as the Linux capture writes them.
pub fn ffmpeg_args(
    folder: &Path,
    session: usize,
    mic: bool,
    system: bool,
) -> Result<Vec<OsString>> {
    let kinds = [("mic", mic), ("system", system)]
        .into_iter()
        .filter_map(|(kind, on)| on.then_some(kind))
        .collect::<Vec<_>>();
    if kinds.is_empty() {
        return Err("Choose a microphone or system monitor".into());
    }
    let levels = |kind: &str| {
        format!(
            "astats=metadata=1:reset=1,ametadata@{kind}=print:key=lavfi.astats.Overall.RMS_level"
        )
    };
    let graph = match kinds.as_slice() {
        [kind] => format!("[0:a]{}[{kind}]", levels(kind)),
        _ => format!(
            "[0:a]channelsplit=channel_layout=stereo[a][b];[a]{}[mic];[b]{}[system]",
            levels("mic"),
            levels("system")
        ),
    };
    let mut args: Vec<OsString> = vec![];
    let rate = RATE.to_string();
    let channels = kinds.len().to_string();
    for arg in [
        "-hide_banner",
        "-loglevel",
        "info",
        "-y",
        "-f",
        "s16le",
        "-ar",
        &rate,
        "-ac",
        &channels,
        "-i",
        "pipe:0",
        "-filter_complex",
        &graph,
    ] {
        args.push(arg.into());
    }
    for kind in kinds {
        let map = format!("[{kind}]");
        for arg in [
            "-map",
            &map,
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
        ] {
            args.push(arg.into());
        }
        args.push(
            folder
                .join(format!("s{session:04}-{kind}-%06d.flac"))
                .into_os_string(),
        );
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
    };

    const MS: u64 = RATE as u64 / 1000;

    #[test]
    fn jittery_packets_join_without_gaps() {
        let mut timeline = Timeline::new(1);
        let mut at = 0;
        for (i, jitter) in [0i64, 3, -2, 4, -4, 1].iter().enumerate() {
            let packet = vec![i as i16 + 1; (10 * MS) as usize];
            timeline.push(0, (at as i64 + jitter * MS as i64).max(0) as u64, &packet);
            at += 10 * MS;
        }
        let out = timeline.take(60 * MS);
        assert_eq!(out.len(), (60 * MS) as usize);
        assert!(out.iter().all(|&s| s != 0), "a jittered packet left a gap");
        assert_eq!(out[(10 * MS) as usize], 2);
    }

    #[test]
    fn a_source_that_hears_nothing_is_silent_and_keeps_its_place() {
        // Loopback delivers nothing while nothing plays, then sound at 1.0 s.
        let mut timeline = Timeline::new(2);
        timeline.push(0, 0, &vec![7; (1500 * MS) as usize]);
        timeline.push(1, 1000 * MS, &vec![9; (100 * MS) as usize]);
        let out = timeline.take(1500 * MS);
        let system = out.iter().skip(1).step_by(2).copied().collect::<Vec<_>>();
        assert!(out.iter().step_by(2).all(|&s| s == 7));
        assert!(system[..(1000 * MS) as usize].iter().all(|&s| s == 0));
        assert!(system[(1000 * MS) as usize..(1100 * MS) as usize]
            .iter()
            .all(|&s| s == 9));
        assert!(system[(1100 * MS) as usize..].iter().all(|&s| s == 0));
    }

    #[test]
    fn a_device_that_comes_back_starts_where_the_clock_says() {
        let mut timeline = Timeline::new(1);
        timeline.push(0, 0, &vec![1; (100 * MS) as usize]);
        timeline.push(0, 600 * MS, &vec![2; (100 * MS) as usize]);
        let out = timeline.take(700 * MS);
        assert!(out[(100 * MS) as usize..(600 * MS) as usize]
            .iter()
            .all(|&s| s == 0));
        assert_eq!(out[(600 * MS) as usize], 2);
    }

    #[test]
    fn late_audio_is_kept_not_dropped_and_the_rest_waits() {
        let mut timeline = Timeline::new(1);
        timeline.push(0, 0, &vec![1; (100 * MS) as usize]);
        assert_eq!(timeline.take(300 * MS).len(), (300 * MS) as usize);
        // Captured at 100 ms but delivered after 300 ms had been written out.
        timeline.push(0, 100 * MS, &vec![2; (50 * MS) as usize]);
        let out = timeline.take(400 * MS);
        assert_eq!(out.iter().filter(|&&s| s == 2).count(), (50 * MS) as usize);
        assert_eq!(out[0], 2);
        // Frames past `until` stay for the next call.
        timeline.push(0, 400 * MS, &vec![3; (100 * MS) as usize]);
        assert_eq!(timeline.take(450 * MS).len(), (50 * MS) as usize);
        assert!(timeline.take(500 * MS).iter().all(|&s| s == 3));
    }

    #[test]
    fn clock_conversion() {
        assert_eq!(frames(10_000_000), RATE as u64);
        assert_eq!(frames(100_000), 480);
        assert_eq!(bytes(&[1, -2]), vec![1, 0, 0xfe, 0xff]);
    }

    #[test]
    fn arguments_write_the_linux_segment_names_and_levels() {
        let folder = Path::new("/data/recordings/id");
        let both = ffmpeg_args(folder, 3, true, true).unwrap();
        let both = both.iter().map(|a| a.to_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(
            &both[..12],
            [
                "-hide_banner",
                "-loglevel",
                "info",
                "-y",
                "-f",
                "s16le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-i",
                "pipe:0"
            ]
        );
        assert_eq!(both[13], "[0:a]channelsplit=channel_layout=stereo[a][b];[a]astats=metadata=1:reset=1,ametadata@mic=print:key=lavfi.astats.Overall.RMS_level[mic];[b]astats=metadata=1:reset=1,ametadata@system=print:key=lavfi.astats.Overall.RMS_level[system]");
        let output = |kind: &str| {
            vec![
                "-map".to_string(),
                format!("[{kind}]"),
                "-ac".into(),
                "1".into(),
                "-ar".into(),
                "16000".into(),
                "-c:a".into(),
                "flac".into(),
                "-f".into(),
                "segment".into(),
                "-segment_time".into(),
                "30".into(),
                "-segment_format_options".into(),
                "flush_packets=1".into(),
                "-reset_timestamps".into(),
                "1".into(),
                Path::new("/data/recordings/id")
                    .join(format!("s0003-{kind}-%06d.flac"))
                    .to_string_lossy()
                    .into_owned(),
            ]
        };
        assert_eq!(both[14..], [output("mic"), output("system")].concat());
        let system = ffmpeg_args(folder, 0, false, true).unwrap();
        assert_eq!(system[9], "1");
        assert_eq!(system[13], "[0:a]astats=metadata=1:reset=1,ametadata@system=print:key=lavfi.astats.Overall.RMS_level[system]");
        assert!(system
            .last()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("s0000-system-%06d.flac"));
        assert!(ffmpeg_args(folder, 0, false, false).is_err());
    }

    /// The graph runs in the FFmpeg on PATH (the bundled one on Windows CI):
    /// both sources' segments hold every sample, and both report levels.
    #[test]
    fn ffmpeg_turns_stdin_into_each_sources_segments() {
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
            .prefix("voxloom café ")
            .tempdir()
            .unwrap();
        let mut ffmpeg = Command::new("ffmpeg")
            .args(ffmpeg_args(dir.path(), 0, true, true).unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = ffmpeg.stderr.take().unwrap();
        let levels = std::thread::spawn(move || {
            let lines = BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok);
            let level = lines
                .filter(|l| l.contains("lavfi.astats.Overall.RMS_level="))
                .collect::<Vec<_>>();
            (
                level.iter().any(|l| l.contains("ametadata@mic")),
                level.iter().any(|l| l.contains("ametadata@system")),
            )
        });
        // 3.5 s: a tone on the microphone, silence then a tone on the system.
        let mut timeline = Timeline::new(2);
        let tone = |n: u64| {
            (0..n)
                .map(|i| ((i as f64 * 0.0576).sin() * 8000.) as i16)
                .collect::<Vec<_>>()
        };
        timeline.push(0, 0, &tone(3500 * MS));
        timeline.push(1, 2000 * MS, &tone(1000 * MS));
        let mut stdin = ffmpeg.stdin.take().unwrap();
        stdin.write_all(&bytes(&timeline.take(3500 * MS))).unwrap();
        drop(stdin);
        assert!(ffmpeg.wait().unwrap().success());
        assert_eq!(
            levels.join().unwrap(),
            (true, true),
            "both sources report levels"
        );
        for kind in ["mic", "system"] {
            let file = dir.path().join(format!("s0000-{kind}-000000.flac"));
            let decoded = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&file)
                .args(["-f", "s16le", "-ac", "1", "-ar", "16000", "pipe:1"])
                .output()
                .unwrap();
            let samples = decoded.stdout.len() as i64 / 2;
            assert!((samples - 56_000).abs() <= 64, "{kind}: {samples} samples");
        }
    }
}
