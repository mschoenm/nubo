//! Downloading files over HTTPS, for the tasks that fetch test data.

use crate::Result;
use std::fs;
use std::io;
use std::path::Path;

/// Downloads `url` to `destination`, finding a proxy and trusted
/// certificates the way the system's own tools do, from the `HTTPS_PROXY`
/// variable and the system's certificate store.
pub fn download(url: &str, destination: &Path) -> Result {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build(),
    );
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
