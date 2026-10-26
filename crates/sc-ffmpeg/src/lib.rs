// Encode path: system FFmpeg. VA-API first, then NVENC, then libx264 so capture
// still works on a machine with no usable GPU encoder.

mod audio_encoder;
mod compress;
mod convert;
mod encoder;
mod hw;
mod mux;
mod player;
mod recorder;
mod thumb;
mod trim;

pub use audio_encoder::AacEncoder;
pub use compress::{compress_to_size, Compressed};
pub use convert::Bgra2Nv12;
pub use encoder::{CodecParams, Encoder};
pub use mux::Mp4Muxer;
pub use player::{media_info, spawn_audio, spawn_video, MediaInfo, VideoFrame, AUDIO_RATE};
pub use recorder::Recorder;
pub use thumb::{frame_at, thumbnail};
pub use trim::{probe_duration, trim_copy};

use ffmpeg_next as ff;
use sc_core::encode::EncoderKind;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("no usable hardware encoder found for {codec}")]
    NoEncoder { codec: &'static str },
    #[error("{}", av_message(*code, ctx))]
    Av { code: i32, ctx: &'static str },
}

/// Plain wording for the FFmpeg errors people actually meet.
fn av_message(code: i32, ctx: &str) -> String {
    match code {
        -2 => "couldn't open the file; it may have been moved or deleted".into(),
        -13 => "couldn't open the file; permission denied".into(),
        -1_094_995_529 => "this file is not a valid video".into(),
        _ => format!("ffmpeg returned {code} at {ctx}"),
    }
}

/// Initialize FFmpeg once. Cheap to call repeatedly.
pub fn init() -> Result<()> {
    ff::init().map_err(|e| Error::Ffmpeg(e.to_string()))
}

/// H.264 encoders in priority order. Hardware first; the last entry is the
/// software fallback (needs an FFmpeg built with libx264).
pub(crate) const H264_CANDIDATES: &[(EncoderKind, &str)] = &[
    (EncoderKind::Vaapi, "h264_vaapi"),
    (EncoderKind::Nvenc, "h264_nvenc"),
    (EncoderKind::Software, "libx264"),
];

/// What `Encoder::new` actually tries, in order: the first that really opens wins.
pub(crate) const ENCODE_ORDER: &[(EncoderKind, &str)] = H264_CANDIDATES;

/// Probe H.264 encoders in priority order and return the first whose FFmpeg
/// codec is present in this build. Actual device-open is verified when an
/// `Encoder` is constructed (§6.1: probe that it really initializes).
pub fn detect_h264_encoder() -> Option<(EncoderKind, &'static str)> {
    H264_CANDIDATES
        .iter()
        .find(|(_, name)| ff::encoder::find_by_name(name).is_some())
        .map(|(kind, name)| (*kind, *name))
}
