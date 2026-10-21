---
name: screenshots
description: Regenerate Nubo's screenshots (the README's and the Flathub listing's) from their recorded scenes and update the docs, or collect new screenshot candidates by playing games headless and show them as a gallery to pick from. Use when asked to regenerate, refresh, update, retake or collect screenshots.
---

# Screenshots

Every screenshot in the README and the Flathub listing is made from a scene: a recorded moment of a game (its save state, what is open on screen, the settings) in `images/scenes/`. `images/screenshots.toml` lists the screenshots, and `cargo xtask screenshots` makes them again with the newest Nubo in a headless GNOME Shell, so it runs in a cloud session too. How it all works: `docs/SCREENSHOTS_PLAN.md`, `xtask/AGENTS.md` and the doc comments of `xtask/src/screenshots.rs`, `xtask/src/scene.rs` and `crates/nubo-frontend/src/scenes.rs`.

Install what the task needs first, once per machine (it prints this line when something is missing; leave out `sudo` as root):

```bash
sudo apt-get install --no-install-recommends gnome-shell xwayland xdotool dbus-daemon libglib2.0-bin xdg-desktop-portal xdg-desktop-portal-gnome fonts-cantarell adwaita-icon-theme
```

## Regenerate the screenshots and update the docs

1. Run `cargo xtask screenshots`. It writes only the screenshots whose pixels changed and says which. Nothing changed means nothing to do: say so and stop.
2. Show the maintainer each changed screenshot (send the files, or put them side by side with the old ones from `git show HEAD:<file>` in one page), and say what changed in the app that explains it.
3. Check the docs that show or describe them: the README (its images, their `alt` texts and captions), the Flathub metainfo (`frontends/nubo/linux/io.github.mschoenm.nubo.metainfo.xml`: its screenshot and the description's credit), and the README's credits, which must name every game a screenshot shows (`cargo test -p xtask` checks the titles).
4. Open a pull request with the new pictures and the doc changes, and see it through as `AGENTS.md` says. Don't merge it until the maintainer asks.

## Collect new screenshots for the maintainer to pick from

1. **Pick games.** Use the games in `images/scenes/games.toml`, or add freely licensed homebrew ones whose licenses let anyone share pictures of them (the gbdev Homebrew Hub database at a fixed commit, as the others; note each game's license and authors). Never commercial games.
2. **Explore.** For each game, play it headless and save candidates:
   ```bash
   cargo xtask scene explore "Moki the Monster Kid.gb" --script "wait 6s; press start; wait 3s; press a; wait 2s; mark" --every 2s
   ```
   The script language is in `xtask/src/scene/script.rs` (`wait`, `press`, `hold`, `release`, `mark`); `--model` picks the hardware (`sgb2` shows a Super Game Boy border). Look at the contact sheet it writes, then go on from a promising candidate with `cargo xtask scene explore target/scenes/<game>-<n>/<candidate> --script ...`. Aim for moments that show the game well: gameplay rather than menus, nothing half-faded, characters in view. Later levels need a person: say which moments would need the maintainer to record them with `nubo --record-scenes <folder>` and Ctrl+F12.
3. **Render the best.** Keep the good candidates as scenes (`cargo xtask scene keep <candidate> <name>`), add them to `images/screenshots.toml` with the frames, settings and variants that show them best (light and dark, the Pocket colors, a GNOME border for the Flathub listing at most 1000×700, the pause menu or a settings tab with `pause_menu` and `settings_tab`), and run `cargo xtask screenshots`.
4. **Show a gallery.** Put every rendered candidate in one page (an Artifact) with its name, game and settings, so the maintainer can pick. Don't change the README yet.
5. **Commit the picks.** Once the maintainer has picked, keep only the chosen scenes and their `images/screenshots.toml` entries, replace the README's and the metainfo's images, captions and `alt` texts, credit every game in the README (title, authors, license) and in the metainfo's description when the Flathub screenshot shows it, and open a pull request.

## Keep in mind

- Scenes and screenshots carry no text of their own; captions stay in the README.
- A scene's save state is kept like a player's: later versions must load it (the save state rule in `AGENTS.md` covers it). Don't edit `game.state` files.
- The screenshots aren't made in CI; this task is the only way they change.
