//! Which game owns each stored name, on the desktop (the `desktop` feature).
//!
//! What Nubo keeps for a game is named after its ROM's file name: `game.gb`
//! keeps its battery save in `saves/game.sav` and `game.rtc`, its save states
//! in `states/game.gb/`, its cheats in `cheats/game.gb.toml` and its pictures
//! in the library as `game.gb.png`. Two different games with the same file
//! name in different folders would share all of that, so each name belongs
//! to one ROM path, and `names.toml` in the data folder records which:
//!
//! - The first game to start under a name owns it, so saves made before
//!   Nubo kept this record keep their plain names. Nothing is renamed.
//! - Another ROM with the same file name, at another path, gets a name of
//!   its own with a hash of its folder in it, such as `game (3f2a91c0).gb`
//!   (so `game (3f2a91c0).sav` and the rest; see [`hashed_name`]).
//! - A game that moved keeps its saves: when the path that owns a name is
//!   gone, the ROM with the same file name that starts next takes the name
//!   over. An owner on a library folder that can't be reached, such as an
//!   unplugged drive, hasn't moved, so its name is never handed on.
//! - A copy (both files still there) is another path, so it gets names of
//!   its own.
//! - A name is only claimed when a game starts ([`claim`]), never by a
//!   library scan. Everything else only looks names up ([`stored_path`]),
//!   which never hands a name to another path.
//!
//! The record and the names in it are players' data: they must read back
//! the same in every later version, and a hashed name must never change, so
//! [`hashed_name`]'s hash is fixed. A record that can't be parsed is kept
//! aside (`names.kept-1.toml`) and Nubo starts a new one; a hashed name
//! comes out the same again, so a game whose name was recorded there finds
//! its saves again. One that can't be read at all is never written over.

use crate::storage::files::{keep_aside, write_atomically};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The record's file in the data folder. Players' data: it must never move.
const FILE_NAME: &str = "names.toml";

/// The record of one data folder: who owns which name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    /// Every name given out, with the path that owns it.
    owners: Vec<Owner>,
    /// Whether the saved record couldn't be read and is still in place, so
    /// saving would write over it. Claims then last for this run only.
    read_only: bool,
}

/// One stored name and the ROM path that owns it, as `names.toml` keeps it.
/// The field names are players' data and must never change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Owner {
    /// The stored name, standing for the ROM's file name in every name made
    /// from it: `game.gb` for the first game, `game (3f2a91c0).gb` for
    /// another.
    name: String,
    /// The ROM path that owns the name, made absolute.
    path: PathBuf,
    /// The title from the game's cartridge header when it last claimed the
    /// name, which picks the name a moved game takes over when several
    /// games with its file name are gone.
    #[serde(default)]
    title: String,
}

/// The contents of `names.toml`.
#[derive(Serialize, Deserialize)]
struct NamesFile {
    #[serde(default)]
    names: Vec<Owner>,
}

/// What [`claim`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The path to name the game's data after: the ROM path itself for the
    /// plain name, or with the name of its own as the file name.
    pub stored: PathBuf,
    /// How the game came by the name.
    pub how: How,
    /// Why the record couldn't be read or saved, for the player. The name
    /// still holds for this run.
    pub problem: Option<String>,
}

/// How a game came by its name as it started (see [`Names::claim`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// It owned the name already.
    Owned,
    /// It took over the name of a game that moved: this one.
    TookOver,
    /// It took the plain name, which nobody owned. Data kept under it
    /// before Nubo kept this record (or by a game of that file name that
    /// hadn't started since) is now this game's.
    Plain,
    /// It got a name of its own, since another game owns the plain one.
    OwnName,
    /// Its path isn't valid UTF-8, so it can't be recorded and keeps the
    /// plain name, as before Nubo kept this record.
    Unrecorded,
}

/// The records read so far, one per data folder, so a lookup doesn't read
/// the file each time. Only this module writes the files.
static RECORDS: Mutex<Vec<(PathBuf, Names)>> = Mutex::new(Vec::new());

/// The records, even if a thread panicked while holding them: each change
/// is one push or one field, so they are never half changed.
fn records() -> MutexGuard<'static, Vec<(PathBuf, Names)>> {
    RECORDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `work` on the record of `data_folder`, reading it first if this run
/// hasn't yet. Returns what `work` returns, and why the record couldn't be
/// read if this was the first time.
fn with_record<T>(data_folder: &Path, work: impl FnOnce(&mut Names) -> T) -> (T, Option<String>) {
    let mut records = records();
    let mut problem = None;
    let index = match records.iter().position(|(folder, _)| folder == data_folder) {
        Some(index) => index,
        None => {
            let (names, read_problem) = Names::load(data_folder);
            problem = read_problem;
            records.push((data_folder.to_path_buf(), names));
            records.len() - 1
        }
    };
    (work(&mut records[index].1), problem)
}

/// The path to name the data of the ROM at `rom_path` after, as the record
/// in `data_folder` has it, without claiming anything (see
/// [`Names::stored_path`]). The ROM path itself when that is the plain name.
pub fn stored_path(data_folder: &Path, rom_path: &Path) -> PathBuf {
    with_record(data_folder, |names| names.stored_path(rom_path, false)).0
}

/// Like [`stored_path`], for the library's pictures of a game that hasn't
/// started yet: with `shared`, when another game in the library has the same
/// file name, a plain name nobody owns isn't used either, so the two don't
/// show each other's pictures before one of them starts.
pub fn picture_path(data_folder: &Path, rom_path: &Path, shared: bool) -> PathBuf {
    with_record(data_folder, |names| names.stored_path(rom_path, shared)).0
}

/// Claims a name for the ROM at `rom_path`, whose header has `title`, as a
/// game starts (see [`Names::claim`]), and saves the record in
/// `data_folder` if it changed. `unreachable` says whether a path is on a
/// library folder that can't be reached now.
pub fn claim(
    data_folder: &Path,
    rom_path: &Path,
    title: &str,
    unreachable: impl Fn(&Path) -> bool,
) -> Claim {
    let ((stored, how, saved), read_problem) = with_record(data_folder, |names| {
        let (stored, how) = names.claim(rom_path, title, unreachable);
        let saved = match how {
            How::TookOver | How::Plain | How::OwnName => names.save(data_folder),
            How::Owned | How::Unrecorded => Ok(()),
        };
        (stored, how, saved)
    });
    Claim {
        stored,
        how,
        problem: read_problem.or(saved.err()),
    }
}

impl Names {
    /// Reads the record in `data_folder`, or an empty one if there is none
    /// yet, and why it couldn't be read. One that can't be parsed is kept
    /// aside first; one that can't be read at all stays in place, and the
    /// record is then read-only for this run.
    fn load(data_folder: &Path) -> (Names, Option<String>) {
        let path = data_folder.join(FILE_NAME);
        let contents = match fs::read(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return (Names::default(), None),
            Err(error) => {
                let names = Names {
                    read_only: true,
                    ..Names::default()
                };
                return (
                    names,
                    Some(format!(
                        "Which game owns which saves couldn't be read from {} ({error}), so new games with the same file name share saves this time",
                        path.display()
                    )),
                );
            }
        };
        let parsed = String::from_utf8(contents)
            .map_err(|_| String::from("it isn't valid UTF-8"))
            .and_then(|text| {
                toml::from_str::<NamesFile>(&text).map_err(|error| error.message().to_owned())
            });
        match parsed {
            Ok(file) => (
                Names {
                    owners: file.names,
                    read_only: false,
                },
                None,
            ),
            Err(error) => match keep_aside(&path) {
                Ok(kept) => (
                    Names::default(),
                    Some(format!(
                        "Which game owns which saves couldn't be read ({error}), so it was kept as {} and started again",
                        kept.display()
                    )),
                ),
                Err(keep_error) => (
                    Names {
                        read_only: true,
                        ..Names::default()
                    },
                    Some(format!(
                        "Which game owns which saves couldn't be read from {} ({error}) or kept aside ({keep_error})",
                        path.display()
                    )),
                ),
            },
        }
    }

    /// Writes the record to `names.toml` in `data_folder`, unless it is
    /// read-only.
    fn save(&self, data_folder: &Path) -> Result<(), String> {
        if self.read_only {
            return Ok(());
        }
        let file = NamesFile {
            names: self.owners.clone(),
        };
        let contents = toml::to_string(&file).map_err(|error| error.to_string())?;
        write_atomically(&data_folder.join(FILE_NAME), contents.as_bytes())
            .map_err(|error| format!("Which game owns which saves couldn't be saved: {error}"))
    }

    /// The path to name the data of the ROM at `rom_path` after, without
    /// claiming anything: its own name if it owns one; else the plain name
    /// if nobody owns that (and the game doesn't `share` its file name with
    /// another one in the library); else the name it would get as another
    /// game. It never hands out a name another path owns, not even one
    /// whose owner is gone: only [`Names::claim`] takes a name over.
    pub fn stored_path(&self, rom_path: &Path, share: bool) -> PathBuf {
        let Some(file_name) = file_name(rom_path) else {
            return rom_path.to_path_buf();
        };
        if let Some(owner) = self.owner_of(rom_path) {
            return rom_path.with_file_name(&owner.name);
        }
        if !share && !self.is_taken(&file_name) {
            return rom_path.to_path_buf();
        }
        rom_path.with_file_name(self.new_name(rom_path, &file_name))
    }

    /// Claims a name for the ROM at `rom_path` as a game starts, and
    /// returns the path to name its data after and how it came by it:
    ///
    /// 1. A name it owns already stays its own.
    /// 2. Else, a name of its file name whose owner is gone, and not on a
    ///    folder that is `unreachable`, was the game's before it moved, so it
    ///    takes that name over: the one whose owner had the same `title`,
    ///    else the plain name, else the first.
    /// 3. Else, the plain name if nobody owns it.
    /// 4. Else, a name of its own (see [`hashed_name`]).
    ///
    /// A path that isn't valid UTF-8 can't be recorded, so it keeps the
    /// plain name, as before Nubo kept this record.
    pub fn claim(
        &mut self,
        rom_path: &Path,
        title: &str,
        unreachable: impl Fn(&Path) -> bool,
    ) -> (PathBuf, How) {
        let Some(file_name) = file_name(rom_path) else {
            return (rom_path.to_path_buf(), How::Unrecorded);
        };
        let absolute = absolute(rom_path);
        if absolute.to_str().is_none() {
            return (rom_path.to_path_buf(), How::Unrecorded);
        }

        if let Some(owner) = self.owner_of(rom_path) {
            let name = owner.name.clone();
            return (rom_path.with_file_name(name), How::Owned);
        }

        let moved = self
            .owners
            .iter()
            .enumerate()
            .filter(|(_, owner)| {
                file_name_of(&owner.path) == Some(file_name.as_str())
                    && is_gone(&owner.path)
                    && !unreachable(&owner.path)
            })
            .min_by_key(|(_, owner)| (owner.title != title, owner.name != file_name))
            .map(|(index, _)| index);
        if let Some(index) = moved {
            let owner = &mut self.owners[index];
            owner.path = absolute;
            owner.title = title.to_owned();
            let name = owner.name.clone();
            return (rom_path.with_file_name(name), How::TookOver);
        }

        let (name, how) = if self.is_taken(&file_name) {
            (self.new_name(rom_path, &file_name), How::OwnName)
        } else {
            (file_name, How::Plain)
        };
        self.owners.push(Owner {
            name: name.clone(),
            path: absolute,
            title: title.to_owned(),
        });
        (rom_path.with_file_name(name), how)
    }

    /// The record of the name the ROM at `rom_path` owns, if it owns one:
    /// recorded under the same path, or one that is the same file (through
    /// a link, or written another way).
    fn owner_of(&self, rom_path: &Path) -> Option<&Owner> {
        let absolute = absolute(rom_path);
        let file_name = file_name(rom_path)?;
        let same_name = || {
            self.owners
                .iter()
                .filter(|owner| file_name_of(&owner.path) == Some(file_name.as_str()))
        };
        same_name()
            .find(|owner| owner.path == absolute)
            .or_else(|| {
                let canonical = fs::canonicalize(rom_path).ok()?;
                same_name()
                    .find(|owner| fs::canonicalize(&owner.path).ok() == Some(canonical.clone()))
            })
    }

    /// Whether some path owns the stored name `name` already.
    fn is_taken(&self, name: &str) -> bool {
        self.owners.iter().any(|owner| owner.name == name)
    }

    /// The first name of [`hashed_name`]'s for the ROM at `rom_path` that
    /// nobody owns.
    fn new_name(&self, rom_path: &Path, file_name: &str) -> String {
        let folder = absolute(rom_path)
            .parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        (0..)
            .map(|attempt| hashed_name(file_name, &folder, attempt))
            .find(|name| !self.is_taken(name))
            .unwrap_or_default()
    }
}

/// The name of its own that a game called `file_name` in `folder` gets when
/// another game owns the plain name: `game.gb` in `/roms/Jam B` becomes
/// `game (3f2a91c0).gb`, with the 32-bit FNV-1a hash of the folder's path
/// (as UTF-8) in lowercase hex in brackets, which the game's shown name
/// leaves out (see `rom::game_name`). In the unlikely case that this name is
/// taken already, `attempt` 1, 2 and on hash `<folder>\n<attempt>` instead.
/// Players' saves are named after it, so it must never change.
pub fn hashed_name(file_name: &str, folder: &str, attempt: u32) -> String {
    let hash = if attempt == 0 {
        super::fnv1a(folder.as_bytes())
    } else {
        super::fnv1a(format!("{folder}\n{attempt}").as_bytes())
    };
    super::name_with_hash(Path::new(file_name), hash)
        .to_string_lossy()
        .into_owned()
}

/// The file name of `path` as text, if it has one that is valid UTF-8.
fn file_name(path: &Path) -> Option<String> {
    file_name_of(path).map(str::to_owned)
}

/// The file name of `path`, if it has one that is valid UTF-8.
fn file_name_of(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

/// `path` made absolute against the current folder, without following
/// links; `path` itself if that fails.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether nothing is at `path` any more. A path that can't be checked
/// (no permission, say) isn't gone.
fn is_gone(path: &Path) -> bool {
    matches!(path.try_exists(), Ok(false))
}

/// Forgets the records read so far, so the next lookup reads them again,
/// for tests that change a record's file themselves.
#[cfg(test)]
pub(crate) fn forget_records() {
    records().clear();
}

#[cfg(test)]
mod tests;
