//! Tests for reading the Flathub runtime listing and the SDK's Rust check.

use super::{check_rust_version_command, newest_runtime, runtime_release};

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
