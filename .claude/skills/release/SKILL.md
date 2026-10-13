---
name: release
description: Prepare and publish a new Nubo release - bump the version, write the AppStream release notes, open the release pull request, and after the maintainer's go merge it and start the Release workflow. Use when asked to make, prepare, cut or publish a release or a new version.
---

# Making a release

Release notes are written once, in the `<releases>` list of `frontends/iced/linux/io.github.mschoenm.nubo.metainfo.xml`, which Linux software centers and Flathub show; CI checks that its newest entry is the version in the root `Cargo.toml`.

1. **Open the release pull request.** Set `version` under `[workspace.package]` in the root `Cargo.toml` (every crate shares it; run `cargo build` to update `Cargo.lock`) and add a `<release version="..." date="YYYY-MM-DD">` with a `<description>` at the top of `<releases>`, dated the day of the release. Put the output of `cargo xtask release-notes v<version>` in the pull request's description: it shows the notes exactly as the GitHub release will, and the pull request is where they are read over, since the release is published without a draft.
2. **Wait for the maintainer's go.** Show the maintainer the whole output of `cargo xtask release-notes` in the conversation, the Install section included, not only the AppStream entry, and wait until they have read it and said to go ahead.
3. **Merge it** following the `merge` skill.
4. **Tag `main`.** Where you can push tags: `git tag v<version> origin/main && git push origin v<version>`. Cloud sessions can't push tags (GitHub answers 403), so start the Release workflow yourself instead, with the version as its `version` input (without the `v`): the GitHub MCP tool `actions_run_trigger` on `release.yml`, or `gh workflow run release.yml -f version=<version>`. It releases `main` as it is then, after checking that the tag is new, and makes the tag itself when it publishes.
5. **Watch the run.** `.github/workflows/release.yml` checks the tag against the version, runs the nightly workflow (CI and the nightly checks: `--verify-boot`, save states, the `eager` comparison, advisories), builds the Flatpaks for x86_64 and ARM and the Windows installer and zip, and publishes the GitHub release with them, in about 12 minutes (the Flatpak build is the slowest part). The release's files have no version in their names (`Nubo-linux-<arch>.flatpak`, `Nubo-windows-x64-setup.exe`, `Nubo-windows-x64.zip`), so the README's Download section can link to them in the latest release.

If a check fails, nothing is published: once the fix is merged, move the tag to it (`git tag -f v<version> origin/main && git push -f origin v<version>`) to run it again, or start a release that was started by hand again, since it only makes its tag once every check has passed (if publishing itself failed after that, re-run its failed jobs instead). The notes of a published release can still be edited on the Releases page.
