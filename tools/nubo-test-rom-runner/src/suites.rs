//! Finds the test ROMs in the downloaded collection and decides which
//! hardware models each one runs on and how its result is checked. The GB
//! Emulator Shootout's tests are listed in `shootout.rs`.

use crate::download::RomDirs;
use crate::{gambatte, shootout};
use nubo_core::Key;
pub(crate) use nubo_core::Model;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// One audio buffer is 1/128 of a second at the runner's sample rate
/// (`boot::SAMPLE_RATE`), and tests count emulated time in audio buffers.
pub(crate) const AUDIO_BUFFERS_PER_SECOND: u32 = 128;
/// T-cycles (ticks of the 4 MHz clock) in one frame of the LCD.
const T_CYCLES_PER_FRAME: u32 = 70224;
const T_CYCLES_PER_AUDIO_BUFFER: u32 = 4_194_304 / AUDIO_BUFFERS_PER_SECOND;

/// The model's name as used in test IDs and `passing.txt`, such as "dmg",
/// "cgb" (for CGB-E) or "cgb-c".
pub(crate) fn model_label(model: Model) -> String {
    match model {
        Model::Dmg => "dmg".to_string(),
        Model::CgbE => "cgb".to_string(),
        _ => model.name().to_ascii_lowercase(),
    }
}

/// Where a test ROM comes from.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// The c-sp/game-boy-test-roms collection.
    Collection,
    /// The GB Emulator Shootout (gbdev/GBEmulatorShootout): the ROMs of its
    /// tests that the collection lacks or has in another version, and all its
    /// reference screenshots.
    Shootout,
}

/// How a test reports its result.
pub(crate) enum Check {
    /// Through the CPU registers, like the Mooneye tests.
    Registers,
    /// By drawing a screen that must match this reference screenshot.
    Screen(PathBuf),
    /// By showing this reference screenshot in every frame until the
    /// timeout, for tests that fail by flashing something for a single frame.
    EveryFrame(PathBuf),
    /// By showing this hexadecimal number in the top-left corner, checked a
    /// fixed time after the boot ROM like Gambatte's tests.
    Number(String),
    /// By playing sound (`true`) or staying silent, checked a fixed time
    /// after the boot ROM like Gambatte's audio tests.
    Sound(bool),
    /// By matching a Gambatte reference screenshot, checked a fixed time
    /// after the boot ROM.
    GambatteScreen(PathBuf),
    /// By writing 0x01 (pass) or 0xFF (fail) to 0xFF82 in high RAM, like
    /// GBMicrotest.
    HighRam,
    /// By matching one of these screenshots a fixed time after the boot ROM,
    /// compared in grayscale like the GB Emulator Shootout does, for
    /// screenshots in other colors than Nubo's, such as the shootout's (which
    /// come from several emulators) and Mooneye's own.
    GrayscaleScreen(Vec<PathBuf>),
    /// The way the GB Emulator Shootout judges its tests: passed once the
    /// screen has matched a `pass` screenshot for as long as the Shootout's
    /// harness could take to see it, failed once it has matched a `fail`
    /// screenshot that long, compared in grayscale (see `shootout.rs`). The
    /// paths are in the Shootout's folder, whichever source the ROM is from.
    Shootout {
        pass: Vec<PathBuf>,
        fail: Vec<PathBuf>,
    },
}

/// How much emulated time a test gets before it fails, and when that time
/// starts counting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Deadline {
    /// Seconds from power-on, so the boot ROM's 2 to 3 seconds count too.
    FromPowerOn(u32),
    /// Seconds after the boot ROM hands over to the cartridge.
    AfterBoot(u32),
    /// Frames after the boot ROM hands over to the cartridge, like Gambatte's
    /// own test runner counts.
    FramesAfterBoot(u32),
    /// Milliseconds from power-on, like the GB Emulator Shootout's
    /// `runtime`. Rounded up to whole audio buffers.
    MillisecondsFromPowerOn(u32),
}

impl Deadline {
    /// How many audio buffers the test may run after the boot ROM, which
    /// took `boot_audio_buffers`. Frames are rounded up to whole buffers.
    pub(crate) fn audio_buffers_after_boot(self, boot_audio_buffers: u32) -> u32 {
        match self {
            Self::FromPowerOn(seconds) => {
                (seconds * AUDIO_BUFFERS_PER_SECOND).saturating_sub(boot_audio_buffers)
            }
            Self::AfterBoot(seconds) => seconds * AUDIO_BUFFERS_PER_SECOND,
            Self::FramesAfterBoot(frames) => {
                (frames * T_CYCLES_PER_FRAME).div_ceil(T_CYCLES_PER_AUDIO_BUFFER)
            }
            Self::MillisecondsFromPowerOn(milliseconds) => (milliseconds
                * AUDIO_BUFFERS_PER_SECOND)
                .div_ceil(1000)
                .saturating_sub(boot_audio_buffers),
        }
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromPowerOn(seconds) => write!(formatter, "{seconds} seconds"),
            Self::AfterBoot(seconds) => {
                write!(formatter, "{seconds} seconds after the boot ROM")
            }
            Self::FramesAfterBoot(frames) => {
                write!(formatter, "{frames} frames after the boot ROM")
            }
            Self::MillisecondsFromPowerOn(milliseconds) => {
                write!(formatter, "{} seconds", f64::from(*milliseconds) / 1000.0)
            }
        }
    }
}

/// One test ROM run on one hardware model.
pub(crate) struct TestCase {
    /// The ROM's path, relative to the root of its source. Reference
    /// screenshots are relative to the same root.
    pub(crate) rom: PathBuf,
    pub(crate) source: Source,
    pub(crate) model: Model,
    pub(crate) check: Check,
    /// When the test fails if it hasn't finished.
    pub(crate) deadline: Deadline,
    /// The part of the ROM this case runs, for ROMs that hold several tests.
    pub(crate) subtest: Option<&'static str>,
    /// Buttons pressed one after another after the boot ROM, such as the
    /// menu choices that start a subtest. Only `Check::Screen` tests press
    /// them.
    pub(crate) input: &'static [Key],
    /// The test's name in the GB Emulator Shootout, for its tests, which
    /// makes their ID `shootout/<name>`.
    pub(crate) shootout_name: Option<&'static str>,
}

impl TestCase {
    /// A test of a ROM from the collection, without subtests or input.
    fn new(rom: PathBuf, model: Model, check: Check, deadline: Deadline) -> Self {
        TestCase {
            rom,
            source: Source::Collection,
            model,
            check,
            deadline,
            subtest: None,
            input: &[],
            shootout_name: None,
        }
    }

    /// The name used in `passing.txt`, such as `dmg blargg/halt_bug.gb`,
    /// `dmg rtc3test/rtc3test.gb (range-tests)` for a subtest, or
    /// `shootout/daid/stop_instr.gb (DMG)` for a test of the GB Emulator
    /// Shootout.
    pub(crate) fn id(&self) -> String {
        if let Some(name) = self.shootout_name {
            return format!("{}{name}", shootout::ID_PREFIX);
        }
        let id = format!("{} {}", model_label(self.model), relative_name(&self.rom));
        match self.subtest {
            Some(subtest) => format!("{id} ({subtest})"),
            None => id,
        }
    }

    /// About how many emulated seconds the test can run at most, so the
    /// longest tests can be started first.
    pub(crate) fn longest_run_seconds(&self) -> u32 {
        match self.deadline {
            Deadline::FromPowerOn(seconds) | Deadline::AfterBoot(seconds) => seconds,
            Deadline::MillisecondsFromPowerOn(milliseconds) => milliseconds.div_ceil(1000),
            // Gambatte's 15 frames are a quarter of a second.
            Deadline::FramesAfterBoot(_) => 0,
        }
    }

    /// The suite's folder name, such as `mooneye-test-suite`, or `shootout`
    /// for the GB Emulator Shootout's tests.
    pub(crate) fn suite(&self) -> String {
        if self.shootout_name.is_some() {
            return "shootout".to_string();
        }
        relative_name(&self.rom)
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

const DEFAULT_MODELS: [Model; 2] = [Model::Dmg, Model::CgbE];
const REGISTER_TIMEOUT_SECONDS: u32 = 30;
const SCREEN_TIMEOUT_SECONDS: u32 = 10;
/// AGE's register tests have no failure signal, so a failing test runs until
/// it times out. They all finish within a few seconds.
const AGE_TIMEOUT_SECONDS: u32 = 10;
/// Seconds after the boot ROM. GBMicrotest's slowest test needs about 0.4.
const GBMICROTEST_SECONDS_AFTER_BOOT: u32 = 1;
/// The CGB revision Gambatte's results were recorded on.
const GAMBATTE_CGB: Model = Model::CgbC;
/// Extra time added to each Blargg, rtc3test and smaller screenshot test's
/// timeout to cover the boot ROM, which takes 1.7 seconds on DMG and 3.1 on
/// CGB.
const BOOT_ROM_SECONDS: u32 = 10;
/// Extra time added to rtc3test's timeouts for picking a subtest from its
/// menu, which takes up to a second.
const RTC3TEST_MENU_SECONDS: u32 = 2;

/// Every Game Boy button, for tests that want them all pressed.
const ALL_KEYS: [Key; 8] = [
    Key::Right,
    Key::Left,
    Key::Up,
    Key::Down,
    Key::A,
    Key::B,
    Key::Select,
    Key::Start,
];

/// A screenshot test from the collection outside the big suites.
struct ScreenshotTest {
    rom: &'static str,
    /// The models it runs on, each with its reference screenshot.
    screens: &'static [(Model, &'static str)],
    /// Emulated seconds it gets on top of `BOOT_ROM_SECONDS`, counted from
    /// power-on.
    seconds: u32,
    /// Whether every frame must match instead of any one.
    every_frame: bool,
    /// Buttons to press after the boot ROM, only for a test that is neither
    /// `every_frame` nor `grayscale`, since only screen tests press them.
    input: &'static [Key],
    /// Whether to compare in grayscale, for a reference made with other
    /// colors than Nubo's.
    grayscale: bool,
}

/// The collection's smaller screenshot tests, on the models c-sp's notes
/// give and for as long as they say each needs. Left out because they have
/// no reference screenshot: `scribbltests/fairylake`, `scribbltests/winpos`,
/// `scribbltests/statcount/statcount.gb` (the manual version of
/// `statcount-auto.gb`) and `mealybug-tearoom-tests/mbc/mbc3_rtc.gb`.
const SCREENSHOT_TESTS: [ScreenshotTest; 15] = [
    // Bully only runs on the CGB: on a DMG even real hardware reports "Bad
    // Echo RAM Reads".
    ScreenshotTest {
        rom: "bully/bully.gb",
        screens: &[(Model::CgbE, "bully/bully.png")],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "strikethrough/strikethrough.gb",
        screens: &[
            (Model::Dmg, "strikethrough/strikethrough-dmg.png"),
            (Model::CgbE, "strikethrough/strikethrough-cgb.png"),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    // firstwhite turns the screen on for one frame at a time. Hardware never
    // shows the first frame after the screen turns on, so it stays white.
    ScreenshotTest {
        rom: "little-things-gb/firstwhite.gb",
        screens: &[
            (Model::Dmg, "little-things-gb/firstwhite-dmg-cgb.png"),
            (Model::CgbE, "little-things-gb/firstwhite-dmg-cgb.png"),
        ],
        seconds: 1,
        every_frame: true,
        input: &[],
        grayscale: false,
    },
    // Telling LYs passes once every button has been pressed.
    ScreenshotTest {
        rom: "little-things-gb/tellinglys.gb",
        screens: &[
            (Model::Dmg, "little-things-gb/tellinglys-dmg.png"),
            (Model::CgbE, "little-things-gb/tellinglys-cgb.png"),
        ],
        seconds: 8,
        every_frame: false,
        input: &ALL_KEYS,
        grayscale: false,
    },
    // The MBC3 Tester repeats its test every few seconds and shows TEST
    // COMPLETE in between: 16 seconds from power-on is in the middle of that
    // on both models.
    ScreenshotTest {
        rom: "mbc3-tester/mbc3-tester.gb",
        screens: &[(Model::Dmg, "mbc3-tester/mbc3-tester-dmg.png")],
        seconds: 6,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    // Its CGB reference was made with other colors: its light green is
    // 0x7BFF4A, where the CGB boot ROM's palette for the game (0x1BEF) is
    // 0x7BFF31.
    ScreenshotTest {
        rom: "mbc3-tester/mbc3-tester.gb",
        screens: &[(Model::CgbE, "mbc3-tester/mbc3-tester-cgb.png")],
        seconds: 6,
        every_frame: false,
        input: &[],
        grayscale: true,
    },
    ScreenshotTest {
        rom: "mooneye-test-suite/manual-only/sprite_priority.gb",
        screens: &[
            (
                Model::Dmg,
                "mooneye-test-suite/manual-only/sprite_priority-dmg.png",
            ),
            (
                Model::CgbE,
                "mooneye-test-suite/manual-only/sprite_priority-cgb.png",
            ),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "mooneye-test-suite-wilbertpol/manual-only/sprite_priority.gb",
        screens: &[
            (
                Model::Dmg,
                "mooneye-test-suite-wilbertpol/manual-only/sprite_priority-dmg.png",
            ),
            (
                Model::CgbE,
                "mooneye-test-suite-wilbertpol/manual-only/sprite_priority-cgb.png",
            ),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "scribbltests/lycscx/lycscx.gb",
        screens: &[
            (Model::Dmg, "scribbltests/lycscx/lycscx-cgb-dmg.png"),
            (Model::CgbE, "scribbltests/lycscx/lycscx-cgb-dmg.png"),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "scribbltests/lycscy/lycscy.gb",
        screens: &[
            (Model::Dmg, "scribbltests/lycscy/lycscy-cgb-dmg.png"),
            (Model::CgbE, "scribbltests/lycscy/lycscy-cgb-dmg.png"),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "scribbltests/palettely/palettely.gb",
        screens: &[
            (Model::Dmg, "scribbltests/palettely/palettely-dmg.png"),
            (Model::CgbE, "scribbltests/palettely/palettely-cgb.png"),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    // The collection's CGB screenshot of scxly is in the green shades of
    // another emulator's DMG palette. The test only draws black and white,
    // which a CGB shows the same way as a DMG.
    ScreenshotTest {
        rom: "scribbltests/scxly/scxly.gb",
        screens: &[
            (Model::Dmg, "scribbltests/scxly/scxly-dmg.png"),
            (Model::CgbE, "scribbltests/scxly/scxly-dmg.png"),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "scribbltests/statcount/statcount-auto.gb",
        screens: &[
            (
                Model::Dmg,
                "scribbltests/statcount/statcount_auto-cgb-dmg.png",
            ),
            (
                Model::CgbE,
                "scribbltests/statcount/statcount_auto-cgb-dmg.png",
            ),
        ],
        seconds: 5,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "turtle-tests/window_y_trigger/window_y_trigger.gb",
        screens: &[
            (
                Model::Dmg,
                "turtle-tests/window_y_trigger/window_y_trigger.png",
            ),
            (
                Model::CgbE,
                "turtle-tests/window_y_trigger/window_y_trigger.png",
            ),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
    ScreenshotTest {
        rom: "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.gb",
        screens: &[
            (
                Model::Dmg,
                "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.png",
            ),
            (
                Model::CgbE,
                "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.png",
            ),
        ],
        seconds: 1,
        every_frame: false,
        input: &[],
        grayscale: false,
    },
];

/// Mooneye's "madness" test, which only runs on the MGB (Game Boy Pocket),
/// as its name says. Its screenshot uses Mooneye's own grays.
const MOONEYE_MADNESS_TESTS: [&str; 2] = [
    "mooneye-test-suite/madness/mgb_oam_dma_halt_sprites",
    "mooneye-test-suite-wilbertpol/madness/mgb_oam_dma_halt_sprites",
];

/// rtc3test's subtests: the name of its reference screenshots, the buttons
/// that pick it from the menu, and the emulated seconds it runs for.
const RTC3TEST_SUBTESTS: [(&str, &[Key], u32); 3] = [
    ("basic-tests", &[Key::A], 13),
    ("range-tests", &[Key::Down, Key::A], 8),
    ("sub-second-writes", &[Key::Down, Key::Down, Key::A], 26),
];

/// A Blargg test ROM and, per model, its reference screenshot and how many
/// seconds it needs.
struct BlarggTest {
    rom: &'static str,
    dmg: Option<(&'static str, u32)>,
    cgb: Option<(&'static str, u32)>,
}

const BLARGG_TESTS: [BlarggTest; 9] = [
    BlarggTest {
        rom: "cgb_sound/cgb_sound.gb",
        dmg: None,
        cgb: Some(("cgb_sound/cgb_sound-cgb.png", 37)),
    },
    BlarggTest {
        rom: "cpu_instrs/cpu_instrs.gb",
        dmg: Some(("cpu_instrs/cpu_instrs-dmg-cgb.png", 55)),
        cgb: Some(("cpu_instrs/cpu_instrs-dmg-cgb.png", 31)),
    },
    BlarggTest {
        rom: "dmg_sound/dmg_sound.gb",
        dmg: Some(("dmg_sound/dmg_sound-dmg.png", 36)),
        cgb: None,
    },
    BlarggTest {
        rom: "halt_bug.gb",
        dmg: Some(("halt_bug-dmg-cgb.png", 2)),
        cgb: Some(("halt_bug-dmg-cgb.png", 2)),
    },
    BlarggTest {
        rom: "instr_timing/instr_timing.gb",
        dmg: Some(("instr_timing/instr_timing-dmg-cgb.png", 1)),
        cgb: Some(("instr_timing/instr_timing-dmg-cgb.png", 1)),
    },
    BlarggTest {
        rom: "interrupt_time/interrupt_time.gb",
        dmg: None,
        cgb: Some(("interrupt_time/interrupt_time-cgb.png", 2)),
    },
    BlarggTest {
        rom: "mem_timing/mem_timing.gb",
        dmg: Some(("mem_timing/mem_timing-dmg-cgb.png", 3)),
        cgb: Some(("mem_timing/mem_timing-dmg-cgb.png", 3)),
    },
    BlarggTest {
        rom: "mem_timing-2/mem_timing.gb",
        dmg: Some(("mem_timing-2/mem_timing-dmg-cgb.png", 4)),
        cgb: Some(("mem_timing-2/mem_timing-dmg-cgb.png", 4)),
    },
    BlarggTest {
        rom: "oam_bug/oam_bug.gb",
        dmg: Some(("oam_bug/oam_bug-dmg.png", 21)),
        cgb: None,
    },
];

/// Lists every test case, sorted by ID. Fails if two cases share an ID, since the run's results
/// and `passing.txt` are kept by ID: a passing case would hide a failing one with its ID.
pub(crate) fn discover(dirs: &RomDirs) -> Result<Vec<TestCase>, String> {
    let root = dirs.root(Source::Collection);
    let mut tests = blargg();
    tests.extend(register_suite(
        root,
        "mooneye-test-suite",
        &["acceptance", "emulator-only", "misc"],
        |_| true,
    ));
    tests.extend(register_suite(
        root,
        "mooneye-test-suite-wilbertpol",
        &["acceptance", "emulator-only", "misc"],
        |_| true,
    ));
    tests.extend(register_suite(
        root,
        "same-suite",
        &["apu", "dma", "interrupt", "ppu"],
        Model::is_cgb,
    ));
    tests.extend(register_suite(
        root,
        "mealybug-tearoom-tests",
        &["dma"],
        |_| true,
    ));
    tests.extend(acid());
    tests.extend(mealybug_ppu(root));
    tests.extend(gambatte(root));
    tests.extend(gbmicrotest(root));
    tests.extend(age(root));
    tests.extend(rtc3test());
    tests.extend(screenshot_tests());
    tests.extend(mooneye_madness());
    tests.extend(shootout_rom_checks());
    tests.extend(super_game_boy(root));
    tests.extend(shootout::test_cases());
    tests.sort_by_key(TestCase::id);
    if let Some(pair) = tests.windows(2).find(|pair| pair[0].id() == pair[1].id()) {
        return Err(format!("two tests share the ID {}", pair[0].id()));
    }
    Ok(tests)
}

/// Blargg's tests, checked against reference screenshots on DMG and CGB.
fn blargg() -> Vec<TestCase> {
    let blargg = Path::new("blargg");
    BLARGG_TESTS
        .iter()
        .flat_map(|test| {
            [(Model::Dmg, test.dmg), (Model::CgbE, test.cgb)]
                .into_iter()
                .filter_map(move |(model, expected)| {
                    expected.map(|(screen, seconds)| {
                        TestCase::new(
                            blargg.join(test.rom),
                            model,
                            Check::Screen(blargg.join(screen)),
                            Deadline::FromPowerOn(seconds + BOOT_ROM_SECONDS),
                        )
                    })
                })
        })
        .collect()
}

/// Every ROM in the given folders of a suite whose tests report through
/// the registers, on each model it supports that `model_filter` accepts.
fn register_suite(
    root: &Path,
    suite: &str,
    folders: &[&str],
    model_filter: fn(Model) -> bool,
) -> Vec<TestCase> {
    folders
        .iter()
        .flat_map(|folder| roms_in(root, &Path::new(suite).join(folder)))
        .flat_map(|rom| {
            supported_models(&rom)
                .into_iter()
                .filter(move |model| model_filter(*model))
                .map(move |model| {
                    TestCase::new(
                        rom.clone(),
                        model,
                        Check::Registers,
                        Deadline::FromPowerOn(REGISTER_TIMEOUT_SECONDS),
                    )
                })
        })
        .collect()
}

/// The dmg-acid2, cgb-acid2 and cgb-acid-hell rendering tests.
fn acid() -> Vec<TestCase> {
    [
        (
            "dmg-acid2/dmg-acid2.gb",
            Model::Dmg,
            "dmg-acid2/dmg-acid2-dmg.png",
        ),
        (
            "dmg-acid2/dmg-acid2.gb",
            Model::CgbE,
            "dmg-acid2/dmg-acid2-cgb.png",
        ),
        (
            "cgb-acid2/cgb-acid2.gbc",
            Model::CgbE,
            "cgb-acid2/cgb-acid2.png",
        ),
        (
            "cgb-acid-hell/cgb-acid-hell.gbc",
            Model::CgbE,
            "cgb-acid-hell/cgb-acid-hell.png",
        ),
    ]
    .into_iter()
    .map(|(rom, model, screen)| {
        TestCase::new(
            PathBuf::from(rom),
            model,
            Check::Screen(PathBuf::from(screen)),
            Deadline::FromPowerOn(SCREEN_TIMEOUT_SECONDS),
        )
    })
    .collect()
}

/// Mealybug Tearoom's PPU (picture processing unit) tests, on every model
/// that has a reference screenshot: DMG uses the DMG-blob screenshots, and
/// CGB-E uses the CGB-D screenshots.
fn mealybug_ppu(root: &Path) -> Vec<TestCase> {
    let screens = [
        (Model::Dmg, "dmg_blob"),
        (Model::CgbC, "cgb_c"),
        (Model::CgbD, "cgb_d"),
        (Model::CgbE, "cgb_d"),
    ];

    roms_in(root, Path::new("mealybug-tearoom-tests/ppu"))
        .into_iter()
        .flat_map(|rom| {
            screens.iter().filter_map(move |(model, revision)| {
                let screen = reference_screen(&rom, revision);
                root.join(&screen).exists().then(|| {
                    TestCase::new(
                        rom.clone(),
                        *model,
                        Check::Screen(screen),
                        Deadline::FromPowerOn(SCREEN_TIMEOUT_SECONDS),
                    )
                })
            })
        })
        .collect()
}

/// Gambatte's tests. The expected result is in the file name: `_out` followed
/// by a hexadecimal number or `audio0`/`audio1`, after `dmg08` (DMG),
/// `cgb04c` (CGB-C) or both. A bare `_out` is CGB only. Screenshot tests have
/// a PNG named after the ROM with the same model suffixes.
fn gambatte(root: &Path) -> Vec<TestCase> {
    let mut tests = Vec::new();
    for rom in roms_in(root, Path::new("gambatte")) {
        let stem = rom
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut add = |model: Model, check: Check| {
            tests.push(TestCase::new(
                rom.clone(),
                model,
                check,
                Deadline::FramesAfterBoot(gambatte::FRAMES_AFTER_BOOT),
            ));
        };

        if let Some(result) = text_after(&stem, "dmg08_cgb04c_out") {
            add(Model::Dmg, gambatte_check(result));
            add(GAMBATTE_CGB, gambatte_check(result));
        } else if let Some(result) = text_after(&stem, "dmg08_out") {
            add(Model::Dmg, gambatte_check(result));
            if let Some(result) = text_after(&stem, "cgb04c_out") {
                add(GAMBATTE_CGB, gambatte_check(result));
            }
        } else if let Some(result) = text_after(&stem, "_out") {
            add(GAMBATTE_CGB, gambatte_check(result));
        }

        let screen = |suffix: &str| {
            let path = rom.with_file_name(format!("{stem}_{suffix}.png"));
            root.join(&path).exists().then_some(path)
        };
        if let Some(path) = screen("dmg08_cgb04c") {
            add(Model::Dmg, Check::GambatteScreen(path.clone()));
            add(GAMBATTE_CGB, Check::GambatteScreen(path));
        } else {
            if let Some(path) = screen("dmg08") {
                add(Model::Dmg, Check::GambatteScreen(path));
            }
            if let Some(path) = screen("cgb04c") {
                add(GAMBATTE_CGB, Check::GambatteScreen(path));
            }
        }
    }
    tests
}

/// The text in `name` after the first `marker`.
fn text_after<'a>(name: &'a str, marker: &str) -> Option<&'a str> {
    name.find(marker).map(|index| &name[index + marker.len()..])
}

/// How a Gambatte test with this expected result is checked: `audio0` and
/// `audio1` are about sound, anything else is a number up to the first
/// character that isn't a hexadecimal digit. A result that starts with no
/// such digit gives an empty number, which the runner fails rather than
/// passing on any screen.
fn gambatte_check(result: &str) -> Check {
    match result {
        _ if result.starts_with("audio0") => Check::Sound(false),
        _ if result.starts_with("audio1") => Check::Sound(true),
        _ => Check::Number(
            result
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .map(|digit| digit.to_ascii_uppercase())
                .collect(),
        ),
    }
}

/// GBMicrotest ROMs that never store a result to 0xFF82: test benches,
/// experiments and unfinished tests, which can neither pass nor fail.
const GBMICROTEST_WITHOUT_RESULT: &[&str] = &[
    "000-oam_lock.gb",
    "000-write_to_x8000.gb",
    "001-vram_unlocked.gb",
    "002-vram_locked.gb",
    "004-tima_boot_phase.gb",
    "004-tima_cycle_timer.gb",
    "007-lcd_on_stat.gb",
    "400-dma.gb",
    "500-scx-timing.gb",
    "800-ppu-latch-scx.gb",
    "801-ppu-latch-scy.gb",
    "802-ppu-latch-tileselect.gb",
    "803-ppu-latch-bgdisplay.gb",
    "audio_testbench.gb",
    "cpu_bus_1.gb",
    "dma_basic.gb",
    "flood_vram.gb",
    "lcdon_write_timing.gb",
    "ly_while_lcd_off.gb",
    "minimal.gb",
    "mode2_stat_int_to_oam_unlock.gb",
    "oam_sprite_trashing.gb",
    "poweron.gb",
    "ppu_scx_vs_bgp.gb",
    "ppu_sprite_testbench.gb",
    "ppu_spritex_vs_scx.gb",
    "ppu_win_vs_wx.gb",
    "ppu_wx_early.gb",
    "temp.gb",
    "toggle_lcdc.gb",
    "wave_write_to_0xC003.gb",
];

/// GBMicrotest, written for and checked on a DMG.
fn gbmicrotest(root: &Path) -> Vec<TestCase> {
    roms_in(root, Path::new("gbmicrotest"))
        .into_iter()
        .filter(|rom| {
            rom.file_name()
                .and_then(|name| name.to_str())
                .is_none_or(|name| !GBMICROTEST_WITHOUT_RESULT.contains(&name))
        })
        .map(|rom| {
            TestCase::new(
                rom,
                Model::Dmg,
                Check::HighRam,
                Deadline::AfterBoot(GBMICROTEST_SECONDS_AFTER_BOOT),
            )
        })
        .collect()
}

/// The AGE test ROMs. Most report through the registers on the models in
/// their name; the rest have one reference screenshot per group of models,
/// named after the ROM followed by the models, such as `m3-bg-scx-dmgC.png`.
fn age(root: &Path) -> Vec<TestCase> {
    let mut tests = Vec::new();
    for rom in roms_in(root, Path::new("age-test-roms")) {
        let stem = rom
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let screens: Vec<(PathBuf, Vec<Model>)> = fs::read_dir(root.join(&rom).with_file_name(""))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let file_name = entry.file_name().to_string_lossy().into_owned();
                let models = file_name
                    .strip_suffix(".png")?
                    .strip_prefix(stem.as_str())?
                    .strip_prefix('-')
                    .and_then(model_list)?;
                Some((rom.with_file_name(file_name), models))
            })
            .collect();

        if screens.is_empty() {
            tests.extend(supported_models(&rom).into_iter().map(|model| {
                TestCase::new(
                    rom.clone(),
                    model,
                    Check::Registers,
                    Deadline::FromPowerOn(AGE_TIMEOUT_SECONDS),
                )
            }));
        }
        for (screen, models) in screens {
            tests.extend(models.into_iter().map(|model| {
                TestCase::new(
                    rom.clone(),
                    model,
                    Check::Screen(screen.clone()),
                    Deadline::FromPowerOn(SCREEN_TIMEOUT_SECONDS),
                )
            }));
        }
    }
    tests
}

/// ax6's rtc3test, which checks the MBC3's real-time clock. Each subtest is
/// picked from the ROM's menu and checked against a screenshot per model.
fn rtc3test() -> Vec<TestCase> {
    let folder = Path::new("rtc3test");
    RTC3TEST_SUBTESTS
        .into_iter()
        .flat_map(|(name, input, seconds)| {
            [(Model::Dmg, "dmg"), (Model::CgbE, "cgb")]
                .into_iter()
                .map(move |(model, suffix)| TestCase {
                    subtest: Some(name),
                    input,
                    ..TestCase::new(
                        folder.join("rtc3test.gb"),
                        model,
                        Check::Screen(folder.join(format!("rtc3test-{name}-{suffix}.png"))),
                        Deadline::FromPowerOn(BOOT_ROM_SECONDS + RTC3TEST_MENU_SECONDS + seconds),
                    )
                })
        })
        .collect()
}

/// The collection's smaller screenshot tests (see `SCREENSHOT_TESTS`).
fn screenshot_tests() -> Vec<TestCase> {
    SCREENSHOT_TESTS
        .iter()
        .flat_map(|test| {
            test.screens.iter().map(|(model, screen)| {
                let screen = PathBuf::from(screen);
                assert!(
                    test.input.is_empty() || !(test.grayscale || test.every_frame),
                    "{} has buttons to press, but only screen tests press them",
                    test.rom
                );
                let check = if test.grayscale {
                    Check::GrayscaleScreen(vec![screen])
                } else if test.every_frame {
                    Check::EveryFrame(screen)
                } else {
                    Check::Screen(screen)
                };
                TestCase {
                    input: test.input,
                    ..TestCase::new(
                        PathBuf::from(test.rom),
                        *model,
                        check,
                        Deadline::FromPowerOn(BOOT_ROM_SECONDS + test.seconds),
                    )
                }
            })
        })
        .collect()
}

/// Mooneye's "madness" tests on the MGB, compared in grayscale a second
/// after the boot ROM.
fn mooneye_madness() -> Vec<TestCase> {
    MOONEYE_MADNESS_TESTS
        .iter()
        .map(|path| {
            TestCase::new(
                PathBuf::from(format!("{path}.gb")),
                Model::Mgb,
                Check::GrayscaleScreen(vec![PathBuf::from(format!("{path}_expected.png"))]),
                Deadline::AfterBoot(1),
            )
        })
        .collect()
}

/// Seconds after the boot ROM at which the shootout's tests are checked. The
/// shootout checks after half a second; these tests keep showing their
/// result once they are done.
const SHOOTOUT_SECONDS_AFTER_BOOT: u32 = 1;

/// Tests of the GB Emulator Shootout that the collection lacks: daid's PPU,
/// STOP and speed switch tests and CasualPokePlayer's MBC3 tests, checked
/// for what the emulator does. Each runs on the model the shootout uses and
/// passes if it matches one of its screenshots (several where hardware units
/// differ) in the screenshots' own grays (see `screen::normalize`). The
/// Shootout's own judgement of them, in the colors the app shows with
/// `--raw-screen`, is in
/// `shootout.rs`. Left out are
/// `daid/rom_and_ram.gb`, which has no expected result, and
/// `cpp/sgb-ext-test.gb`, which takes longer and runs in `super_game_boy`.
const SHOOTOUT_ROM_CHECKS: [(&str, Model, &[&str]); 11] = [
    (
        "daid/ppu_scanline_bgp.gb",
        Model::Dmg,
        &[
            "daid/ppu_scanline_bgp_0.dmg.png",
            "daid/ppu_scanline_bgp_1.dmg.png",
            "daid/ppu_scanline_bgp_2.dmg.png",
        ],
    ),
    (
        "daid/ppu_scanline_bgp.gb",
        Model::CgbE,
        &["daid/ppu_scanline_bgp.gbc.png"],
    ),
    (
        "daid/stop_instr.gb",
        Model::Dmg,
        &["daid/stop_instr.dmg.png"],
    ),
    (
        "daid/stop_instr.gb",
        Model::CgbE,
        &["daid/stop_instr.gbc.png"],
    ),
    (
        "daid/stop_instr_gbc_mode3.gb",
        Model::CgbE,
        &["daid/stop_instr_gbc_mode3.png"],
    ),
    (
        "daid/speed_switch_timing_div.gbc",
        Model::CgbE,
        &["daid/speed_switch_timing_div.png"],
    ),
    (
        "daid/speed_switch_timing_ly.gbc",
        Model::CgbE,
        &["daid/speed_switch_timing_ly.png"],
    ),
    (
        "daid/speed_switch_timing_stat.gbc",
        Model::CgbE,
        &["daid/speed_switch_timing_stat.png"],
    ),
    (
        "cpp/rtc-invalid-banks-test.gb",
        Model::Dmg,
        &["cpp/rtc-invalid-banks-test.png"],
    ),
    (
        "cpp/latch-rtc-test.gb",
        Model::Dmg,
        &["cpp/latch-rtc-test.png"],
    ),
    (
        "cpp/ramg-mbc3-test.gb",
        Model::Dmg,
        &["cpp/ramg-mbc3-test.png"],
    ),
];

/// The tests of `SHOOTOUT_ROM_CHECKS`.
fn shootout_rom_checks() -> Vec<TestCase> {
    SHOOTOUT_ROM_CHECKS
        .into_iter()
        .map(|(rom, model, screens)| TestCase {
            source: Source::Shootout,
            ..TestCase::new(
                PathBuf::from(rom),
                model,
                Check::GrayscaleScreen(screens.iter().map(PathBuf::from).collect()),
                Deadline::AfterBoot(SHOOTOUT_SECONDS_AFTER_BOOT),
            )
        })
        .collect()
}

/// Seconds after the boot ROM at which `cpp/sgb-ext-test.gb` is checked: it
/// sends 27 commands with long pauses between them, which takes about five
/// seconds.
const SGB_EXT_TEST_SECONDS_AFTER_BOOT: u32 = 8;

/// The tests of the Super Game Boy's commands, on both Super Game Boys:
/// SameSuite's `MLT_REQ` tests, which report through the registers, and
/// CasualPokePlayer's `sgb-ext-test`, which tests the packet protocol and is
/// checked against its screenshot in the Shootout's grays.
fn super_game_boy(root: &Path) -> Vec<TestCase> {
    let models = [Model::Sgb, Model::Sgb2];
    let same_suite = roms_in(root, Path::new("same-suite/sgb"))
        .into_iter()
        .flat_map(|rom| {
            models.map(|model| {
                TestCase::new(
                    rom.clone(),
                    model,
                    Check::Registers,
                    Deadline::FromPowerOn(REGISTER_TIMEOUT_SECONDS),
                )
            })
        });
    let packet_protocol = models.map(|model| TestCase {
        source: Source::Shootout,
        ..TestCase::new(
            PathBuf::from("cpp/sgb-ext-test.gb"),
            model,
            Check::GrayscaleScreen(vec![PathBuf::from("cpp/sgb-ext-test.png")]),
            Deadline::AfterBoot(SGB_EXT_TEST_SECONDS_AFTER_BOOT),
        )
    });
    same_suite.chain(packet_protocol).collect()
}

/// The path of a Mealybug reference screenshot, such as `m3_scx_low_3_bits_cgb_d.png`.
fn reference_screen(rom: &Path, revision: &str) -> PathBuf {
    let stem = rom.file_stem().unwrap_or_default().to_string_lossy();
    rom.with_file_name(format!("{stem}_{revision}.png"))
}

/// All `.gb` and `.gbc` files under `folder` (searched recursively), as
/// paths relative to `root`. A missing folder yields no ROMs.
fn roms_in(root: &Path, folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root.join(folder)) else {
        return Vec::new();
    };

    let mut roms = Vec::new();
    for entry in entries.flatten() {
        let relative = folder.join(entry.file_name());
        if entry.path().is_dir() {
            roms.extend(roms_in(root, &relative));
        } else if matches!(
            relative
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("gb" | "gbc")
        ) {
            roms.push(relative);
        }
    }
    roms
}

/// The models a ROM should run on, read from the end of its file name:
/// Mooneye's groups (`-GS`: G = DMG/MGB, S = SGB/SGB2, C = CGB/AGB,
/// A = AGB) or revision lists (`-dmgABCmgb`, `-cgb0BC`), of which AGE puts
/// several in a row (`-dmgC-cgbBCE`). Without them it runs on DMG and CGB-E.
fn supported_models(rom: &Path) -> Vec<Model> {
    let stem = rom.file_stem().unwrap_or_default().to_string_lossy();
    let segments: Vec<&str> = stem.split('-').skip(1).collect();
    let Some(last) = segments.last() else {
        return DEFAULT_MODELS.to_vec();
    };
    if let Some(groups) = model_groups(last) {
        return groups;
    }

    let lists: Vec<Vec<Model>> = segments
        .iter()
        .rev()
        .map_while(|segment| model_list(segment))
        .collect();
    let mut models = Vec::new();
    for model in lists.into_iter().rev().flatten() {
        if !models.contains(&model) {
            models.push(model);
        }
    }
    if models.is_empty() {
        DEFAULT_MODELS.to_vec()
    } else {
        models
    }
}

/// The models in a Mooneye group suffix such as `GS`, or `None` if `text`
/// isn't one.
fn model_groups(text: &str) -> Option<Vec<Model>> {
    if !text.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    let mut models = Vec::new();
    for group in text.chars() {
        let group_models: &[Model] = match group {
            'G' => &[Model::Dmg, Model::Mgb],
            'S' => &[Model::Sgb, Model::Sgb2],
            'C' => &[Model::CgbE, Model::Agb],
            'A' => &[Model::Agb],
            _ => return None,
        };
        for model in group_models {
            if !models.contains(model) {
                models.push(*model);
            }
        }
    }
    Some(models)
}

/// The models in a revision list such as `dmgABCmgb` or `cgb0BC`, or `None`
/// if `text` isn't one. AGE's `ncm` means a CGB running a DMG game.
fn model_list(text: &str) -> Option<Vec<Model>> {
    let mut models = Vec::new();
    let mut add = |model: Model| {
        if !models.contains(&model) {
            models.push(model);
        }
    };

    let mut rest = text;
    while !rest.is_empty() {
        let name = ["sgb2", "dmg", "mgb", "sgb", "cgb", "ncm", "agb", "ags"]
            .into_iter()
            .find(|name| rest.starts_with(name))?;
        rest = &rest[name.len()..];
        let revisions: String = rest
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .collect();
        rest = &rest[revisions.len()..];

        match name {
            "dmg" if revisions.is_empty() => add(Model::Dmg),
            "dmg" => revisions.chars().for_each(|revision| {
                add(if revision == '0' {
                    Model::Dmg0
                } else {
                    Model::Dmg
                });
            }),
            "cgb" | "ncm" if revisions.is_empty() => add(Model::CgbE),
            "cgb" | "ncm" => revisions
                .chars()
                .filter_map(cgb_revision)
                .for_each(&mut add),
            "mgb" => add(Model::Mgb),
            "sgb" => add(Model::Sgb),
            "sgb2" => add(Model::Sgb2),
            _ => add(Model::Agb),
        }
    }
    Some(models)
}

/// The CGB model for a revision letter in a ROM name, such as 'C' for CGB-C.
fn cgb_revision(revision: char) -> Option<Model> {
    match revision {
        '0' => Some(Model::Cgb0),
        'A' => Some(Model::CgbA),
        'B' => Some(Model::CgbB),
        'C' => Some(Model::CgbC),
        'D' => Some(Model::CgbD),
        'E' => Some(Model::CgbE),
        _ => None,
    }
}

/// The path with `/` separators on every platform, so IDs match across
/// systems.
fn relative_name(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(name: &str) -> Vec<Model> {
        supported_models(Path::new(name))
    }

    #[test]
    fn counts_each_deadline_from_its_own_start() {
        let boot = 300;
        assert_eq!(
            Deadline::FromPowerOn(30).audio_buffers_after_boot(boot),
            30 * 128 - boot
        );
        assert_eq!(Deadline::FromPowerOn(2).audio_buffers_after_boot(boot), 0);
        assert_eq!(Deadline::AfterBoot(1).audio_buffers_after_boot(boot), 128);
        assert_eq!(
            Deadline::MillisecondsFromPowerOn(3500).audio_buffers_after_boot(boot),
            448 - boot
        );
        assert_eq!(
            Deadline::MillisecondsFromPowerOn(2000).audio_buffers_after_boot(boot),
            0
        );
        // 15 frames of 70224 T-cycles are 32.1 buffers of 32768.
        assert_eq!(
            Deadline::FramesAfterBoot(15).audio_buffers_after_boot(boot),
            33
        );
    }

    #[test]
    fn runs_tests_without_a_suffix_on_dmg_and_cgb() {
        assert_eq!(
            models("acceptance/div_timing.gb"),
            vec![Model::Dmg, Model::CgbE]
        );
    }

    #[test]
    fn reads_model_groups() {
        assert_eq!(
            models("acceptance/di_timing-GS.gb"),
            vec![Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2]
        );
        assert_eq!(models("misc/boot_hwio-C.gb"), vec![Model::CgbE, Model::Agb]);
        assert_eq!(models("misc/boot_div-A.gb"), vec![Model::Agb]);
        assert_eq!(
            models("acceptance/boot_div-S.gb"),
            vec![Model::Sgb, Model::Sgb2]
        );
    }

    #[test]
    fn reads_model_revisions() {
        assert_eq!(models("acceptance/boot_regs-dmgABC.gb"), vec![Model::Dmg]);
        assert_eq!(
            models("acceptance/boot_div-dmgABCmgb.gb"),
            vec![Model::Dmg, Model::Mgb]
        );
        assert_eq!(models("acceptance/boot_regs-dmg0.gb"), vec![Model::Dmg0]);
        assert_eq!(models("acceptance/boot_regs-sgb2.gb"), vec![Model::Sgb2]);
        assert_eq!(models("misc/boot_regs-cgb.gb"), vec![Model::CgbE]);
        assert_eq!(
            models("misc/boot_div-cgbABCDE.gb"),
            vec![
                Model::CgbA,
                Model::CgbB,
                Model::CgbC,
                Model::CgbD,
                Model::CgbE
            ]
        );
        assert_eq!(
            models("channel_1_freq_change_timing-cgb0BC.gb"),
            vec![Model::Cgb0, Model::CgbB, Model::CgbC]
        );
        assert_eq!(
            models("channel_1_freq_change_timing-cgbDE.gb"),
            vec![Model::CgbD, Model::CgbE]
        );
    }

    #[test]
    fn reads_several_model_lists() {
        assert_eq!(
            models("halt-m0-interrupt-dmgC-cgbBCE.gb"),
            vec![Model::Dmg, Model::CgbB, Model::CgbC, Model::CgbE]
        );
        assert_eq!(models("stat-mode-ncmBC.gb"), vec![Model::CgbB, Model::CgbC]);
        assert_eq!(models("lcd-align-ly-cgbE.gb"), vec![Model::CgbE]);
    }

    #[test]
    fn only_screen_tests_have_buttons_to_press() {
        for test in screenshot_tests() {
            assert!(
                test.input.is_empty() || matches!(test.check, Check::Screen(_)),
                "{} has buttons to press, but only screen tests press them",
                test.rom.display()
            );
        }
    }

    #[test]
    fn reads_gambatte_results() {
        assert!(matches!(gambatte_check("0A_x"), Check::Number(number) if number == "0A"));
        assert!(matches!(gambatte_check("audio1"), Check::Sound(true)));
        assert!(matches!(gambatte_check("audio0"), Check::Sound(false)));
    }
}
