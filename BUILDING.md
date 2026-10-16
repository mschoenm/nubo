# Building Nubo

How to build, run and package Nubo from its source code. To play, the [downloads in the README](README.md#download) are all you need; this page is for building it yourself (on macOS, for now, the only way to get the app) and for working on it.

## What You Need

A Rust toolchain installed with [rustup](https://rustup.rs). `rust-toolchain.toml` asks for the newest stable Rust, so run `rustup update` if a build complains. Everything is driven by cargo, on Windows, macOS and Linux.

The test ROMs, the CPU tests and the benchmark games are downloaded by the tools themselves, through the proxy in `HTTPS_PROXY` if one is set. `cargo xtask web` adds the WebAssembly target to your toolchain with rustup the first time.

On Linux, sound and controllers also need the ALSA and udev development files, for example `sudo apt install libasound2-dev libudev-dev pkg-config` on Debian or Ubuntu.

## Running

```
cargo play                          # Run the desktop app
cargo play path/to/rom.gb           # Run a specific ROM (.gb, .gbc, or a .zip containing one)
cargo play path/to/rom.gbc --cgb    # Run in Game Boy Color mode
cargo play path/to/rom.gb --model mgb  # Run on another model: dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e, agb
cargo play path/to/rom.gb --scale 2 --fresh --speed 4 --raw-screen  # Only the screen at 2×, keeping nothing, fast forwarding at 4×, without color correction (for test harnesses)
cargo xtask web                     # Build the app for the browser and serve it at http://127.0.0.1:8080
cargo screenshot path/to/rom.gb --seconds 10  # Run headless for 10 emulated seconds (5 without --seconds) and save a screenshot to target/screenshots/
```

`cargo play --help` lists the command-line options, and `frontends/nubo/src/command_line.rs` explains them.

## Packaging

Each package goes into `target/bundle/`. Releases and the nightly build make these on GitHub, so you only need them to build a package yourself.

### macOS

`cargo xtask macos` builds `Nubo.app`, with its own icon in the Dock. Drag it to the Applications folder to install it.

### Windows

`cargo xtask windows` builds `Nubo-<version>-windows-x64.msi`, which installs Nubo for your user (no administrator rights) with a Start menu entry, and can offer Nubo for opening `.gb` and `.gbc` files and add a desktop shortcut. It also writes a zip of the same files, to run Nubo without installing. It needs the [WiX Toolset](https://wixtoolset.org): `dotnet tool install --global wix`, then `wix eula accept wix7` to accept its license (Nubo owes no fee).

The `nightly` pre-release's installer shows as "Nubo (nightly <date>)" once installed (`cargo xtask windows --nightly <date>` names it so).

### Linux

`cargo xtask flatpak` builds the app as a Flatpak and installs it for your user, to try it in the sandbox that Flathub apps run in. It needs flatpak-builder; the first build downloads the freedesktop runtime from Flathub, adding Flathub for your user if it isn't there yet.

It also writes `Nubo-<version>-linux-<architecture>.flatpak`, a single file to give to others, the same as the ones in each release.

### Installing on This Computer

`cargo xtask install` builds the package for the computer it runs on and installs it there: `Nubo.app` in `/Applications` on macOS, the Flatpak on Linux and the MSI on Windows, with no questions or windows. A running Nubo is closed first, the way closing its window does, so it saves where you left off. It needs what the package of that system needs.

It first fetches from GitHub. When the working tree is the newest `origin/main` without changes (new files count as changes) it builds that; otherwise it asks whether to build the working tree as it is or the newest `origin/main`. `--build` decides without asking: `checkout`, `main`, a version (`0.12` is the newest 0.12.x release), or any tag, branch or commit. Anything but the working tree is built in a worktree under `target/install/`, leaving your files alone. Versions from before this command existed can't be installed with it.

### RetroArch

`cargo xtask libretro` builds the RetroArch core's zip; [docs/RETROARCH.md](docs/RETROARCH.md) says how to install it.

### Licenses

Every package (the macOS app, the Windows installer and zip, the Flatpak, the web version and the RetroArch core) comes with `THIRD_PARTY_LICENSES.txt`, the licenses of the Rust crates Nubo is built from. `cargo xtask licenses <folder>` writes it with the other license files; the packaging tasks run it for you.

## Testing

```
cargo test                          # The emulator's own tests
cargo test --workspace              # Every crate, the apps and tools included
cargo test-roms                     # Download and run the test ROM suites (about 30 s)
cargo cpu-tests                     # Every CPU instruction, checked cycle by cycle
```

[docs/ACCURACY.md](docs/ACCURACY.md) lists the test ROM suites, the results and the tests that aren't run.

## Screenshots

The README's and the Flathub listing's screenshots are made again from recorded scenes ([docs/SCREENSHOTS_PLAN.md](docs/SCREENSHOTS_PLAN.md)): moments of free homebrew games, kept with their save states in `images/scenes/`. On Linux, with GNOME Shell installed (the task prints the `apt` line when it isn't), the task takes them in a headless GNOME Shell, with GNOME's own window border where a screenshot asks for one:

```
cargo xtask screenshots [filter]    # Make the screenshots images/screenshots.toml lists, writing the ones whose pixels changed
cargo xtask screenshots --check     # Only say which differ, with pictures of the differences in target/screenshots/check/
cargo xtask scene explore "Rebound.gbc" --script "wait 5s; press start; wait 2s; mark"   # Play headless and save candidate scenes, with a contact sheet
cargo xtask scene keep target/scenes/rebound-1/01 rebound-start   # Keep a candidate in images/scenes/
cargo play rom.gb --record-scenes path/to/folder   # Play by hand; Ctrl+F12 saves the moment on screen as a scene
```

## Where Things Are

- `crates/nubo-core`: the emulator itself, a plain library with no platform dependencies.
- `crates/nubo-frontend`: everything an app needs besides drawing: settings, saves, the game library and running a game with its sound.
- `frontends/nubo`: the desktop and browser app, drawn with [Iced](https://iced.rs).
- `frontends/nubo-libretro`: the RetroArch core.
- `tools/`: the developer tools, such as the test ROM runner and the benchmark, with `crates/nubo-download`, which downloads their test data.
- `xtask/`: the web build, packaging and the other project tasks ([`xtask/AGENTS.md`](xtask/AGENTS.md) lists them all).
- `images/`: the logo and the screenshots, with the scenes the screenshots are made from in `images/scenes/`.

## Contributing

Pull requests are welcome. [`AGENTS.md`](AGENTS.md) holds the project's rules, its code style and the checks to run before you push; it is written for AI coding agents, but it is the same for people. The folders with an `AGENTS.md` of their own explain how their code fits together.
