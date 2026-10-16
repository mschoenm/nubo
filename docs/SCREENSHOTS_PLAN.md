# Plan: screenshots that can always be made again

The README and the Flathub listing show screenshots of Nubo, and they go out of date whenever the app
changes: the Flathub screenshot still showed the game bar from before it was simplified, in the system's
font. This plan builds the tools to make every screenshot again, at any time, with the newest version of
the app, from a recorded moment of real play, with any settings, light or dark, windowed or fullscreen,
and with real GNOME window borders where a listing wants them.

## Status

Steps 1 to 7 are built (see Order of work); step 8, the full nightly run after merging, is left, and then
the new screenshots. Checked in a cloud session: every kind of picture (`none`, `gnome`, `fullscreen`,
`game`, variants side by side, a scene of the library) comes out the same pixel for pixel when made again,
a scene recorded with Ctrl+F12 renders exactly like its preview, and a scene from `explore` renders exactly
like its preview at 1×. Every screenshot in the README and the Flathub listing is now made from a scene:
`images/screenshots.toml` lists them, and their scenes are in `images/scenes/`, made with `explore` (the
library's scene is written by hand, since it has no save state).

Where the build differs from the plan below:

- **A fourth frame, `game`:** the game's picture alone at a whole scale, as the app's `--scale` shows it, for
  pictures such as the palettes side by side.
- **Screenshots can open another page:** `pause_menu` and `settings_tab` in `images/screenshots.toml` (and the
  `capture` command's `--pause-menu` and `--settings-tab`) open another page of the pause menu or another
  settings tab than the scene has open, or close it with `none`.
- **`cargo save-states` doesn't load the scenes' states.** It would need the scenes' games downloaded in the
  nightly run, which keeps the screenshots out of CI. The save state rule covers them all the same: a scene's
  state is stored in a layout `cargo save-states` keeps, and `cargo xtask screenshots` loads every state it
  renders.
- **GNOME 46's border:** GNOME draws the border of an XWayland window with GTK 4, so on GNOME 46 (Ubuntu
  24.04, which cloud sessions and GitHub's runners have) it has the shadow and rounded top corners, square
  bottom corners, and plain GTK 4's beige title bar with bare buttons. A stylesheet the task gives GTK
  (`TITLE_BAR_STYLE` in `xtask/src/screenshots/gnome.rs`) draws libadwaita's title bar instead: the window's
  color with round buttons, as GNOME's own apps and newer GNOME versions look. GNOME's version still decides
  the pixels of those pictures.
- **The recorder's preview** leaves out the toasts and isn't taken while a question is open, since scenes keep
  neither; nor while the player's own data shows (box art, a cheat that is on, or a pause menu page listing
  saved states or cheats), which a capture never has. A scene's game keeps no battery saves, with its cartridge
  clock on emulated time, as in `explore`, and the scene keeps the light or dark look on screen rather than "System". `--record-scenes`
  refuses `--fresh` too, which hides the save state rows a capture shows.
- **Explore's previews** show the game's picture alone, as the app shows it with the default settings, not
  the whole window.

## Scenes: recorded moments

A scene is a folder with:

- `scene.toml`: the game (its file name and SHA-256), the model, a copy of all settings, the window size,
  and what is open on screen (playing, the pause menu, a settings tab);
- `game.state`: a save state a few frames before the moment, with how many frames to run from it, so
  frame blending (which mixes in the frame before) comes out right;
- `preview.png`: what was on screen, to check the renders against.

The scenes that published screenshots use are kept in `images/scenes/` (about 30 KB each in git). They
fall under the rule that players never lose a save state to an update.

Scenes are made two ways:

1. **The recorder, for a person playing.** `nubo --record-scenes <folder>` is a hidden developer option.
   While a game plays, Ctrl+F12 saves a scene there. This is the way to reach later levels and moments
   that need skill.
2. **Explore, for a session or a script.** `cargo xtask scene explore <rom | scene>` plays headless,
   from power-on or from a scene's state, with an input script such as
   `wait 4s; press start; hold right 2s; press a; wait 1s`. It saves a candidate scene every few seconds
   or at marked points of the script, and draws a contact sheet: one image with every candidate,
   numbered. Any candidate can be the starting point of a new run. Candidates go to `target/`; only the
   ones chosen are committed.

## Rendering screenshots

`images/screenshots.toml` lists what is published. Each entry names the output file, the scene, and:

- `frame`: `none` (the window's contents), `gnome` (GNOME's real border, with rounded corners, shadow
  and a see-through background) or `fullscreen` (the whole screen, 1920×1080 unless the entry says
  otherwise);
- `settings`: changes to the scene's settings, by the names `settings.toml` uses, so every setting works,
  later ones included (`theme = "dark"` is dark mode);
- `variants` with a `layout` (a row or a grid): the same scene with different settings side by side, such
  as the palettes, light and dark, or a setting on and off.

Images carry no text; the captions stay in the README.

`cargo xtask screenshots [filter] [--check]` downloads the games by SHA-256 from `images/scenes/games.toml`
(where each game's address, license and credit are), starts the app in a hidden `capture` mode (it applies
the settings, loads the state, runs the frames, opens the view and holds still) and takes the picture:

- `frame = none`: the app saves its own window's contents, anywhere.
- `frame = gnome` and `fullscreen`: the app runs through XWayland under a headless GNOME Shell, which
  captures it, in a cloud session too (checked: GNOME Shell 46 from Ubuntu 24.04 runs with a private
  system bus and draws its own border around an XWayland window). The border follows the screenshot's
  light or dark theme. When GNOME Shell is missing, the task prints the `apt` line that installs it.
  Under Wayland, Nubo's title bar is winit's own, which GNOME's window screenshot leaves out, so the
  border is always GNOME's.

Iced's software renderer is always used, so a scene gives the same pixels every time. A file whose pixels
did not change is left as it is, so git only stores the images that really changed (15 to 70 KB each).
`--check` only compares with the committed images and writes diff images. The app looks the same as before
on every system: nothing it shows changes.

## Upkeep

- **No CI and no nightly job for now.** The task runs when asked. A job would take about 4 to 7 minutes
  next to the other nightly jobs (building the app, installing GNOME Shell, about ten screenshots), if the
  maintainer wants one later.
- **The `screenshots` skill** (`.claude/skills/screenshots/SKILL.md`, with the pointer in `.agents/skills/`)
  holds the steps: to regenerate (install GNOME Shell, run the task, show the changed images, update the
  README, the Flathub metainfo and the credits, open a pull request), and to collect new screenshots (pick
  freely licensed games, explore them, render the best candidates in their final form, show them as one
  gallery, then commit the ones the maintainer picks and update the docs).
- **Credits:** a test checks that every game in a published screenshot is credited in the README.

## The Shootout and every other result stay as they are

The GB Emulator Shootout starts the real app with `--raw-screen`, `--scale` and `--fresh` and judges
screenshots of its window; none of that may change.

- `capture` and `--record-scenes` are hidden, do nothing unless given, and refuse `--scale` and
  `--raw-screen`.
- Changed settings go through the existing settings and `ScreenLook`; `ScreenLook::RAW` keeps turning every
  palette and screen effect off, later ones included.
- `capture` keeps nothing between runs, like `--fresh`: it reads and writes no settings, saves or library,
  so nothing it does reaches a later Shootout run.
- The emulation is not touched: explore runs games through the core as `cargo screenshot` does, and
  captures show the frames the core draws.
- Ctrl+F12 only works with `--record-scenes` and takes no key from F12 (Screenshot) or any other binding.

Checked before each pull request, besides `AGENTS.md`'s list: the command line's tests (the new options and
what they refuse), `cargo test-roms` (the Shootout's tests run headless), fingerprints compared
(`cargo benchmark --record` / `--compare`) where the app's game session changes, and the Shootout workflow
started by hand when `command_line.rs` or the app's start changes.

**After merging:** the full nightly run started by hand on `main` (the Windows build, the Shootout's harness
on the real `nubo.exe`, the kept save states, `eager`, the benchmark and the other platforms). Anything red
is fixed, or reported with what failed and why when it isn't this work's. Then common sense: open the
Shootout job's artifact and look at some of its screenshots, and compare its start time and score with the
night before, so nothing changed while staying green.

## Order of work

1. This plan.
2. The `capture` mode, the two lists and `cargo xtask screenshots` with `frame = none`. Done when a scene
   renders the same twice and like its preview.
3. The GNOME harness: windowed with real borders and fullscreen, the border following light or dark.
4. The recorder (Ctrl+F12).
5. `cargo xtask scene explore`: input scripts, candidates, the contact sheet and branching.
6. Settings variants and side-by-side layouts.
7. The `screenshots` skill and the docs (`xtask/AGENTS.md`, `BUILDING.md`, `AGENTS.md`).
8. After merging: the full nightly run on `main`, the Shootout's screenshots and score looked at, and
   anything that broke fixed.

Then a new session collects screenshots with the skill, the maintainer picks from its gallery, and the README
and Flathub images are replaced (done: the README's and Flathub's pictures were made again from scenes, and six
new ones added from the first gallery).
