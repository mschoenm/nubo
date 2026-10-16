//! Tests for the entries' names and the storage without a platform. The
//! files tests write real files in the temporary directory, so they need the
//! `desktop` feature.

use super::*;

#[test]
fn entries_are_named_after_the_rom_file() {
    let rom_path = Path::new("roms").join("Pocket Game.gbc");

    assert_eq!(Entry::Settings.file_name(), Path::new("settings.toml"));
    assert_eq!(
        Entry::BatteryRam(&rom_path).file_name(),
        Path::new("Pocket Game.sav")
    );
    assert_eq!(
        Entry::BatteryClock(&rom_path).file_name(),
        Path::new("Pocket Game.rtc")
    );
    assert_eq!(
        Entry::Cheats(&rom_path).file_name(),
        Path::new("Pocket Game.gbc.toml")
    );
}

#[test]
fn no_storage_keeps_nothing() {
    let rom_path = Path::new("game.gb");

    assert_eq!(NoStorage.read(Entry::BatteryRam(rom_path)), Ok(None));
    assert!(
        NoStorage
            .write(Entry::BatteryRam(rom_path), &[1, 2])
            .is_err()
    );
}

#[cfg(feature = "desktop")]
mod files {
    use super::super::files::{Files, write_atomically};
    use super::*;
    use std::fs;

    /// An empty folder in the temporary directory.
    fn temporary_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("nubo-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn entries_go_where_earlier_versions_put_them() {
        let files = Files::in_folders(Some(PathBuf::from("config")), Some(PathBuf::from("data")));
        let rom_path = Path::new("roms").join("game.gb");

        assert_eq!(
            files.path(Entry::Settings).unwrap(),
            Path::new("config").join("settings.toml")
        );
        assert_eq!(
            files.path(Entry::BatteryRam(&rom_path)).unwrap(),
            Path::new("data").join("saves").join("game.sav")
        );
        assert_eq!(
            files.path(Entry::BatteryClock(&rom_path)).unwrap(),
            Path::new("data").join("saves").join("game.rtc")
        );
        assert_eq!(
            files.path(Entry::Cheats(&rom_path)).unwrap(),
            Path::new("data").join("cheats").join("game.gb.toml")
        );
    }

    #[test]
    fn written_entries_read_back_and_missing_ones_read_as_none() {
        let folder = temporary_folder("round-trip");
        let files = Files::in_folders(Some(folder.join("config")), Some(folder.join("data")));
        let entry = Entry::BatteryRam(Path::new("game.gb"));

        assert_eq!(files.read(entry), Ok(None));
        files.write(entry, &[1, 2, 3]).unwrap();

        assert_eq!(files.read(entry), Ok(Some(vec![1, 2, 3])));
        assert_eq!(
            files.location(entry),
            folder
                .join("data")
                .join("saves")
                .join("game.sav")
                .display()
                .to_string()
        );
    }

    #[test]
    fn writing_again_replaces_the_file_and_leaves_no_temporary_file() {
        let folder = temporary_folder("replace");
        let path = folder.join("game.sav");

        write_atomically(&path, &[1, 2, 3]).unwrap();
        write_atomically(&path, &[4]).unwrap();

        assert_eq!(fs::read(&path).unwrap(), [4]);
        assert!(!folder.join("game.sav.tmp").exists());
    }

    #[test]
    fn without_folders_nothing_reads_and_writing_fails() {
        let files = Files::default();
        let entry = Entry::BatteryRam(Path::new("game.gb"));

        assert!(files.read(Entry::Settings).is_err());
        assert_eq!(
            files.write(entry, &[1, 2, 3]),
            Err(String::from(
                "the system has no data folder to keep saves in"
            ))
        );
        assert_eq!(files.location(entry), "game.sav");
    }

    #[test]
    fn deleting_battery_saves_leaves_the_other_files() {
        let folder = temporary_folder("delete-saves");
        let files = Files::in_folders(Some(folder.join("config")), Some(folder.join("data")));
        let rom_path = Path::new("game.gb");
        files.write(Entry::BatteryRam(rom_path), &[1]).unwrap();
        files.write(Entry::BatteryClock(rom_path), &[2]).unwrap();
        files.write(Entry::Cheats(rom_path), b"cheats").unwrap();
        files.write(Entry::Settings, b"settings").unwrap();
        let notes = folder.join("data").join("saves").join("notes.txt");
        fs::write(&notes, "mine").unwrap();

        files.delete_battery_saves().unwrap();

        assert_eq!(files.read(Entry::BatteryRam(rom_path)), Ok(None));
        assert_eq!(files.read(Entry::BatteryClock(rom_path)), Ok(None));
        assert!(files.read(Entry::Cheats(rom_path)).unwrap().is_some());
        assert!(files.read(Entry::Settings).unwrap().is_some());
        assert!(notes.is_file());
    }

    #[test]
    fn deleting_battery_saves_that_were_never_kept_is_fine() {
        let folder = temporary_folder("delete-no-saves");
        let files = Files::in_folders(None, Some(folder));

        assert_eq!(files.delete_battery_saves(), Ok(()));
        assert_eq!(Files::default().delete_battery_saves(), Ok(()));
    }
}
