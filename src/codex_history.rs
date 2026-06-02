use crate::paths::{ensure_dir_private, ensure_parent, set_mode};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

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
