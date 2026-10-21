//! Finding out whether a newer Nubo is out, from the newest release on
//! GitHub, so an app can say so and offer the download for this computer
//! (`DOWNLOAD_PAGE`). Only the desktop build looks (the
//! `desktop` feature, with `ureq`), and only once the player said yes to it:
//! looking tells GitHub that someone runs Nubo. The browser always runs the
//! newest version.

/// Whether this build can look for a newer version: only the desktop build
/// can.
pub const AVAILABLE: bool = cfg!(feature = "desktop");

/// This computer as the download page names it, or nothing when no download
/// is made for it, so the page guesses from the browser.
#[cfg(target_os = "windows")]
macro_rules! platform {
    () => {
        "windows-x64"
    };
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
macro_rules! platform {
    () => {
        "linux-x86_64"
    };
}
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
macro_rules! platform {
    () => {
        "linux-aarch64"
    };
}
#[cfg(target_os = "macos")]
macro_rules! platform {
    () => {
        "macos"
    };
}
#[cfg(not(any(
    target_os = "windows",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    target_os = "macos"
)))]
macro_rules! platform {
    () => {
        ""
    };
}

/// Nubo's download page, which puts the download for this computer first:
/// the address names this computer (`for`) and the version that runs
/// (`from`), so the page can also say how to update it. The page itself is
/// `frontends/nubo/web/download/index.html`.
pub const DOWNLOAD_PAGE: &str = concat!(
    "https://mschoenm.github.io/nubo/download/?for=",
    platform!(),
    "&from=",
    env!("CARGO_PKG_VERSION")
);

/// The newest release's version, such as "0.9.0", or why it couldn't be
/// found out. It waits for the network, so apps run it with
/// `background::run`.
pub fn latest_version() -> Result<String, String> {
    #[cfg(feature = "desktop")]
    {
        use std::io::Read;
        use std::time::Duration;

        let api = env!("CARGO_PKG_REPOSITORY")
            .replace("https://github.com/", "https://api.github.com/repos/");
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(15)))
                .user_agent(concat!("Nubo/", env!("CARGO_PKG_VERSION")))
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                        .build(),
                )
                .build(),
        );
        let mut body = String::new();
        agent
            .get(format!("{api}/releases/latest"))
            .call()
            .map_err(|error| error.to_string())?
            .into_body()
            .into_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body)
            .map_err(|error| error.to_string())?;
        tag_name(&body).ok_or_else(|| String::from("the answer named no version"))
    }
    #[cfg(not(feature = "desktop"))]
    {
        Err(String::from("this build doesn't look for new versions"))
    }
}

/// The version in the `tag_name` of GitHub's answer about a release, such as
/// "0.9.0" for `"tag_name": "v0.9.0"`.
#[cfg(any(feature = "desktop", test))]
fn tag_name(answer: &str) -> Option<String> {
    let after = &answer[answer.find("\"tag_name\"")? + "\"tag_name\"".len()..];
    let start = after.find('"')? + 1;
    let length = after[start..].find('"')?;
    Some(
        after[start..start + length]
            .trim_start_matches('v')
            .to_owned(),
    )
}

/// Whether `latest` is a newer version than `current`, both like "0.8.1".
/// A version that isn't numbers and dots, such as a nightly build's, never
/// counts as newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let numbers = |version: &str| -> Option<Vec<u64>> {
        version.split('.').map(|part| part.parse().ok()).collect()
    };
    match (numbers(latest), numbers(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
