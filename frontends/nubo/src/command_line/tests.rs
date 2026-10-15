//! Tests for reading the command line.

use super::*;
use clap::error::ErrorKind;

/// Reads `args` as the command line, after the program's name.
fn parse_args(args: &[&str]) -> Result<Request, clap::Error> {
    parse(std::iter::once("nubo").chain(args.iter().copied()))
}

/// Reads `args` as the command line of a normal start of the app.
fn options(args: &[&str]) -> Options {
    match parse_args(args) {
        Ok(Request::Play(options)) => options,
        other => panic!("{args:?} gave {other:?}"),
    }
}

/// The kind of mistake clap finds in `args`.
fn mistake(args: &[&str]) -> ErrorKind {
    match parse_args(args) {
        Err(error) => error.kind(),
        Ok(request) => panic!("{args:?} gave {request:?}"),
    }
}

#[test]
fn no_arguments_start_the_library() {
    assert_eq!(options(&[]), Options::default());
}

#[test]
fn a_relative_rom_path_is_made_absolute() {
    let path = options(&["roms/game.gb"]).rom_path.unwrap();
    assert!(path.is_absolute());
    assert!(path.ends_with("roms/game.gb"));
}

#[test]
fn a_rom_and_a_model() {
    let options_with_model = options(&["game.gb", "--model", "cgb-c"]);
    assert_eq!(
        options_with_model.rom_path,
        Some(std::path::absolute("game.gb").unwrap())
    );
    assert_eq!(options_with_model.model, Some(Model::CgbC));

    assert_eq!(options(&["--cgb", "game.gb"]).model, Some(Model::CgbE));
}

#[test]
fn the_shootouts_options() {
    assert_eq!(
        options(&[
            "game.gb", "--model", "dmg", "--scale", "1", "--fresh", "--speed", "4",
        ]),
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
        let options = options(&["game.gb", "--scale", &scale.to_string()]);
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
        let options = options(&["--speed", &speed.multiplier().to_string()]);
        assert_eq!(options.speed, Some(speed));
    }
    for wrong in ["1", "5", "fast"] {
        assert_eq!(mistake(&["--speed", wrong]), ErrorKind::ValueValidation);
    }
}

#[test]
fn options_without_their_value() {
    for option in ["--model", "--scale", "--speed"] {
        assert_eq!(
            mistake(&["game.gb", option]),
            ErrorKind::InvalidValue,
            "{option}"
        );
    }
}

#[test]
fn the_model_is_given_once() {
    assert_eq!(
        mistake(&["--cgb", "--model", "dmg"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(
        mistake(&["--model", "dmg", "--cgb"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(
        mistake(&["--model", "dmg", "--model", "sgb"]),
        ErrorKind::ArgumentConflict
    );
}

#[test]
fn an_unknown_model_or_option() {
    let error = parse_args(&["--model", "virtual boy"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
    assert!(
        error.to_string().contains("unknown model 'virtual boy'"),
        "{error}"
    );
    assert_eq!(mistake(&["--fast"]), ErrorKind::UnknownArgument);
    assert_eq!(mistake(&["one.gb", "two.gb"]), ErrorKind::UnknownArgument);
}

#[test]
fn the_screen_alone_needs_a_rom() {
    assert_eq!(
        mistake(&["--scale", "2"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        options(&["--fresh", "--speed", "4"]).speed,
        Some(FastForwardSpeed::try_from(4).unwrap())
    );
}

#[test]
fn help_and_version() {
    assert_eq!(mistake(&["--help"]), ErrorKind::DisplayHelp);
    assert_eq!(mistake(&["--version"]), ErrorKind::DisplayVersion);
    let help = parse_args(&["--help"]).unwrap_err().to_string();
    assert!(help.contains("--scale"), "{help}");
    assert!(!help.contains("--measure-efficiency"), "{help}");
}

#[test]
fn measuring_efficiency_needs_a_rom_and_runs_on_dmg() {
    assert_eq!(
        parse_args(&["--measure-efficiency", "game.gb"]).unwrap(),
        Request::MeasureEfficiency {
            rom_path: std::path::absolute("game.gb").unwrap(),
            model: Model::Dmg,
        }
    );
    assert_eq!(
        parse_args(&["--measure-efficiency", "game.gb", "--model", "cgb"]).unwrap(),
        Request::MeasureEfficiency {
            rom_path: std::path::absolute("game.gb").unwrap(),
            model: Model::CgbE,
        }
    );
    assert_eq!(
        mistake(&["--measure-efficiency"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        mistake(&["--measure-efficiency", "game.gb", "--fresh"]),
        ErrorKind::ArgumentConflict
    );
}
