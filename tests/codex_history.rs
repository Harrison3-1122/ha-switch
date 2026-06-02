use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::process::Command as ProcessCommand;

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

#[test]
fn codex_history_rewrite_providers_dry_run_does_not_write() {
    if !sqlite3_available() {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    create_state_db(codex_home.path(), &["openai", "ikun", "token", "ha-shared"]);

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args([
            "codex-history",
            "rewrite-providers",
            "--to",
            "ha-shared",
            "--exclude",
            "openai",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(contains("provider-rewrite\tdry-run"))
        .stdout(contains(
            "providers\tto=ha-shared\texcluded=openai\tchanged=2",
        ))
        .stdout(contains("provider\tfrom=ikun\tcount=1"))
        .stdout(contains("provider\tfrom=token\tcount=1"))
        .stdout(contains("session_files\tchanged=2"));

    assert_eq!(
        provider_counts(codex_home.path()),
        vec![
            ("ha-shared".to_string(), 1),
            ("ikun".to_string(), 1),
            ("openai".to_string(), 1),
            ("token".to_string(), 1),
        ]
    );
    assert_eq!(session_provider(codex_home.path(), 1), "ikun");
    assert_eq!(session_provider(codex_home.path(), 2), "token");
}

#[test]
fn codex_history_rewrite_providers_merges_non_excluded_rows() {
    if !sqlite3_available() {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    create_state_db(
        codex_home.path(),
        &["openai", "ikun", "token", "IkunCoding", "ha-shared"],
    );
    write_session(codex_home.path(), 99, "IkunCoding");

    Command::cargo_bin("ha-switch")
        .unwrap()
        .env("HA_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args([
            "codex-history",
            "rewrite-providers",
            "--to",
            "ha-shared",
            "--exclude",
            "openai",
            "--yes",
            "--allow-running",
        ])
        .assert()
        .success()
        .stdout(contains("provider-rewrite\trewritten"))
        .stdout(contains(
            "providers\tto=ha-shared\texcluded=openai\tchanged=3",
        ))
        .stdout(contains("session_files\tchanged=4"))
        .stdout(contains("backup\t"))
        .stdout(contains("session_backup\t"));

    assert_eq!(
        provider_counts(codex_home.path()),
        vec![("ha-shared".to_string(), 4), ("openai".to_string(), 1)]
    );
    assert_eq!(session_provider(codex_home.path(), 0), "openai");
    assert_eq!(session_provider(codex_home.path(), 1), "ha-shared");
    assert_eq!(session_provider(codex_home.path(), 2), "ha-shared");
    assert_eq!(session_provider(codex_home.path(), 3), "ha-shared");
    assert_eq!(session_provider(codex_home.path(), 99), "ha-shared");
    assert!(fs::read_dir(codex_home.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("provider-rewrite")));
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

fn sqlite3_available() -> bool {
    ProcessCommand::new("sqlite3")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn create_state_db(codex_home: &std::path::Path, providers: &[&str]) {
    let db = codex_home.join("state_5.sqlite");
    let mut sql = String::from(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL, archived INTEGER NOT NULL DEFAULT 0);",
    );
    for (index, provider) in providers.iter().enumerate() {
        let session = write_session(codex_home, index, provider);
        sql.push_str(&format!(
            "INSERT INTO threads (id, rollout_path, model_provider, archived) VALUES ('thread-{index}', '{}', '{}', 0);",
            session.display().to_string().replace('\'', "''"),
            provider.replace('\'', "''")
        ));
    }
    let output = ProcessCommand::new("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_session(codex_home: &std::path::Path, index: usize, provider: &str) -> std::path::PathBuf {
    let session = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("02")
        .join(format!("thread-{index}.jsonl"));
    fs::create_dir_all(session.parent().unwrap()).unwrap();
    fs::write(
        &session,
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"thread-{index}\",\"model_provider\":\"{}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\"}}}}\n",
            provider.replace('"', "\\\"")
        ),
    )
    .unwrap();
    session
}

fn provider_counts(codex_home: &std::path::Path) -> Vec<(String, usize)> {
    let output = ProcessCommand::new("sqlite3")
        .args(["-batch", "-noheader", "-separator", "\t"])
        .arg(codex_home.join("state_5.sqlite"))
        .arg("SELECT model_provider, COUNT(*) FROM threads GROUP BY model_provider ORDER BY model_provider;")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let (provider, count) = line.split_once('\t').unwrap();
            (provider.to_string(), count.parse().unwrap())
        })
        .collect()
}

fn session_provider(codex_home: &std::path::Path, index: usize) -> String {
    let session = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("02")
        .join(format!("thread-{index}.jsonl"));
    let line = fs::read_to_string(session)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    value["payload"]["model_provider"]
        .as_str()
        .unwrap()
        .to_string()
}
