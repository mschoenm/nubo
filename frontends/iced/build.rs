//! Gives the Windows executable the Nubo logo as its icon and the app's name
//! and version in its file properties, which Explorer, the taskbar and the
//! installer show. Does nothing when building for other platforms.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("nubo.ico");
    fs::write(&icon, nubo_logo::ico()).expect("failed to write the icon");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(
            icon.to_str()
                .expect("the build folder's path is valid Unicode"),
        )
        .set("ProductName", "Nubo")
        .set("FileDescription", "Nubo")
        .set("LegalCopyright", "GPL-3.0-or-later");
    // The resource compiler comes with the Windows SDK. Without it the app
    // still builds, only without its icon.
    if let Err(error) = resource.compile() {
        println!("cargo::warning=nubo.exe is built without its icon: {error}");
    }
}
