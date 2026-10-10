use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const QUEUE_LOW_THRESHOLD: usize = 2048;
const FALLBACK_SAMPLE_RATE: u32 = 44100;

type SampleQueue = Arc<Mutex<VecDeque<(f32, f32)>>>;

pub struct AudioOutput {
    queue: SampleQueue,
    sample_rate: u32,
    stream: Option<Stream>,
    last_drain: Instant,
}

impl AudioOutput {
    pub fn open() -> Self {
        let queue = SampleQueue::default();

        let (stream, sample_rate) = match open_stream(queue.clone()) {
            Ok((stream, sample_rate)) => (Some(stream), sample_rate),
            Err(error) => {
                eprintln!("Audio output unavailable, running without sound: {}", error);
                (None, FALLBACK_SAMPLE_RATE)
            }
        };

        AudioOutput {
            queue,
            sample_rate,
            stream,
            last_drain: Instant::now(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn needs_samples(&mut self) -> bool {
        if self.stream.is_none() {
            self.drain_elapsed();
        }

        self.queue
            .lock()
            .map(|queue| queue.len() < QUEUE_LOW_THRESHOLD)
            .unwrap_or(false)
    }

    pub fn queue(&self, left: &[f32], right: &[f32]) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.extend(left.iter().copied().zip(right.iter().copied()));
        }
    }

    fn drain_elapsed(&mut self) {
        let consumed = (self.last_drain.elapsed().as_secs_f64() * self.sample_rate as f64) as usize;
        if consumed == 0 {
            return;
        }

        self.last_drain += Duration::from_secs_f64(consumed as f64 / self.sample_rate as f64);

        if let Ok(mut queue) = self.queue.lock() {
            let drained = consumed.min(queue.len());
            queue.drain(..drained);
        }
    }
}

fn open_stream(queue: SampleQueue) -> Result<(Stream, u32), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device found")?;
    let supported = device.default_output_config().map_err(|error| error.to_string())?;
    let config = supported.config();
    let sample_rate = config.sample_rate;

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, config, queue),
        SampleFormat::I16 => build_stream::<i16>(&device, config, queue),
        SampleFormat::U16 => build_stream::<u16>(&device, config, queue),
        format => Err(format!("unsupported sample format {:?}", format)),
    }?;

    stream.play().map_err(|error| error.to_string())?;

    Ok((stream, sample_rate))
}

fn build_stream<T>(device: &Device, config: StreamConfig, queue: SampleQueue) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| fill(data, channels, &queue),
            |error| eprintln!("Audio stream error: {}", error),
            None,
        )
        .map_err(|error| error.to_string())
}

fn fill<T>(data: &mut [T], channels: usize, queue: &Mutex<VecDeque<(f32, f32)>>)
where
    T: SizedSample + FromSample<f32>,
{
    let Ok(mut queue) = queue.lock() else {
        data.fill(T::EQUILIBRIUM);
        return;
    };

    for frame in data.chunks_mut(channels) {
        let (left, right) = queue.pop_front().unwrap_or((0.0, 0.0));

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
}
