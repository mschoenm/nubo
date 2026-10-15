//! A running game as an app sees it: its emulator session, how it plays
//! (pausing, fast forward, volume), its cheats, the save state slots a menu
//! lists, and the picture on screen in the chosen palette. What the app
//! draws over it, such as a pause menu, stays with the app.

#[cfg(not(target_arch = "wasm32"))]
use super::FrameListener;
use super::{Command, Playback, Saves, Session};
use crate::cheats::{self, Cheat};
use crate::controls::Controls;
use crate::lcd;
use crate::notices::Notices;
use crate::palettes::DmgPalette;
use crate::picture::{Frame, Picture};
use crate::rom::Rom;
use crate::save_states::{self, SaveState, Slot, SlotInfo};
use crate::settings::{FastForwardSpeed, ScreenLook, Settings};
use crate::sgb_menu::SgbMenu;
use nubo_core::{CartridgeHeader, ControllerState, Key, Model, SgbBorder};
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
    /// What each controller was last said to be doing.
    controller_states: Vec<ControllerState>,
}

/// How [`GameSession::start`] starts a game, besides its ROM and model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOptions {
    /// Whether the game reads and writes its battery saves. Without, it
    /// keeps none (see [`Saves::Discarded`]).
    pub keep_saves: bool,
    /// A speed to fast forward at from the very first frame and for as long
    /// as the game runs, instead of the settings' speed, such as one asked
    /// for on the command line. `None` starts at normal speed.
    pub fast_forward: Option<FastForwardSpeed>,
}

impl GameSession {
    /// Starts `rom` on `model` as `options` say, running (or paused, without
    /// `running`) as loud as `settings` say, returning the load error if it
    /// couldn't. Problems while the game runs, such as a missing sound
    /// device, go to `notices`.
    pub fn start(
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        options: StartOptions,
        notices: &Notices,
        running: bool,
        settings: &Settings,
    ) -> Result<GameSession, String> {
        let playback = PlaybackControl::new(running, options.fast_forward, settings);
        let saves = Saves::for_rom(&rom.path, options.keep_saves, notices);
        let session = Session::start(&rom, saves, model, playback.sent, notices)?;

        Ok(GameSession {
            session,
            rom,
            header,
            model,
            screen: Screen::new(),
            slots: Vec::new(),
            slot_listing: 0,
            thumbnails_requested: false,
            cheats: Vec::new(),
            playback,
            controller_states: Vec::new(),
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

    /// The picture on screen: the newest frame, in the chosen palette, with
    /// a Super Game Boy's border unless it is switched off.
    pub fn screen(&self) -> Option<&Picture> {
        self.screen.picture.as_ref()
    }

    /// The Game Boy screen alone (160×144 RGBA pixels), in the chosen
    /// palette and without any border, for pictures of the game such as a
    /// save state's.
    pub fn last_frame(&self) -> Option<Vec<u8>> {
        self.screen.screen_pixels()
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

    /// Shows the game as `look` says from now on, repainting the frame on
    /// screen. Colors are corrected on a Game Boy Color only: the Game Boy
    /// Advance's screen and a Super Game Boy's TV showed them differently.
    pub fn set_screen_look(&mut self, look: ScreenLook) {
        let color_screen = self.model.is_cgb() && self.model != Model::Agb;
        self.screen.set_palette(look.dmg_palette);
        self.screen
            .set_effects(look.color_correction && color_screen, look.frame_blending);
    }

    /// Shows a Super Game Boy's border around the screen from now on, or
    /// not, showing the frame on screen again.
    pub fn set_sgb_border(&mut self, border: bool) {
        self.screen.set_border(border);
    }

    /// On a Super Game Boy, what its menu's choices are, as the frame on
    /// screen was drawn with them; `None` on other models.
    pub fn sgb_menu(&self) -> Option<SgbMenu> {
        self.screen.raw.as_ref()?.sgb_menu
    }

    /// On a Super Game Boy, colors the screen with `palette` (RGB555,
    /// lightest first), or with the game's colors for `None`. The emulator
    /// shows it on a new frame at once, even while the game is paused: in
    /// the browser the command has run by the time this returns, and on the
    /// desktop the frame comes through the frame listener.
    pub fn set_sgb_player_palette(&mut self, palette: Option<[u16; 4]>) {
        self.session.send(Command::SetSgbPlayerPalette(palette));
        self.refresh_screen();
    }

    /// On a Super Game Boy, picks the border around the screen: the game's
    /// or Nubo's. Whether a border shows at all is
    /// [`GameSession::set_sgb_border`].
    pub fn choose_sgb_border(&mut self, border: SgbBorder) {
        self.session.send(Command::SetSgbBorder(border));
        self.refresh_screen();
    }

    /// Whether the game fast forwards: the key is held, or fast forward is
    /// switched on.
    pub fn is_fast_forwarding(&self) -> bool {
        self.playback.is_fast_forwarding()
    }

    /// How fast the game fast forwards: at its own speed (see
    /// [`StartOptions::fast_forward`]) or else the one in `settings`.
    pub fn fast_forward_speed(&self, settings: &Settings) -> FastForwardSpeed {
        self.playback.speed.unwrap_or(settings.fast_forward_speed)
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

    /// Goes back through the game's recent past while `held`, such as while
    /// a key is held down.
    pub fn hold_rewind(&mut self, held: bool) {
        self.playback.rewinding = held;
    }

    /// Whether the game goes back through its recent past.
    pub fn is_rewinding(&self) -> bool {
        self.playback.rewinding
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

    /// Passes on what each controller is doing, the first controller's
    /// first, unless the game already has that (each command wakes the
    /// emulator). They all play as the first player, until a Super Game Boy
    /// game asks for more players (see `Emulator::set_controller_states`).
    pub fn set_controller_states(&mut self, states: Vec<ControllerState>) {
        if states != self.controller_states {
            self.controller_states.clone_from(&states);
            self.session.send(Command::SetControllerStates(states));
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

    /// Lets go of every Game Boy button the keyboard holds, and of fast
    /// forward and rewind, such as when the window loses focus and won't
    /// hear the keys come up. Controllers' buttons stay as they are.
    pub fn release_held_keys(&mut self) {
        for key in [
            Key::Down,
            Key::Up,
            Key::Left,
            Key::Right,
            Key::Start,
            Key::Select,
            Key::B,
            Key::A,
        ] {
            self.release(key);
        }
        self.hold_fast_forward(false);
        self.hold_rewind(false);
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
        save_states::save(&self.rom.path, slot, &state, self.last_frame().as_deref())
    }

    /// The game's state right now, kept in memory rather than in a slot,
    /// such as to undo a load or a restart.
    pub fn snapshot(&mut self) -> Result<SaveState, String> {
        Ok(SaveState {
            model: self.model,
            data: self.session.save_state()?,
        })
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
        if let Some((sequence, frame)) = self.session.frame_since(self.screen.sequence) {
            self.screen.sequence = sequence;
            self.screen.show(frame);
        }
    }
}

/// The game's picture as an app shows it: the newest frame, in the
/// chosen palette, with or without a Super Game Boy's border.
#[derive(Debug)]
struct Screen {
    /// The picture. Screenshots save it as it is.
    picture: Option<Picture>,
    /// The newest frame as the emulator drew it, kept to show it again when
    /// the palette or the border setting changes.
    raw: Option<Frame>,
    /// The colors original Game Boy games are shown in.
    palette: DmgPalette,
    /// Whether a Super Game Boy's border shows around the screen.
    border: bool,
    /// Whether the colors are shown as on a Game Boy Color's screen (see
    /// `lcd::correct_colors`).
    correct_colors: bool,
    /// Whether each frame is mixed with the one before (see `lcd::blend`).
    blend_frames: bool,
    /// The pixels of the frame before the newest, as the emulator drew
    /// them, to blend the newest with.
    previous: Option<Vec<u8>>,
    /// The sequence number of the frame shown, to skip unchanged frames.
    sequence: u64,
}

impl Screen {
    fn new() -> Self {
        Screen {
            picture: None,
            raw: None,
            palette: DmgPalette::default(),
            border: true,
            correct_colors: false,
            blend_frames: false,
            previous: None,
            sequence: 0,
        }
    }

    /// The Game Boy screen alone (160×144) as shown, in the chosen palette,
    /// without any border. Save states and the library keep it as their
    /// picture.
    fn screen_pixels(&self) -> Option<Vec<u8>> {
        let mut raw = self.raw.clone()?;
        self.correct(&mut raw);
        let picture = raw.shown(self.palette, false);
        Some(picture.pixels().to_vec())
    }

    /// Shows a frame as the emulator drew it, in the chosen palette, mixed
    /// with the one before and with its colors corrected if the player wants.
    fn show(&mut self, frame: Frame) {
        let mut shown = frame.clone();
        if self.blend_frames
            && let Some(previous) = &self.previous
        {
            lcd::blend(&mut shown.pixels, previous);
        }
        self.correct(&mut shown);
        self.picture = Some(shown.shown(self.palette, self.border));
        self.previous = Some(frame.pixels.clone());
        self.raw = Some(frame);
    }

    /// Corrects the colors of a Game Boy Color frame, if the player wants.
    /// Frames in the Game Boy's grays and Super Game Boy frames, which a TV
    /// showed, stay as they are.
    fn correct(&self, frame: &mut Frame) {
        if self.correct_colors && !frame.in_dmg_shades && frame.sgb_menu.is_none() {
            lcd::correct_colors(&mut frame.pixels);
        }
    }

    /// Shows Game Boy Color colors as on its screen, and mixes frames, from
    /// now on as these say, repainting the frame on screen.
    fn set_effects(&mut self, correct_colors: bool, blend_frames: bool) {
        if (correct_colors, blend_frames) != (self.correct_colors, self.blend_frames) {
            self.correct_colors = correct_colors;
            self.blend_frames = blend_frames;
            self.show_again();
        }
    }

    /// Shows original Game Boy games in `palette` from now on, repainting
    /// the frame on screen.
    fn set_palette(&mut self, palette: DmgPalette) {
        if palette != self.palette {
            self.palette = palette;
            self.show_again();
        }
    }

    /// Shows a Super Game Boy's border around the screen from now on, or
    /// not.
    fn set_border(&mut self, border: bool) {
        if border != self.border {
            self.border = border;
            self.show_again();
        }
    }

    /// Shows the newest frame again, after a setting changed.
    fn show_again(&mut self) {
        if let Some(raw) = self.raw.take() {
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
    /// The fast forward speed for this game instead of the settings' one
    /// (see [`StartOptions::fast_forward`]).
    speed: Option<FastForwardSpeed>,
    /// Whether the rewind key is held down.
    rewinding: bool,
    /// How the emulator was last told to play.
    sent: Playback,
}

impl PlaybackControl {
    /// How a new game plays: running or not, as loud as `settings` say, at
    /// normal speed or fast forwarding at `fast_forward` (see
    /// [`StartOptions::fast_forward`]).
    fn new(running: bool, fast_forward: Option<FastForwardSpeed>, settings: &Settings) -> Self {
        PlaybackControl {
            fast_forward_held: false,
            fast_forward_on: fast_forward.is_some(),
            speed: fast_forward,
            rewinding: false,
            sent: Playback {
                running,
                fast_forward,
                volume: settings.effective_volume(),
                rewinding: false,
            },
        }
    }

    fn is_fast_forwarding(&self) -> bool {
        self.fast_forward_held || self.fast_forward_on
    }

    /// How the game should play now: only while `running`, at the fast
    /// forward speed in `settings` (or this game's own) while fast
    /// forwarding. Returns `None` if the emulator already plays that way.
    fn change(&mut self, running: bool, settings: &Settings) -> Option<Playback> {
        let wanted = Playback {
            running,
            fast_forward: self
                .is_fast_forwarding()
                .then_some(self.speed.unwrap_or(settings.fast_forward_speed)),
            volume: settings.effective_volume(),
            rewinding: running && self.rewinding,
        };

        (wanted != self.sent).then(|| {
            self.sent = wanted;
            wanted
        })
    }
}

#[cfg(test)]
mod tests;
