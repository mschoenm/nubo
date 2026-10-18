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

#[test]
fn a_state_that_is_not_an_object_is_an_error() {
    let state = std::env::temp_dir().join(format!(
        "nubo-review-units-state-{}.json",
        std::process::id()
    ));
    fs::write(&state, "[]").unwrap();
    let result = next_part(&NextOptions {
        state: state.clone(),
        date: String::from("2026-10-16"),
        commit: String::from("0000000000000000000000000000000000000000"),
    });
    fs::remove_file(&state).unwrap();
    assert!(result.unwrap_err().contains("is not a JSON object"));
}

#[test]
fn only_a_full_commit_hash_is_taken_as_the_last_commit() {
    let path = Path::new("rotation.json");
    let hash = "0123456789abcdef0123456789abcdef01234567";
    let state_with = |commit: &str| {
        let Value::Object(state) = json!({ "last_commit": commit }) else {
            unreachable!()
        };
        state
    };
    assert_eq!(last_commit(&state_with(hash), path).as_deref(), Some(hash));
    for invalid in ["--output=/tmp/x", "x$(touch y)", "0123456", ""] {
        assert_eq!(last_commit(&state_with(invalid), path), None);
    }
    assert_eq!(last_commit(&Map::new(), path), None);
}

#[test]
fn a_last_commit_git_does_not_know_starts_again_from_tonight() {
    let state = std::env::temp_dir().join(format!(
        "nubo-review-units-unknown-commit-{}.json",
        std::process::id()
    ));
    fs::write(
        &state,
        r#"{"last_commit": "0123456789abcdef0123456789abcdef01234567", "files": {}}"#,
    )
    .unwrap();
    let tonight_commit = "89abcdef0123456789abcdef0123456789abcdef";
    let result = next_part(&NextOptions {
        state: state.clone(),
        date: String::from("2026-10-16"),
        commit: String::from(tonight_commit),
    });
    let written = fs::read_to_string(&state);
    fs::remove_file(&state).unwrap();

    let tonight: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(tonight["changes_since"], Value::Null);
    assert_eq!(tonight["changed_files"], json!([]));
    let written: Value = serde_json::from_str(&written.unwrap()).unwrap();
    assert_eq!(written["last_commit"], Value::from(tonight_commit));
}
