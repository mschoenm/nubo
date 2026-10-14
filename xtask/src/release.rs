//! Checks that a release is ready to tag and writes its notes for the GitHub
//! release. Release notes are written once, in the `<releases>` list of the
//! AppStream description (the metainfo file that Linux software centers and
//! Flathub show them from), and the newest entry there must be the version in
//! the root `Cargo.toml`.

use super::{APP_ID, LINUX_FILES, Result, workspace_root};
use roxmltree::{Document, Node};
use std::fs;

/// The version every crate of the workspace shares, from the root
/// `Cargo.toml`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
/// `version`, followed by how to install the release.
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

    let mut notes = markdown(description);
    notes.push_str(
        "\n## Install\n\
         \n\
         **Windows:** run `Nubo-windows-x64-setup.exe`, which installs Nubo for your \
         user and updates an installed Nubo, or unpack `Nubo-windows-x64.zip` to run it \
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

/// Whether `text` looks like an AppStream date, `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
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
            text.push_str(&format!("`{}`", text_of(child)));
        } else if child.has_tag_name("em") {
            text.push_str(&format!("*{}*", text_of(child)));
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
        assert!(notes.starts_with(
            "Faster, with a `--model` option.\n\n\
             - Save states load twice as fast\n\
             - Fixes *Pokémon* crashing\n\n## Install\n"
        ));
        assert!(notes.contains("flatpak install --user Nubo-linux-x86_64.flatpak"));
        assert!(notes.contains("`Nubo-windows-x64-setup.exe`"));
        assert!(notes.contains("`Nubo-libretro-<system>.zip`"));
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
