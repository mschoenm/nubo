use super::{
    check_rust_version_command, file_source, git_source, manifest_contents, newest_runtime,
    runtime_release,
};

#[test]
fn the_newest_runtime_is_the_highest_release_of_the_freedesktop_platform() {
    let listing = "\
org.freedesktop.Platform\t24.08
org.freedesktop.Platform\t26.08
org.freedesktop.Platform\t1.6
org.freedesktop.Platform\t25.08
org.freedesktop.Platform.GL.default\t27.08
org.freedesktop.Sdk\t27.08
org.gnome.Platform\t49
";
    assert_eq!(newest_runtime(listing).as_deref(), Some("26.08"));
}

#[test]
fn branches_that_are_not_releases_are_left_out() {
    let listing = "org.freedesktop.Platform    stable\norg.freedesktop.Platform    25.08\n";
    assert_eq!(newest_runtime(listing).as_deref(), Some("25.08"));
    assert_eq!(newest_runtime("org.gnome.Platform\t49\n"), None);
    assert_eq!(runtime_release("26.08"), Some((26, 8)));
    assert_eq!(runtime_release("stable"), None);
}

#[test]
fn the_rust_version_check_compares_major_and_minor_versions() {
    let command = check_rust_version_command("1.98");
    assert!(command.contains("= 1.98 ]"), "{command}");
    let command = check_rust_version_command("1.98.1");
    assert!(command.contains("= 1.98 ]"), "{command}");
}

#[test]
fn flathub_builds_the_tagged_commit_from_github() {
    let source = git_source("0123abcd", Some("v1.2.3"));
    assert_eq!(source["type"], "git");
    assert_eq!(source["url"], "https://github.com/mschoenm/nubo.git");
    assert_eq!(source["tag"], "v1.2.3");
    assert_eq!(source["commit"], "0123abcd");
    assert!(git_source("0123abcd", None).get("tag").is_none());
}

#[test]
fn only_the_local_build_checks_the_rust_version() {
    let commands = |rust_version| {
        manifest_contents(git_source("0123abcd", None), Vec::new(), rust_version)["modules"][0]
            ["build-commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command.as_str().unwrap().starts_with("rustc --version"))
    };
    assert!(commands(Some("1.98")));
    assert!(!commands(None));
}

#[test]
fn every_generated_file_is_a_source_of_its_own_after_nubo_and_before_the_crates() {
    let files = vec![
        file_source("flatpak-files", "io.github.mschoenm.nubo.svg"),
        file_source("flatpak-files/licenses", "LICENSE.txt"),
    ];
    let manifest = manifest_contents(git_source("0123abcd", None), files, None);
    let sources = manifest["modules"][0]["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 4);
    assert_eq!(sources[0]["type"], "git");
    assert_eq!(
        sources[2],
        serde_json::json!({
            "type": "file",
            "path": "flatpak-files/licenses/LICENSE.txt",
            "dest": "flatpak-files/licenses",
        })
    );
    assert_eq!(sources[3], "cargo-sources.json");
}
