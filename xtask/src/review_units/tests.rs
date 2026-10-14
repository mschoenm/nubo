use super::*;

#[test]
fn reads_edits_and_moves_from_git() {
    let output = "M\tcrates/nubo-core/src/apu.rs\n\
                  R100\tfrontends/benchmark/src/main.rs\ttools/nubo-benchmark/src/main.rs\n\
                  R087\tfrontends/headless/src/main.rs\ttools/nubo-headless/src/main.rs\n\
                  D\tfrontends/old.rs\n\
                  C090\tcrates/nubo-core/src/apu.rs\tcrates/nubo-core/src/apu_copy.rs\n";
    assert_eq!(
        parse_name_status(output),
        vec![
            Change::Edited(String::from("crates/nubo-core/src/apu.rs")),
            Change::Moved {
                from: String::from("frontends/benchmark/src/main.rs"),
                to: String::from("tools/nubo-benchmark/src/main.rs"),
                unchanged: true,
            },
            Change::Moved {
                from: String::from("frontends/headless/src/main.rs"),
                to: String::from("tools/nubo-headless/src/main.rs"),
                unchanged: false,
            },
            Change::Edited(String::from("frontends/old.rs")),
            Change::Edited(String::from("crates/nubo-core/src/apu_copy.rs")),
        ]
    );
}

#[test]
fn a_moved_file_keeps_its_review_date() {
    let mut reviewed = Map::new();
    reviewed.insert(
        String::from("frontends/benchmark/src/main.rs"),
        Value::from("2026-10-01"),
    );
    reviewed.insert(
        String::from("crates/nubo-core/src/apu.rs"),
        Value::from("2026-10-02"),
    );
    let changes = [Change::Moved {
        from: String::from("frontends/benchmark/src/main.rs"),
        to: String::from("tools/nubo-benchmark/src/main.rs"),
        unchanged: true,
    }];

    carry_over_moves(&mut reviewed, &changes);

    assert_eq!(reviewed.get("frontends/benchmark/src/main.rs"), None);
    assert_eq!(
        reviewed.get("tools/nubo-benchmark/src/main.rs"),
        Some(&Value::from("2026-10-01"))
    );
    assert_eq!(
        reviewed.get("crates/nubo-core/src/apu.rs"),
        Some(&Value::from("2026-10-02"))
    );
    assert_eq!(changed_files(&changes).count(), 0);
}

#[test]
fn a_file_moved_with_changes_counts_as_changed() {
    let changes = [
        Change::Edited(String::from("crates/nubo-core/src/apu.rs")),
        Change::Moved {
            from: String::from("frontends/headless/src/main.rs"),
            to: String::from("tools/nubo-headless/src/main.rs"),
            unchanged: false,
        },
    ];
    assert_eq!(
        changed_files(&changes).collect::<Vec<_>>(),
        [
            "crates/nubo-core/src/apu.rs",
            "tools/nubo-headless/src/main.rs"
        ]
    );
}

#[test]
fn deleted_files_leave_the_state() {
    let mut reviewed = Map::new();
    reviewed.insert(
        String::from("crates/nubo-core/src/apu.rs"),
        Value::from("2026-10-01"),
    );
    reviewed.insert(String::from("frontends/old.rs"), Value::from("2026-10-02"));

    forget_missing(&mut reviewed, |file| file != "frontends/old.rs");

    assert_eq!(
        reviewed.keys().collect::<Vec<_>>(),
        ["crates/nubo-core/src/apu.rs"]
    );
}
