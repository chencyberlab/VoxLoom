//! Windows capture through WASAPI: microphones, and loopback of the default
//! output for system audio. Until it lands, recording reports this plainly
//! and everything else (import, transcription, search, summaries) works.
use crate::Result;
use serde_json::Value;
use std::{path::Path, process::Child};

const NOT_YET: &str =
    "Recording isn't available in the Windows preview yet. Import an audio or video file instead.";
pub const START_FAILED: &str = NOT_YET;

pub fn devices() -> Result<Value> {
    Err(NOT_YET.into())
}

pub fn spawn(
    _folder: &Path,
    _session: usize,
    _mic: Option<&str>,
    _system: Option<&str>,
) -> Result<Child> {
    Err(NOT_YET.into())
}
