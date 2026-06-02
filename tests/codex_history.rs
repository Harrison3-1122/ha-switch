use assert_cmd::Command;
use predicates::str::contains;
use std::fs;

mod support;
use support::*;

#[test]
fn codex_profile_switch_preserves_history_files_and_sessions() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::create_dir_all(codex_home.path().join("sessions/2026/06/02")).unwrap();
    fs::write(codex_home.path().join("history.jsonl"), "{\"h\":1}\n").unwrap();
    fs::write(codex_home.path().join("session_index.jsonl"), "{\"s\":1}\n").unwrap();
    fs::write(
        codex_home
            .path()
            .join("sessions/2026/06/02/session-a.jsonl"),
        "{\"turn\":1}\n",
    )
    .unwrap();

    let history_before = snapshot_codex_history(codex_home.path());
    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a");
    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "a", "--kind", "oauth_capture"])
        .assert()
        .success();
    write_codex_oauth(codex_home.path(), "acct-b", "refresh-b");
    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "b", "--kind", "oauth_capture"])
        .assert()
        .success();

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-a", "--yes"])
        .assert()
        .success();
    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-b", "--yes"])
        .assert()
        .success();

    assert_eq!(snapshot_codex_history(codex_home.path()), history_before);
}

#[test]
fn codex_history_migrate_dry_run_does_not_write() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let from = tempfile::Builder::new()
        .prefix(".test-codex-from-")
        .tempdir_in(&cwd)
        .unwrap();
    let to = tempfile::Builder::new()
        .prefix(".test-codex-to-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::write(from.path().join("history.jsonl"), "{\"h\":1}\n").unwrap();
    fs::create_dir_all(from.path().join("sessions/2026/06/02")).unwrap();
    fs::write(
        from.path().join("sessions/2026/06/02/session-a.jsonl"),
        "{\"turn\":1}\n",
    )
    .unwrap();

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .args([
            "codex-history",
            "migrate",
            "--from",
            from.path().to_str().unwrap(),
            "--to",
            to.path().to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(contains("codex_history\tdry-run"))
        .stdout(contains("jsonl\thistory.jsonl\tadded=1"))
        .stdout(contains("sessions\tcopied=1"));

    assert!(!to.path().join("history.jsonl").exists());
    assert!(!to
        .path()
        .join("sessions/2026/06/02/session-a.jsonl")
        .exists());
}

#[test]
fn codex_history_migrate_merges_without_touching_auth_or_config() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let from = tempfile::Builder::new()
        .prefix(".test-codex-from-")
        .tempdir_in(&cwd)
        .unwrap();
    let to = tempfile::Builder::new()
        .prefix(".test-codex-to-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::write(from.path().join("history.jsonl"), "{\"h\":1}\n{\"h\":2}\n").unwrap();
    fs::write(to.path().join("history.jsonl"), "{\"h\":1}\n").unwrap();
    fs::write(from.path().join("session_index.jsonl"), "{\"s\":1}\n").unwrap();
    fs::write(to.path().join("auth.json"), "{\"auth\":\"keep\"}\n").unwrap();
    fs::write(to.path().join("config.toml"), "model = \"keep\"\n").unwrap();
    fs::create_dir_all(from.path().join("sessions/2026/06/02")).unwrap();
    fs::write(
        from.path().join("sessions/2026/06/02/session-a.jsonl"),
        "{\"turn\":1}\n",
    )
    .unwrap();

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .args([
            "codex-history",
            "migrate",
            "--from",
            from.path().to_str().unwrap(),
            "--to",
            to.path().to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .success()
        .stdout(contains("codex_history\tmigrated"))
        .stdout(contains("jsonl\thistory.jsonl\tadded=1\texisting=1"))
        .stdout(contains("sessions\tcopied=1"));

    assert_eq!(
        fs::read_to_string(to.path().join("history.jsonl")).unwrap(),
        "{\"h\":1}\n{\"h\":2}\n"
    );
    assert_eq!(
        fs::read_to_string(to.path().join("session_index.jsonl")).unwrap(),
        "{\"s\":1}\n"
    );
    assert_eq!(
        fs::read_to_string(to.path().join("sessions/2026/06/02/session-a.jsonl")).unwrap(),
        "{\"turn\":1}\n"
    );
    assert_eq!(
        fs::read_to_string(to.path().join("auth.json")).unwrap(),
        "{\"auth\":\"keep\"}\n"
    );
    assert_eq!(
        fs::read_to_string(to.path().join("config.toml")).unwrap(),
        "model = \"keep\"\n"
    );
}

#[test]
fn codex_history_migrate_rejects_conflicting_session_file() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let from = tempfile::Builder::new()
        .prefix(".test-codex-from-")
        .tempdir_in(&cwd)
        .unwrap();
    let to = tempfile::Builder::new()
        .prefix(".test-codex-to-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::create_dir_all(from.path().join("sessions/2026/06/02")).unwrap();
    fs::create_dir_all(to.path().join("sessions/2026/06/02")).unwrap();
    let relative = "sessions/2026/06/02/session-a.jsonl";
    fs::write(from.path().join(relative), "from\n").unwrap();
    fs::write(to.path().join(relative), "to\n").unwrap();

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .args([
            "codex-history",
            "migrate",
            "--from",
            from.path().to_str().unwrap(),
            "--to",
            to.path().to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(contains("HistoryConflict"));

    assert_eq!(
        fs::read_to_string(to.path().join(relative)).unwrap(),
        "to\n"
    );
}

#[test]
fn doctor_codex_reports_shared_history_home() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains("codex_home"))
        .stdout(contains("codex_history\tshared"));
}

fn snapshot_codex_history(codex_home: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for relative in [
        "history.jsonl",
        "session_index.jsonl",
        "sessions/2026/06/02/session-a.jsonl",
    ] {
        files.push((
            relative.to_string(),
            fs::read(codex_home.join(relative)).unwrap(),
        ));
    }
    files
}
