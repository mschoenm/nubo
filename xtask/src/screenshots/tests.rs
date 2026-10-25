//! Tests for the screenshot list, the settings each picture is taken with,
//! putting variants side by side, and the README's credits.

use super::lists::{Frame, GameList, Layout, ScreenshotList, Style};
use super::*;
use image::{Rgba, RgbaImage};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Reads `text` as `images/screenshots.toml`, from a folder of its own, so
/// tests running at the same time never share one.
fn list(text: &str) -> std::result::Result<ScreenshotList, String> {
    static LISTS: AtomicUsize = AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!(
        "nubo-screenshot-list-{}-{}",
        std::process::id(),
        LISTS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("screenshots.toml");
    fs::write(&path, text).unwrap();
    let list = ScreenshotList::read(&path);
    fs::remove_dir_all(&folder).unwrap();
    list
}

/// A scene with these settings, as `scene.toml` would say them.
fn scene_with(settings: &str) -> Scene {
    Scene::from_toml(&format!(
        "[game]\nfile = \"game.gb\"\nsha256 = \"00\"\nmodel = \"DMG\"\nframes = 2\n[settings]\n{settings}"
    ))
    .unwrap()
}

#[test]
fn a_screenshot_with_variants_takes_one_picture_each() {
    let list = list(
        r#"
        [[screenshot]]
        file = "images/screenshots/palettes.png"
        scene = "saloon"
        frame = "game"
        scale = 4
        settings = { original_screen = true }
        layout = "grid"
        columns = 2
        gap = 16
        [[screenshot.variant]]
        settings = { dmg_palette = "green" }
        [[screenshot.variant]]
        scene = "town"
        settings = { dmg_palette = "pocket", original_screen = false }
        pause_menu = "main"
        "#,
    )
    .unwrap();
    let screenshot = &list.screenshots[0];
    assert_eq!(screenshot.frame, Frame::Game);
    assert_eq!(screenshot.layout, Layout::Grid);

    let tiles = screenshot.tiles();
    assert_eq!(tiles.len(), 2);
    assert_eq!(tiles[0].scene, "saloon");
    assert_eq!(tiles[0].settings["dmg_palette"].as_str(), Some("green"));
    assert_eq!(tiles[0].settings["original_screen"].as_bool(), Some(true));
    assert_eq!(tiles[0].pause_menu, None);
    assert_eq!(tiles[1].scene, "town");
    assert_eq!(tiles[1].settings["original_screen"].as_bool(), Some(false));
    assert_eq!(tiles[1].pause_menu.as_deref(), Some("main"));
}

#[test]
fn options_that_dont_fit_the_frame_are_mistakes() {
    for text in [
        "file = \"a.png\"\nscene = \"s\"\nscale = 2",
        "file = \"a.png\"\nscene = \"s\"\nframe = \"game\"\nsize = [10, 10]",
        "file = \"a.png\"\nscene = \"s\"\nframe = \"game\"\nscale = 9",
        "file = \"a.png\"\nscene = \"s\"\nframe = \"fullscreen\"\nsize = [0, 0]",
        "file = \"a.png\"\nscene = \"s\"\nsize = [800, 0]",
        "file = \"a.png\"\nscene = \"s\"\ncolumns = 2",
        "file = \"a.png\"\nscene = \"s\"\nbackground = \"black\"",
        "file = \"a.jpg\"\nscene = \"s\"",
        "file = \"a.png\"\nscene = \"s\"\nframe = \"window\"",
        "file = \"a.png\"\nscene = \"s\"\nsetings = {}",
    ] {
        assert!(list(&format!("[[screenshot]]\n{text}")).is_err(), "{text}");
    }
}

#[test]
fn settings_become_changes_for_the_capture_command() {
    let screenshot = &list(
        r#"
        [[screenshot]]
        file = "a.png"
        scene = "s"
        settings = { appearance = "dark", volume = 0.5, controls = { keyboard = { a = "KeyK" } } }
        "#,
    )
    .unwrap()
    .screenshots[0];
    let (mut changes, dark) = screenshot.tiles()[0].changes(&scene_with(""));
    changes.sort();
    assert_eq!(
        changes,
        [
            "appearance=\"dark\"",
            "controls.keyboard.a=\"KeyK\"",
            "volume=0.5"
        ]
    );
    assert!(dark);
}

#[test]
fn an_app_that_follows_the_system_gets_gnomes_style() {
    let screenshot = |extra: &str| {
        list(&format!(
            "[[screenshot]]\nfile = \"a.png\"\nscene = \"s\"\n{extra}"
        ))
        .unwrap()
        .screenshots[0]
            .clone()
    };

    let (changes, dark) = screenshot("").tiles()[0].changes(&scene_with(""));
    assert_eq!(changes, ["appearance=\"light\""]);
    assert!(!dark);

    let (changes, dark) = screenshot("style = \"dark\"").tiles()[0].changes(&scene_with(""));
    assert_eq!(changes, ["appearance=\"dark\""]);
    assert!(dark);

    // The scene's own appearance decides the style, unless the screenshot
    // says otherwise.
    let (changes, dark) = screenshot("").tiles()[0].changes(&scene_with("appearance = \"dark\""));
    assert!(changes.is_empty());
    assert!(dark);
    assert_eq!(screenshot("style = \"light\"").style, Some(Style::Light));
    let (_, dark) =
        screenshot("style = \"light\"").tiles()[0].changes(&scene_with("appearance = \"dark\""));
    assert!(!dark);
}

#[test]
fn variants_are_laid_out_in_cells_as_big_as_the_biggest() {
    let folder = std::env::temp_dir().join(format!("nubo-compose-{}", std::process::id()));
    let red = Rgba([255, 0, 0, 255]);
    let tiles: Vec<PathBuf> = [(10, 20), (30, 10), (20, 20)]
        .into_iter()
        .enumerate()
        .map(|(index, (width, height))| {
            let path = folder.join(format!("{index}.png"));
            pictures::write(&path, &RgbaImage::from_pixel(width, height, red)).unwrap();
            path
        })
        .collect();
    let screenshot = |layout: &str| {
        list(&format!(
            "[[screenshot]]\nfile = \"a.png\"\nscene = \"s\"\n{layout}\ngap = 2\npadding = 1\n\
             background = \"#0000ff\"\n[[screenshot.variant]]\n[[screenshot.variant]]\n[[screenshot.variant]]"
        ))
        .unwrap()
        .screenshots[0]
            .clone()
    };

    let row = pictures::compose(&screenshot("layout = \"row\""), &tiles).unwrap();
    assert_eq!(row.dimensions(), (1 + 3 * 30 + 2 * 2 + 1, 1 + 20 + 1));
    assert_eq!(*row.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
    // The first picture is centered in its 30 pixel wide cell.
    assert_eq!(*row.get_pixel(1 + 10, 1), red);
    assert_eq!(*row.get_pixel(1 + 9, 1), Rgba([0, 0, 255, 255]));

    let grid = pictures::compose(&screenshot("layout = \"grid\"\ncolumns = 2"), &tiles).unwrap();
    assert_eq!(grid.dimensions(), (1 + 2 * 30 + 2 + 1, 1 + 2 * 20 + 2 + 1));
    let column = pictures::compose(&screenshot("layout = \"column\""), &tiles).unwrap();
    assert_eq!(column.dimensions(), (1 + 30 + 1, 1 + 3 * 20 + 2 * 2 + 1));
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn differences_are_painted_red() {
    let old = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
    let mut new = old.clone();
    new.put_pixel(1, 0, Rgba([10, 10, 10, 255]));
    let difference = pictures::difference(&old, &new);
    assert_eq!(*difference.get_pixel(0, 0), Rgba([128, 128, 128, 255]));
    assert_eq!(*difference.get_pixel(1, 0), Rgba([255, 0, 0, 255]));

    // A part cropped away is red too.
    let cropped = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    let difference = pictures::difference(&old, &cropped);
    assert_eq!(difference.dimensions(), (2, 1));
    assert_eq!(*difference.get_pixel(0, 0), Rgba([128, 128, 128, 255]));
    assert_eq!(*difference.get_pixel(1, 0), Rgba([255, 0, 0, 255]));
}

#[test]
fn every_game_in_the_published_screenshots_is_credited() {
    let root = workspace_root();
    let list = ScreenshotList::read(&root.join(SCREENSHOTS_FILE)).unwrap();
    let games = GameList::read(&root.join(GAMES_FILE)).unwrap();
    assert!(!list.screenshots.is_empty() && !games.games.is_empty());
    check_credits(&root, &list, &games).unwrap();
}

#[test]
fn a_list_that_cant_be_read_is_an_error_but_a_missing_one_is_empty() {
    let folder = std::env::temp_dir();
    assert!(ScreenshotList::read(&folder).is_err());
    assert!(GameList::read(&folder).is_err());
    let missing = folder.join("nubo-no-such-list.toml");
    assert!(
        ScreenshotList::read(&missing)
            .unwrap()
            .screenshots
            .is_empty()
    );
    assert!(GameList::read(&missing).unwrap().games.is_empty());
}

#[test]
fn the_games_list_names_each_file_once() {
    let games = GameList::read(&workspace_root().join(GAMES_FILE)).unwrap();
    assert!(!games.games.is_empty());
    let mut files: Vec<&str> = games.games.iter().map(|game| game.file.as_str()).collect();
    files.sort_unstable();
    let count = files.len();
    files.dedup();
    assert_eq!(files.len(), count);
    for game in &games.games {
        assert_eq!(game.sha256.len(), 64, "{}", game.file);
        assert!(game.url.starts_with("https://"), "{}", game.file);
    }
}

#[test]
fn a_game_missing_from_the_credits_is_named() {
    let root = std::env::temp_dir().join(format!("nubo-credits-{}", std::process::id()));
    fs::create_dir_all(root.join(SCENES_FOLDER).join("s")).unwrap();
    fs::write(
        root.join("README.md"),
        "Credits: Big2Small by Matthew D. Steele",
    )
    .unwrap();
    fs::write(
        root.join(SCENES_FOLDER).join("s").join("scene.toml"),
        scene_with("").to_toml(),
    )
    .unwrap();
    let games: GameList = toml::from_str(
        r#"
        [[game]]
        file = "game.gb"
        title = "Unknown Game"
        authors = "Someone"
        url = "https://example.com/game.gb"
        sha256 = "00"
        license = "MIT"
        "#,
    )
    .unwrap();
    let list = list("[[screenshot]]\nfile = \"a.png\"\nscene = \"s\"").unwrap();

    let error = check_credits(&root, &list, &games).unwrap_err();
    assert!(error.contains("Unknown Game by Someone (MIT)"), "{error}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_screen_fits_the_games_picture_at_every_scale() {
    assert_eq!(game_monitor(4), (1920, 1200));
    for scale in 1..=8 {
        let (width, height) = game_monitor(scale);
        assert!(width >= SGB_FRAME_WIDTH * scale && height >= SGB_FRAME_HEIGHT * scale);
    }
}
