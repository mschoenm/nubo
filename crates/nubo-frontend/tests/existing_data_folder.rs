//! A player's data folder from before the stored data moved to
//! `nubo-frontend` must keep working unchanged.
//!
//! `existing_data_folder/` was written by Nubo's app, through the same
//! functions it saves with, into the folders `ProjectDirs` picked (`config/`
//! and `data/` stand for `~/.config/nubo` and `~/.local/share/nubo` on
//! Linux). As of step 3 of `docs/RESTRUCTURE_PLAN.md` it wrote the settings,
//! the battery save and clock of `Pocket Game.gbc`, two of its save states
//! (one with its picture) and its cheats. As of step 4 it wrote the game
//! library: `library.toml` with five games (four found by a scan of the two
//! library folders, one played from elsewhere), their thumbnails, title
//! screens and box art. These tests read all of it with this version, and
//! check that saving the same things again writes the same bytes.
//!
//! Puzzle Time's title screen and thumbnail were redrawn in the evenly spaced
//! grays when the emulator's own grays changed to them. The maintainer chose
//! that over recognizing the old grays, since Nubo had no players yet whose
//! pictures would be in them.

#![cfg(feature = "desktop")]

use futures::executor::block_on;
use nubo_core::{CartridgeEffects, ControllerButton, DMG_SHADES, Emulator, Model, RtcState};
use nubo_frontend::battery::{BatteryCartridgeEffects, BatterySaves};
use nubo_frontend::cheats::{self, Cheat};
use nubo_frontend::controls::{Action, KeyCode, PadButton};
use nubo_frontend::library::{Cache, DownloadedArt, Library};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::notices::Notices;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::save_states::{self, SaveState, Slot};
use nubo_frontend::settings::{self, LibraryFolder, Settings};
use nubo_frontend::storage::files::Files;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the game was when its data was saved; the file name names its data.
/// The library has it as played from outside the library folders.
const ROM_PATH: &str = "/home/player/Games/Pocket Game.gbc";
/// A Game Boy Color game in the library with box art and a title screen.
const COLOR_QUEST: &str = "/home/player/Games/Game Boy/Color Quest.gbc";
/// An original Game Boy game in a subfolder, played, with a title screen and
/// a thumbnail but no box art.
const PUZZLE_TIME: &str = "/home/player/Games/Game Boy/Puzzle/Puzzle Time.gb";
/// A game whose title screen and box art weren't looked for yet.
const HOMEBREW: &str = "/home/player/Games/Game Boy/Homebrew.gb";
/// A game on the USB stick, with neither a title screen nor box art.
const HANDHELD_HERO: &str = "/media/player/USB stick/roms/Handheld Hero.gb";

/// The data folder in the tests.
fn existing() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/existing_data_folder")
}

/// The data folder as a storage, as the app finds it in the player's folders.
fn existing_files() -> Files {
    Files::in_folders(
        Some(existing().join("config")),
        Some(existing().join("data")),
    )
}

/// An empty folder in the temporary directory, to save to.
fn temporary_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("nubo-existing-data-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    folder
}

/// The battery-backed RAM that was saved.
fn saved_ram() -> Vec<u8> {
    (0..0x2000_u32).map(|index| (index * 7) as u8).collect()
}

/// The clock that was saved.
fn saved_clock() -> RtcState {
    RtcState {
        milliseconds: 999,
        seconds: 59,
        minutes: 30,
        hours: 23,
        days: 511,
        base_timestamp: 1_791_633_740_867.5,
        halted: true,
        day_carry: true,
    }
}

/// The screen saved with the quick save state.
fn saved_screen() -> Vec<u8> {
    (0..144_u32)
        .flat_map(|y| (0..160_u32).flat_map(move |x| [x as u8, y as u8, (x ^ y) as u8, 255]))
        .collect()
}

/// A frame of the Game Boy screen whose pixel at `(x, y)` is `pixel(x, y)`.
fn screen(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..144_u32)
        .flat_map(|y| (0..160_u32).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y))
        .collect()
}

/// The last screen played of Puzzle Time, in the emulator's grays.
fn puzzle_time_thumbnail() -> Vec<u8> {
    screen(|x, y| DMG_SHADES[((x / 8 + y / 8) % 4) as usize])
}

/// The shade of each pixel of Puzzle Time's title screen, 0 (white) to 3
/// (black).
fn puzzle_time_title_shade(x: u32) -> usize {
    ((x / 4) % 4) as usize
}

/// The title screen of Puzzle Time, in the emulator's grays.
fn puzzle_time_title_screen() -> Vec<u8> {
    screen(|x, _| DMG_SHADES[puzzle_time_title_shade(x)])
}

/// The title screen of Puzzle Time as the library shows it in `palette`.
fn puzzle_time_title_screen_in(palette: DmgPalette) -> Vec<u8> {
    let colors = palette.colors();
    screen(|x, _| {
        let [red, green, blue] = colors[puzzle_time_title_shade(x)];
        [red, green, blue, 0xFF]
    })
}

/// The title screen of Color Quest.
fn color_quest_title_screen() -> Vec<u8> {
    screen(|x, y| [(x * 2) as u8, y as u8, 128, 255])
}

/// The box art of Color Quest, 64 by 80 pixels.
fn color_quest_box_art() -> Vec<u8> {
    (0..80_u32)
        .flat_map(|y| (0..64_u32).flat_map(move |x| [(x * 4) as u8, (y * 3) as u8, 200, 255]))
        .collect()
}

/// The library as the app finds it in the player's data folder.
fn existing_library() -> Library {
    let (library, problem) = Library::load_from(Cache::in_folder(existing().join("data")));
    assert_eq!(problem, None);
    library
}

/// The cheats that were saved.
fn saved_cheats() -> Vec<Cheat> {
    let mut disabled = Cheat::new("", "00a17bc49").unwrap();
    disabled.enabled = false;
    vec![Cheat::new("Infinite health", "01ff56d0").unwrap(), disabled]
}

#[test]
fn the_settings_read_the_same() {
    let loaded = settings::load(&existing_files(), |contents| {
        toml::from_str::<Settings>(contents)
    });
    let settings = loaded.settings;

    assert_eq!(loaded.problem, None);
    assert_eq!(settings.model, ModelPreference::Fixed(Model::CgbC));
    assert_eq!(
        settings.library_folders[1],
        LibraryFolder {
            path: PathBuf::from("/media/player/USB stick/roms"),
            include_subfolders: false,
        }
    );
    assert!(!settings.download_box_art);
    assert!(settings.resume_games);
    assert_eq!(settings.fast_forward_speed.multiplier(), 3);
    assert_eq!(settings.volume, 0.65);
    assert!(settings.muted);
    assert_eq!(settings.dmg_palette, DmgPalette::Pocket);
    assert_eq!(
        settings.controls.keyboard.input(Action::Up),
        Some(KeyCode::KeyW)
    );
    assert_eq!(settings.controls.keyboard.input(Action::Screenshot), None);
    assert_eq!(
        settings.controls.controller.input(Action::PauseMenu),
        Some(PadButton(ControllerButton::RightStick))
    );
}

#[test]
fn the_battery_save_and_clock_load_the_same() {
    let saves = BatterySaves::in_storage(
        Arc::new(existing_files()),
        Path::new(ROM_PATH),
        Notices::default(),
    );
    let effects = BatteryCartridgeEffects::new(saves);

    assert_eq!(effects.load_ram(), Some(saved_ram()));
    let clock = effects.load_rtc_state().unwrap();
    assert_eq!(
        format!("{clock:?}"),
        format!("{:?}", saved_clock()),
        "the clock"
    );
}

#[test]
fn the_battery_save_and_clock_are_written_the_same() {
    let folder = temporary_folder("battery");
    let notices = Notices::default();
    let saves = BatterySaves::in_storage(
        Arc::new(Files::in_folders(None, Some(folder.clone()))),
        Path::new(ROM_PATH),
        notices.clone(),
    );
    let mut effects = BatteryCartridgeEffects::new(saves.clone());

    effects.save_ram(&saved_ram());
    effects.save_rtc_state(&saved_clock());
    saves.flush();

    assert!(notices.take().is_empty());
    for file in ["saves/Pocket Game.sav", "saves/Pocket Game.rtc"] {
        assert_eq!(
            fs::read(folder.join(file)).unwrap(),
            fs::read(existing().join("data").join(file)).unwrap(),
            "{file}"
        );
    }
}

#[test]
fn the_cheats_read_and_write_the_same() {
    let folder = temporary_folder("cheats");
    let files = Files::in_folders(None, Some(folder.clone()));
    let rom_path = Path::new(ROM_PATH);

    assert_eq!(
        cheats::load_from(&existing_files(), rom_path).unwrap(),
        saved_cheats()
    );

    cheats::save_to(&files, rom_path, &saved_cheats()).unwrap();
    let file = "cheats/Pocket Game.gbc.toml";
    assert_eq!(
        fs::read(folder.join(file)).unwrap(),
        fs::read(existing().join("data").join(file)).unwrap()
    );
}

#[test]
fn the_save_states_and_their_pictures_load_the_same() {
    let states = existing().join("data/states");
    let rom_path = Path::new(ROM_PATH);

    let quick = save_states::load_in(&states, rom_path, Slot::Quick)
        .unwrap()
        .unwrap();
    assert_eq!(quick.model, Model::Dmg);
    Emulator::new(Model::Dmg).load_state(&quick.data).unwrap();
    let thumbnail = save_states::thumbnail_in(&states, rom_path, Slot::Quick).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (160, 144));
    assert_eq!(thumbnail.pixels(), &saved_screen()[..]);
    assert!(
        save_states::slot_info_in(&states, rom_path, Slot::Quick)
            .saved_at
            .is_some()
    );

    let numbered = save_states::load_in(&states, rom_path, Slot::Numbered(2)).unwrap();
    assert_eq!(numbered.unwrap().data, quick.data);
    assert!(save_states::thumbnail_in(&states, rom_path, Slot::Numbered(2)).is_none());

    assert!(
        save_states::load_in(&states, rom_path, Slot::Numbered(1))
            .unwrap()
            .is_none()
    );
    assert!(
        save_states::slot_info_in(&states, rom_path, Slot::Resume)
            .saved_at
            .is_none()
    );
}

#[test]
fn a_save_state_and_its_picture_are_written_the_same() {
    let states = existing().join("data/states");
    let folder = temporary_folder("states");
    let rom_path = Path::new(ROM_PATH);
    let quick = save_states::load_in(&states, rom_path, Slot::Quick)
        .unwrap()
        .unwrap();

    let state = SaveState {
        model: quick.model,
        data: quick.data,
    };
    save_states::save_in(
        &folder,
        rom_path,
        Slot::Quick,
        &state,
        Some(&saved_screen()),
    )
    .unwrap();

    for file in ["Pocket Game.gbc/quick.state", "Pocket Game.gbc/quick.png"] {
        assert_eq!(
            fs::read(folder.join(file)).unwrap(),
            fs::read(states.join(file)).unwrap(),
            "{file}"
        );
    }
}

#[test]
fn the_library_reads_the_same_without_a_scan() {
    let mut library = existing_library();

    let games: Vec<_> = library
        .games("")
        .map(|game| (game.name(), game.title.as_str(), game.supports_cgb))
        .collect();
    assert_eq!(
        games,
        [
            (String::from("Color Quest"), "COLOR QUEST", true),
            (String::from("Handheld Hero"), "HANDHELD HERO", false),
            (String::from("Homebrew"), "HOMEBREW", false),
            (String::from("Puzzle Time"), "PUZZLE TIME", false),
        ]
    );

    let played: Vec<_> = library
        .recently_played(10)
        .into_iter()
        .map(|game| (game.path.as_path(), game.in_folder, game.last_played))
        .collect();
    assert_eq!(
        played,
        [
            (Path::new(PUZZLE_TIME), true, Some(1_791_934_251)),
            (Path::new(ROM_PATH), false, Some(1_791_934_250)),
        ]
    );

    // Only the game that was never looked at gets its title screen captured
    // and its box art looked up.
    assert_eq!(library.next_captures(), [PathBuf::from(HOMEBREW)]);
    assert_eq!(library.next_downloads(), [(PathBuf::from(HOMEBREW), false)]);
}

#[test]
fn the_library_pictures_read_the_same() {
    let mut library = existing_library();

    let pictures = block_on(library.load_pictures(DmgPalette::Pocket, true, false)).unwrap();
    library.set_pictures(pictures);

    let box_art = library.box_art(Path::new(COLOR_QUEST)).unwrap();
    assert_eq!((box_art.width(), box_art.height()), (64, 80));
    assert_eq!(box_art.pixels(), &color_quest_box_art()[..]);
    assert_eq!(
        library
            .title_screen(Path::new(PUZZLE_TIME))
            .unwrap()
            .pixels(),
        &puzzle_time_title_screen_in(DmgPalette::Pocket)[..]
    );
    assert_eq!(
        library.thumbnail(Path::new(ROM_PATH)).unwrap().pixels(),
        &saved_screen()[..]
    );
    for game in [HOMEBREW, HANDHELD_HERO] {
        let game = Path::new(game);
        assert!(library.box_art(game).is_none(), "{game:?}");
        assert!(library.title_screen(game).is_none(), "{game:?}");
        assert!(library.thumbnail(game).is_none(), "{game:?}");
    }

    let pictures = block_on(library.load_pictures(DmgPalette::Gray, false, false)).unwrap();
    library.set_pictures(pictures);

    assert!(library.box_art(Path::new(COLOR_QUEST)).is_none());
    assert_eq!(
        library
            .title_screen(Path::new(COLOR_QUEST))
            .unwrap()
            .pixels(),
        &color_quest_title_screen()[..]
    );
    assert_eq!(
        library
            .title_screen(Path::new(PUZZLE_TIME))
            .unwrap()
            .pixels(),
        &puzzle_time_title_screen()[..]
    );
}

#[test]
fn the_library_and_its_pictures_are_written_the_same() {
    let folder = temporary_folder("library");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(
        existing().join("data/library.toml"),
        folder.join("library.toml"),
    )
    .unwrap();
    let (mut library, problem) = Library::load_from(Cache::in_folder(folder.clone()));
    assert_eq!(problem, None);
    let box_art_png = fs::read(existing().join("data/box_art/Color Quest.gbc.png")).unwrap();

    library
        .finish_capture(
            Path::new(COLOR_QUEST),
            Some(color_quest_title_screen()),
            DmgPalette::Pocket,
        )
        .unwrap();
    library
        .finish_capture(
            Path::new(PUZZLE_TIME),
            Some(puzzle_time_title_screen()),
            DmgPalette::Pocket,
        )
        .unwrap();
    library
        .finish_download(
            Path::new(COLOR_QUEST),
            Ok(Some(DownloadedArt::decode(box_art_png).unwrap())),
        )
        .unwrap();
    library
        .set_thumbnail(Path::new(ROM_PATH), saved_screen())
        .unwrap();
    library
        .set_thumbnail(Path::new(PUZZLE_TIME), puzzle_time_thumbnail())
        .unwrap();

    for file in [
        "library.toml",
        "title_screens/Color Quest.gbc.png",
        "title_screens/Puzzle Time.gb.png",
        "box_art/Color Quest.gbc.png",
        "thumbnails/Pocket Game.gbc.png",
        "thumbnails/Puzzle Time.gb.png",
    ] {
        assert_eq!(
            fs::read(folder.join(file)).unwrap(),
            fs::read(existing().join("data").join(file)).unwrap(),
            "{file}"
        );
    }
}
