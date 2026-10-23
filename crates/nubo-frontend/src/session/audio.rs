//! Sound output with cpal: the sound card's on the desktop, WebAudio in the
//! browser build (cpal picks its backend by the target, and the `desktop` or
//! `web` feature brings cpal). Samples wait in a queue that the sound card's
//! callback drains; the emulator watches the queue's length to know when to
//! produce more, which keeps emulation running at real-time speed. On the
//! desktop the callback also wakes the emulator thread when the queue runs
//! low, so the thread can sleep until then.

use crate::notices::Notices;
use crate::time::{Duration, Instant};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::Thread;

/// The emulator produces more audio when fewer stereo samples than this are
/// queued.
const QUEUE_LOW_THRESHOLD: usize = 2048;
/// The sample rate used to pace emulation when there is no sound device.
const FALLBACK_SAMPLE_RATE: u32 = 44100;

/// Stereo samples (left, right) waiting to be played, shared with the sound
/// card's callback.
#[derive(Default)]
struct SampleQueue {
    samples: Mutex<VecDeque<(f32, f32)>>,
    /// The thread the callback wakes when it leaves fewer than
    /// `QUEUE_LOW_THRESHOLD` samples, if one asked for that. It only wakes it
    /// as the queue drops below that, since the thread only waits while the
    /// queue has more.
    low_waker: OnceLock<Thread>,
}

/// The sound output and its sample queue. Without a sound device it still
/// drains the queue at the sample rate, so emulation keeps its speed.
pub struct AudioOutput {
    queue: Arc<SampleQueue>,
    sample_rate: u32,
    /// `None` when no sound device could be opened.
    stream: Option<Stream>,
    /// When `drain_elapsed` last discarded samples (only used without a
    /// sound device).
    last_drain: Instant,
    /// How loud to play, from 0 (silent) to 1 (as loud as the game).
    volume: f32,
    /// Where problems restarting the sound go.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "only the desktop build pauses the sound")
    )]
    notices: Notices,
}

impl AudioOutput {
    /// Opens the default sound device, or runs silently (reporting why to
    /// `notices`) if there is none.
    pub fn open(notices: &Notices) -> Self {
        let queue = Arc::new(SampleQueue::default());

        let (stream, sample_rate) = match open_stream(queue.clone(), notices) {
            Ok((stream, sample_rate)) => (Some(stream), sample_rate),
            Err(error) => {
                notices.report(format!("No sound: {error}"));
                (None, FALLBACK_SAMPLE_RATE)
            }
        };

        AudioOutput {
            queue,
            sample_rate,
            stream,
            last_drain: Instant::now(),
            volume: 1.0,
            notices: notices.clone(),
        }
    }

    /// Stops the sound card while the game is paused, so it doesn't keep
    /// waking up to play silence, and starts it again when the game goes on.
    /// Samples still queued are dropped, so the game doesn't go on with the
    /// sound from before the pause. A sound card that can't pause keeps
    /// playing silence instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_paused(&mut self, paused: bool) {
        self.drop_queued_samples();

        if let Some(stream) = &self.stream {
            if paused {
                let _ = stream.pause();
            } else if let Err(error) = stream.play() {
                self.notices.report(format!("Sound error: {error}"));
            }
        }
    }

    /// Drops the samples still queued, so the game doesn't go on with the
    /// sound from before the pause. The browser's sound keeps running, and
    /// plays silence while the game is paused: a browser may refuse to start
    /// it again without a click or key press.
    #[cfg(target_arch = "wasm32")]
    pub fn set_paused(&mut self, _paused: bool) {
        self.drop_queued_samples();
    }

    /// Empties the queue, and starts counting what a missing sound device
    /// plays from now.
    fn drop_queued_samples(&mut self) {
        if let Ok(mut samples) = self.queue.samples.lock() {
            samples.clear();
        }
        self.last_drain = Instant::now();
    }

    /// An output without a sound device, which drains its queue at the
    /// speed a sound card would, for tests.
    #[cfg(test)]
    pub fn without_device() -> Self {
        AudioOutput {
            queue: Arc::new(SampleQueue::default()),
            sample_rate: FALLBACK_SAMPLE_RATE,
            stream: None,
            last_drain: Instant::now(),
            volume: 1.0,
            notices: Notices::default(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether the queue is running low and the emulator should produce more
    /// audio.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn needs_samples(&mut self) -> bool {
        self.queued_samples() < QUEUE_LOW_THRESHOLD
    }

    /// How many stereo samples are waiting to be played.
    pub fn queued_samples(&mut self) -> usize {
        if self.stream.is_none() {
            self.drain_elapsed();
        }

        self.queue.samples.lock().map_or(0, |samples| samples.len())
    }

    /// Has the sound card's callback wake `thread` (with [`Thread::unpark`])
    /// whenever it leaves the queue running low.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wake_when_low(&self, thread: Thread) {
        let _ = self.queue.low_waker.set(thread);
    }

    /// Sleeps until the queue runs low, something unparks this thread, or
    /// `limit` has passed. With a sound device that relies on
    /// [`AudioOutput::wake_when_low`] having been called for this thread;
    /// without one it works out when the queue will have drained that far.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_until_samples_needed(&mut self, limit: Duration) {
        let wait = if self.stream.is_some() {
            limit
        } else {
            let excess = self
                .queued_samples()
                .saturating_sub(QUEUE_LOW_THRESHOLD - 1);
            self.playing_time(excess).min(limit)
        };
        std::thread::park_timeout(wait);
    }

    /// How many stereo samples the sound card takes from the queue at a
    /// time, as far as it says, or `QUEUE_LOW_THRESHOLD` when it doesn't.
    #[cfg(target_arch = "wasm32")]
    pub fn buffer_samples(&self) -> usize {
        self.stream
            .as_ref()
            .and_then(|stream| stream.buffer_size().ok())
            .and_then(|frames| usize::try_from(frames).ok())
            .filter(|&frames| frames > 0)
            .unwrap_or(QUEUE_LOW_THRESHOLD)
    }

    /// Sets how loud to play, from 0 (silent) to 1 (as loud as the game).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Adds samples for the left and right channels to the queue, and
    /// returns how long they take to play.
    pub fn queue(&self, left: &[f32], right: &[f32]) -> Duration {
        let volume = self.volume;
        if let Ok(mut queue) = self.queue.samples.lock() {
            queue.extend(
                left.iter()
                    .zip(right)
                    .map(|(left, right)| (left * volume, right * volume)),
            );
        }
        self.playing_time(left.len())
    }

    /// How long `samples` stereo samples take to play at the sample rate.
    pub fn playing_time(&self, samples: usize) -> Duration {
        Duration::from_secs_f64(samples as f64 / f64::from(self.sample_rate))
    }

    /// Without a sound device, discards as many samples as a sound card would
    /// have played since the last call.
    fn drain_elapsed(&mut self) {
        let consumed =
            (self.last_drain.elapsed().as_secs_f64() * f64::from(self.sample_rate)) as usize;
        if consumed == 0 {
            return;
        }

        self.last_drain += self.playing_time(consumed);

        if let Ok(mut queue) = self.queue.samples.lock() {
            let drained = consumed.min(queue.len());
            queue.drain(..drained);
        }
    }
}

/// Opens and starts an output stream on the default device that plays from
/// `queue`, and returns it with the device's sample rate. Errors while it
/// plays go to `notices`.
fn open_stream(queue: Arc<SampleQueue>, notices: &Notices) -> Result<(Stream, u32), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device found")?;
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let config = supported.config();
    let sample_rate = config.sample_rate;

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, config, queue, notices.clone()),
        SampleFormat::I16 => build_stream::<i16>(&device, config, queue, notices.clone()),
        SampleFormat::U16 => build_stream::<u16>(&device, config, queue, notices.clone()),
        format => Err(format!("unsupported sample format {format:?}")),
    }?;

    stream.play().map_err(|error| error.to_string())?;

    Ok((stream, sample_rate))
}

/// Builds an output stream for the device's sample type `T`.
fn build_stream<T>(
    device: &Device,
    config: StreamConfig,
    queue: Arc<SampleQueue>,
    notices: Notices,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| fill_output_buffer(data, channels, &queue),
            move |error| notices.report(format!("Sound error: {error}")),
            None,
        )
        .map_err(|error| error.to_string())
}

/// Fills the sound card's buffer from the queue, and wakes the thread that
/// asked for it when this leaves the queue running low. Mono devices get the
/// average of both channels, extra channels get silence, and silence also
/// fills in when the queue runs dry.
///
/// This runs on the sound card's real-time thread and locks the queue,
/// which the emulator thread locks too. Real-time code usually avoids locks,
/// but this one is harmless: the emulator thread only holds it for
/// microseconds (to read the queue's length, or to add or drop a few
/// hundred samples, into room the queue kept from earlier buffers), and the
/// queue holds tens of milliseconds of sound beyond what this call takes,
/// so a short wait can't make the sound skip. A lock-free ring buffer (such
/// as the rtrb crate) would avoid even that wait, but then only this
/// callback could drop the samples queued before a pause, which the
/// emulator thread does as the game pauses and goes on.
fn fill_output_buffer<T>(data: &mut [T], channels: usize, queue: &SampleQueue)
where
    T: SizedSample + FromSample<f32>,
{
    let Ok(mut samples) = queue.samples.lock() else {
        data.fill(T::EQUILIBRIUM);
        return;
    };
    let was_enough = samples.len() >= QUEUE_LOW_THRESHOLD;

    for frame in data.chunks_mut(channels) {
        let (left, right) = samples.pop_front().unwrap_or((0.0, 0.0));

        match frame {
            [mono] => *mono = T::from_sample((left + right) / 2.0),
            [first, second, rest @ ..] => {
                *first = T::from_sample(left);
                *second = T::from_sample(right);
                rest.fill(T::EQUILIBRIUM);
            }
            [] => {}
        }
    }

    let became_low = was_enough && samples.len() < QUEUE_LOW_THRESHOLD;
    drop(samples);
    if became_low && let Some(thread) = queue.low_waker.get() {
        thread.unpark();
    }
}

#[cfg(test)]
mod tests;
