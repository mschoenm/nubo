//! Scenes in the app (see `nubo_frontend::scenes`), for the documentation's
//! screenshots (`docs/SCREENSHOTS_PLAN.md`), desktop only:
//!
//! - The hidden `capture` command shows a scene: [`prepare`] reads it and
//!   sets up the run's own folder, then [`App::start_scene`] starts the game
//!   paused, loads the scene's state, runs its frames and opens what the
//!   scene has open. Once everything shows (and for a scene of the library,
//!   once its games and title screens are found), it saves a picture of the
//!   window and quits, or prints `ready` and keeps showing it for a harness
//!   that captures the window with its border.
//! - The hidden `--record-scenes` option makes Ctrl+F12 save the moment on
//!   screen as a scene ([`App::record_scene`]): the game pauses, its state
//!   is kept, the scene's frames run from it, and a picture of the window
//!   becomes the scene's preview, exactly what `capture` shows of it later.
//!   Scenes keep no toasts or questions: the toasts are left out of the
//!   preview, and nothing is recorded while a question is open. Nor do they
//!   keep the player's own data, so nothing is recorded while it shows (see
//!   [`unkept`]): box art, a last screen played on a library card, the game
//!   folders on the Library settings tab, a cheat that is on, or a page of
//!   the pause menu that lists saved states or cheats; nor while the library
//!   is still finding its games or holds a search. The buttons held are let
//!   go first and what scrolls goes back to the top, as a capture starts.
//!
//! A scene's game keeps no battery saves, with its cartridge clock on
//! emulated time, so a clock doesn't make two pictures of a scene differ.
//!
//! A scene never changes the emulation: it only pauses the game and runs
//! frames the way the game would have.

use super::{App, Message};
use crate::command_line::{CaptureRequest, Options};
use crate::game::PauseMenu;
use crate::settings::{Appearance, Settings};
use crate::ui::toasts::Toasts;
use crate::views::settings::SettingsTab;
use iced::theme::Mode;
use iced::{Task, window};
use nubo_core::Emulator;
use nubo_frontend::background;
use nubo_frontend::rom::Rom;
use nubo_frontend::scenes::{self, PlayedGame, Scene, SceneGame, SceneLibrary, View, WindowSize};
use nubo_frontend::storage::files::{encode_png, use_folder};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a capture waits after the window opens before it looks whether
/// the scene is ready: long enough for the window to settle at its size or
/// fill the screen.
const FIRST_CHECK: Duration = Duration::from_millis(1500);
/// How often a capture looks again while the scene isn't ready yet, such as
/// while a library scans its folder.
const NEXT_CHECK: Duration = Duration::from_millis(250);
/// How long the recorder waits for the window to be drawn again before it
/// takes the picture of it, which shows what was last drawn.
const REDRAW_TIME: Duration = Duration::from_millis(200);

/// A scene being shown by the `capture` command.
#[derive(Debug, Clone)]
pub struct SceneRun {
    scene: Scene,
    /// The game to start and its save state, for a scene of a game.
    game: Option<(Rom, Vec<u8>)>,
    /// Where to save the window's picture, or `None` to keep showing the
    /// scene once it is ready.
    output: Option<PathBuf>,
    /// The folder of the scene's library, for a scene of the library.
    library_folder: Option<PathBuf>,
    /// Whether the library's played games were recorded yet. That waits
    /// for the scan, which adds them to the library.
    played_recorded: bool,
    /// Whether the scene showed and was announced or pictured.
    done: bool,
}

/// What a scene being shown or recorded hears back.
#[derive(Debug, Clone)]
pub enum SceneMessage {
    /// Time to look whether the scene being shown is ready.
    Check,
    /// The window's picture was taken for the scene being shown.
    Pictured(window::Screenshot),
    /// The window's picture was taken for the scene being recorded.
    Recorded(window::Screenshot),
    /// There was no window to take the picture of for the scene being
    /// recorded.
    NoWindowToRecord,
}

impl From<SceneMessage> for Message {
    fn from(message: SceneMessage) -> Self {
        Message::Scene(message)
    }
}

/// A scene the recorder is saving, waiting for the picture of the window.
#[derive(Debug)]
pub struct PendingRecording {
    folder: PathBuf,
    scene: Scene,
    state: Option<Vec<u8>>,
}

/// What [`prepare`] sets up for the `capture` command: the options to start
/// the app with, the scene to show and the window's size.
pub struct PreparedCapture {
    pub options: Options,
    pub run: SceneRun,
    /// The window's inside, or `None` for the size the app opens at
    /// without saved settings.
    pub size: Option<(f32, f32)>,
    pub fullscreen: bool,
}

/// Reads the scene `request` names, with the pause menu page and settings
/// tab it asks for, and sets up the run's own folder
/// (`--home`), which then replaces the player's for the whole run: the
/// scene's settings with `--set`'s changes as `settings.toml`, never asking
/// about box art or new versions, and for a scene of the library a folder
/// with its games. Checks that the game is the one the scene was made with.
pub fn prepare(request: &CaptureRequest) -> Result<PreparedCapture, String> {
    let mut scene = Scene::read(&request.scene)?;
    let chosen = |name: &String| (name != "none").then(|| name.clone());
    if let Some(page) = &request.pause_menu {
        scene.view.pause_menu = chosen(page);
    }
    if let Some(tab) = &request.settings_tab {
        scene.view.settings_tab = chosen(tab);
    }
    if !use_folder(&request.home) {
        return Err(String::from("the run's folder was already set"));
    }

    let library_folder = scene
        .library
        .as_ref()
        .map(|library| copy_library(library, &request.games, &request.home))
        .transpose()?;

    let mut settings = scene.settings.clone();
    scenes::change_settings(&mut settings, &request.changes)?;
    settings.remove("window_size");
    settings.insert(String::from("box_art_asked"), true.into());
    settings.insert(String::from("download_box_art"), false.into());
    settings.insert(String::from("check_for_updates"), false.into());
    let folders = library_folder.iter().map(|folder| {
        toml::Value::Table(toml::Table::from_iter([
            (
                String::from("path"),
                toml::Value::String(folder.display().to_string()),
            ),
            (String::from("include_subfolders"), false.into()),
        ]))
    });
    settings.insert(
        String::from("library_folders"),
        toml::Value::Array(folders.collect()),
    );
    let text = toml::to_string(&settings).map_err(|error| error.to_string())?;
    Settings::from_toml(&text).map_err(|error| format!("the scene's settings: {error}"))?;
    write_file(
        &request.home.join("config").join("settings.toml"),
        text.as_bytes(),
    )?;

    let game = scene
        .game
        .as_ref()
        .map(|game| read_game(game, &request.games, &request.scene))
        .transpose()?;
    let model = scene.game.as_ref().map(SceneGame::model).transpose()?;

    let size = request
        .size
        .or_else(|| scene.window.map(|window| (window.width, window.height)));
    Ok(PreparedCapture {
        options: Options {
            model,
            scale: request.scale,
            ..Options::default()
        },
        run: SceneRun {
            scene,
            game,
            output: request.output.clone(),
            library_folder,
            played_recorded: false,
            done: false,
        },
        size,
        fullscreen: request.fullscreen,
    })
}

/// Reads the scene's game from the `games` folder and its save state from
/// the scene's folder, checking the ROM's checksum.
fn read_game(
    game: &SceneGame,
    games: &Path,
    scene_folder: &Path,
) -> Result<(Rom, Vec<u8>), String> {
    let rom = Rom::read(&games.join(&game.file))?;
    if scenes::sha256_hex(&rom.bytes) != game.sha256 {
        return Err(format!(
            "{} isn't the ROM the scene was made with (its SHA-256 differs)",
            game.file
        ));
    }
    let state_path = scene_folder.join(scenes::STATE_FILE);
    let state = std::fs::read(&state_path)
        .map_err(|error| format!("couldn't read {}: {error}", state_path.display()))?;
    Ok((rom, state))
}

/// Copies the games of a scene's library from `games` into a folder of
/// the run's own, which the library then shows, and returns it.
fn copy_library(library: &SceneLibrary, games: &Path, home: &Path) -> Result<PathBuf, String> {
    let folder = home.join("library");
    for file in &library.games {
        let contents = std::fs::read(games.join(file))
            .map_err(|error| format!("couldn't read the game {file}: {error}"))?;
        write_file(&folder.join(file), &contents)?;
    }
    Ok(folder)
}

/// Writes `contents` to `path`, creating its folder.
fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
    }
    std::fs::write(path, contents)
        .map_err(|error| format!("couldn't write {}: {error}", path.display()))
}

/// Ends a capture that can't go on, saying why.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

/// Sends [`SceneMessage::Check`] after `delay`.
fn check_after(delay: Duration) -> Task<Message> {
    Task::perform(background::run(move || std::thread::sleep(delay)), |_| {
        SceneMessage::Check.into()
    })
}

impl App {
    /// Shows the scene the `capture` command prepared (see the module's
    /// documentation). The game starts paused and stays paused.
    pub fn start_scene(&mut self, mut run: SceneRun) -> Task<Message> {
        self.toasts = Toasts::printed();
        let game = run.game.take();
        let view = run.scene.view.clone();
        let frames = run.scene.game.as_ref().map_or(0, |game| game.frames);
        self.scene_run = Some(run);

        if let Some((rom, state)) = game {
            self.start_game(rom, false);
            let Some(game) = &mut self.game else {
                fail("the scene's game didn't start");
            };
            if let Err(error) = game
                .session
                .load(state)
                .and_then(|()| game.session.run_frames(frames))
            {
                fail(&format!("the scene's state couldn't be shown: {error}"));
            }
            game.hide_controls();
            if let Some(page) = &view.pause_menu {
                let page = pause_menu_page(page).unwrap_or_else(|error| fail(&error));
                game.show_menu(Some(page));
            }
        }
        if let Some(tab) = &view.settings_tab {
            self.settings_tab = Some(settings_tab(tab).unwrap_or_else(|error| fail(&error)));
        }

        check_after(FIRST_CHECK)
    }

    /// Handles what a scene being shown or recorded hears back.
    pub(super) fn update_scene(&mut self, message: SceneMessage) -> Task<Message> {
        match message {
            SceneMessage::Check => self.check_scene(),
            SceneMessage::Pictured(screenshot) => {
                let output = self
                    .scene_run
                    .as_ref()
                    .and_then(|run| run.output.clone())
                    .unwrap_or_else(|| fail("no picture was asked for"));
                let png = encode_png(
                    screenshot.size.width,
                    screenshot.size.height,
                    &screenshot.rgba,
                )
                .and_then(|png| write_file(&output, &png));
                if let Err(error) = png {
                    fail(&error);
                }
                iced::exit()
            }
            SceneMessage::Recorded(screenshot) => {
                self.finish_recording(&screenshot);
                Task::none()
            }
            SceneMessage::NoWindowToRecord => {
                self.recording = None;
                self.toasts
                    .error("Couldn't record the scene: there was no window to take a picture of");
                Task::none()
            }
        }
    }

    /// Looks whether the scene being shown is ready, and then takes its
    /// picture or says it is ready; otherwise looks again a moment later.
    fn check_scene(&mut self) -> Task<Message> {
        let Some(run) = &mut self.scene_run else {
            return Task::none();
        };
        if run.done {
            return Task::none();
        }

        if run.library_folder.is_some() {
            if self.library.is_busy() {
                return check_after(NEXT_CHECK);
            }
            if !run.played_recorded {
                run.played_recorded = true;
                let played = run
                    .scene
                    .library
                    .as_ref()
                    .map(|library| library.played.clone())
                    .unwrap_or_default();
                let folder = run.library_folder.clone().unwrap_or_default();
                for (order, game) in (0..).zip(&played) {
                    if let Err(error) = record_played(&mut self.library, &folder, game, order) {
                        fail(&error);
                    }
                }
                return check_after(NEXT_CHECK);
            }
        }

        run.done = true;
        if run.output.is_some() {
            window::latest().then(|id| match id {
                Some(id) => window::screenshot(id).map(|shot| SceneMessage::Pictured(shot).into()),
                None => fail("the window didn't open"),
            })
        } else {
            println!("ready");
            let _ = std::io::stdout().flush();
            Task::none()
        }
    }

    /// Saves the moment on screen as a scene in the `--record-scenes`
    /// folder: the game pauses until the picture of the window is taken,
    /// its state is kept, and the scene's frames run from it, so the
    /// picture is exactly what the `capture` command shows of the scene.
    pub(super) fn record_scene(&mut self) -> Task<Message> {
        let Some(scenes_folder) = self.scene_recorder.clone() else {
            return Task::none();
        };
        if self.recording.is_some() {
            return Task::none();
        }
        if self.dialog.is_some() {
            self.toasts
                .error("Answer the question first: scenes don't keep questions");
            return Task::none();
        }
        if let Some(reason) = unkept(&self.on_screen()) {
            self.toasts
                .error(format!("Couldn't record the scene: {reason}"));
            return Task::none();
        }

        let (scene, state) = match self.scene_now() {
            Ok(scene) => scene,
            Err(error) => {
                self.toasts
                    .error(format!("Couldn't record the scene: {error}"));
                return Task::none();
            }
        };
        let name = self
            .game
            .as_ref()
            .map_or_else(|| String::from("library"), |game| game.session.name());
        self.recording = Some(PendingRecording {
            folder: free_folder(&scenes_folder, &name),
            scene,
            state,
        });
        // A capture shows the library, the settings and the pause menu
        // scrolled to the top, as they open. A picture of the window shows
        // what was last drawn, so it waits until the window has been drawn
        // scrolled, without the toasts and with the scene's frames.
        let to_top = |id| {
            iced::widget::operation::snap_to(
                id,
                iced::widget::scrollable::RelativeOffset {
                    x: Some(0.0),
                    y: Some(0.0),
                },
            )
        };
        let scrolled = Task::batch([
            to_top(crate::views::library::scroll_id()),
            to_top(crate::views::settings::scroll_id()),
            to_top(crate::views::game::pause_scroll_id()),
        ]);
        let wait = Task::perform(background::run(|| std::thread::sleep(REDRAW_TIME)), |_| ());
        scrolled
            .chain(wait)
            .then(|()| window::latest())
            .then(|id| match id {
                Some(id) => window::screenshot(id).map(|shot| SceneMessage::Recorded(shot).into()),
                None => Task::done(SceneMessage::NoWindowToRecord.into()),
            })
    }

    /// What is on screen that a scene might not keep.
    fn on_screen(&self) -> OnScreen {
        let mut on_screen = match &self.game {
            Some(game) => {
                let cheats = game.session.cheats();
                OnScreen {
                    box_art: false,
                    cheats_on: cheats.iter().any(|cheat| cheat.enabled),
                    cheats_listed: !cheats.is_empty(),
                    page: game.menu(),
                    saved_slots: game
                        .session
                        .slots()
                        .iter()
                        .any(|slot| slot.saved_at.is_some()),
                    ..OnScreen::default()
                }
            }
            None => OnScreen {
                box_art: self.settings.shared.download_box_art && self.library.has_box_art(),
                library_busy: self.library.is_busy(),
                searching: !self.search.is_empty(),
                last_screens: self
                    .library
                    .games("")
                    .chain(
                        self.library
                            .recently_played(crate::views::library::most_recent_shown()),
                    )
                    .any(|entry| {
                        self.library.title_screen(&entry.path).is_none()
                            && self.library.thumbnail(&entry.path).is_some()
                    }),
                ..OnScreen::default()
            },
        };
        on_screen.library_folders = self.settings_tab == Some(SettingsTab::Library)
            && !self.settings.shared.library_folders.is_empty();
        on_screen
    }

    /// The scene on screen right now, with the game's state if a game is
    /// open. The game is paused first, and its frames run.
    fn scene_now(&mut self) -> Result<(Scene, Option<Vec<u8>>), String> {
        let mut settings =
            toml::Table::try_from(&self.settings).map_err(|error| error.to_string())?;
        settings.remove("library_folders");
        settings.remove("window_size");
        // The light or dark look on screen, rather than "follow the system",
        // which another system would show otherwise.
        let dark = match self.settings.appearance {
            Appearance::System => self.system_mode != Mode::Light,
            Appearance::Light => false,
            Appearance::Dark => true,
        };
        settings.insert(
            String::from("appearance"),
            (if dark { "dark" } else { "light" }).into(),
        );

        let window = (!self.fullscreen).then_some(WindowSize {
            width: self.window_width,
            height: self.window_height,
        });
        let mut view = View {
            settings_tab: self
                .settings_tab
                .map(|tab| settings_tab_name(tab).to_owned()),
            fullscreen: self.fullscreen,
            ..View::default()
        };

        let Some(game) = &mut self.game else {
            view.screen = scenes::Screen::Library;
            let now = SystemTime::now();
            let mut library = SceneLibrary {
                games: self
                    .library
                    .games("")
                    .filter_map(|entry| file_name(&entry.path))
                    .collect(),
                played: self
                    .library
                    .recently_played(crate::views::library::most_recent_shown())
                    .into_iter()
                    .filter_map(|entry| {
                        let played = SystemTime::UNIX_EPOCH
                            .checked_add(Duration::from_secs(entry.last_played?))?;
                        Some(PlayedGame {
                            file: file_name(&entry.path)?,
                            minutes_ago: now
                                .duration_since(played)
                                .map_or(0, |ago| ago.as_secs() / 60),
                        })
                    })
                    .collect(),
            };
            // A game played from outside the library's folders shows under
            // "Continue playing" too, so its file goes into the scene's
            // library with the others.
            for played in &library.played {
                if !library.games.contains(&played.file) {
                    library.games.push(played.file.clone());
                }
            }
            let scene = Scene {
                game: None,
                window,
                view,
                library: Some(library),
                settings,
            };
            return Ok((scene, None));
        };

        view.pause_menu = game.menu().map(|page| pause_menu_name(page).to_owned());
        game.session.apply(false, &self.settings.shared);
        // Save states don't keep the buttons held, and a capture starts with
        // none, so the scene's frames run without any too.
        game.session.release_held_keys();
        game.session.set_controller_states(Vec::new());
        let state = game.session.snapshot()?;
        game.session.run_frames(scenes::RECORDED_FRAMES)?;
        let rom = game.session.rom();
        let scene = Scene {
            game: Some(SceneGame {
                file: file_name(&rom.path).ok_or("the game has no file name")?,
                sha256: scenes::sha256_hex(&rom.bytes),
                model: game.session.model().name().to_owned(),
                frames: scenes::RECORDED_FRAMES,
            }),
            window,
            view,
            library: None,
            settings,
        };
        Ok((scene, Some(state.data)))
    }

    /// Writes the scene being recorded, with the picture of the window as
    /// its preview, and lets the game go on.
    fn finish_recording(&mut self, screenshot: &window::Screenshot) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let written = encode_png(
            screenshot.size.width,
            screenshot.size.height,
            &screenshot.rgba,
        )
        .and_then(|preview| {
            recording.scene.write(
                &recording.folder,
                recording.state.as_deref(),
                Some(&preview),
            )
        });
        match written {
            Ok(()) => self
                .toasts
                .info(format!("Scene saved in {}", recording.folder.display())),
            Err(error) => self
                .toasts
                .error(format!("Couldn't save the scene: {error}")),
        }
    }

    /// Whether the game must stay paused for a scene: one being shown, or
    /// one being recorded until the picture of the window is taken.
    pub(super) fn held_for_scene(&self) -> bool {
        self.scene_run.is_some() || self.recording.is_some()
    }
}

/// What is on screen that a scene doesn't keep, because it is the
/// player's own data rather than the game's state or the settings.
#[derive(Debug, Default)]
struct OnScreen {
    /// Whether the library shows box art, which captures never download.
    box_art: bool,
    /// Whether the library is still finding its games or their title
    /// screens, which a capture waits for.
    library_busy: bool,
    /// Whether the library's search field holds text, which scenes don't
    /// keep.
    searching: bool,
    /// Whether a library card shows the last screen played instead of a
    /// title screen, which scenes don't keep.
    last_screens: bool,
    /// Whether the settings' Library tab shows the player's game folders,
    /// which scenes don't keep.
    library_folders: bool,
    /// Whether a cheat is on, which save states don't keep.
    cheats_on: bool,
    /// Whether the game has cheats for the cheats page to list.
    cheats_listed: bool,
    /// The page of the pause menu that is open.
    page: Option<PauseMenu>,
    /// Whether the save state pages list saved slots.
    saved_slots: bool,
}

/// Why the moment `on_screen` can't be recorded as a scene, if it can't:
/// a scene shows again only what it keeps (its state, what is open and the
/// settings), so it can't show the player's box art, cheats or save states.
fn unkept(on_screen: &OnScreen) -> Option<&'static str> {
    if on_screen.box_art {
        Some("scenes don't keep box art; turn it off in Settings › Library first")
    } else if on_screen.library_busy {
        Some("the library is still finding its games; wait until it is done")
    } else if on_screen.searching {
        Some("scenes don't keep the search; clear it first")
    } else if on_screen.last_screens {
        Some("a game shows the last screen you played, which scenes don't keep")
    } else if on_screen.library_folders {
        Some("scenes don't keep your game folders; close the Library settings first")
    } else if on_screen.cheats_on {
        Some("scenes don't keep cheats; turn them off first")
    } else if matches!(on_screen.page, Some(PauseMenu::Save | PauseMenu::Load))
        && on_screen.saved_slots
    {
        Some("scenes don't keep save states, so this page would show empty slots")
    } else if on_screen.page == Some(PauseMenu::Cheats) && on_screen.cheats_listed {
        Some("scenes don't keep cheats, so this page would list none")
    } else {
        None
    }
}

/// Notes that a game of a scene's library was played as long ago as the
/// scene says. It is the `order`th game the scene lists, newest first, and
/// that many seconds older still, so games played in the same minute keep
/// the order the scene recorded them in, without showing another "time ago".
fn record_played(
    library: &mut nubo_frontend::library::Library,
    folder: &Path,
    game: &PlayedGame,
    order: u64,
) -> Result<(), String> {
    let path = folder.join(&game.file);
    let rom = Rom::read(&path)?;
    let header = Emulator::read_rom_header(&rom.bytes).map_err(|error| error.to_string())?;
    let time = game
        .minutes_ago
        .checked_mul(60)
        .and_then(|seconds| seconds.checked_add(order))
        .and_then(|seconds| SystemTime::now().checked_sub(Duration::from_secs(seconds)))
        .ok_or_else(|| format!("{} was played too long ago", game.file))?;
    library.record_played_at(&path, &header, time)
}

/// The file name of `path`, as scenes name games.
fn file_name(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_string_lossy().into_owned())
}

/// A folder in `parent` named after `name` that doesn't exist yet, such as
/// `tobu-tobu-girl-deluxe-2`.
fn free_folder(parent: &Path, name: &str) -> PathBuf {
    let base: String = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let base = base.trim_matches('-');
    (1..)
        .map(|number| parent.join(format!("{base}-{number}")))
        .find(|folder| !folder.exists())
        .expect("some number is free")
}

/// The name `scene.toml` gives a page of the pause menu.
fn pause_menu_name(page: PauseMenu) -> &'static str {
    match page {
        PauseMenu::Main => "main",
        PauseMenu::Save => "save",
        PauseMenu::Load => "load",
        PauseMenu::Cheats => "cheats",
        PauseMenu::SuperGameBoy => "super-game-boy",
    }
}

/// The page of the pause menu `scene.toml` names.
fn pause_menu_page(name: &str) -> Result<PauseMenu, String> {
    [
        PauseMenu::Main,
        PauseMenu::Save,
        PauseMenu::Load,
        PauseMenu::Cheats,
        PauseMenu::SuperGameBoy,
    ]
    .into_iter()
    .find(|&page| pause_menu_name(page) == name)
    .ok_or_else(|| format!("the pause menu has no page '{name}'"))
}

/// The name `scene.toml` gives a settings tab.
fn settings_tab_name(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::Display => "display",
        SettingsTab::Playing => "playing",
        SettingsTab::Controls => "controls",
        SettingsTab::Library => "library",
        SettingsTab::About => "about",
    }
}

/// The settings tab `scene.toml` names.
fn settings_tab(name: &str) -> Result<SettingsTab, String> {
    SettingsTab::all()
        .iter()
        .copied()
        .find(|&tab| settings_tab_name(tab) == name)
        .ok_or_else(|| format!("the settings have no tab '{name}'"))
}

#[cfg(test)]
mod tests;
