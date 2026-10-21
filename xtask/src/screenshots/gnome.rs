//! Taking the app's pictures in a headless GNOME Shell, so a screenshot can
//! show the window with GNOME's own border and shadow, in a container
//! without a screen too, such as a cloud session.
//!
//! [`run_plan`] starts xtask again as `cargo xtask gnome-session` (hidden)
//! on a D-Bus session bus of its own (`dbus-run-session`). As root, as in a
//! container, it does so in a mount namespace of its own (`unshare`), with
//! a system bus of its own and empty `/run/systemd`, `/run/dbus` and
//! `/tmp/.X11-unix` folders: GNOME Shell then runs without logind, which a
//! container lacks, and nothing of the container's is changed. As another user, as on a
//! Linux desktop, the system's own system bus is used.
//!
//! [`run_session`] then starts GNOME Shell for each screen size and style
//! the pictures need (`--headless --virtual-monitor`, with `--unsafe-mode`
//! so its screenshot and `Eval` D-Bus calls are allowed), with its folders
//! in the work folder so nothing of the user's is read or changed. The app
//! runs as an X11 app through XWayland, since GNOME only draws its own
//! border around X11 windows (a Wayland app draws its own, which for Nubo
//! is winit's, and GNOME's window screenshot leaves that out). GTK draws
//! that border, dark with `GTK_THEME=Adwaita:dark`. GNOME 46 draws it with
//! plain GTK 4, whose title bar is beige with bare buttons, while GNOME's
//! own apps and newer versions of GNOME draw libadwaita's: the window's
//! color with round buttons. A stylesheet in the session's GTK settings
//! folder ([`TITLE_BAR_STYLE`]) gives the border libadwaita's look, so the
//! pictures look like GNOME on a desktop today. The app draws its text in
//! the font the desktop names through the desktop portal, which D-Bus
//! starts when the app first asks: the session hands the portal its
//! display and folders, as GNOME's session manager does on a desktop, and
//! a `gnome-portals.conf` of its own ([`PORTALS`]) has GNOME's part of the
//! portal answer. GNOME's settings are their defaults, so the font is
//! GNOME's own, Cantarell, as in the title bar. Before each picture
//! the X pointer goes to the screen's bottom-right corner (with xdotool),
//! so it hovers over nothing in the window: an X server starts with its
//! pointer in the middle of the screen. Each picture is then either taken
//! by the app itself (`capture --output`), or by GNOME once the app says
//! the scene is ready (`ScreenshotWindow`, with the border and shadow).

use crate::{Result, on_path, run};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The programs the GNOME session needs, with the Ubuntu package of each.
const TOOLS: [(&str, &str); 6] = [
    ("gnome-shell", "gnome-shell"),
    ("Xwayland", "xwayland"),
    ("xdotool", "xdotool"),
    ("gdbus", "libglib2.0-bin"),
    ("dbus-run-session", "dbus-daemon"),
    ("dbus-daemon", "dbus-daemon"),
];
/// The D-Bus services the session needs, as files in D-Bus's folder of
/// services, with the Ubuntu package of each: the desktop portal and
/// GNOME's part of it, which name GNOME's font to the app.
const SERVICES: [(&str, &str); 2] = [
    (
        "org.freedesktop.portal.Desktop.service",
        "xdg-desktop-portal",
    ),
    (
        "org.freedesktop.impl.portal.desktop.gnome.service",
        "xdg-desktop-portal-gnome",
    ),
];
/// Where D-Bus looks for the [`SERVICES`] it starts on request.
const SERVICES_FOLDER: &str = "/usr/share/dbus-1/services";
/// What else GNOME's border needs to look as it does on a desktop: the
/// title's font and the icons of its buttons.
const LOOKS: [&str; 2] = ["fonts-cantarell", "adwaita-icon-theme"];
/// The stylesheet that gives GNOME's border libadwaita's title bar (see
/// the module's documentation): `{background}` and `{foreground}` are the
/// light or dark colors of [`title_bar_style`]. GNOME draws the border of
/// an X11 window with GTK 4 (`mutter-x11-frames`), which reads it from
/// `gtk-4.0/gtk.css` in `XDG_CONFIG_HOME`.
const TITLE_BAR_STYLE: &str = "\
headerbar, headerbar:backdrop {
  background: {background};
  color: {foreground};
  box-shadow: none;
  border: none;
}
headerbar:backdrop .title { color: {foreground}; }
windowcontrols button, windowcontrols button:hover, windowcontrols button:backdrop {
  background: none;
  border: none;
  box-shadow: none;
  padding: 0;
  margin: 0 5px;
  min-width: 24px;
  min-height: 24px;
  color: {foreground};
}
windowcontrols button > image {
  border-radius: 100%;
  padding: 4px;
  background-color: alpha(currentColor, 0.1);
}
";
/// Which programs answer for the desktop portal in GNOME, as GNOME's own
/// `gnome-portals.conf` says, which Ubuntu 24.04 doesn't install: GNOME's,
/// then GTK's. The portal reads it from `xdg-desktop-portal` in
/// `XDG_CONFIG_HOME`.
const PORTALS: &str = "[preferred]\ndefault=gnome;gtk;\n";
/// How long GNOME Shell may take to start.
const SHELL_START: Duration = Duration::from_secs(60);
/// How long the app may take to show a scene and take its picture, which
/// for a scene of the library includes capturing its title screens.
const CAPTURE_TIME: Duration = Duration::from_secs(180);
/// What the namespace runs as root before starting xtask again on a
/// session bus of its own: empty folders over the container's
/// `/run/systemd`, `/run/dbus` and `/tmp/.X11-unix`, and a system bus,
/// which it stops once the session ends, failed or not (the namespace only
/// isolates mounts, so the bus would outlive it).
const NAMESPACE_SCRIPT: &str = "set -e; \
    for folder in /run/systemd /run/dbus /tmp/.X11-unix; do mkdir -p $folder; done; \
    mount -t tmpfs none /run/systemd; mount -t tmpfs none /run/dbus; \
    mount -t tmpfs -o mode=1777 none /tmp/.X11-unix; \
    bus=$(dbus-daemon --system --fork --print-pid); \
    status=0; dbus-run-session -- \"$@\" || status=$?; \
    kill \"$bus\" 2>/dev/null || true; \
    exit $status";

/// Every picture to take, in the order they were added.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    captures: Vec<Capture>,
}

/// One picture: the app's `capture` command and how to take its picture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    /// The app's executable.
    pub app: PathBuf,
    /// The app's arguments, starting with `capture`.
    pub arguments: Vec<String>,
    /// Whether GNOME takes the picture, with the border; otherwise the app
    /// saves it itself (its arguments then hold `--output`).
    pub framed: bool,
    /// Where the picture goes.
    pub output: PathBuf,
    /// Whether GNOME's style is dark.
    pub dark: bool,
    /// The size of the screen.
    pub monitor: (u32, u32),
}

impl Plan {
    /// Adds a picture to take.
    pub fn add(&mut self, capture: Capture) {
        self.captures.push(capture);
    }

    /// How many pictures there are to take.
    pub fn len(&self) -> usize {
        self.captures.len()
    }
}

/// Checks that this is Linux and that the programs the GNOME session needs
/// are installed, or says how to install them.
pub fn check_tools() -> Result {
    if !cfg!(target_os = "linux") {
        return Err(String::from(
            "the screenshots are taken in a headless GNOME Shell, so only on Linux",
        ));
    }
    let mut missing: Vec<&str> = TOOLS
        .iter()
        .filter(|(program, _)| !on_path(program))
        .map(|&(_, package)| package)
        .chain(
            SERVICES
                .iter()
                .filter(|(service, _)| !Path::new(SERVICES_FOLDER).join(service).exists())
                .map(|&(_, package)| package),
        )
        .collect();
    if is_root() && !on_path("unshare") {
        missing.push("util-linux");
    }
    if missing.is_empty() {
        return Ok(());
    }
    missing.dedup();
    let sudo = if is_root() { "" } else { "sudo " };
    Err(format!(
        "the screenshots need GNOME Shell and a few tools; on Ubuntu install them with\n  \
         {sudo}apt-get install --no-install-recommends {} {}",
        missing.join(" "),
        LOOKS.join(" "),
    ))
}

/// Takes every picture of `plan` in headless GNOME Shells (see the
/// module's documentation), keeping their files in `work`.
pub fn run_plan(plan: &Plan, work: &Path) -> Result {
    let plan_path = work.join("plan.json");
    fs::create_dir_all(work)
        .map_err(|error| format!("couldn't create {}: {error}", work.display()))?;
    let text = serde_json::to_string_pretty(plan).map_err(|error| error.to_string())?;
    fs::write(&plan_path, text)
        .map_err(|error| format!("couldn't write {}: {error}", plan_path.display()))?;

    let xtask = env::current_exe().map_err(|error| format!("couldn't find xtask: {error}"))?;
    let inner = [
        xtask.into_os_string(),
        "gnome-session".into(),
        plan_path.into_os_string(),
    ];
    let mut command = if is_root() {
        let mut command = Command::new("unshare");
        command.args([
            "--mount",
            "--fork",
            "--",
            "sh",
            "-c",
            NAMESPACE_SCRIPT,
            "sh",
        ]);
        command
    } else {
        let mut command = Command::new("dbus-run-session");
        command.arg("--");
        command
    };
    // The buses and GNOME's services report a lot that doesn't matter; it
    // is shown only when something fails.
    let log_path = work.join("session.log");
    let log = File::create(&log_path)
        .map_err(|error| format!("couldn't create {}: {error}", log_path.display()))?;
    let result = run(command.args(inner).stderr(log));
    result.map_err(|error| {
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let lines: Vec<&str> = log
            .lines()
            .filter(|line| !line.starts_with("dbus-daemon["))
            .collect();
        format!(
            "{error}\n{}",
            lines[lines.len().saturating_sub(30)..].join("\n")
        )
    })
}

/// `cargo xtask gnome-session <plan>`: takes the plan's pictures, one GNOME
/// Shell for each screen size and style (see the module's documentation).
/// It runs on the session bus [`run_plan`] started.
pub fn run_session(plan_path: &Path) -> Result {
    let text = fs::read_to_string(plan_path)
        .map_err(|error| format!("couldn't read {}: {error}", plan_path.display()))?;
    let plan: Plan = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let work = plan_path.parent().unwrap_or(Path::new(".")).join("gnome");
    // A session's folders must start empty: an XWayland authority file left
    // from an earlier run would be found instead of the new one.
    if work.exists() {
        fs::remove_dir_all(&work)
            .map_err(|error| format!("couldn't clear {}: {error}", work.display()))?;
    }

    let mut groups: BTreeMap<((u32, u32), bool), Vec<&Capture>> = BTreeMap::new();
    for capture in &plan.captures {
        groups
            .entry((capture.monitor, capture.dark))
            .or_default()
            .push(capture);
    }

    for (number, ((monitor, dark), captures)) in groups.into_iter().enumerate() {
        let session = Session::start(monitor, dark, &work.join(number.to_string()))?;
        let result = captures.iter().try_for_each(|capture| {
            println!("  {}", capture.output.display());
            session.take(capture)
        });
        session.stop();
        result?;
    }
    Ok(())
}

/// A headless GNOME Shell and what its apps need to reach it.
struct Session {
    /// The running `gnome-shell`.
    shell: Child,
    /// The variables that point at this session's folders, display and
    /// screen.
    environment: Vec<(String, String)>,
    /// The width and height of its virtual screen, in pixels.
    monitor: (u32, u32),
    /// The file GNOME Shell's output goes to, to say why it failed.
    log: PathBuf,
}

impl Session {
    /// Starts GNOME Shell on a `monitor`-sized screen, in the dark style or
    /// not, with its folders in `folder`, and waits until it answers.
    fn start(monitor: (u32, u32), dark: bool, folder: &Path) -> Result<Session> {
        let runtime = folder.join("runtime");
        fs::create_dir_all(&runtime)
            .map_err(|error| format!("couldn't create {}: {error}", runtime.display()))?;
        restrict_to_owner(&runtime)?;
        let mut environment: Vec<(String, String)> = [
            ("XDG_RUNTIME_DIR", runtime.clone()),
            ("XDG_CONFIG_HOME", folder.join("config")),
            ("XDG_DATA_HOME", folder.join("data")),
            ("XDG_CACHE_HOME", folder.join("cache")),
            ("XDG_STATE_HOME", folder.join("state")),
        ]
        .into_iter()
        .map(|(name, path)| (name.to_owned(), path.display().to_string()))
        .collect();
        environment.extend(
            [
                ("XDG_SESSION_TYPE", "wayland"),
                ("GTK_A11Y", "none"),
                ("NO_AT_BRIDGE", "1"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned())),
        );
        if dark {
            environment.push((String::from("GTK_THEME"), String::from("Adwaita:dark")));
        }
        let gtk_settings = folder.join("config").join("gtk-4.0");
        fs::create_dir_all(&gtk_settings)
            .map_err(|error| format!("couldn't create {}: {error}", gtk_settings.display()))?;
        let stylesheet = gtk_settings.join("gtk.css");
        fs::write(&stylesheet, title_bar_style(dark))
            .map_err(|error| format!("couldn't write {}: {error}", stylesheet.display()))?;

        let portals = folder.join("config").join("xdg-desktop-portal");
        fs::create_dir_all(&portals)
            .map_err(|error| format!("couldn't create {}: {error}", portals.display()))?;
        let portals_file = portals.join("gnome-portals.conf");
        fs::write(&portals_file, PORTALS)
            .map_err(|error| format!("couldn't write {}: {error}", portals_file.display()))?;

        let log = folder.join("gnome-shell.log");
        let output = File::create(&log)
            .map_err(|error| format!("couldn't create {}: {error}", log.display()))?;
        let errors = output.try_clone().map_err(|error| error.to_string())?;
        let shell = Command::new("gnome-shell")
            .args([
                "--headless",
                "--wayland",
                "--unsafe-mode",
                "--virtual-monitor",
            ])
            .arg(format!("{}x{}", monitor.0, monitor.1))
            .envs(environment.iter().map(|(name, value)| (name, value)))
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(output)
            .stderr(errors)
            .spawn()
            .map_err(|error| format!("couldn't start gnome-shell: {error}"))?;
        let mut session = Session {
            shell,
            environment,
            monitor,
            log,
        };

        let started = Instant::now();
        let display = loop {
            if let Ok(display) = shell_eval(
                "Main.overview.hide(); Main.messageTray._bannerBin.hide(); \
                 imports.gi.GLib.getenv('DISPLAY')",
            ) && display.starts_with(':')
            {
                break display;
            }
            if started.elapsed() > SHELL_START || session.shell.try_wait().ok().flatten().is_some()
            {
                let reason = session.log_tail();
                session.stop();
                return Err(format!("GNOME Shell didn't start:\n{reason}"));
            }
            thread::sleep(Duration::from_millis(250));
        };
        let authority = loop {
            let found = fs::read_dir(&runtime).ok().and_then(|files| {
                files.flatten().map(|file| file.path()).find(|path| {
                    path.file_name().is_some_and(|name| {
                        name.to_string_lossy().starts_with(".mutter-Xwaylandauth")
                    })
                })
            });
            if let Some(authority) = found {
                break authority;
            }
            if started.elapsed() > SHELL_START {
                session.stop();
                return Err(String::from("GNOME Shell started no XWayland"));
            }
            thread::sleep(Duration::from_millis(250));
        };
        session.environment.push((String::from("DISPLAY"), display));
        session
            .environment
            .push((String::from("XAUTHORITY"), authority.display().to_string()));

        // GNOME starts XWayland when the first X app connects, and xdotool
        // fails until it is up.
        while !session.xdotool(&["getmouselocation"]) {
            if started.elapsed() > SHELL_START {
                session.stop();
                return Err(String::from("XWayland didn't start"));
            }
            thread::sleep(Duration::from_millis(250));
        }
        session.share_with_portal()?;
        Ok(session)
    }

    /// Gives the desktop portal, which D-Bus starts when the app first asks
    /// it for the desktop's font, this session's display, folders and
    /// desktop, as GNOME's session manager does on a desktop, and checks
    /// that it answers.
    fn share_with_portal(&self) -> Result {
        let variables: Vec<String> = self
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .chain([("XDG_CURRENT_DESKTOP", "GNOME")])
            .map(|(name, value)| format!("'{name}': '{}'", value.replace('\'', "\\'")))
            .collect();
        gdbus(&[
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.UpdateActivationEnvironment",
            &format!("{{{}}}", variables.join(", ")),
        ])?;
        gdbus(&[
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.gnome.desktop.interface",
            "font-name",
        ])
        .map(|_| ())
        .map_err(|error| format!("the desktop portal didn't name GNOME's font: {error}"))
    }

    /// Runs xdotool on the session's X display, returning whether it
    /// worked.
    fn xdotool(&self, arguments: &[&str]) -> bool {
        Command::new("xdotool")
            .args(arguments)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Takes one picture (see the module's documentation).
    fn take(&self, capture: &Capture) -> Result {
        let (width, height) = self.monitor;
        if !self.xdotool(&[
            "mousemove",
            &(width - 1).to_string(),
            &(height - 1).to_string(),
        ]) {
            return Err(String::from("xdotool couldn't move the pointer"));
        }

        let errors_path = capture.output.with_extension("log");
        if let Some(folder) = errors_path.parent() {
            fs::create_dir_all(folder)
                .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
        }
        let errors = File::create(&errors_path)
            .map_err(|error| format!("couldn't create {}: {error}", errors_path.display()))?;
        let mut app = Command::new(&capture.app)
            .args(&capture.arguments)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .env_remove("WAYLAND_DISPLAY")
            .env("ICED_BACKEND", "tiny-skia")
            .stdout(Stdio::piped())
            .stderr(errors)
            .spawn()
            .map_err(|error| format!("couldn't start the app: {error}"))?;
        let app_errors = || fs::read_to_string(&errors_path).unwrap_or_default();

        let result = if capture.framed {
            Session::take_framed(&mut app, &capture.output)
        } else {
            wait_for(&mut app, CAPTURE_TIME)
        };
        let _ = app.kill();
        let _ = app.wait();
        result
            .and_then(|()| {
                if capture.output.exists() {
                    Ok(())
                } else {
                    Err(String::from("no picture was saved"))
                }
            })
            .map_err(|error| format!("{}: {error}\n{}", capture.output.display(), app_errors()))
    }

    /// Waits until `app` says its scene is ready, then has GNOME take the
    /// picture of its window, border and shadow included.
    fn take_framed(app: &mut Child, output: &Path) -> Result {
        let stdout = app.stdout.take().ok_or("the app has no output")?;
        let (ready, said_ready) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if line == "ready" {
                    let _ = ready.send(());
                }
            }
        });
        said_ready
            .recv_timeout(CAPTURE_TIME)
            .map_err(|_| String::from("the app never said the scene was ready"))?;

        // GNOME places, focuses and draws the window meanwhile.
        thread::sleep(Duration::from_secs(1));
        shell_eval(
            "global.display.get_tab_list(0, null)[0].activate(global.get_current_time()); 'ok'",
        )?;
        thread::sleep(Duration::from_millis(500));
        let path = output.display().to_string();
        gdbus(&[
            "--dest",
            "org.gnome.Shell.Screenshot",
            "--object-path",
            "/org/gnome/Shell/Screenshot",
            "--method",
            "org.gnome.Shell.Screenshot.ScreenshotWindow",
            "true",
            "false",
            "false",
            &path,
        ])
        .map(|_| ())
    }

    /// The end of GNOME Shell's log, to say why it failed.
    fn log_tail(&self) -> String {
        let log = fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(20)..].join("\n")
    }

    /// Stops GNOME Shell.
    fn stop(mut self) {
        let _ = self.shell.kill();
        let _ = self.shell.wait();
    }
}

/// [`TITLE_BAR_STYLE`] in libadwaita's light or dark colors: a white or
/// dark gray title bar, with nearly black or white text and buttons.
fn title_bar_style(dark: bool) -> String {
    let (background, foreground) = if dark {
        ("#2e2e32", "#ffffff")
    } else {
        ("#ffffff", "rgba(0, 0, 6, 0.8)")
    };
    TITLE_BAR_STYLE
        .replace("{background}", background)
        .replace("{foreground}", foreground)
}

/// Runs `script` in the GNOME Shell on the session bus and returns what it
/// gave, as JSON.
fn shell_eval(script: &str) -> Result<String> {
    let answer = gdbus(&[
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Eval",
        script,
    ])?;
    let value = answer
        .strip_prefix("(true, '")
        .and_then(|rest| rest.strip_suffix("')"))
        .ok_or_else(|| format!("GNOME Shell answered {answer}"))?;
    Ok(value.trim_matches('"').to_owned())
}

/// Calls a method of GNOME Shell's on the session bus with `gdbus`.
fn gdbus(arguments: &[&str]) -> Result<String> {
    let output = Command::new("gdbus")
        .args(["call", "--session"])
        .args(arguments)
        .output()
        .map_err(|error| format!("couldn't run gdbus: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Waits until `app` ends, for at most `limit`, and fails unless it ended
/// well.
fn wait_for(app: &mut Child, limit: Duration) -> Result {
    let started = Instant::now();
    loop {
        match app.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("the app ended with {status}")),
            Ok(None) if started.elapsed() > limit => {
                return Err(String::from("the app took too long"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Whether xtask runs as root, as in a container.
fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
}

/// Lets only its owner into `folder`, as the folder of a session's sockets
/// must be.
#[cfg(unix)]
fn restrict_to_owner(folder: &Path) -> Result {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(folder, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("couldn't restrict {}: {error}", folder.display()))
}

/// Only Unix has permissions to restrict; the session runs on Linux only.
#[cfg(not(unix))]
fn restrict_to_owner(_folder: &Path) -> Result {
    Ok(())
}
