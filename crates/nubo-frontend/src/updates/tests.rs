//! Tests for reading GitHub's answer and comparing versions.

use super::*;

#[test]
fn the_version_comes_from_the_tag_name() {
    let answer = r#"{"url": "x", "tag_name": "v0.9.0", "name": "Nubo 0.9.0"}"#;

    assert_eq!(tag_name(answer).as_deref(), Some("0.9.0"));
    assert_eq!(tag_name("{}"), None);
}

#[test]
fn versions_compare_by_their_numbers() {
    assert!(is_newer("0.9.0", "0.8.1"));
    assert!(is_newer("0.10.0", "0.9.3"));
    assert!(!is_newer("0.8.1", "0.8.1"));
    assert!(!is_newer("0.8.0", "0.8.1"));
    assert!(!is_newer("nightly", "0.8.1"));
}

#[test]
fn the_download_page_is_told_this_computer_and_version() {
    assert!(DOWNLOAD_PAGE.starts_with("https://mschoenm.github.io/nubo/download/?for="));
    assert!(DOWNLOAD_PAGE.ends_with(&format!("&from={}", env!("CARGO_PKG_VERSION"))));
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    assert!(DOWNLOAD_PAGE.contains("?for=linux-x86_64&"));
}
