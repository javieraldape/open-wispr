use std::{
    io::Error,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, Device, Sample, SampleFormat, SampleRate, SizedSample, StreamConfig,
};

use crate::audio_toolkit::{
    audio::{AudioVisualiser, FrameResampler},
    constants,
    vad::{self, VadFrame},
    VoiceActivityDetector,
};

enum Cmd {
    Start(VadPolicy),
    Stop(mpsc::Sender<Vec<f32>>),
    Shutdown,
}

enum AudioChunk {
    Samples(Vec<f32>),
    EndOfStream,
}

// Freshly built or ad-hoc signed macOS bundles can block inside CoreAudio while
// macOS resolves microphone permission. The stream usually opens quickly after
// permission settles, but a 5s timeout makes the first real recording fail.
#[cfg(target_os = "macos")]
const MICROPHONE_INIT_TIMEOUT: Duration = Duration::from_secs(360);

#[cfg(not(target_os = "macos"))]
const MICROPHONE_INIT_TIMEOUT: Duration = Duration::from_secs(5);

/// How 16 kHz mono frames should be filtered for one recording session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadPolicy {
    /// Bypass VAD and forward every frame.
    Disabled,
    /// Current offline-tuned VAD profile.
    Offline,
    /// VAD profile with a longer post-speech tail for streaming-capable models.
    Streaming,
}

/// A single VAD engine plus the two hangover-tail lengths its smoothing wrapper
/// should use. The offline and streaming policies are never active
/// concurrently, so one detector is reconfigured per session (see `Cmd::Start`)
/// rather than kept as two resident engines.
#[derive(Clone)]
struct VadConfig {
    detector: Arc<Mutex<Box<dyn vad::VoiceActivityDetector>>>,
    offline_hangover_frames: usize,
    streaming_hangover_frames: usize,
}

impl VadConfig {
    /// Post-speech hangover tail (in 30 ms frames) for the given policy.
    /// `Disabled` never reaches the detector, so it maps to the offline value.
    fn hangover_for(&self, policy: VadPolicy) -> usize {
        match policy {
            VadPolicy::Streaming => self.streaming_hangover_frames,
            VadPolicy::Offline | VadPolicy::Disabled => self.offline_hangover_frames,
        }
    }
}

/// Callback invoked with each 16 kHz mono frame that passes the active capture
/// policy while recording. Used to feed a live streaming transcription as audio arrives.
pub type AudioFrameCallback = Arc<dyn Fn(&[f32]) + Send + Sync + 'static>;

pub struct AudioRecorder {
    device: Option<Device>,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    vad: Option<VadConfig>,
    level_cb: Option<Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>>,
    audio_cb: Option<AudioFrameCallback>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(AudioRecorder {
            device: None,
            cmd_tx: None,
            worker_handle: None,
            vad: None,
            level_cb: None,
            audio_cb: None,
        })
    }

    /// Attach a single VAD engine, reconfigured per session for the offline vs
    /// streaming hangover tail. The two policies are mutually exclusive within a
    /// recording, so one engine covers both instead of two resident instances.
    pub fn with_vad(
        mut self,
        detector: Box<dyn VoiceActivityDetector>,
        offline_hangover_frames: usize,
        streaming_hangover_frames: usize,
    ) -> Self {
        self.vad = Some(VadConfig {
            detector: Arc::new(Mutex::new(detector)),
            offline_hangover_frames,
            streaming_hangover_frames,
        });
        self
    }

    pub fn with_level_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(Vec<f32>) + Send + Sync + 'static,
    {
        self.level_cb = Some(Arc::new(cb));
        self
    }

    /// Register a callback that receives real-time 16 kHz frames after the active
    /// VAD policy has been applied. Frames arrive in real time, in order, on the
    /// recorder's consumer thread — keep the callback cheap (e.g. forward to a
    /// channel) so it never stalls capture.
    pub fn with_audio_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&[f32]) + Send + Sync + 'static,
    {
        self.audio_cb = Some(Arc::new(cb));
        self
    }

    pub fn open(&mut self, device: Option<Device>) -> Result<(), Box<dyn std::error::Error>> {
        if self.worker_handle.is_some() {
            return Ok(()); // already open
        }

        let (sample_tx, sample_rx) = mpsc::channel::<AudioChunk>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let vad = self.vad.clone();
        // Move the optional level callback into the worker thread
        let level_cb = self.level_cb.clone();
        // Move the optional real-time audio frame callback into the worker thread
        let audio_cb = self.audio_cb.clone();

        let worker = std::thread::spawn(move || {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_for_stream = stop_flag.clone();
            let init_result = (|| -> Result<(cpal::Stream, u32), String> {
                let init_start = Instant::now();
                log::debug!("AudioRecorder init: resolving input device");
                let host = crate::audio_toolkit::get_cpal_host();
                let thread_device = match device {
                    Some(dev) => dev,
                    None => host
                        .default_input_device()
                        .ok_or_else(|| "No input device found".to_string())?,
                };
                log::debug!(
                    "AudioRecorder init: resolved input device in {:?}",
                    init_start.elapsed()
                );

                let build_start = Instant::now();
                log::debug!("AudioRecorder init: resolving stream config");
                let configs = AudioRecorder::preferred_configs(&thread_device)
                    .map_err(|e| format!("Failed to fetch preferred config: {e}"))?;
                log::debug!(
                    "AudioRecorder init: resolved {} stream config candidate(s) in {:?}",
                    configs.len(),
                    build_start.elapsed()
                );

                let mut last_build_error = None;
                let mut built_stream = None;

                for (config, sample_format) in configs {
                    let sample_rate = config.sample_rate.0;
                    let channels = config.channels as usize;

                    log::info!(
                        "Using device: {:?}\nSample rate: {}\nChannels: {}\nFormat: {:?}",
                        thread_device.name(),
                        sample_rate,
                        channels,
                        sample_format
                    );
                    log::debug!("AudioRecorder init: building input stream");

                    let build_result = match sample_format {
                        SampleFormat::U8 => AudioRecorder::build_stream::<u8>(
                            &thread_device,
                            &config,
                            sample_tx.clone(),
                            channels,
                            stop_flag_for_stream.clone(),
                        ),
                        SampleFormat::I8 => AudioRecorder::build_stream::<i8>(
                            &thread_device,
                            &config,
                            sample_tx.clone(),
                            channels,
                            stop_flag_for_stream.clone(),
                        ),
                        SampleFormat::I16 => AudioRecorder::build_stream::<i16>(
                            &thread_device,
                            &config,
                            sample_tx.clone(),
                            channels,
                            stop_flag_for_stream.clone(),
                        ),
                        SampleFormat::I32 => AudioRecorder::build_stream::<i32>(
                            &thread_device,
                            &config,
                            sample_tx.clone(),
                            channels,
                            stop_flag_for_stream.clone(),
                        ),
                        SampleFormat::F32 => AudioRecorder::build_stream::<f32>(
                            &thread_device,
                            &config,
                            sample_tx.clone(),
                            channels,
                            stop_flag_for_stream.clone(),
                        ),
                        sample_format => {
                            last_build_error =
                                Some(format!("Unsupported sample format: {sample_format:?}"));
                            continue;
                        }
                    };

                    match build_result {
                        Ok(stream) => {
                            built_stream = Some((stream, sample_rate));
                            break;
                        }
                        Err(error) => {
                            last_build_error = Some(format!(
                                "Failed to build input stream for {:?}: {error}",
                                config
                            ));
                        }
                    }
                }

                let (stream, sample_rate) = built_stream.ok_or_else(|| {
                    last_build_error.unwrap_or_else(|| "No input stream configs available".into())
                })?;
                log::debug!(
                    "AudioRecorder init: built input stream in {:?}",
                    build_start.elapsed()
                );

                let play_start = Instant::now();
                log::debug!("AudioRecorder init: starting input stream");
                stream
                    .play()
                    .map_err(|e| format!("Failed to start microphone stream: {e}"))?;
                log::debug!(
                    "AudioRecorder init: started input stream in {:?}",
                    play_start.elapsed()
                );

                Ok((stream, sample_rate))
            })();

            match init_result {
                Ok((stream, sample_rate)) => {
                    let _ = init_tx.send(Ok(()));
                    // Keep the stream alive while we process samples.
                    run_consumer(
                        sample_rate,
                        vad,
                        sample_rx,
                        cmd_rx,
                        level_cb,
                        audio_cb,
                        stop_flag,
                    );
                    drop(stream);
                }
                Err(error_message) => {
                    log::error!("{error_message}");
                    let _ = init_tx.send(Err(error_message));
                }
            }
        });

        match init_rx.recv_timeout(MICROPHONE_INIT_TIMEOUT) {
            Ok(Ok(())) => {
                self.cmd_tx = Some(cmd_tx);
                self.worker_handle = Some(worker);
                Ok(())
            }
            Ok(Err(error_message)) => {
                let _ = worker.join();
                let kind = if is_microphone_access_denied(&error_message) {
                    std::io::ErrorKind::PermissionDenied
                } else {
                    std::io::ErrorKind::Other
                };
                Err(Box::new(Error::new(kind, error_message)))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                log::warn!(
                    "Timed out initializing microphone stream after {:?}; queued worker shutdown",
                    MICROPHONE_INIT_TIMEOUT
                );
                let _ = cmd_tx.send(Cmd::Shutdown);
                Err(Box::new(Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "Timed out initializing microphone stream after {:?}",
                        MICROPHONE_INIT_TIMEOUT
                    ),
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = worker.join();
                Err(Box::new(Error::other(
                    "Failed to initialize microphone worker: channel disconnected",
                )))
            }
        }
    }

    pub fn start(&self, vad_policy: VadPolicy) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tx) = &self.cmd_tx {
            tx.send(Cmd::Start(vad_policy))?;
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let (resp_tx, resp_rx) = mpsc::channel();
        if let Some(tx) = &self.cmd_tx {
            tx.send(Cmd::Stop(resp_tx))?;
        }
        Ok(resp_rx.recv()?) // wait for the samples
    }

    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        self.device = None;
        Ok(())
    }

    fn build_stream<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        sample_tx: mpsc::Sender<AudioChunk>,
        channels: usize,
        stop_flag: Arc<AtomicBool>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + SizedSample + Send + 'static,
        f32: cpal::FromSample<T>,
    {
        let mut output_buffer = Vec::new();
        let mut eos_sent = false;

        let stream_cb = move |data: &[T], _: &cpal::InputCallbackInfo| {
            if stop_flag.load(Ordering::Relaxed) {
                if !eos_sent {
                    let _ = sample_tx.send(AudioChunk::EndOfStream);
                    eos_sent = true;
                }
                return;
            }
            eos_sent = false;

            output_buffer.clear();

            if channels == 1 {
                output_buffer.extend(data.iter().map(|&sample| sample.to_sample::<f32>()));
            } else {
                let frame_count = data.len() / channels;
                output_buffer.reserve(frame_count);

                for frame in data.chunks_exact(channels) {
                    let mono_sample = frame
                        .iter()
                        .map(|&sample| sample.to_sample::<f32>())
                        .sum::<f32>()
                        / channels as f32;
                    output_buffer.push(mono_sample);
                }
            }

            if sample_tx
                .send(AudioChunk::Samples(output_buffer.clone()))
                .is_err()
            {
                log::error!("Failed to send samples");
            }
        };

        device.build_input_stream(
            config,
            stream_cb,
            |err| log::error!("Stream error: {}", err),
            None,
        )
    }

    fn preferred_configs(
        device: &cpal::Device,
    ) -> Result<Vec<(StreamConfig, SampleFormat)>, Box<dyn std::error::Error>> {
        // Use the device's native/default sample rate and let the FrameResampler
        // in run_consumer() downsample to 16kHz. This avoids forcing hardware into
        // a non-native rate which can cause issues on some devices (Bluetooth
        // codecs, certain ALSA drivers, etc.).
        #[cfg(target_os = "macos")]
        {
            let _ = device;
            // Avoid CoreAudio config queries in the push-to-talk hot path. Both
            // `supported_input_configs()` and `default_input_config()` can block
            // for tens of seconds with some aggregate or virtual devices.
            return Ok(vec![
                (
                    StreamConfig {
                        channels: 1,
                        sample_rate: SampleRate(48_000),
                        buffer_size: BufferSize::Default,
                    },
                    SampleFormat::F32,
                ),
                (
                    StreamConfig {
                        channels: 2,
                        sample_rate: SampleRate(48_000),
                        buffer_size: BufferSize::Default,
                    },
                    SampleFormat::F32,
                ),
                (
                    StreamConfig {
                        channels: 1,
                        sample_rate: SampleRate(44_100),
                        buffer_size: BufferSize::Default,
                    },
                    SampleFormat::F32,
                ),
                (
                    StreamConfig {
                        channels: 2,
                        sample_rate: SampleRate(44_100),
                        buffer_size: BufferSize::Default,
                    },
                    SampleFormat::F32,
                ),
            ]);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let default_config = device.default_input_config()?;
            Ok(vec![(
                default_config.config(),
                default_config.sample_format(),
            )])
        }
    }
}

pub fn is_microphone_access_denied(error_message: &str) -> bool {
    let normalized = error_message.to_lowercase();
    normalized.contains("access is denied")
        || normalized.contains("permission denied")
        || normalized.contains("0x80070005")
}

pub fn is_no_input_device_error(error_message: &str) -> bool {
    let normalized = error_message.to_lowercase();
    normalized.contains("no input device found")
        || (normalized.contains("failed to fetch preferred config")
            && normalized.contains("coreaudio"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{is_microphone_access_denied, is_no_input_device_error, MICROPHONE_INIT_TIMEOUT};

    #[test]
    fn detects_access_is_denied() {
        assert!(is_microphone_access_denied("Access is denied"));
    }

    #[test]
    fn detects_permission_denied() {
        assert!(is_microphone_access_denied("permission denied"));
    }

    #[test]
    fn detects_windows_error_code() {
        assert!(is_microphone_access_denied("WASAPI error: 0x80070005"));
    }

    #[test]
    fn does_not_match_unrelated_errors() {
        assert!(!is_microphone_access_denied("device not found"));
    }

    #[test]
    fn detects_no_input_device() {
        assert!(is_no_input_device_error("No input device found"));
    }

    #[test]
    fn detects_coreaudio_config_error() {
        assert!(is_no_input_device_error(
            "Failed to fetch preferred config: A backend-specific error has occurred: An unknown error unknown to the coreaudio-rs API occurred"
        ));
    }

    #[test]
    fn does_not_match_other_errors_for_no_device() {
        assert!(!is_no_input_device_error("permission denied"));
        assert!(!is_no_input_device_error("device not found"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn microphone_init_timeout_allows_slow_macos_permission_prompt() {
        assert!(MICROPHONE_INIT_TIMEOUT >= Duration::from_secs(300));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn microphone_init_timeout_stays_short_off_macos() {
        assert!(MICROPHONE_INIT_TIMEOUT <= Duration::from_secs(10));
    }
}

#[allow(clippy::too_many_arguments)]
fn run_consumer(
    in_sample_rate: u32,
    vad: Option<VadConfig>,
    sample_rx: mpsc::Receiver<AudioChunk>,
    cmd_rx: mpsc::Receiver<Cmd>,
    level_cb: Option<Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>>,
    audio_cb: Option<AudioFrameCallback>,
    stop_flag: Arc<AtomicBool>,
) {
    let mut frame_resampler = FrameResampler::new(
        in_sample_rate as usize,
        constants::WHISPER_SAMPLE_RATE as usize,
        Duration::from_millis(30),
    );

    let mut processed_samples = Vec::<f32>::new();
    let mut recording = false;
    let mut vad_policy = VadPolicy::Offline;

    // ---------- spectrum visualisation setup ---------------------------- //
    const BUCKETS: usize = 16;
    // Scale the FFT window to the device sample rate so the analysis window
    // (~33 ms) and frequency resolution (~30 Hz/bin) stay roughly constant
    // across devices. A fixed 512-sample window collapses the low vocal
    // buckets onto a single bin at 48 kHz (e.g. built-in laptop mics), and
    // would stutter at ~4-8 updates/sec on an 8-16 kHz Bluetooth headset.
    // Targets: 48 kHz -> 2048, 16 kHz -> 512, 8 kHz -> 256.
    let target_window = (f64::from(in_sample_rate) / 30.0).round() as usize;
    let window_size = [256usize, 512, 1024, 2048]
        .into_iter()
        .min_by_key(|w| w.abs_diff(target_window))
        .unwrap();
    let mut visualizer = AudioVisualiser::new(
        in_sample_rate,
        window_size,
        BUCKETS,
        400.0,  // vocal_min_hz
        4000.0, // vocal_max_hz
    );

    fn handle_frame(
        samples: &[f32],
        recording: bool,
        vad_policy: VadPolicy,
        vad: &Option<VadConfig>,
        audio_cb: &Option<AudioFrameCallback>,
        out_buf: &mut Vec<f32>,
    ) {
        if !recording {
            return;
        }

        let mut emit = |buf: &[f32]| {
            out_buf.extend_from_slice(buf);
            if let Some(cb) = audio_cb {
                cb(buf);
            }
        };

        if vad_policy == VadPolicy::Disabled {
            emit(samples);
            return;
        }

        if let Some(cfg) = vad {
            let mut det = cfg.detector.lock().unwrap();
            match det.push_frame(samples).unwrap_or(VadFrame::Speech(samples)) {
                VadFrame::Speech(buf) => emit(buf),
                VadFrame::Noise => {}
            }
        } else {
            emit(samples);
        }
    }

    // Runs until the stream closes and `recv` returns `Err`.
    while let Ok(chunk) = sample_rx.recv() {
        let raw = match chunk {
            AudioChunk::Samples(s) => s,
            AudioChunk::EndOfStream => continue,
        };

        // ---------- spectrum processing ---------------------------------- //
        if let Some(buckets) = visualizer.feed(&raw) {
            if let Some(cb) = &level_cb {
                cb(buckets);
            }
        }

        // ---------- existing pipeline ------------------------------------ //
        frame_resampler.push(&raw, &mut |frame: &[f32]| {
            handle_frame(
                frame,
                recording,
                vad_policy,
                &vad,
                &audio_cb,
                &mut processed_samples,
            )
        });

        // non-blocking check for a command
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Start(policy) => {
                    stop_flag.store(false, Ordering::Relaxed);
                    vad_policy = policy;
                    processed_samples.clear();
                    recording = true;
                    visualizer.reset();
                    // Reconfigure the single VAD engine for this session's policy
                    // and clear its smoothing + recurrent state before it sees
                    // any frames.
                    if vad_policy != VadPolicy::Disabled {
                        if let Some(cfg) = &vad {
                            let mut det = cfg.detector.lock().unwrap();
                            det.set_hangover_frames(cfg.hangover_for(vad_policy));
                            det.reset();
                        }
                    }
                }
                Cmd::Stop(reply_tx) => {
                    recording = false;
                    stop_flag.store(true, Ordering::Relaxed);

                    // Drain all remaining audio until the producer confirms end-of-stream.
                    // The cpal callback sees the stop flag, sends EndOfStream, and goes
                    // silent — guaranteeing every captured sample is in the channel
                    // ahead of the sentinel.
                    loop {
                        match sample_rx.recv_timeout(Duration::from_secs(2)) {
                            Ok(AudioChunk::Samples(remaining)) => {
                                frame_resampler.push(&remaining, &mut |frame: &[f32]| {
                                    handle_frame(
                                        frame,
                                        true,
                                        vad_policy,
                                        &vad,
                                        &audio_cb,
                                        &mut processed_samples,
                                    )
                                });
                            }
                            Ok(AudioChunk::EndOfStream) => break,
                            Err(_) => {
                                log::warn!("Timed out waiting for EndOfStream from audio callback");
                                break;
                            }
                        }
                    }

                    frame_resampler.finish(&mut |frame: &[f32]| {
                        handle_frame(
                            frame,
                            true,
                            vad_policy,
                            &vad,
                            &audio_cb,
                            &mut processed_samples,
                        )
                    });

                    let _ = reply_tx.send(std::mem::take(&mut processed_samples));

                    // Resume the audio callback so the consumer loop can continue
                    // receiving chunks (important for always-on microphone mode).
                    stop_flag.store(false, Ordering::Relaxed);
                }
                Cmd::Shutdown => {
                    stop_flag.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    }
}
