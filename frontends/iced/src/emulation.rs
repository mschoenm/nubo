use crate::audio::AudioOutput;
use crate::cartridge_effects::DesktopCartridgeEffects;
use retroboy_core::emulator::{Emulator, Key};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const IDLE_SLEEP: Duration = Duration::from_millis(1);

enum Command {
    Press(Key),
    Release(Key),
    SetPaused(bool),
}

#[derive(Default)]
struct SharedFrame {
    sequence: u64,
    pixels: Vec<u8>,
}

pub struct Session {
    title: String,
    commands: Sender<Command>,
    frame: Arc<Mutex<SharedFrame>>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    pub fn start(rom: Vec<u8>, cgb_mode: bool) -> Result<Session, String> {
        let (commands, command_receiver) = mpsc::channel();
        let (loaded_sender, loaded_receiver) = mpsc::channel();
        let frame = Arc::new(Mutex::new(SharedFrame::default()));
        let running = Arc::new(AtomicBool::new(true));

        let thread = {
            let frame = frame.clone();
            let running = running.clone();

            thread::Builder::new()
                .name(String::from("emulator"))
                .spawn(move || {
                    let audio = AudioOutput::open();
                    let mut emulator = Emulator::new(false);
                    emulator.set_cgb_mode(cgb_mode);
                    emulator.set_sample_rate(audio.sample_rate());

                    match emulator.load_rom(&rom, Box::new(DesktopCartridgeEffects)) {
                        Ok(header) => {
                            let _ = loaded_sender.send(Ok(header.title));
                            run(emulator, audio, command_receiver, frame, running);
                        }
                        Err(error) => {
                            let _ = loaded_sender.send(Err(error.to_string()));
                        }
                    }
                })
                .map_err(|error| error.to_string())?
        };

        match loaded_receiver.recv() {
            Ok(Ok(title)) => Ok(Session {
                title,
                commands,
                frame,
                running,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(String::from("The emulator thread stopped unexpectedly"))
            }
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn press(&self, key: Key) {
        let _ = self.commands.send(Command::Press(key));
    }

    pub fn release(&self, key: Key) {
        let _ = self.commands.send(Command::Release(key));
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.commands.send(Command::SetPaused(paused));
    }

    pub fn frame_since(&self, sequence: u64) -> Option<(u64, Vec<u8>)> {
        let frame = self.frame.lock().ok()?;
        (frame.sequence != sequence).then(|| (frame.sequence, frame.pixels.clone()))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    mut emulator: Emulator,
    mut audio: AudioOutput,
    commands: Receiver<Command>,
    frame: Arc<Mutex<SharedFrame>>,
    running: Arc<AtomicBool>,
) {
    let mut paused = false;

    while running.load(Ordering::Relaxed) {
        for command in commands.try_iter() {
            match command {
                Command::Press(key) => emulator.handle_key_press(&key),
                Command::Release(key) => emulator.handle_key_release(&key),
                Command::SetPaused(value) => paused = value,
            }
        }

        if paused || !audio.needs_samples() {
            thread::sleep(IDLE_SLEEP);
            continue;
        }

        let (left, right) = emulator.step_until_next_audio_buffer();
        audio.queue(left, right);

        if let Some(pixels) = emulator.take_frame() {
            publish(&frame, pixels);
        }
    }
}

fn publish(frame: &Mutex<SharedFrame>, pixels: &[u8]) {
    if let Ok(mut shared) = frame.lock() {
        shared.pixels.clear();
        shared.pixels.extend_from_slice(pixels);
        shared.sequence += 1;
    }
}
