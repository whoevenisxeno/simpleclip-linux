//! Screen and audio capture on Linux: the xdg-desktop-portal ScreenCast session
//! plus PipeWire for video, and PipeWire streams for desktop audio (a sink
//! monitor) and the microphone. Video, desktop audio and mic are stamped
//! against one shared `Instant` epoch so they stay aligned in the replay buffer.

mod audio;
mod devices;
mod portal;
mod stream;

pub use devices::{list_audio_devices, list_monitors};
pub use portal::forget_token as forget_screen;

use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::capture::{FramePool, PixelFormat, VideoFrame};
use sc_core::time::Timestamp;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("portal: {0}")]
    Portal(String),
    #[error("pipewire: {0}")]
    Pipewire(String),
    #[error("capture ended before a format was negotiated")]
    NoFormat,
    #[error("{0}")]
    Other(String),
}

/// What to capture and how. The screen itself is chosen in the portal's own
/// dialog (once; the choice is remembered), so there is no monitor option.
#[derive(Debug, Clone)]
pub struct Options {
    pub show_cursor: bool,
    pub fps: u32,
    /// Capture desktop audio (a sink monitor).
    pub desktop_audio: bool,
    /// Output (sink) node name for desktop audio; empty for the default sink.
    pub audio_device: String,
    /// Also capture a microphone.
    pub mic_enabled: bool,
    /// Input (source) node name; empty for the default input device.
    pub mic_device: String,
}

/// The latest captured frame. The compositor only delivers when the screen
/// changes, so the pacer re-emits this at the target rate: a static desktop
/// still yields regular frames (and keyframes), and a 240 Hz display is capped.
#[derive(Default)]
pub(crate) struct Slot {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub has_frame: bool,
}

/// A running capture. Dropping it stops video and audio and joins the threads.
pub struct CaptureHandle {
    pub width: u32,
    pub height: u32,
    stop: Arc<AtomicBool>,
    slot: Arc<Mutex<Slot>>,
    video_stop: pipewire::channel::Sender<()>,
    video_thread: Option<JoinHandle<()>>,
    pacer: Option<JoinHandle<()>>,
    audio_stops: Vec<pipewire::channel::Sender<()>>,
    audio_threads: Vec<JoinHandle<()>>,
}

impl CaptureHandle {
    /// The most recent captured frame (for screenshots), cropped like the video stream.
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        let s = self.slot.lock().unwrap();
        if !s.has_frame {
            return None;
        }
        let (width, height) = (s.width & !1, s.height & !1);
        Some(VideoFrame {
            width,
            height,
            stride: s.stride,
            format: PixelFormat::Bgra8,
            timestamp: Timestamp::from_nanos(0),
            data: s.data[..(s.stride * height) as usize].to_vec().into(),
        })
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.video_stop.send(());
        for s in &self.audio_stops {
            let _ = s.send(());
        }
        if let Some(t) = self.video_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.pacer.take() {
            let _ = t.join();
        }
        for t in self.audio_threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Start capturing. Pops the portal's share dialog the first time (afterwards
/// the remembered choice is reused), then blocks until the first frame fixes the
/// real picture size. Desktop audio and the microphone, if requested, are
/// captured on the same clock so they stay in sync with the video.
pub fn start(
    frames: crossbeam_channel::Sender<VideoFrame>,
    audio: Option<crossbeam_channel::Sender<AudioBuffer>>,
    opts: &Options,
) -> Result<CaptureHandle> {
    let slot = Arc::new(Mutex::new(Slot::default()));
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<()>(1);
    let (stop_tx, stop_rx) = pipewire::channel::channel::<()>();

    let video_thread = {
        let (slot, show_cursor) = (slot.clone(), opts.show_cursor);
        std::thread::Builder::new()
            .name("sc-capture-video".into())
            .spawn(move || {
                if let Err(e) =
                    pollster::block_on(portal::run(slot, ready_tx, stop_rx, show_cursor))
                {
                    tracing::error!(error = %e, "video capture ended with error");
                }
            })
            .map_err(|e| Error::Pipewire(e.to_string()))?
    };

    // The user may take a while to answer the share dialog the first time.
    if ready_rx.recv_timeout(Duration::from_secs(120)).is_err() {
        let _ = stop_tx.send(());
        let _ = video_thread.join();
        return Err(Error::NoFormat);
    }
    // The frame size, not the negotiated format, is authoritative.
    let deadline = Instant::now() + Duration::from_secs(5);
    let (width, height) = loop {
        {
            let s = slot.lock().unwrap();
            if s.has_frame {
                // NV12 needs even dimensions; crop one pixel row/column if odd.
                break (s.width & !1, s.height & !1);
            }
        }
        if Instant::now() > deadline {
            let _ = stop_tx.send(());
            let _ = video_thread.join();
            return Err(Error::NoFormat);
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    // The shared clock starts now, not when `start` was called: the share dialog
    // can take any amount of time, and the encoder times its packets from the
    // first frame it is given.
    let epoch = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let fps = opts.fps.clamp(1, 240);
    let pacer = {
        let (slot, stop) = (slot.clone(), stop.clone());
        std::thread::Builder::new()
            .name("sc-capture-pacer".into())
            .spawn(move || pace(slot, frames, stop, epoch, fps, width, height))
            .map_err(|e| Error::Other(e.to_string()))?
    };

    let mut audio_stops = Vec::new();
    let mut audio_threads = Vec::new();
    if let Some(tx) = audio {
        let mut sources = Vec::new();
        if opts.desktop_audio {
            sources.push((AudioRole::Desktop, opts.audio_device.clone()));
        }
        if opts.mic_enabled {
            sources.push((AudioRole::Microphone, opts.mic_device.clone()));
        }
        for (role, device) in sources {
            let (stop_tx, stop_rx) = pipewire::channel::channel::<()>();
            let tx = tx.clone();
            audio_threads.push(
                std::thread::Builder::new()
                    .name(format!("sc-capture-{role:?}").to_lowercase())
                    .spawn(move || {
                        if let Err(e) = audio::run(tx, epoch, stop_rx, role, &device) {
                            tracing::warn!(error = %e, ?role, "audio capture ended with error");
                        }
                    })
                    .map_err(|e| Error::Other(e.to_string()))?,
            );
            audio_stops.push(stop_tx);
        }
    }

    Ok(CaptureHandle {
        width,
        height,
        stop,
        slot,
        video_stop: stop_tx,
        video_thread: Some(video_thread),
        pacer: Some(pacer),
        audio_stops,
        audio_threads,
    })
}

fn pace(
    slot: Arc<Mutex<Slot>>,
    tx: crossbeam_channel::Sender<VideoFrame>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    fps: u32,
    width: u32,
    height: u32,
) {
    let period = Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut next = Instant::now();
    let pool = FramePool::new(1);
    let mut warned = false;
    let (mut sent, mut dropped, mut report) = (0u32, 0u32, Instant::now());
    while !stop.load(Ordering::Relaxed) {
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > period * 4 {
            next = now; // fell far behind (system sleep, debugger): don't burst
        }
        let frame = {
            let s = slot.lock().unwrap();
            if s.width & !1 != width || s.height & !1 != height {
                if !warned {
                    tracing::warn!(
                        "capture size changed ({}x{} -> {}x{}); frames paused until restart",
                        width,
                        height,
                        s.width,
                        s.height
                    );
                    warned = true;
                }
                continue;
            }
            // Crop to even size by trimming trailing rows; the stride keeps the
            // row layout intact so trailing columns are simply ignored.
            VideoFrame {
                width,
                height,
                stride: s.stride,
                format: PixelFormat::Bgra8,
                timestamp: Timestamp::from_nanos(epoch.elapsed().as_nanos() as i64),
                data: pool.copy_of(&s.data[..(s.stride * height) as usize]),
            }
        };
        // Drop on a full queue rather than stalling the pacer: video may skip,
        // the clock must not.
        match tx.try_send(frame) {
            Ok(()) => sent += 1,
            Err(_) => dropped += 1,
        }
        if report.elapsed() >= Duration::from_secs(5) {
            tracing::debug!(sent, dropped, "capture pacer, last 5s");
            (sent, dropped, report) = (0, 0, Instant::now());
        }
    }
}
