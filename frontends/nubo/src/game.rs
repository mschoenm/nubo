//! A running game in the window: the game itself
//! (`nubo_frontend::session::GameSession`, which runs it and keeps its
//! playback, cheats, palette, save state slots and picture) and what the
//! window draws over it: the auto-hiding bar, the pause menu and the cheat
//! the player is typing.

use nubo_frontend::cheats::Cheat;
use nubo_frontend::save_states::Slot;
use nubo_frontend::session::GameSession;
use nubo_frontend::time::{Duration, Instant};

/// How long the fullscreen bar stays after the mouse stops moving.
const CONTROLS_TIMEOUT: Duration = Duration::from_millis(2500);
/// While the mouse is this close to the top of the screen, the bar there
/// stays.
const CONTROLS_HOVER_HEIGHT: f32 = 64.0;

/// A page of the pause menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenu {
    Main,
    Save,
    Load,
    Cheats,
    /// The colors and border of a game on a Super Game Boy.
    SuperGameBoy,
}

/// The borders a player can pick for a Super Game Boy game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgbBorderChoice {
    /// The game's own border, or Nubo's until the game sends one.
    Game,
    /// Nubo's border, even when the game sent one.
    Nubo,
    /// No border: the Game Boy screen alone.
    None,
}

impl SgbBorderChoice {
    /// Every choice, in the order the menu lists them.
    pub const ALL: [SgbBorderChoice; 3] = [
        SgbBorderChoice::Game,
        SgbBorderChoice::Nubo,
        SgbBorderChoice::None,
    ];

    /// The choice's name, for the menu.
    pub fn label(self) -> &'static str {
        match self {
            SgbBorderChoice::Game => "The game's border",
            SgbBorderChoice::Nubo => "Nubo's border",
            SgbBorderChoice::None => "No border",
        }
    }
}

/// A running game and the controls the window draws over it.
pub struct Game {
    /// The game itself.
    pub session: GameSession,
    /// The pause menu page on show. The game is paused while it's open.
    menu: Option<PauseMenu>,
    /// The highlighted row of the pause menu, moved with the D-pad.
    menu_selection: usize,
    /// What the player is typing on the cheats page.
    pub cheat_form: CheatForm,
    /// Whether the Super Game Boy page shows the editor for the player's
    /// own colors.
    pub sgb_editor_open: bool,
    /// In fullscreen the bar shows until this moment, after the mouse last
    /// moved.
    controls_shown_until: Instant,
    /// Whether the mouse is over the bar along the top in fullscreen.
    cursor_near_bar: bool,
    /// Whether the game keeps save states, cheats, screenshots, its
    /// thumbnail and where the player left off, which a run started with
    /// `--fresh` doesn't (see `command_line`).
    pub keeps_data: bool,
}

impl Game {
    /// Shows `session`, a game that just started, with its bar showing and
    /// no menu open. Without `keeps_data` it offers no save states, cheats
    /// or screenshots.
    pub fn new(session: GameSession, keeps_data: bool) -> Game {
        Game {
            session,
            menu: None,
            menu_selection: 0,
            cheat_form: CheatForm::default(),
            sgb_editor_open: false,
            controls_shown_until: Instant::now() + CONTROLS_TIMEOUT,
            cursor_near_bar: false,
            keeps_data,
        }
    }

    pub fn menu(&self) -> Option<PauseMenu> {
        self.menu
    }

    pub fn is_paused(&self) -> bool {
        self.menu.is_some()
    }

    /// Opens the pause menu on `page`, or closes it with `None`. The save and
    /// load pages list their slots as they open (see
    /// `GameSession::list_slots`).
    pub fn show_menu(&mut self, page: Option<PauseMenu>) {
        let slots: &[Slot] = match page {
            Some(PauseMenu::Save) => &Slot::SAVABLE,
            Some(PauseMenu::Load) => &Slot::LOADABLE,
            _ => &[],
        };
        self.session.list_slots(slots);
        if page != self.menu {
            self.menu_selection = 0;
        }
        self.menu = page;
    }

    pub fn menu_selection(&self) -> usize {
        self.menu_selection
    }

    /// Moves the highlighted menu row by `step` on a page of `rows` rows.
    /// With `wrap`, going up from the first row lands on the last and down
    /// from the last on the first; without it, the selection stops there.
    pub fn move_menu_selection(&mut self, step: isize, rows: usize, wrap: bool) {
        self.menu_selection = moved_selection(self.menu_selection, step, rows, wrap);
    }

    /// Adds the cheat the player typed, or keeps the form's error.
    pub fn add_cheat(&mut self) -> Result<(), String> {
        match Cheat::new(&self.cheat_form.name, &self.cheat_form.code) {
            Ok(cheat) => {
                self.cheat_form = CheatForm::default();
                self.session.add_cheat(cheat)
            }
            Err(error) => {
                self.cheat_form.error = Some(error);
                Ok(())
            }
        }
    }

    /// Whether to show the bar in fullscreen: while paused, while the mouse
    /// is near the top, and for a moment after it moved.
    pub fn controls_visible(&self) -> bool {
        self.is_paused() || self.cursor_near_bar || Instant::now() < self.controls_shown_until
    }

    /// Whether to hide the mouse pointer over the picture: while the game
    /// runs and the mouse hasn't moved for a moment, as for the fullscreen
    /// bar.
    pub fn pointer_hidden(&self) -> bool {
        !self.is_paused() && !self.cursor_near_bar && Instant::now() >= self.controls_shown_until
    }

    /// Whether the fullscreen bar is shown only until a moment still to
    /// come, so the window has to be redrawn then to hide it. The browser
    /// build redraws on every frame anyway.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn controls_hide_later(&self) -> bool {
        !self.is_paused() && !self.cursor_near_bar && Instant::now() < self.controls_shown_until
    }

    /// Shows the bar because the mouse moved to `cursor_y` pixels below the
    /// top of the window.
    pub fn mouse_moved(&mut self, cursor_y: f32) {
        self.controls_shown_until = Instant::now() + CONTROLS_TIMEOUT;
        self.cursor_near_bar = cursor_y < CONTROLS_HOVER_HEIGHT;
    }
}

/// Row `selection` of `rows` moved by `step`: around the other end with
/// `wrap`, or stopped at the first or last row without it.
fn moved_selection(selection: usize, step: isize, rows: usize, wrap: bool) -> usize {
    if rows == 0 {
        return 0;
    }
    let last = rows - 1;
    match selection.checked_add_signed(step) {
        Some(moved) if moved <= last => moved,
        _ if !wrap => selection.saturating_add_signed(step).min(last),
        _ if step < 0 => last,
        _ => 0,
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
