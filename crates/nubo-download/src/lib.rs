//! Downloads test data over HTTPS and checks it, for Nubo's developer tools:
//! the test ROM runner, the CPU test runner and xtask. Nothing players get
//! uses it. The HTTP client finds a proxy and trusted certificates the way
//! the system's own tools do, from the `HTTPS_PROXY` variable and the
//! system's certificate store, so it works behind the proxies of CI and
//! cloud machines.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// How many files [`download_all`] downloads at the same time.
const PARALLEL_DOWNLOADS: usize = 16;

/// An HTTP client that finds a proxy and trusted certificates the way the
/// system's own tools do.
#[must_use]
pub fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build(),
    )
}

/// Downloads `url` to `destination` with `agent`.
///
/// # Errors
///
/// When the download fails or `destination` can't be written.
pub fn download_with(agent: &ureq::Agent, url: &str, destination: &Path) -> Result<(), String> {
    let response = agent
        .get(url)
        .call()
        .map_err(|error| format!("failed to download {url}: {error}"))?;
    let mut file = fs::File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    io::copy(&mut response.into_body().into_reader(), &mut file)
        .map_err(|error| format!("failed to download {url}: {error}"))?;
    Ok(())
}

/// Downloads `url` to `destination`.
///
/// # Errors
///
/// When the download fails or `destination` can't be written.
pub fn download(url: &str, destination: &Path) -> Result<(), String> {
    download_with(&agent(), url, destination)
}

/// Downloads each URL to its destination, several at a time, and stops at
/// the first that fails.
///
/// # Errors
///
/// The first download that failed.
pub fn download_all(downloads: &[(PathBuf, String)]) -> Result<(), String> {
    let agent = agent();
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..PARALLEL_DOWNLOADS.min(downloads.len()))
            .map(|_| {
                scope.spawn(|| {
                    while let Some((destination, url)) =
                        downloads.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        download_with(&agent, url, destination)?;
                    }
                    Ok(())
                })
            })
            .collect();
        workers
            .into_iter()
            .try_for_each(|worker| worker.join().expect("a download thread panicked"))
    })
}

/// The SHA-256 checksum of `bytes` in lowercase hexadecimal.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether the file at `path` exists and has the SHA-256 checksum
/// `expected`, in lowercase hexadecimal.
#[must_use]
pub fn has_checksum(path: &Path, expected: &str) -> bool {
    fs::read(path).is_ok_and(|bytes| sha256_hex(&bytes) == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_are_lowercase_hexadecimal_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_missing_file_has_no_checksum() {
        assert!(!has_checksum(Path::new("/no/such/file"), &sha256_hex(b"")));
    }

    #[test]
    fn nothing_to_download_downloads_nothing() {
        assert_eq!(download_all(&[]), Ok(()));
    }
}
