//! A running game as an app sees it: its emulator session, how it plays
//! (pausing, fast forward, volume), its cheats, the save state slots a menu
//! lists, and the picture on screen in the chosen palette. What the app
//! draws over it, such as a pause menu, stays with the app.

#[cfg(not(target_arch = "wasm32"))]
use super::FrameListener;
use super::{Command, Playback, Session};
use crate::cheats::{self, Cheat};
use crate::controls::Controls;
use crate::notices::Notices;
use crate::palettes::DmgPalette;
use crate::picture::{Picture, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::rom::Rom;
use crate::save_states::{self, SaveState, Slot, SlotInfo};
use crate::settings::Settings;
use nubo_core::{CartridgeHeader, ControllerState, Key, Model};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The number the next listing of save state slots gets (see
/// [`GameSession::list_slots`]).
static NEXT_SLOT_LISTING: AtomicU64 = AtomicU64::new(1);

/// A running game: the emulator [`Session`] it runs in, and what an app
/// shows and keeps of it. Dropping it stops the game and writes its battery
/// saves still waiting.
pub struct GameSession {
    session: Session,
    rom: Rom,
    header: CartridgeHeader,
    /// The model the game is running on.
    model: Model,
    screen: Screen,
    /// The save state slots last listed (see [`GameSession::list_slots`]).
    slots: Vec<SlotInfo>,
    /// Which listing `slots` are, unique for the whole run of the app, so
    /// pictures read for an earlier one are left out.
    slot_listing: u64,
    /// Whether the pictures of `slots` were asked for since they were
    /// listed.
    thumbnails_requested: bool,
    cheats: Vec<Cheat>,
    /// Fast forward, and how the emulator was last told to play.
    playback: PlaybackControl,
    /// What the controllers were last said to be doing.
    controller_state: ControllerState,
}

impl GameSession {
    /// Starts `rom` on `model`, running (or paused, without `running`) at
    /// normal speed and as loud as `settings` say, returning the load error
    /// if it couldn't. Problems while the game runs, such as a missing sound
    /// device, go to `notices`.
    pub fn start(
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        notices: &Notices,
        running: bool,
        settings: &Settings,
    ) -> Result<GameSession, String> {
        let playback = PlaybackControl::new(running, settings);
        let session = Session::start(&rom, model, playback.sent, notices)?;

        Ok(GameSession {
            session,
            rom,
            header,
            model,
            screen: Screen::new(!model.is_cgb()),
            slots: Vec::new(),
            slot_listing: 0,
            thumbnails_requested: false,
            cheats: Vec::new(),
            playback,
            controller_state: ControllerState::new(),
        })
    }

    /// The ROM the game was started from.
    pub fn rom(&self) -> &Rom {
        &self.rom
    }

    /// The header of the game's cartridge.
    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    /// The model the game runs on.
    pub fn model(&self) -> Model {
        self.model
    }

    /// The game's name, taken from its file name.
    pub fn name(&self) -> String {
        self.rom.name()
    }

    /// Tells whoever listens to it whenever the emulator finishes a new
    /// frame (see `Session::frame_listener`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn frame_listener(&self) -> FrameListener {
        self.session.frame_listener()
    }

    /// The picture on screen: the newest frame, in the chosen palette.
    pub fn screen(&self) -> Option<&Picture> {
        self.screen.picture.as_ref()
    }

    /// The pixels on screen, in the chosen palette.
    pub fn last_frame(&self) -> Option<&[u8]> {
        self.screen.pixels()
    }

    /// Lists `slots` with what is saved in them, read from disk, such as for
    /// a menu page that opens; no slots clear the list. Each call is a
    /// listing of its own: pictures read for an earlier one are left out
    /// (see [`GameSession::set_thumbnails`]).
    pub fn list_slots(&mut self, slots: &[Slot]) {
        self.slots = save_states::slot_infos(&self.rom.path, slots);
        self.slot_listing = NEXT_SLOT_LISTING.fetch_add(1, Ordering::Relaxed);
        self.thumbnails_requested = false;
    }

    /// The slots last listed with [`GameSession::list_slots`].
    pub fn slots(&self) -> &[SlotInfo] {
        &self.slots
    }

    /// The ROM and the saved slots whose pictures the latest listing still
    /// needs, once per listing, with the listing they are for. Decoding them
    /// is slow work, so the app reads them with `save_states::thumbnails` on
    /// a background thread and hands them to [`GameSession::set_thumbnails`].
    pub fn thumbnails_to_load(&mut self) -> Option<(u64, PathBuf, Vec<Slot>)> {
        if std::mem::replace(&mut self.thumbnails_requested, true) {
            return None;
        }
        let slots: Vec<Slot> = self
            .slots
            .iter()
            .filter(|info| info.saved_at.is_some())
            .map(|info| info.slot)
            .collect();
        (!slots.is_empty()).then(|| (self.slot_listing, self.rom.path.clone(), slots))
    }

    /// Shows the pictures `save_states::thumbnails` read for the listing
    /// `slot_listing`, if it is still the latest: pictures read for an
    /// earlier listing may be older than a save made since.
    pub fn set_thumbnails(&mut self, slot_listing: u64, thumbnails: Vec<(Slot, Option<Picture>)>) {
        if slot_listing != self.slot_listing {
            return;
        }
        for (slot, thumbnail) in thumbnails {
            if let Some(info) = self.slots.iter_mut().find(|info| info.slot == slot) {
                info.thumbnail = thumbnail;
            }
        }
    }

    /// The game's cheats, on or off.
    pub fn cheats(&self) -> &[Cheat] {
        &self.cheats
    }

    /// Replaces the game's cheats and turns on the enabled ones.
    pub fn set_cheats(&mut self, cheats: Vec<Cheat>) {
        for cheat in &cheats {
            self.session.send(Command::SetCheat(cheat.clone()));
        }
        self.cheats = cheats;
    }

    /// Turns on `cheat` and keeps it with the game's cheats. A cheat with
    /// the same code is turned on and renamed instead of added again.
    pub fn add_cheat(&mut self, cheat: Cheat) -> Result<(), String> {
        if let Some(existing) = self
            .cheats
            .iter_mut()
            .find(|known| known.code == cheat.code)
        {
            existing.enabled = true;
            existing.name.clone_from(&cheat.name);
        } else {
            self.cheats.push(cheat.clone());
        }
        self.session.send(Command::SetCheat(cheat));
        self.save_cheats()
    }

    /// Turns the cheat at `index` on or off.
    pub fn set_cheat_enabled(&mut self, index: usize, enabled: bool) -> Result<(), String> {
        let Some(cheat) = self.cheats.get_mut(index) else {
            return Ok(());
        };
        cheat.enabled = enabled;
        self.session.send(Command::SetCheat(cheat.clone()));
        self.save_cheats()
    }

    /// Turns off and forgets the cheat at `index`.
    pub fn remove_cheat(&mut self, index: usize) -> Result<(), String> {
        if index >= self.cheats.len() {
            return Ok(());
        }
        let mut cheat = self.cheats.remove(index);
        cheat.enabled = false;
        self.session.send(Command::SetCheat(cheat));
        self.save_cheats()
    }

    /// Writes the game's cheats to its cheat list.
    fn save_cheats(&self) -> Result<(), String> {
        cheats::save(&self.rom.path, &self.cheats)
    }

    /// Shows original Game Boy games in `palette` from now on, repainting
    /// the frame on screen.
    pub fn set_palette(&mut self, palette: DmgPalette) {
        self.screen.set_palette(palette);
    }

    /// Whether the game fast forwards: the key is held, or fast forward is
    /// switched on.
    pub fn is_fast_forwarding(&self) -> bool {
        self.playback.is_fast_forwarding()
    }

    /// Whether fast forward is switched on (see
    /// [`GameSession::toggle_fast_forward`]).
    pub fn is_fast_forward_on(&self) -> bool {
        self.playback.fast_forward_on
    }

    /// Fast forwards while `held`, such as while a key is held down.
    pub fn hold_fast_forward(&mut self, held: bool) {
        self.playback.fast_forward_held = held;
    }

    /// Switches fast forward on or off, such as with a button.
    pub fn toggle_fast_forward(&mut self) {
        self.playback.fast_forward_on = !self.playback.fast_forward_on;
    }

    /// Applies the settings and pause state to the emulator: it runs only
    /// when `running` is set, at fast forward speed while fast forwarding.
    /// Nothing is sent when nothing changed since the last call, since every
    /// command wakes the emulator.
    pub fn apply(&mut self, running: bool, settings: &Settings) {
        if let Some(playback) = self.playback.change(running, settings) {
            self.session.send(Command::SetPlayback(playback));
        }
    }

    /// Passes on what the controllers are doing, unless the game already
    /// has that (each command wakes the emulator).
    pub fn set_controller_state(&mut self, state: ControllerState) {
        if state != self.controller_state {
            self.controller_state = state;
            self.session.send(Command::SetControllerState(state));
        }
    }

    /// Sets which controller buttons press which Game Boy buttons.
    pub fn set_controls(&mut self, controls: &Controls) {
        self.session
            .send(Command::SetControllerMapping(controls.controller_mapping()));
    }

    /// Presses a Game Boy button.
    pub fn press(&mut self, key: Key) {
        self.session.send(Command::Press(key));
    }

    /// Lets go of a Game Boy button.
    pub fn release(&mut self, key: Key) {
        self.session.send(Command::Release(key));
    }

    /// Saves the game's state in `slot`, with the current screen as its
    /// picture.
    pub fn save(&mut self, slot: Slot) -> Result<(), String> {
        // The app may not take in new frames while its window is minimized,
        // and a controller can still ask for a save then.
        self.refresh_screen();
        let state = SaveState {
            model: self.model,
            data: self.session.save_state()?,
        };
        save_states::save(&self.rom.path, slot, &state, self.last_frame())
    }

    /// Restores a save state made on this game's model.
    pub fn load(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.session.load_state(data)
    }

    /// Emulates the frames due by the screen's latest refresh (see
    /// `Session::run_due_frames`).
    #[cfg(target_arch = "wasm32")]
    pub fn run_due_frames(&mut self) {
        self.session.run_due_frames();
    }

    /// Replaces the picture on screen if the emulator finished a new frame.
    pub fn refresh_screen(&mut self) {
        if let Some((sequence, pixels)) = self.session.frame_since(self.screen.sequence) {
            self.screen.sequence = sequence;
            self.screen.show(pixels);
        }
    }
}

/// The game's picture as an app shows it: the newest frame, in the
/// chosen palette.
#[derive(Debug)]
struct Screen {
    /// The picture. Its pixels also go into thumbnails, save states and
    /// screenshots.
    picture: Option<Picture>,
    /// The newest frame as the emulator drew it, kept to repaint it when the
    /// palette changes, while the palette repaints it. Otherwise `picture`
    /// has the same pixels.
    raw: Option<Vec<u8>>,
    /// The colors original Game Boy games are shown in.
    palette: DmgPalette,
    /// Whether the game runs on an original Game Boy, whose grays the
    /// palette repaints.
    repaintable: bool,
    /// The sequence number of the frame shown, to skip unchanged frames.
    sequence: u64,
}

impl Screen {
    fn new(repaintable: bool) -> Self {
        Screen {
            picture: None,
            raw: None,
            palette: DmgPalette::default(),
            repaintable,
            sequence: 0,
        }
    }

    fn pixels(&self) -> Option<&[u8]> {
        self.picture.as_ref().map(Picture::pixels)
    }

    /// Shows a frame as the emulator drew it, in the chosen palette. Only a
    /// frame that is repainted keeps a copy as the emulator drew it.
    fn show(&mut self, pixels: Vec<u8>) {
        let shown = if self.repaintable && self.palette != DmgPalette::Gray {
            let mut repainted = pixels.clone();
            self.palette.recolor(&mut repainted);
            self.raw = Some(pixels);
            repainted
        } else {
            self.raw = None;
            pixels
        };

        self.picture = Some(Picture::new(SCREEN_WIDTH, SCREEN_HEIGHT, shown));
    }

    /// Shows original Game Boy games in `palette` from now on, repainting
    /// the frame on screen.
    fn set_palette(&mut self, palette: DmgPalette) {
        if palette == self.palette {
            return;
        }

        let raw = self
            .raw
            .take()
            .or_else(|| self.pixels().map(<[u8]>::to_vec));
        self.palette = palette;
        if self.repaintable
            && let Some(raw) = raw
        {
            self.show(raw);
        }
    }
}

/// Whether fast forward is on, and how the emulator was last told to play,
/// so unchanged playback isn't sent again (each command wakes the
/// emulator).
#[derive(Debug)]
struct PlaybackControl {
    /// Whether the fast forward key is held down.
    fast_forward_held: bool,
    /// Whether fast forward was switched on, such as with a button.
    fast_forward_on: bool,
    /// How the emulator was last told to play.
    sent: Playback,
}

impl PlaybackControl {
    /// How a new game plays: running or not, at normal speed, as loud as
    /// `settings` say.
    fn new(running: bool, settings: &Settings) -> Self {
        PlaybackControl {
            fast_forward_held: false,
            fast_forward_on: false,
            sent: Playback {
                running,
                fast_forward: None,
                volume: settings.effective_volume(),
            },
        }
    }

    fn is_fast_forwarding(&self) -> bool {
        self.fast_forward_held || self.fast_forward_on
    }

    /// How the game should play now: only while `running`, at the fast
    /// forward speed in `settings` while fast forwarding. Returns `None` if
    /// the emulator already plays that way.
    fn change(&mut self, running: bool, settings: &Settings) -> Option<Playback> {
        let wanted = Playback {
            running,
            fast_forward: self
                .is_fast_forwarding()
                .then_some(settings.fast_forward_speed),
            volume: settings.effective_volume(),
        };

        (wanted != self.sent).then(|| {
            self.sent = wanted;
            wanted
        })
    }
}

#[cfg(test)]
mod tests;
