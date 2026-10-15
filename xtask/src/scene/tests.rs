//! Tests for the input scripts of `cargo xtask scene explore`, and for its
//! contact sheet.

use super::script::{self, Step};
use super::*;
use image::RgbaImage;

#[test]
fn a_script_becomes_its_steps() {
    let steps = script::parse(
        "wait 2s; press start  # past the title\n\
         hold right\n\
         press a+b 10f; mark\n\
         release right; release",
    )
    .unwrap();
    assert_eq!(
        steps,
        [
            Step::Wait(119),
            Step::Press(vec![Key::Start]),
            Step::Wait(6),
            Step::Release(vec![Key::Start]),
            Step::Press(vec![Key::Right]),
            Step::Press(vec![Key::A, Key::B]),
            Step::Wait(10),
            Step::Release(vec![Key::A, Key::B]),
            Step::Mark,
            Step::Release(vec![Key::Right]),
            Step::Release(Vec::new()),
        ]
    );
    assert_eq!(script::parse("").unwrap(), []);
}

#[test]
fn times_are_counted_in_the_game_boys_frames() {
    // The Game Boy shows about 59.73 frames a second.
    assert_eq!(script::parse_frames("1s"), Ok(60));
    assert_eq!(script::parse_frames("10s"), Ok(597));
    assert_eq!(script::parse_frames("500ms"), Ok(30));
    assert_eq!(script::parse_frames("0.5s"), Ok(30));
    assert_eq!(script::parse_frames("42f"), Ok(42));
    for mistake in ["", "5", "s", "-1s", "2h", "1.5f"] {
        assert!(script::parse_frames(mistake).is_err(), "{mistake}");
    }
    assert!((script::seconds(597) - 10.0).abs() < 0.01);
}

#[test]
fn mistakes_in_a_script_are_named() {
    for mistake in ["jump", "press", "press x", "wait", "hold a 2s", "mark now"] {
        let error = script::parse(mistake).unwrap_err();
        assert!(!error.is_empty(), "{mistake}");
    }
}

#[test]
fn the_contact_sheet_holds_every_candidate_in_rows_of_four() {
    let picture = RgbaImage::from_pixel(160, 144, image::Rgba([1, 2, 3, 255]));
    let candidates: Vec<(u32, RgbaImage)> =
        (1..=5).map(|number| (number, picture.clone())).collect();
    let sheet = sheet::draw(&candidates);
    let (cell_width, cell_height) = (2 * 160, 2 * 144 + 23);
    assert_eq!(
        sheet.dimensions(),
        (12 + 4 * (cell_width + 12), 12 + 2 * (cell_height + 12))
    );
    assert_eq!(*sheet.get_pixel(12, 12 + 23), image::Rgba([1, 2, 3, 255]));
}

#[test]
fn candidates_go_into_a_new_folder_named_after_the_game() {
    let parent = std::env::temp_dir().join(format!("nubo-explore-{}", std::process::id()));
    let first = free_folder(&parent, "Moki the Monster Kid.gb");
    assert_eq!(first, parent.join("moki-the-monster-kid-1"));
    fs::create_dir_all(&first).unwrap();
    assert_eq!(
        free_folder(&parent, "Moki the Monster Kid.gb"),
        parent.join("moki-the-monster-kid-2")
    );
    fs::remove_dir_all(&parent).unwrap();
}
