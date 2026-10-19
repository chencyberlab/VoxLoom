//! Windows capture through WASAPI: microphones, and loopback of an output
//! device for system audio. FFmpeg cannot open these itself, so a capture
//! thread reads them, places them on one clock (`pcm::Timeline`) and feeds
//! FFmpeg raw PCM on stdin; FFmpeg writes the same segments and level lines
//! as on Linux. Desktop apps get no permission prompt on Windows: the
//! microphone works only while Settings → Privacy & security → Microphone →
//! "Let desktop apps access your microphone" is on, and the app appears in
//! that list once it has used the microphone.
use super::pcm::{self, Timeline, RATE};
use crate::{process, Result};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{HRESULT, HSTRING},
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::E_ACCESSDENIED,
        Media::Audio::{
            eCapture, eConsole, eRender, EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice,
            IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
            AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
            AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
            WAVE_FORMAT_PCM,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_MULTITHREADED, STGM_READ,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        },
    },
};

pub const START_FAILED: &str = "Recording could not start. Refresh the sources and try again.";
const PRIVACY: &str = "Windows is blocking VoxLoom from using the microphone. Turn on Settings → Privacy & security → Microphone → Let desktop apps access your microphone, then start again.";
/// ERROR_NOT_FOUND as an HRESULT: the device id no longer exists.
const E_NOTFOUND: HRESULT = HRESULT(0x80070490_u32 as i32);
/// WASAPI's own buffer (100 ns units); the thread empties it every 10 ms.
const BUFFER: i64 = 2_000_000;
/// How far behind the clock frames are written out: packets arrive a few
/// milliseconds after they are captured, and one that arrives later still
/// would have to go after its place instead of in it.
const MARGIN: u64 = RATE as u64 * 3 / 10;
/// How long a "now recording from" notice stays up.
const NOTICE: Duration = Duration::from_secs(20);

pub fn devices() -> Result<Value> {
    thread::spawn(|| {
        let _com = Com::init();
        list().map_err(|e| format!("Windows audio is not available ({})", e.message()))
    })
    .join()
    .map_err(|_| "Windows audio is not available".to_string())?
}

fn list() -> windows::core::Result<Value> {
    let enumerator = enumerator()?;
    let mut sources = vec![];
    for (flow, monitor) in [(eCapture, false), (eRender, true)] {
        let default = default_id(&enumerator, flow);
        let collection = unsafe { enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)? };
        for i in 0..unsafe { collection.GetCount()? } {
            let device = unsafe { collection.Item(i)? };
            let id = device_id(&device)?;
            let is_default = default.as_deref() == Some(id.as_str());
            sources.push(
                json!({"name":id,"label":label(&device),"monitor":monitor,"default":is_default}),
            );
        }
    }
    Ok(json!(sources))
}

/// Opens the chosen devices, then starts FFmpeg reading what the capture
/// thread writes. The returned FFmpeg's stderr carries the levels; `finish`
/// ends it cleanly.
pub fn spawn(
    folder: &Path,
    session: usize,
    mic: Option<&str>,
    system: Option<&str>,
) -> Result<Child> {
    let args = pcm::ffmpeg_args(folder, session, mic.is_some(), system.is_some())?;
    let wanted = [(mic, eCapture), (system, eRender)]
        .into_iter()
        .filter_map(|(id, flow)| id.map(|id| (id.to_string(), flow)))
        .collect::<Vec<_>>();
    let stop = Arc::new(AtomicBool::new(false));
    let notices: Arc<Mutex<Vec<Notice>>> = Arc::new(Mutex::new(vec![None; wanted.len()]));
    let (ready_tx, ready) = mpsc::channel();
    let (stdin_tx, stdin_rx) = mpsc::channel::<ChildStdin>();
    let (done_tx, done) = mpsc::channel::<()>();
    let (thread_stop, thread_notices) = (stop.clone(), notices.clone());
    thread::spawn(move || {
        let _done = done_tx;
        let _com = Com::init();
        match Capture::open(wanted) {
            Ok(mut capture) => {
                let _ = ready_tx.send(Ok(()));
                if let Ok(stdin) = stdin_rx.recv() {
                    capture.run(stdin, &thread_stop, &thread_notices);
                }
            }
            Err(message) => {
                let _ = ready_tx.send(Err(message));
            }
        }
    });
    ready
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| START_FAILED.to_string())??;
    // If FFmpeg cannot start, dropping `stdin_tx` ends the capture thread.
    let mut child = process::spawn(
        process::command("ffmpeg")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )
    .map_err(|_| "FFmpeg could not start. Reinstall VoxLoom.".to_string())?;
    if let Some(stdin) = child.stdin.take() {
        let _ = stdin_tx.send(stdin);
    }
    CAPTURES.lock().unwrap().push(Running {
        pid: child.id(),
        stop,
        notices,
        done,
    });
    Ok(child)
}

/// Ends capture: the thread writes out what it has and closes FFmpeg's input,
/// so FFmpeg finishes its files and exits; `audio.rs` waits for that.
pub fn finish(child: &mut Child) {
    let running = {
        let mut captures = CAPTURES.lock().unwrap();
        let index = captures.iter().position(|r| r.pid == child.id());
        index.map(|i| captures.swap_remove(i))
    };
    if let Some(running) = running {
        running.stop.store(true, Ordering::SeqCst);
        let _ = running.done.recv_timeout(Duration::from_secs(3));
    }
}

/// What the user should know while recording: a disconnected device, or the
/// device recording continues from.
pub fn warning(child: &Child) -> Option<String> {
    let captures = CAPTURES.lock().unwrap();
    let running = captures.iter().find(|r| r.pid == child.id())?;
    let mut notices = running.notices.lock().unwrap();
    for notice in notices.iter_mut() {
        if notice
            .as_ref()
            .is_some_and(|(_, until)| until.is_some_and(|t| Instant::now() > t))
        {
            *notice = None;
        }
    }
    let text = notices
        .iter()
        .flatten()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>();
    (!text.is_empty()).then(|| text.join(" "))
}

pub fn open_microphone_settings() -> Result<()> {
    process::hidden("explorer")
        .arg("ms-settings:privacy-microphone")
        .status()
        .map(|_| ())
        .map_err(|_| "Windows Settings could not be opened".to_string())
}

/// A notice and, for a temporary one, when it goes.
type Notice = Option<(String, Option<Instant>)>;

struct Running {
    pid: u32,
    stop: Arc<AtomicBool>,
    notices: Arc<Mutex<Vec<Notice>>>,
    /// Disconnects when the capture thread has finished.
    done: mpsc::Receiver<()>,
}
static CAPTURES: Mutex<Vec<Running>> = Mutex::new(Vec::new());

/// COM for the current thread, released when dropped.
struct Com(bool);
impl Com {
    fn init() -> Self {
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn device_id(device: &IMMDevice) -> windows::core::Result<String> {
    unsafe {
        let id = device.GetId()?;
        let text = id.to_string();
        CoTaskMemFree(Some(id.0 as *const _));
        text.map_err(|_| E_NOTFOUND.into())
    }
}

fn label(device: &IMMDevice) -> String {
    unsafe { device.OpenPropertyStore(STGM_READ) }
        .and_then(|store| unsafe { store.GetValue(&PKEY_Device_FriendlyName) })
        .map(|name| name.to_string())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Audio device".into())
}

fn default_id(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) }.ok()?;
    device_id(&device).ok()
}

/// QueryPerformanceCounter in WASAPI's 100 ns units, from the session start.
struct Clock {
    frequency: u128,
    start: u64,
}
impl Clock {
    fn new() -> Self {
        let mut frequency = 0;
        let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
        let mut clock = Clock {
            frequency: frequency.max(1) as u128,
            start: 0,
        };
        clock.start = clock.hundred_ns();
        clock
    }
    fn hundred_ns(&self) -> u64 {
        let mut count = 0;
        let _ = unsafe { QueryPerformanceCounter(&mut count) };
        (count as u128 * 10_000_000 / self.frequency) as u64
    }
    /// The current frame.
    fn now(&self) -> u64 {
        pcm::frames(self.hundred_ns().saturating_sub(self.start))
    }
    /// The frame a packet captured at `qpc` (100 ns units) belongs at.
    fn at(&self, qpc: u64) -> u64 {
        pcm::frames(qpc.saturating_sub(self.start))
    }
}

struct Stream {
    id: String,
    label: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

/// A source the user chose. A source chosen as the default device follows
/// the default: plugging in a headset mid-meeting moves the recording to it.
struct Source {
    flow: EDataFlow,
    id: String,
    follow_default: bool,
    stream: Option<Stream>,
}
impl Source {
    fn name(&self) -> &'static str {
        if self.flow == eCapture {
            "microphone"
        } else {
            "sound output"
        }
    }
}

struct Capture {
    enumerator: IMMDeviceEnumerator,
    sources: Vec<Source>,
}

impl Capture {
    fn open(wanted: Vec<(String, EDataFlow)>) -> std::result::Result<Self, String> {
        let enumerator = enumerator().map_err(|_| START_FAILED.to_string())?;
        let mut sources = vec![];
        for (id, flow) in wanted {
            let mut source = Source {
                follow_default: default_id(&enumerator, flow).as_deref() == Some(id.as_str()),
                flow,
                id,
                stream: None,
            };
            let stream = open_stream(&enumerator, flow, &source.id)
                .map_err(|e| problem(&source, e.code()))?;
            source.stream = Some(stream);
            sources.push(source);
        }
        Ok(Capture {
            enumerator,
            sources,
        })
    }

    fn run(&mut self, mut stdin: ChildStdin, stop: &AtomicBool, notices: &Mutex<Vec<Notice>>) {
        let clock = Clock::new();
        for source in &mut self.sources {
            if let Some(stream) = &source.stream {
                if unsafe { stream.client.Start() }.is_err() {
                    source.stream = None;
                }
            }
        }
        let mut timeline = Timeline::new(self.sources.len());
        let mut check = Instant::now();
        loop {
            let stopping = stop.load(Ordering::SeqCst);
            for (index, source) in self.sources.iter_mut().enumerate() {
                if let Some(stream) = &source.stream {
                    let drained = drain(stream, &clock, |at, data| timeline.push(index, at, data));
                    if drained.is_err() {
                        source.stream = None;
                        check = Instant::now();
                    }
                }
            }
            if !stopping && Instant::now() >= check {
                self.check_devices(notices);
                check = Instant::now() + Duration::from_secs(1);
            }
            let until = if stopping {
                clock.now()
            } else {
                clock.now().saturating_sub(MARGIN)
            };
            let frames = timeline.take(until);
            if !frames.is_empty() && stdin.write_all(&pcm::bytes(&frames)).is_err() {
                return; // FFmpeg has ended.
            }
            if stopping {
                return; // Dropping stdin ends FFmpeg's input.
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Follows the default device and replaces a disconnected one. Until a
    /// replacement exists, that source is silent and the user is told.
    fn check_devices(&mut self, notices: &Mutex<Vec<Notice>>) {
        for (index, source) in self.sources.iter_mut().enumerate() {
            let default = default_id(&self.enumerator, source.flow);
            let current = source.stream.as_ref().map(|s| s.id.clone());
            let wanted = match (&current, &default) {
                (Some(current), Some(default)) if source.follow_default && current != default => {
                    vec![default.clone()]
                }
                (Some(_), _) => continue,
                (None, _) if source.follow_default => default.clone().into_iter().collect(),
                (None, _) => [Some(source.id.clone()), default.clone()]
                    .into_iter()
                    .flatten()
                    .collect(),
            };
            let mut opened = None;
            let mut denied = false;
            for id in wanted {
                match open_stream(&self.enumerator, source.flow, &id) {
                    Ok(stream) if unsafe { stream.client.Start() }.is_ok() => {
                        opened = Some(stream);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => denied |= e.code() == E_ACCESSDENIED,
                }
            }
            let notice = match (opened, current) {
                (Some(stream), _) => {
                    let text = format!("Now recording from {}.", stream.label);
                    source.stream = Some(stream);
                    Some((text, Some(Instant::now() + NOTICE)))
                }
                (None, Some(_)) => continue, // Still on the old device.
                (None, None) if denied && source.flow == eCapture => Some((PRIVACY.to_string(), None)),
                (None, None) => Some((
                    format!(
                        "The {} was disconnected. Recording continues; this part stays silent until a {} is available.",
                        source.name(),
                        source.name()
                    ),
                    None,
                )),
            };
            notices.lock().unwrap()[index] = notice;
        }
    }
}

fn open_stream(
    enumerator: &IMMDeviceEnumerator,
    flow: EDataFlow,
    id: &str,
) -> windows::core::Result<Stream> {
    unsafe {
        let device = enumerator.GetDevice(&HSTRING::from(id))?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        // Windows converts the device's own format to 16-bit mono at RATE.
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let loopback = if flow == eRender {
            AUDCLNT_STREAMFLAGS_LOOPBACK
        } else {
            0
        };
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY | loopback,
            BUFFER,
            0,
            &format,
            None,
        )?;
        let capture: IAudioCaptureClient = client.GetService()?;
        Ok(Stream {
            id: id.to_string(),
            label: label(&device),
            client,
            capture,
        })
    }
}

/// Moves every waiting packet onto the timeline.
fn drain(
    stream: &Stream,
    clock: &Clock,
    mut push: impl FnMut(u64, &[i16]),
) -> windows::core::Result<()> {
    unsafe {
        while stream.capture.GetNextPacketSize()? > 0 {
            let (mut data, mut count, mut flags, mut qpc) = (std::ptr::null_mut(), 0, 0, 0);
            stream
                .capture
                .GetBuffer(&mut data, &mut count, &mut flags, None, Some(&mut qpc))?;
            let at = clock.at(qpc);
            if data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                push(at, &vec![0; count as usize]);
            } else {
                push(
                    at,
                    std::slice::from_raw_parts(data as *const i16, count as usize),
                );
            }
            stream.capture.ReleaseBuffer(count)?;
        }
    }
    Ok(())
}

/// What to tell the user when a chosen device cannot be opened.
fn problem(source: &Source, code: HRESULT) -> String {
    let name = source.name();
    if code == E_ACCESSDENIED && source.flow == eCapture {
        PRIVACY.into()
    } else if code == E_NOTFOUND || code == AUDCLNT_E_DEVICE_INVALIDATED {
        format!("That {name} is no longer connected. Refresh the sources and choose another.")
    } else if code == AUDCLNT_E_DEVICE_IN_USE {
        format!("Another app is using that {name} on its own. Close it, or choose another {name}.")
    } else {
        START_FAILED.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub's Windows machines have no audio devices; listing must still
    /// work there (an empty list), and a laptop lists its own.
    #[test]
    fn devices_list_without_audio_hardware() {
        match devices() {
            Ok(list) => {
                for device in list.as_array().unwrap() {
                    assert!(device["name"].as_str().is_some_and(|n| !n.is_empty()));
                    assert!(device["monitor"].is_boolean() && device["default"].is_boolean());
                }
            }
            Err(message) => assert!(
                message.starts_with("Windows audio is not available"),
                "{message}"
            ),
        }
    }

    /// A device that is gone is refused with a plain message before FFmpeg
    /// starts, and nothing is left registered.
    #[test]
    fn a_missing_device_is_refused_before_ffmpeg_starts() {
        let folder = tempfile::tempdir().unwrap();
        let missing = "{0.0.1.00000000}.{00000000-0000-0000-0000-000000000000}";
        let error = spawn(folder.path(), 0, Some(missing), None).unwrap_err();
        assert!(
            [
                START_FAILED.to_string(),
                "That microphone is no longer connected. Refresh the sources and choose another."
                    .into()
            ]
            .contains(&error),
            "{error}"
        );
        assert!(std::fs::read_dir(folder.path()).unwrap().next().is_none());
        assert!(CAPTURES.lock().unwrap().is_empty());
    }
}
