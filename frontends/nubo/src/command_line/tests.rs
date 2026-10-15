//! Tests for reading the command line.

use super::*;

/// Reads `args` as the command line.
fn parse_args(args: &[&str]) -> Result<Options, String> {
    parse(args.iter().map(|&arg| arg.to_owned()))
}

#[test]
fn no_arguments_start_the_library() {
    assert_eq!(parse_args(&[]), Ok(Options::default()));
}

#[test]
fn a_relative_rom_path_is_made_absolute() {
    let options = parse_args(&["roms/game.gb"]).unwrap();
    let path = options.rom_path.unwrap();
    assert!(path.is_absolute());
    assert!(path.ends_with("roms/game.gb"));
}

#[test]
fn a_rom_and_a_model() {
    let options = parse_args(&["game.gb", "--model", "cgb-c"]).unwrap();
    assert_eq!(
        options.rom_path,
        Some(std::path::absolute("game.gb").unwrap())
    );
    assert_eq!(options.model, Some(Model::CgbC));

    let options = parse_args(&["--cgb", "game.gb"]).unwrap();
    assert_eq!(options.model, Some(Model::CgbE));
}

#[test]
fn the_shootouts_options() {
    let options = parse_args(&[
        "game.gb", "--model", "dmg", "--scale", "1", "--fresh", "--speed", "4",
    ])
    .unwrap();
    assert_eq!(
        options,
        Options {
            rom_path: Some(std::path::absolute("game.gb").unwrap()),
            model: Some(Model::Dmg),
            scale: Some(1),
            fresh: true,
            speed: Some(FastForwardSpeed::try_from(4).unwrap()),
        }
    );
}

#[test]
fn every_scale_from_1_to_8() {
    for scale in 1..=8 {
        let options = parse_args(&["game.gb", "--scale", &scale.to_string()]).unwrap();
        assert_eq!(options.scale, Some(scale));
    }
    for wrong in ["0", "9", "-1", "2.5", "big"] {
        assert!(
            parse_args(&["game.gb", "--scale", wrong]).is_err(),
            "{wrong}"
        );
    }
}

#[test]
fn only_the_fast_forward_speeds() {
    for speed in FastForwardSpeed::ALL {
        let options = parse_args(&["--speed", &speed.multiplier().to_string()]).unwrap();
        assert_eq!(options.speed, Some(speed));
    }
    for wrong in ["1", "5", "fast"] {
        assert!(parse_args(&["--speed", wrong]).is_err(), "{wrong}");
    }
}

#[test]
fn options_without_their_value() {
    for option in ["--model", "--scale", "--speed"] {
        assert_eq!(
            parse_args(&["game.gb", option]),
            Err(format!("{option} needs a value"))
        );
    }
}

#[test]
fn the_model_is_given_once() {
    let error = Err(String::from(
        "give the model once, with --cgb or --model, not both",
    ));
    assert_eq!(parse_args(&["--cgb", "--model", "dmg"]), error);
    assert_eq!(parse_args(&["--model", "dmg", "--cgb"]), error);
    assert_eq!(parse_args(&["--model", "dmg", "--model", "sgb"]), error);
}

#[test]
fn an_unknown_model_or_option() {
    assert_eq!(
        parse_args(&["--model", "virtual boy"]),
        Err(String::from("unknown model 'virtual boy'"))
    );
    assert_eq!(
        parse_args(&["--fast"]),
        Err(String::from("--fast isn't an option"))
    );
}

#[test]
fn the_screen_alone_needs_a_rom() {
    assert!(parse_args(&["--scale", "2"]).is_err());
    assert!(parse_args(&["--fresh", "--speed", "4"]).is_ok());
}
