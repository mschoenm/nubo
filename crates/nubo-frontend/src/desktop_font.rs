//! The font the player picked for their desktop, so an app can draw its text
//! in it. Only Linux has a way to ask: the desktop portal (the service that
//! lets apps, Flatpak ones too, read the desktop's settings) tells GNOME's
//! interface font or KDE's general font. Elsewhere, and when no portal
//! answers, the desktop names no font and the app keeps its toolkit's.

/// The names the desktop's font may have as a font family, the likeliest
/// first, or none when the desktop names no font. The app takes the first
/// one that is installed (see `family_names`).
pub fn families() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        portal::font()
            .map(|font| family_names(&font))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// The family names a desktop's font setting may mean, the longest first.
/// GNOME names a font with its style and size (`Noto Sans Medium 11`), and
/// KDE with its size and other numbers after commas (`Noto Sans,10,-1,5`).
/// The size goes, and what's left gives every name from the whole of it to
/// its first word: `Noto Sans Medium`, `Noto Sans`, `Noto`.
fn family_names(font: &str) -> Vec<String> {
    let family = font.split(',').next().unwrap_or_default();
    let mut words: Vec<&str> = family.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|word| word.trim_end_matches("px").parse::<f32>().is_ok())
    {
        words.pop();
    }
    (1..=words.len())
        .rev()
        .map(|count| words[..count].join(" "))
        .collect()
}

/// Reading the font from the desktop portal over D-Bus.
#[cfg(target_os = "linux")]
mod portal {
    use std::time::Duration;
    use zbus::blocking::connection;
    use zbus::zvariant::Value;

    /// The settings that name the desktop's font, as a namespace and a key
    /// of the portal's Settings interface: GNOME's, then KDE's.
    const FONT_SETTINGS: [(&str, &str); 2] = [
        ("org.gnome.desktop.interface", "font-name"),
        ("org.kde.kdeglobals.General", "font"),
    ];
    /// How long the portal may take to answer, since the app waits for it
    /// before it opens its window.
    const TIMEOUT: Duration = Duration::from_secs(1);

    /// The desktop's font setting as the portal gives it, or `None` when
    /// there is no portal or it names no font.
    pub fn font() -> Option<String> {
        let connection = connection::Builder::session()
            .ok()?
            .method_timeout(TIMEOUT)
            .build()
            .ok()?;
        FONT_SETTINGS.iter().find_map(|(namespace, key)| {
            let reply = connection
                .call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop",
                    Some("org.freedesktop.portal.Settings"),
                    "Read",
                    &(namespace, key),
                )
                .ok()?;
            let body = reply.body();
            let mut value: Value<'_> = body.deserialize().ok()?;
            // `Read` wraps the setting in a second variant; the newer
            // `ReadOne` doesn't, but older portals lack it.
            while let Value::Value(inner) = value {
                value = *inner;
            }
            String::try_from(&value)
                .ok()
                .filter(|font| !font.is_empty())
        })
    }
}

#[cfg(test)]
mod tests;
