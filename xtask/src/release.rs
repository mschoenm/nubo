//! Checks that a release is ready to tag and writes its notes for the GitHub
//! release. Release notes are written once, in the `<releases>` list of the
//! AppStream description (the metainfo file that Linux software centers and
//! Flathub show them from), and the newest entry there must be the version in
//! the root `Cargo.toml`. The GitHub release shows them between a table of its
//! downloads, which links to the download page too, and how to install them.

use super::download_page::DOWNLOAD_PAGE;
use super::{APP_ID, LINUX_FILES, Result, workspace_root};
use roxmltree::{Document, Node};
use std::fmt::Write;
use std::fs;

/// The version every crate of the workspace shares, from the root
/// `Cargo.toml`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where GitHub keeps each release's files, under its tag.
const RELEASE_FILES: &str = "https://github.com/mschoenm/nubo/releases/download";

/// The web version, which always runs the newest Nubo.
const WEB_VERSION: &str = "https://mschoenm.github.io/nubo/";

/// Checks that the newest release in the metainfo file is the workspace's
/// version (and, when given, that `tag` names it, as in `v1.2.3`), and returns
/// that release's notes as Markdown for the GitHub release.
pub fn release_notes(tag: Option<&str>) -> Result<String> {
    if let Some(tag) = tag
        && tag != format!("v{VERSION}")
    {
        return Err(format!(
            "the tag {tag} doesn't match version {VERSION} in Cargo.toml; tag v{VERSION} or \
             change the version"
        ));
    }
    let path = workspace_root().join(format!("{LINUX_FILES}/{APP_ID}.metainfo.xml"));
    let metainfo = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    notes_for(&metainfo, VERSION)
}

/// The notes of the newest `<release>` in `metainfo`, which must be
/// `version`, after a table of the release's downloads and before how to
/// install them.
fn notes_for(metainfo: &str, version: &str) -> Result<String> {
    let document = Document::parse(metainfo)
        .map_err(|error| format!("the metainfo file is invalid: {error}"))?;
    let newest = document
        .descendants()
        .find(|node| node.has_tag_name("releases"))
        .and_then(|releases| {
            releases
                .children()
                .find(|node| node.has_tag_name("release"))
        })
        .ok_or_else(|| {
            format!("the metainfo file lists no releases; add a <release> for {version}")
        })?;

    let newest_version = newest.attribute("version").unwrap_or_default();
    if newest_version != version {
        return Err(format!(
            "the newest release in the metainfo file is {newest_version:?}, but Cargo.toml says \
             {version}; add a <release> for {version} at the top of <releases>"
        ));
    }
    if newest.attribute("date").is_none_or(|date| !is_date(date)) {
        return Err(format!(
            "the release {version} in the metainfo file needs a date=\"YYYY-MM-DD\""
        ));
    }
    let description = newest
        .children()
        .find(|node| node.has_tag_name("description"))
        .ok_or_else(|| {
            format!("the release {version} in the metainfo file has no <description>")
        })?;

    let mut notes = downloads(version);
    notes.push_str("\n## What's new\n\n");
    notes.push_str(&markdown(description));
    notes.push_str(
        "\n## Install\n\
         \n\
         **Windows:** run `Nubo-windows-x64.msi`, which installs Nubo for your user and \
         updates an installed Nubo, or unpack `Nubo-windows-x64.zip` to run it \
         without installing. The files aren't signed yet, so Windows may say it protected your \
         PC: choose *More info* and then *Run anyway*.\n\
         \n\
         **Linux:** download the `.flatpak` file for your computer (`x86_64` for most PCs, \
         `aarch64` for ARM) and open it with your software center, or run:\n\
         \n\
         ```bash\n\
         flatpak install --user Nubo-linux-x86_64.flatpak\n\
         ```\n\
         \n\
         Flatpak downloads the runtime Nubo needs from Flathub. To replace a Nubo you installed \
         from an earlier file or a nightly build, add `--reinstall`; your saves and settings \
         stay.\n\
         \n\
         **RetroArch:** download the core for your system (`Nubo-libretro-<system>.zip`), \
         then copy the core into RetroArch's `cores` folder and `nubo_libretro.info` into its \
         `info` folder. The README's RetroArch Core section has the details.\n",
    );
    Ok(notes)
}

/// A table with a link to each of the files the release of `version`
/// publishes (named as `release.yml` names them), and to the download page,
/// which picks the right one for the reader's computer.
fn downloads(version: &str) -> String {
    let file = |name: &str| format!("{RELEASE_FILES}/v{version}/{name}");
    format!(
        "## Download\n\
         \n\
         Not sure which file you need? The [download page]({DOWNLOAD_PAGE}) picks the right one \
         for your computer.\n\
         \n\
         | System | Download |\n\
         | ------ | -------- |\n\
         | Windows | [Installer]({}) · [Zip]({}) (runs without installing) |\n\
         | Linux | [Flatpak for x86_64]({}) (most PCs) · [Flatpak for ARM]({}) |\n\
         | Browser | [Play in your browser]({WEB_VERSION}), nothing to install |\n\
         | RetroArch | The core for [Windows]({}), [macOS]({}), [Linux x86_64]({}) and \
         [Linux ARM]({}) |\n",
        file("Nubo-windows-x64.msi"),
        file("Nubo-windows-x64.zip"),
        file("Nubo-linux-x86_64.flatpak"),
        file("Nubo-linux-aarch64.flatpak"),
        file("Nubo-libretro-windows-x64.zip"),
        file("Nubo-libretro-macos-universal.zip"),
        file("Nubo-libretro-linux-x86_64.zip"),
        file("Nubo-libretro-linux-aarch64.zip"),
    )
}

/// Reads a YYYY-MM-DD date given on the command line, such as the value of
/// `cargo xtask windows --nightly`.
pub(crate) fn parse_date(text: &str) -> Result<String> {
    if is_date(text) {
        Ok(text.to_owned())
    } else {
        Err(String::from("expected a YYYY-MM-DD date"))
    }
}

/// Whether `text` looks like an AppStream date, `YYYY-MM-DD`.
pub(crate) fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// An AppStream `<description>` as Markdown: paragraphs, and lists with one
/// item per line.
fn markdown(description: Node) -> String {
    let mut blocks = Vec::new();
    for element in description.children().filter(Node::is_element) {
        match element.tag_name().name() {
            "ul" | "ol" => {
                let ordered = element.has_tag_name("ol");
                let items: Vec<String> = element
                    .children()
                    .filter(|node| node.has_tag_name("li"))
                    .enumerate()
                    .map(|(index, item)| {
                        let marker = if ordered {
                            format!("{}.", index + 1)
                        } else {
                            String::from("-")
                        };
                        format!("{marker} {}", text_of(item))
                    })
                    .collect();
                blocks.push(items.join("\n"));
            }
            _ => blocks.push(text_of(element)),
        }
    }
    let mut markdown = blocks.join("\n\n");
    markdown.push('\n');
    markdown
}

/// The text inside `node` with its line breaks and indentation collapsed to
/// single spaces, keeping `<code>` as Markdown code and `<em>` as emphasis.
fn text_of(node: Node) -> String {
    let mut text = String::new();
    for child in node.children() {
        if child.is_text() {
            text.push_str(child.text().unwrap_or_default());
        } else if child.has_tag_name("code") {
            let _ = write!(text, "`{}`", text_of(child));
        } else if child.has_tag_name("em") {
            let _ = write!(text, "*{}*", text_of(child));
        } else {
            text.push_str(&text_of(child));
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const METAINFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.mschoenm.nubo</id>
  <releases>
    <release version="0.2.0" date="2026-11-01">
      <description>
        <p>
          Faster, with a
          <code>--model</code> option.
        </p>
        <ul>
          <li>Save states load
            twice as fast</li>
          <li>Fixes <em>Pokémon</em> crashing</li>
        </ul>
      </description>
    </release>
    <release version="0.1.0" date="2026-10-12">
      <description><p>The first release.</p></description>
    </release>
  </releases>
</component>"#;

    #[test]
    fn the_newest_release_becomes_markdown_with_install_instructions() {
        let notes = notes_for(METAINFO, "0.2.0").unwrap();
        assert!(notes.starts_with("## Download\n"));
        assert!(notes.contains(
            "\n## What's new\n\n\
             Faster, with a `--model` option.\n\n\
             - Save states load twice as fast\n\
             - Fixes *Pokémon* crashing\n\n## Install\n"
        ));
        assert!(notes.contains("flatpak install --user Nubo-linux-x86_64.flatpak"));
        assert!(notes.contains("`Nubo-windows-x64.msi`"));
        assert!(notes.contains("`Nubo-libretro-<system>.zip`"));
    }

    #[test]
    fn the_download_table_links_to_this_release_and_the_download_page() {
        let notes = notes_for(METAINFO, "0.2.0").unwrap();
        assert!(notes.contains(
            "[Installer](https://github.com/mschoenm/nubo/releases/download/v0.2.0/\
             Nubo-windows-x64.msi)"
        ));
        assert!(notes.contains(
            "[Linux ARM](https://github.com/mschoenm/nubo/releases/download/v0.2.0/\
             Nubo-libretro-linux-aarch64.zip)"
        ));
        assert!(notes.contains("[download page](https://mschoenm.github.io/nubo/download/)"));
    }

    #[test]
    fn a_version_that_isnt_the_newest_release_is_refused() {
        let error = notes_for(METAINFO, "0.1.0").unwrap_err();
        assert!(error.contains("the newest release in the metainfo file is \"0.2.0\""));
        assert!(notes_for(METAINFO, "0.3.0").is_err());
    }

    #[test]
    fn a_release_without_a_valid_date_is_refused() {
        let metainfo = METAINFO.replace("2026-11-01", "1 November");
        assert!(
            notes_for(&metainfo, "0.2.0")
                .unwrap_err()
                .contains("needs a date")
        );
    }

    #[test]
    fn metainfo_without_releases_is_refused() {
        let metainfo = "<component><id>io.github.mschoenm.nubo</id></component>";
        assert!(
            notes_for(metainfo, "0.1.0")
                .unwrap_err()
                .contains("lists no releases")
        );
    }

    #[test]
    fn the_metainfo_file_in_the_repository_matches_the_version() {
        release_notes(None).unwrap();
        release_notes(Some(&format!("v{VERSION}"))).unwrap();
        assert!(release_notes(Some("v0.0.0-not-this")).is_err());
    }
}
