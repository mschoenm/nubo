//! The download page, for players: it puts the download for the visitor's
//! computer first, with every other download below it. It is published with
//! the web version, at `download/` under the web version's address
//! (`DOWNLOAD_PAGE`), and the app's new-version buttons and the release notes
//! link to it. The page is one file, `frontends/nubo/web/download/index.html`;
//! the pictures it shows come from `images/` and are copied next to it, each
//! one the page names.

use super::{Result, workspace_root};
use std::fs;
use std::path::{Component, Path};

/// The download page's address once published.
pub const DOWNLOAD_PAGE: &str = "https://mschoenm.github.io/nubo/download/";

/// The page in the repository, from the workspace root.
const PAGE_FILE: &str = "frontends/nubo/web/download/index.html";

/// Writes the download page and the pictures it shows into `download/` in
/// `output_dir`, and fails when the page names a picture that isn't in
/// `images/`.
pub fn write_download_page(output_dir: &Path) -> Result {
    let source = workspace_root().join(PAGE_FILE);
    let page = fs::read_to_string(&source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
    let folder = output_dir.join("download");
    for picture in pictures(&page)? {
        let from = workspace_root().join(picture);
        let to = folder.join(picture);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::copy(&from, &to).map_err(|error| {
            format!("the download page shows {picture}, which can't be copied: {error}")
        })?;
    }
    fs::write(folder.join("index.html"), page)
        .map_err(|error| format!("failed to write the download page: {error}"))
}

/// The files under `images/` that `page` names, each once, such as
/// `images/icon.svg`, whether by a path next to the page or by the full
/// address it has once published (for link previews). A path that would
/// leave `images/` is refused.
fn pictures(page: &str) -> Result<Vec<&str>> {
    let mut found: Vec<&str> = page
        .split('"')
        .map(|part| part.strip_prefix(DOWNLOAD_PAGE).unwrap_or(part))
        .filter(|part| part.starts_with("images/"))
        .collect();
    if let Some(outside) = found.iter().find(|picture| {
        !Path::new(picture)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    }) {
        return Err(format!(
            "the download page names {outside}, which isn't a file in images/"
        ));
    }
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_found_by_their_path_and_their_published_address() {
        let page = r#"<img src="images/icon.svg" /><meta content="https://mschoenm.github.io/nubo/download/images/screenshots/a.png" /><img src="images/icon.svg" /><a href="../">"#;
        assert_eq!(
            pictures(page).unwrap(),
            ["images/icon.svg", "images/screenshots/a.png"]
        );
    }

    #[test]
    fn a_picture_outside_images_is_refused() {
        assert!(pictures(r#"<img src="images/../Cargo.toml" />"#).is_err());
    }

    #[test]
    fn every_picture_the_page_shows_is_in_the_repository() {
        let page = fs::read_to_string(workspace_root().join(PAGE_FILE)).unwrap();
        let pictures = pictures(&page).unwrap();
        assert!(pictures.contains(&"images/icon.svg"));
        for picture in pictures {
            assert!(
                workspace_root().join(picture).is_file(),
                "the download page shows {picture}, which isn't there"
            );
        }
    }
}
