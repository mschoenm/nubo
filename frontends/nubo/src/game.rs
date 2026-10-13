//! A running game: its emulator session, what the window shows of it, and
//! the state of the controls drawn over it (the top bar, the pause menu and
//! fast forward).

use crate::app::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::cheats::{self, Cheat};
use crate::controls::Controls;
#[cfg(not(target_arch = "wasm32"))]
use crate::emulation::FrameListener;
use crate::emulation::{Command, Playback, Session};
use crate::notices::Notices;
use crate::rom::Rom;
use crate::save_states::{self, SaveState, Slot, SlotInfo};
use crate::settings::Settings;
use nubo_core::{CartridgeHeader, ControllerState, Key, Model};
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::picture::Picture;
use nubo_frontend::time::{Duration, Instant};

/// How long the top bar stays after the mouse stops moving.
const CONTROLS_TIMEOUT: Duration = Duration::from_millis(2500);
/// While the mouse is this close to the bottom of the screen, the bar there
/// stays.
const CONTROLS_HOVER_HEIGHT: f32 = 64.0;

/// A page of the pause menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenu {
    Main,
    Save,
    Load,
    Cheats,
}

pub struct Game {
    session: Session,
    rom: Rom,
    header: CartridgeHeader,
    /// The model the game is running on.
    model: Model,
    screen: Screen,
    /// The pause menu page on show. The game is paused while it's open.
    menu: Option<PauseMenu>,
    /// The slots listed on the menu's save or load page.
    slots: Vec<SlotInfo>,
    /// The highlighted row of the pause menu, moved with the D-pad.
    menu_selection: usize,
    cheats: Vec<Cheat>,
    /// What the player is typing on the cheats page.
    pub cheat_form: CheatForm,
    /// Fast forward, and how the emulator was last told to play.
    playback: PlaybackControl,
    /// What the controllers were last said to be doing.
    controller_state: ControllerState,
    /// In fullscreen the bar shows until this moment, after the mouse last
    /// moved.
    controls_shown_until: Instant,
    /// Whether the mouse is over the bar along the bottom in fullscreen.
    cursor_near_bar: bool,
}

impl Game {
    /// Starts `rom` on `model`, running (or paused, without `running`) at
    /// normal speed and as loud as `settings` say.
    pub fn start(
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        notices: &Notices,
        running: bool,
        settings: &Settings,
    ) -> Result<Game, String> {
        let playback = PlaybackControl::new(running, settings);
        let session = Session::start(&rom, model, playback.sent, notices)?;

        Ok(Game {
            session,
            rom,
            header,
            model,
            screen: Screen::new(!model.is_cgb()),
            menu: None,
            slots: Vec::new(),
            menu_selection: 0,
            cheats: Vec::new(),
            cheat_form: CheatForm::default(),
            playback,
            controller_state: ControllerState::new(),
            controls_shown_until: Instant::now() + CONTROLS_TIMEOUT,
            cursor_near_bar: false,
        })
    }

    pub fn rom(&self) -> &Rom {
        &self.rom
    }

    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    pub fn model(&self) -> Model {
        self.model
    }

    /// The game's name, taken from its file name.
    pub fn name(&self) -> String {
        self.rom.name()
    }

    /// Tells whoever listens to it whenever the emulator finishes a new
    /// frame (see [`Session::frame_listener`]).
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

    pub fn menu(&self) -> Option<PauseMenu> {
        self.menu
    }

    pub fn slots(&self) -> &[SlotInfo] {
        &self.slots
    }

    pub fn is_paused(&self) -> bool {
        self.menu.is_some()
    }

    /// Opens the pause menu on `page`, or closes it with `None`. The save and
    /// load pages read their slots from disk as they open.
    pub fn show_menu(&mut self, page: Option<PauseMenu>) {
        self.slots = match page {
            Some(PauseMenu::Save) => save_states::slot_infos(&self.rom.path, &Slot::SAVABLE),
            Some(PauseMenu::Load) => save_states::slot_infos(&self.rom.path, &Slot::LOADABLE),
            _ => Vec::new(),
        };
        if page != self.menu {
            self.menu_selection = 0;
        }
        self.menu = page;
    }

    pub fn menu_selection(&self) -> usize {
        self.menu_selection
    }

    /// Moves the highlighted menu row by `step`, staying within the
    /// `rows` rows of the current page.
    pub fn move_menu_selection(&mut self, step: isize, rows: usize) {
        let last = rows.saturating_sub(1);
        self.menu_selection = self.menu_selection.saturating_add_signed(step).min(last);
    }

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

    /// Adds the cheat the player typed, or keeps the form's error.
    pub fn add_cheat(&mut self) -> Result<(), String> {
        let cheat = match Cheat::new(&self.cheat_form.name, &self.cheat_form.code) {
            Ok(cheat) => cheat,
            Err(error) => {
                self.cheat_form.error = Some(error);
                return Ok(());
            }
        };

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
        self.cheat_form = CheatForm::default();
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

    fn save_cheats(&self) -> Result<(), String> {
        cheats::save(&self.rom.path, &self.cheats)
    }

    /// Shows original Game Boy games in `palette` from now on, repainting
    /// the frame on screen.
    pub fn set_palette(&mut self, palette: DmgPalette) {
        self.screen.set_palette(palette);
    }

    pub fn is_fast_forwarding(&self) -> bool {
        self.playback.is_fast_forwarding()
    }

    pub fn is_fast_forward_on(&self) -> bool {
        self.playback.fast_forward_on
    }

    pub fn hold_fast_forward(&mut self, held: bool) {
        self.playback.fast_forward_held = held;
    }

    pub fn toggle_fast_forward(&mut self) {
        self.playback.fast_forward_on = !self.playback.fast_forward_on;
    }

    /// Whether to show the bar in fullscreen: while paused, while the mouse
    /// is near the bottom, and for a moment after it moved.
    pub fn controls_visible(&self) -> bool {
        self.is_paused() || self.cursor_near_bar || Instant::now() < self.controls_shown_until
    }

    /// Whether the fullscreen bar is shown only until a moment still to
    /// come, so the window has to be redrawn then to hide it. The browser
    /// build redraws on every frame anyway.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn controls_hide_later(&self) -> bool {
        !self.is_paused() && !self.cursor_near_bar && Instant::now() < self.controls_shown_until
    }

    /// Shows the bar because the mouse moved to height `cursor_y` in a window
    /// `window_height` high.
    pub fn mouse_moved(&mut self, cursor_y: f32, window_height: f32) {
        self.controls_shown_until = Instant::now() + CONTROLS_TIMEOUT;
        self.cursor_near_bar = window_height - cursor_y < CONTROLS_HOVER_HEIGHT;
    }

    /// Applies the settings and pause state to the emulator: it runs only
    /// when `running` is set, at fast forward speed while fast forwarding.
    /// Nothing is sent when nothing changed since the last call.
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

    pub fn press(&mut self, key: Key) {
        self.session.send(Command::Press(key));
    }

    pub fn release(&mut self, key: Key) {
        self.session.send(Command::Release(key));
    }

    /// Saves the game's state in `slot`, with the current screen as its
    /// picture.
    pub fn save(&mut self, slot: Slot) -> Result<(), String> {
        // The window doesn't show new frames while it is minimized, and a
        // controller can still ask for a save then.
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

    /// Emulates the frames due by the screen's latest refresh (browser).
    #[cfg(target_arch = "wasm32")]
    pub fn advance(&mut self) {
        self.session.advance();
    }

    /// Replaces the shown image if the emulator finished a new frame.
    pub fn refresh_screen(&mut self) {
        if let Some((sequence, pixels)) = self.session.frame_since(self.screen.sequence) {
            self.screen.sequence = sequence;
            self.screen.show(pixels);
        }
    }
}

/// The game's picture as the window shows it: the newest frame, in the
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
    /// Whether fast forward was switched on with the top bar button.
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

/// The new cheat the player is typing on the cheats page, and what was
/// wrong with the last one they tried to add.
#[derive(Debug, Clone, Default)]
pub struct CheatForm {
    pub name: String,
    pub code: String,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests;
