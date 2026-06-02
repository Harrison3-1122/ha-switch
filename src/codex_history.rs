use crate::app_definitions::system_definition;
use crate::paths::{ensure_dir_private, ensure_parent, set_mode};
use crate::process;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct MigrationOptions {
    pub from: PathBuf,
    pub to: PathBuf,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationSummary {
    pub from: PathBuf,
    pub to: PathBuf,
    pub dry_run: bool,
    pub history_added: usize,
    pub history_existing: usize,
    pub session_index_added: usize,
    pub session_index_existing: usize,
    pub sessions_copied: usize,
    pub sessions_skipped: usize,
    pub session_conflicts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRewriteOptions {
    pub codex_home: PathBuf,
    pub database: Option<PathBuf>,
    pub to: String,
    pub exclude: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRewriteSummary {
    pub database: PathBuf,
    pub dry_run: bool,
    pub to: String,
    pub excluded: Vec<String>,
    pub changed: usize,
    pub backup: Option<PathBuf>,
    pub session_backup_dir: Option<PathBuf>,
    pub session_files_changed: usize,
    pub providers: Vec<ProviderRewriteProvider>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRewriteProvider {
    pub from: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
struct ProviderRewriteThread {
    rollout_path: PathBuf,
}

#[derive(Debug, Clone)]
struct SessionPlan {
    relative_path: PathBuf,
    source: PathBuf,
    target: PathBuf,
    action: SessionAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionAction {
    Copy,
    Skip,
    Conflict,
}

pub fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.join(".codex")
        })
}

pub fn expand_user_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        home::home_dir().unwrap_or_else(|| path.to_path_buf())
    } else if let Some(rest) = text.strip_prefix("~/") {
        home::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

pub fn migrate(options: &MigrationOptions) -> Result<MigrationSummary> {
    let from = expand_user_path(&options.from);
    let to = expand_user_path(&options.to);
    if from == to {
        return Err(anyhow!(
            "HistoryInvalid: --from and --to must be different Codex homes"
        ));
    }
    if !from.exists() {
        return Err(anyhow!("HistoryMissing: {} does not exist", from.display()));
    }
    if !from.is_dir() {
        return Err(anyhow!(
            "HistoryInvalid: {} is not a directory",
            from.display()
        ));
    }

    let mut summary = MigrationSummary {
        from: from.clone(),
        to: to.clone(),
        dry_run: options.dry_run,
        ..MigrationSummary::default()
    };

    let history = jsonl_plan(&from.join("history.jsonl"), &to.join("history.jsonl"))?;
    summary.history_added = history.added.len();
    summary.history_existing = history.existing;
    let session_index = jsonl_plan(
        &from.join("session_index.jsonl"),
        &to.join("session_index.jsonl"),
    )?;
    summary.session_index_added = session_index.added.len();
    summary.session_index_existing = session_index.existing;

    let sessions = session_plan(&from.join("sessions"), &to.join("sessions"))?;
    for item in &sessions {
        match item.action {
            SessionAction::Copy => summary.sessions_copied += 1,
            SessionAction::Skip => summary.sessions_skipped += 1,
            SessionAction::Conflict => summary
                .session_conflicts
                .push(item.relative_path.display().to_string()),
        }
    }
    if !summary.session_conflicts.is_empty() {
        return Err(anyhow!(
            "HistoryConflict: {} session file(s) already exist with different contents: {}",
            summary.session_conflicts.len(),
            summary.session_conflicts.join(", ")
        ));
    }
    if options.dry_run {
        return Ok(summary);
    }

    append_jsonl_lines(&to.join("history.jsonl"), &history.added)?;
    append_jsonl_lines(&to.join("session_index.jsonl"), &session_index.added)?;
    for item in sessions {
        if item.action == SessionAction::Copy {
            if let Some(parent) = item.target.parent() {
                ensure_dir_private(parent)?;
            }
            fs::copy(&item.source, &item.target).with_context(|| {
                format!(
                    "copy session {} to {}",
                    item.source.display(),
                    item.target.display()
                )
            })?;
            set_mode(&item.target, 0o600)?;
        }
    }
    Ok(summary)
}

pub fn rewrite_providers(options: &ProviderRewriteOptions) -> Result<ProviderRewriteSummary> {
    if options.to.trim().is_empty() {
        return Err(anyhow!("ProviderRewriteInvalid: --to must not be empty"));
    }
    let codex_home = expand_user_path(&options.codex_home);
    let database = options
        .database
        .as_ref()
        .map(|path| expand_user_path(path))
        .unwrap_or_else(|| codex_home.join("state_5.sqlite"));
    if !database.exists() {
        return Err(anyhow!(
            "ProviderRewriteMissing: {} does not exist",
            database.display()
        ));
    }
    let where_clause = provider_rewrite_where_clause(&options.to, &options.exclude);
    let rows = sqlite3_query(
        &database,
        &format!(
            "SELECT model_provider, COUNT(*) FROM threads WHERE {where_clause} GROUP BY model_provider ORDER BY COUNT(*) DESC;"
        ),
    )?;
    let providers = parse_provider_counts(&rows)?;
    let changed = providers.iter().map(|provider| provider.count).sum();
    let threads = provider_rewrite_threads(&database, &where_clause)?;
    let session_files =
        provider_rewrite_session_files(&codex_home, &threads, &options.to, &options.exclude)?;
    let mut summary = ProviderRewriteSummary {
        database: database.clone(),
        dry_run: options.dry_run,
        to: options.to.clone(),
        excluded: options.exclude.clone(),
        changed,
        backup: None,
        session_backup_dir: None,
        session_files_changed: session_files.len(),
        providers,
    };
    if options.dry_run || (changed == 0 && session_files.is_empty()) {
        return Ok(summary);
    }

    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let backup = database.with_file_name(format!(
        "{}.provider-rewrite-{}.bak",
        database
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state_5.sqlite"),
        timestamp
    ));
    let session_backup_dir = database.with_file_name(format!(
        "{}.provider-rewrite-{}.sessions",
        database
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state_5.sqlite"),
        timestamp
    ));
    sqlite3_backup(&database, &backup)?;
    rewrite_provider_session_files(
        &codex_home,
        &session_files,
        &session_backup_dir,
        &options.to,
        &options.exclude,
    )?;
    sqlite3_query(
        &database,
        &format!(
            "BEGIN IMMEDIATE; UPDATE threads SET model_provider = {} WHERE {where_clause}; COMMIT;",
            sqlite_quote(&options.to)
        ),
    )?;
    summary.backup = Some(backup);
    summary.session_backup_dir = Some(session_backup_dir);
    Ok(summary)
}

pub fn running_codex_message() -> Result<Option<String>> {
    let Some(definition) = system_definition("codex")? else {
        return Ok(None);
    };
    let running = process::detect_running(&definition)?
        .into_iter()
        .filter(|process| !is_ignorable_codex_helper(&process.command))
        .collect::<Vec<_>>();
    if running.is_empty() {
        Ok(None)
    } else {
        Ok(Some(process::format_app_running("codex", &running)))
    }
}

fn is_ignorable_codex_helper(command: &str) -> bool {
    command.contains("crashpad_handler")
        || command.contains("SkyComputerUseClient turn-ended")
        || command.contains("Codex Computer Use.app/Contents/MacOS/SkyComputerUseService")
}

fn provider_rewrite_where_clause(to: &str, exclude: &[String]) -> String {
    let mut clauses = vec![format!("model_provider <> {}", sqlite_quote(to))];
    if !exclude.is_empty() {
        let excluded = exclude
            .iter()
            .map(|value| sqlite_quote(value))
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("model_provider NOT IN ({excluded})"));
    }
    clauses.join(" AND ")
}

fn provider_rewrite_threads(
    database: &Path,
    where_clause: &str,
) -> Result<Vec<ProviderRewriteThread>> {
    let rows = sqlite3_query(
        database,
        &format!("SELECT rollout_path FROM threads WHERE {where_clause};"),
    )?;
    rows.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            Ok(ProviderRewriteThread {
                rollout_path: PathBuf::from(line),
            })
        })
        .collect()
}

fn provider_rewrite_session_files(
    codex_home: &Path,
    threads: &[ProviderRewriteThread],
    to: &str,
    exclude: &[String],
) -> Result<Vec<PathBuf>> {
    let mut files = threads
        .iter()
        .map(|thread| thread.rollout_path.clone())
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<BTreeSet<_>>();
    collect_rewrite_session_files(&codex_home.join("sessions"), to, exclude, &mut files)?;
    Ok(files.into_iter().collect())
}

fn collect_rewrite_session_files(
    dir: &Path,
    to: &str,
    exclude: &[String],
    files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_rewrite_session_files(&path, to, exclude, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
            && session_file_needs_provider_rewrite(&path, to, exclude)?
        {
            files.insert(path);
        }
    }
    Ok(())
}

fn session_file_needs_provider_rewrite(path: &Path, to: &str, exclude: &[String]) -> Result<bool> {
    let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("parse Codex session JSONL {}", path.display()))?;
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let Some(provider) = value
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("model_provider"))
            .and_then(Value::as_str)
        else {
            return Ok(false);
        };
        return Ok(provider != to && !exclude.iter().any(|excluded| excluded == provider));
    }
    Ok(false)
}

fn rewrite_provider_session_files(
    codex_home: &Path,
    session_files: &[PathBuf],
    backup_dir: &Path,
    to: &str,
    exclude: &[String],
) -> Result<()> {
    let mut wrote_any = false;
    for path in session_files {
        if !path.exists() {
            continue;
        }
        let original = fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let (rewritten, changed) = rewrite_provider_session_text(&original, to, exclude)?;
        if !changed {
            continue;
        }
        if !wrote_any {
            ensure_dir_private(backup_dir)?;
            wrote_any = true;
        }
        let backup = session_backup_path(codex_home, backup_dir, path);
        if let Some(parent) = backup.parent() {
            ensure_dir_private(parent)?;
        }
        fs::copy(path, &backup).with_context(|| {
            format!("backup session {} to {}", path.display(), backup.display())
        })?;
        fs::write(path, rewritten).with_context(|| path.display().to_string())?;
        set_mode(path, 0o600)?;
    }
    Ok(())
}

fn rewrite_provider_session_text(
    text: &str,
    to: &str,
    exclude: &[String],
) -> Result<(String, bool)> {
    let has_trailing_newline = text.ends_with('\n');
    let mut changed = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            lines.push(line.to_string());
            continue;
        }
        let mut value: Value = serde_json::from_str(line)
            .with_context(|| "parse Codex session JSONL line for provider rewrite")?;
        let line_changed = rewrite_provider_session_value(&mut value, to, exclude);
        if line_changed {
            changed = true;
            lines.push(serde_json::to_string(&value)?);
        } else {
            lines.push(line.to_string());
        }
    }
    let mut rewritten = lines.join("\n");
    if has_trailing_newline {
        rewritten.push('\n');
    }
    Ok((rewritten, changed))
}

fn rewrite_provider_session_value(value: &mut Value, to: &str, exclude: &[String]) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return false;
    }
    let Some(provider) = value
        .get_mut("payload")
        .and_then(Value::as_object_mut)
        .and_then(|payload| payload.get_mut("model_provider"))
    else {
        return false;
    };
    let Some(current) = provider.as_str() else {
        return false;
    };
    if current == to || exclude.iter().any(|excluded| excluded == current) {
        return false;
    }
    *provider = Value::String(to.to_string());
    true
}

fn session_backup_path(codex_home: &Path, backup_dir: &Path, path: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(codex_home) {
        backup_dir.join(relative)
    } else {
        backup_dir.join(path.file_name().unwrap_or_default())
    }
}

fn sqlite3_query(database: &Path, sql: &str) -> Result<String> {
    let output = Command::new("sqlite3")
        .args(["-batch", "-noheader", "-separator", "\t"])
        .arg(database)
        .arg(sql)
        .output()
        .map_err(|err| anyhow!("ProviderRewriteUnavailable: failed to run sqlite3: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "ProviderRewriteSqliteFailed: sqlite3 exited with {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn sqlite3_backup(database: &Path, backup: &Path) -> Result<()> {
    let command = format!(".backup {}", sqlite_quote(&backup.display().to_string()));
    sqlite3_query(database, &command).map(|_| ())
}

fn parse_provider_counts(rows: &str) -> Result<Vec<ProviderRewriteProvider>> {
    rows.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let Some((from, count)) = line.split_once('\t') else {
                return Err(anyhow!("ProviderRewriteInvalidRow: {line}"));
            };
            Ok(ProviderRewriteProvider {
                from: from.to_string(),
                count: count.parse::<usize>()?,
            })
        })
        .collect()
}

fn sqlite_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[derive(Debug, Default)]
struct JsonlPlan {
    added: Vec<String>,
    existing: usize,
}

fn jsonl_plan(source: &Path, target: &Path) -> Result<JsonlPlan> {
    if !source.exists() {
        return Ok(JsonlPlan::default());
    }
    let mut seen = BTreeSet::new();
    let mut plan = JsonlPlan::default();
    if target.exists() {
        for line in read_nonempty_lines(target)? {
            seen.insert(line);
        }
    }
    for line in read_nonempty_lines(source)? {
        if seen.insert(line.clone()) {
            plan.added.push(line);
        } else {
            plan.existing += 1;
        }
    }
    Ok(plan)
}

fn read_nonempty_lines(path: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
    Ok(text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn append_jsonl_lines(path: &Path, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    ensure_parent(path)?;
    let needs_leading_newline =
        path.exists() && fs::metadata(path)?.len() > 0 && !fs::read(path)?.ends_with(b"\n");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if needs_leading_newline {
        writeln!(file)?;
    }
    for line in lines {
        writeln!(file, "{line}")?;
    }
    set_mode(path, 0o600)?;
    Ok(())
}

fn session_plan(source_dir: &Path, target_dir: &Path) -> Result<Vec<SessionPlan>> {
    if !source_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    collect_session_plan(source_dir, source_dir, target_dir, &mut out)?;
    out.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(out)
}

fn collect_session_plan(
    root: &Path,
    current: &Path,
    target_dir: &Path,
    out: &mut Vec<SessionPlan>,
) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| current.display().to_string())? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_session_plan(root, &path, target_dir, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .with_context(|| format!("strip session root {}", path.display()))?
            .to_path_buf();
        let target = target_dir.join(&relative_path);
        let action = if target.exists() {
            if fs::read(&path)? == fs::read(&target)? {
                SessionAction::Skip
            } else {
                SessionAction::Conflict
            }
        } else {
            SessionAction::Copy
        };
        out.push(SessionPlan {
            relative_path,
            source: path,
            target,
            action,
        });
    }
    Ok(())
}
