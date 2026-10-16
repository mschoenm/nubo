//! Fetching the other emulators at their pinned commits and building them,
//! each with its harness, into `target/emulator-comparison/bin`.

use super::{
    Emulator, Source, executable, has_extension, in_parallel, parallelism, sorted_entries,
    source_folder,
};
use crate::{Result, cargo, on_path, run, target_dir, workspace_root};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The harnesses' sources, relative to the workspace root.
const HARNESS_FOLDER: &str = "xtask/emulator_harnesses";

/// SameBoy's release build (`CONF=release` in its Makefile), used for the
/// harness that links its core.
const SAMEBOY_FLAGS: [&str; 5] = ["-std=gnu11", "-O3", "-ffast-math", "-DNDEBUG", "-flto"];

/// What Gambatte's own build (`libgambatte/SConstruct`) compiles with.
const GAMBATTE_FLAGS: [&str; 6] = [
    "-O2",
    "-fomit-frame-pointer",
    "-fno-exceptions",
    "-fno-rtti",
    "-std=c++11",
    "-DHAVE_STDINT_H",
];

/// mGBA's release build: CMake's Release type, with the link-time
/// optimization and the options its `CMakeLists.txt` sets up, and everything
/// the core doesn't need (the GBA core, frontends, optional libraries) off.
const MGBA_CMAKE_OPTIONS: [&str; 27] = [
    "-DCMAKE_BUILD_TYPE=Release",
    "-DM_CORE_GBA=OFF",
    "-DM_CORE_GB=ON",
    "-DBUILD_QT=OFF",
    "-DBUILD_SDL=OFF",
    "-DBUILD_LIBRETRO=OFF",
    "-DBUILD_GL=OFF",
    "-DBUILD_GLES2=OFF",
    "-DBUILD_GLES3=OFF",
    "-DUSE_EPOXY=OFF",
    "-DBUILD_STATIC=ON",
    "-DBUILD_SHARED=OFF",
    "-DUSE_FFMPEG=OFF",
    "-DUSE_LIBZIP=OFF",
    "-DUSE_MINIZIP=OFF",
    "-DUSE_ZLIB=OFF",
    "-DUSE_PNG=OFF",
    "-DUSE_SQLITE3=OFF",
    "-DUSE_LUA=OFF",
    "-DUSE_JSON_C=OFF",
    "-DUSE_FREETYPE=OFF",
    "-DUSE_ELF=OFF",
    "-DUSE_LZMA=OFF",
    "-DUSE_DISCORD_RPC=OFF",
    "-DUSE_EDITLINE=OFF",
    "-DENABLE_GDB_STUB=OFF",
    "-DENABLE_SCRIPTING=OFF",
];

/// The flags the harness that links mGBA's core is compiled with, matching
/// how mGBA compiles its library.
const MGBA_FLAGS: [&str; 5] = ["-std=gnu11", "-O3", "-flto", "-fwrapv", "-pthread"];

/// The flags of Gearboy's libretro core (`platforms/libretro/Makefile`), the
/// build that runs it inside other frontends, with the disassembler and the
/// VGM recorder left out as there.
const GEARBOY_FLAGS: [&str; 5] = [
    "-O3",
    "-flto",
    "-DNDEBUG",
    "-DGEARBOY_DISABLE_DISASSEMBLER",
    "-DGEARBOY_DISABLE_VGMRECORDER",
];

/// What Gearboy's libretro core adds for its C++ files.
const GEARBOY_CXX_FLAGS: [&str; 3] = ["-std=c++11", "-fno-rtti", "-fno-exceptions"];

/// binjgb's release build (CMake's Release type).
const BINJGB_FLAGS: [&str; 2] = ["-O3", "-DNDEBUG"];

/// The parts of binjgb its emulator core is made of.
const BINJGB_SOURCES: [&str; 4] = ["memory.c", "common.c", "emulator.c", "joypad.c"];

/// What to install for a missing tool.
fn describe_tool(tool: &str) -> &str {
    match tool {
        "cc" => "cc (a C compiler)",
        "c++" => "c++ (a C++ compiler)",
        "rgbasm" => "rgbasm (RGBDS, https://rgbds.gbdev.io)",
        other => other,
    }
}

/// Builds each emulator, returning the ones that are ready to run and why
/// the others were left out.
pub(super) fn prepare(
    emulators: &[Emulator],
    root: &Path,
) -> (Vec<Emulator>, Vec<(Emulator, String)>) {
    let mut ready = Vec::new();
    let mut left_out = Vec::new();
    let mut boot_roms: Option<Result> = None;
    for &emulator in emulators {
        let missing: Vec<&str> = emulator
            .tools()
            .iter()
            .copied()
            .filter(|tool| !on_path(tool))
            .collect();
        if !missing.is_empty() {
            let missing: Vec<&str> = missing.into_iter().map(describe_tool).collect();
            left_out.push((
                emulator,
                format!("building it needs {}", missing.join(", ")),
            ));
            continue;
        }
        if emulator.runs_sameboy_boot_roms()
            && emulator != Emulator::SameBoy
            && let Err(error) = boot_roms.get_or_insert_with(|| build_boot_roms(root))
        {
            left_out.push((
                emulator,
                format!("SameBoy's boot ROMs didn't build: {error}"),
            ));
            continue;
        }
        println!("Building {}...", emulator.name());
        match build(emulator, root) {
            Ok(()) => ready.push(emulator),
            Err(error) => left_out.push((emulator, error)),
        }
    }
    (ready, left_out)
}

/// Where `emulator`'s object files and CMake build go, under `root`.
fn build_folder(root: &Path, emulator: Emulator) -> PathBuf {
    root.join(format!("{}-build", emulator.id()))
}

/// The folder with the harnesses' sources, which time each emulator.
fn harness_folder() -> PathBuf {
    workspace_root().join(HARNESS_FOLDER)
}

/// Builds one emulator with its harness into `bin/`, unless the build there
/// is from the same commit, harness and build steps.
fn build(emulator: Emulator, root: &Path) -> Result {
    let Some(source) = emulator.source() else {
        return build_nubo(emulator, root);
    };
    let stamp_path = root.join("bin").join(format!("{}.stamp", emulator.id()));
    let stamp = build_stamp(emulator, source)?;
    if executable(root, emulator).is_file()
        && fs::read_to_string(&stamp_path).is_ok_and(|built| built == stamp)
    {
        return Ok(());
    }
    let _ = fs::remove_file(&stamp_path);
    fetch(source, &source_folder(root, emulator))?;
    fs::create_dir_all(root.join("bin"))
        .map_err(|error| format!("failed to create {}: {error}", root.join("bin").display()))?;
    match emulator {
        Emulator::Nubo | Emulator::NuboEager => unreachable!("Nubo has no source to fetch"),
        Emulator::SameBoy => build_sameboy(root),
        Emulator::Gambatte => build_gambatte(root),
        Emulator::Mgba => build_mgba(root),
        Emulator::Binjgb => build_binjgb(root),
        Emulator::Gearboy => build_gearboy(root),
    }?;
    fs::write(&stamp_path, stamp)
        .map_err(|error| format!("failed to write {}: {error}", stamp_path.display()))
}

/// What a build depends on: the emulator's commit, its harness and these
/// build steps.
fn build_stamp(emulator: Emulator, source: Source) -> Result<String> {
    let mut hasher = Sha256::new();
    for file in ["harness.h", emulator.harness()] {
        let path = harness_folder().join(file);
        hasher.update(
            fs::read(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?,
        );
    }
    hasher.update(include_str!("build.rs"));
    let hash: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("{} {hash}", source.commit))
}

/// Builds nubo-benchmark, with the `eager` feature for [`Emulator::NuboEager`],
/// and copies it into `bin/`.
fn build_nubo(emulator: Emulator, root: &Path) -> Result {
    let mut command = cargo();
    command
        .args([
            "build",
            "--release",
            "--locked",
            "--package",
            "nubo-benchmark",
        ])
        .current_dir(workspace_root());
    if emulator == Emulator::NuboEager {
        command.args(["--features", "nubo-core/eager"]);
    }
    run(&mut command)?;
    let built = target_dir().join("release").join("nubo-benchmark");
    let copy = executable(root, emulator);
    fs::create_dir_all(root.join("bin"))
        .map_err(|error| format!("failed to create {}: {error}", root.join("bin").display()))?;
    fs::copy(&built, &copy).map(|_| ()).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            built.display(),
            copy.display()
        )
    })
}

/// Checks `source` out into `folder`, fetching only that commit.
fn fetch(source: Source, folder: &Path) -> Result {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(folder)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    if head.as_deref() == Some(source.commit) && folder.join(".git").is_dir() {
        return Ok(());
    }
    if folder.exists() {
        fs::remove_dir_all(folder)
            .map_err(|error| format!("failed to remove {}: {error}", folder.display()))?;
    }
    println!(
        "Fetching {} at {}...",
        source.repository,
        &source.commit[..7]
    );
    run_quietly(Command::new("git").args(["init", "--quiet"]).arg(folder))?;
    run_quietly(
        Command::new("git")
            .args([
                "fetch",
                "--quiet",
                "--depth",
                "1",
                source.repository,
                source.commit,
            ])
            .current_dir(folder),
    )?;
    run_quietly(
        Command::new("git")
            .args(["checkout", "--quiet", "--detach", "FETCH_HEAD"])
            .current_dir(folder),
    )
}

/// The C compiler SameBoy's Makefile picks: clang when there is one.
fn sameboy_compiler() -> &'static str {
    if on_path("clang") { "clang" } else { "cc" }
}

/// Fetches SameBoy and builds its boot ROMs, for the emulators that run them
/// (`Emulator::runs_sameboy_boot_roms`).
fn build_boot_roms(root: &Path) -> Result {
    let source = Emulator::SameBoy
        .source()
        .expect("SameBoy is fetched from git");
    let folder = source_folder(root, Emulator::SameBoy);
    fetch(source, &folder)?;
    run_quietly(
        Command::new("make")
            .arg("bootroms")
            .arg(format!("-j{}", parallelism()))
            .current_dir(&folder),
    )
}

/// Builds SameBoy's core library and boot ROMs with its release flags, and
/// links the harness against the library.
fn build_sameboy(root: &Path) -> Result {
    let source = source_folder(root, Emulator::SameBoy);
    let compiler = sameboy_compiler();
    run_quietly(
        Command::new("make")
            .args(["lib", "bootroms", "CONF=release"])
            .arg(format!("CC={compiler}"))
            .arg(format!("-j{}", parallelism()))
            .current_dir(&source),
    )?;
    run_quietly(
        Command::new(compiler)
            .args(SAMEBOY_FLAGS)
            .arg("-I")
            .arg(&source)
            .arg("-I")
            .arg(harness_folder())
            .arg(harness_folder().join(Emulator::SameBoy.harness()))
            .arg(source.join("build/lib/libsameboy.a"))
            .arg("-lm")
            .arg("-o")
            .arg(executable(root, Emulator::SameBoy)),
    )
}

/// Compiles Gambatte's core and its resampler with the flags of its own
/// build, leaving out the C interface and zip support, and links the harness
/// with them.
fn build_gambatte(root: &Path) -> Result {
    let source = source_folder(root, Emulator::Gambatte);
    let library = source.join("libgambatte");
    let resample = source.join("common/resample/src");
    let mut sources: Vec<PathBuf> = files_with_extension(&library.join("src"), "cpp", true)?
        .into_iter()
        // The C interface and zip support aren't part of the core: Gambatte's
        // own build compiles without zlib.
        .filter(|path| !path.ends_with("cinterface.cpp") && !path.ends_with("file_zip.cpp"))
        .collect();
    sources.extend(files_with_extension(&resample, "cpp", false)?);
    let mut flags: Vec<String> = GAMBATTE_FLAGS
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
    for include in [
        library.join("src"),
        library.join("include"),
        source.join("common"),
        resample,
    ] {
        flags.push(format!("-I{}", include.display()));
    }
    let objects = compile(
        "c++",
        &with_flag(&flags, "-w"),
        &sources,
        &build_folder(root, Emulator::Gambatte),
    )?;
    run_quietly(
        Command::new("c++")
            .args(&flags)
            .arg(format!("-I{}", harness_folder().display()))
            .arg(harness_folder().join(Emulator::Gambatte.harness()))
            .args(&objects)
            .arg("-o")
            .arg(executable(root, Emulator::Gambatte)),
    )
}

/// Builds mGBA's library with CMake, and compiles the harness with the same
/// feature defines, since mGBA's structs depend on them.
fn build_mgba(root: &Path) -> Result {
    let source = source_folder(root, Emulator::Mgba);
    let build = build_folder(root, Emulator::Mgba);
    run_quietly(
        Command::new("cmake")
            .arg("-S")
            .arg(&source)
            .arg("-B")
            .arg(&build)
            .args(["-G", "Unix Makefiles"])
            .args(MGBA_CMAKE_OPTIONS),
    )?;
    run_quietly(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .arg("--parallel")
            .arg(parallelism().to_string()),
    )?;
    // The harness includes mGBA's headers, whose structs depend on the
    // features the library was built with, so it needs the same defines.
    let flags_file = build.join("CMakeFiles/mgba.dir/flags.make");
    let flags = fs::read_to_string(&flags_file)
        .map_err(|error| format!("failed to read {}: {error}", flags_file.display()))?;
    let defines = flags
        .lines()
        .find_map(|line| line.strip_prefix("C_DEFINES = "))
        .ok_or_else(|| format!("{} lists no C_DEFINES", flags_file.display()))?;
    run_quietly(
        Command::new("cc")
            .args(MGBA_FLAGS)
            .args(defines.split_whitespace())
            .arg(format!("-I{}", source.join("include").display()))
            .arg(format!("-I{}", build.join("include").display()))
            .arg(format!("-I{}", source.join("src").display()))
            .arg(format!("-I{}", harness_folder().display()))
            .arg(harness_folder().join(Emulator::Mgba.harness()))
            .arg(build.join("libmgba.a"))
            .arg("-lm")
            .arg("-o")
            .arg(executable(root, Emulator::Mgba)),
    )
}

/// Compiles the binjgb sources the harness needs and links the harness with
/// them.
fn build_binjgb(root: &Path) -> Result {
    let source = source_folder(root, Emulator::Binjgb).join("src");
    let sources: Vec<PathBuf> = BINJGB_SOURCES
        .iter()
        .map(|file| source.join(file))
        .collect();
    let mut flags: Vec<String> = BINJGB_FLAGS.iter().map(|flag| (*flag).to_owned()).collect();
    flags.push(format!("-I{}", source.display()));
    let objects = compile(
        "cc",
        &with_flag(&flags, "-w"),
        &sources,
        &build_folder(root, Emulator::Binjgb),
    )?;
    run_quietly(
        Command::new("cc")
            .args(&flags)
            .arg(format!("-I{}", harness_folder().display()))
            .arg(harness_folder().join(Emulator::Binjgb.harness()))
            .args(&objects)
            .arg("-lm")
            .arg("-o")
            .arg(executable(root, Emulator::Binjgb)),
    )
}

/// Compiles Gearboy's core, its audio code and miniz, and links the harness
/// with them.
fn build_gearboy(root: &Path) -> Result {
    let source = source_folder(root, Emulator::Gearboy);
    let core = source.join("src");
    let miniz = source.join("platforms/shared/dependencies/miniz");
    let mut sources = files_with_extension(&core, "cpp", false)?;
    sources.extend(files_with_extension(&core.join("audio"), "cpp", false)?);
    let mut flags: Vec<String> = GEARBOY_FLAGS
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
    flags.push(format!("-I{}", core.display()));
    flags.push(format!("-I{}", miniz.display()));
    let mut cxx_flags = flags.clone();
    cxx_flags.extend(GEARBOY_CXX_FLAGS.iter().map(|flag| (*flag).to_owned()));
    let objects_folder = build_folder(root, Emulator::Gearboy);
    let mut objects = compile(
        "c++",
        &with_flag(&cxx_flags, "-w"),
        &sources,
        &objects_folder,
    )?;
    objects.extend(compile(
        "cc",
        &with_flag(&with_flag(&flags, "-std=gnu99"), "-w"),
        &[miniz.join("miniz.c")],
        &objects_folder.join("miniz"),
    )?);
    run_quietly(
        Command::new("c++")
            .args(&cxx_flags)
            .arg(format!("-I{}", harness_folder().display()))
            .arg(harness_folder().join(Emulator::Gearboy.harness()))
            .args(&objects)
            .arg("-lm")
            .arg("-o")
            .arg(executable(root, Emulator::Gearboy)),
    )
}

/// A copy of `flags` with `flag` added at the end.
fn with_flag(flags: &[String], flag: &str) -> Vec<String> {
    let mut flags = flags.to_vec();
    flags.push(flag.to_owned());
    flags
}

/// The files in `folder` (and its subfolders when `recursive`) with
/// `extension`, sorted.
fn files_with_extension(folder: &Path, extension: &str, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in sorted_entries(folder)? {
        if path.is_dir() {
            if recursive {
                files.extend(files_with_extension(&path, extension, true)?);
            }
        } else if has_extension(&path, &[extension]) {
            files.push(path);
        }
    }
    Ok(files)
}

/// Compiles each source file into an object file in `objects`, several at a
/// time, and returns the object files.
fn compile(
    compiler: &str,
    flags: &[String],
    sources: &[PathBuf],
    objects: &Path,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(objects)
        .map_err(|error| format!("failed to create {}: {error}", objects.display()))?;
    let jobs: Vec<(PathBuf, PathBuf)> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let stem = source.file_stem().unwrap_or_default().to_string_lossy();
            let name = format!("{index:03}-{stem}.o");
            (source.clone(), objects.join(name))
        })
        .collect();
    in_parallel(jobs.clone(), |(source, object)| {
        run_quietly(
            Command::new(compiler)
                .args(flags)
                .arg("-c")
                .arg(&source)
                .arg("-o")
                .arg(&object),
        )
    })
    .into_iter()
    .collect::<Result<Vec<()>>>()?;
    Ok(jobs.into_iter().map(|(_, object)| object).collect())
}

/// Runs a command without showing its output, which only comes out when it
/// fails.
fn run_quietly(command: &mut Command) -> Result {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .map_err(|error| format!("failed to run {program}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    Err(format!("{program} exited with {}:\n{tail}", output.status))
}
