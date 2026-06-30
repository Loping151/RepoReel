use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};

const MAX_DIFF_LINES_PER_KIND: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub commit_oid: String,
    pub author: String,
    pub date: DateTime<Utc>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

pub fn recent_file_diff(repo_path: &Path, file_path: &str) -> Result<Option<FileDiff>> {
    let log_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg("-1")
        .arg("--date=unix")
        .arg("--pretty=format:%H%x00%an%x00%at%x00")
        .arg("-z")
        .arg("--")
        .arg(file_path)
        .output()
        .with_context(|| format!("failed to run git log in {}", repo_path.display()))?;

    if !log_output.status.success() {
        let stderr = String::from_utf8_lossy(&log_output.stderr);
        if stderr.contains("does not have any commits yet") {
            return Ok(None);
        }

        bail!(
            "git log failed in {}: {}",
            repo_path.display(),
            stderr.trim()
        );
    }
    if log_output.stdout.is_empty() {
        return Ok(None);
    }

    let (commit_oid, author, date) = parse_recent_commit(&log_output.stdout)?;
    let show_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg("--format=")
        .arg("--no-ext-diff")
        .arg("--unified=0")
        .arg(&commit_oid)
        .arg("--")
        .arg(file_path)
        .output()
        .with_context(|| format!("failed to run git show in {}", repo_path.display()))?;

    if !show_output.status.success() {
        bail!(
            "git show failed in {}: {}",
            repo_path.display(),
            String::from_utf8_lossy(&show_output.stderr).trim()
        );
    }

    let (added, removed) = parse_unified_diff(&show_output.stdout);
    Ok(Some(FileDiff {
        commit_oid,
        author,
        date,
        added,
        removed,
    }))
}

fn parse_recent_commit(bytes: &[u8]) -> Result<(String, String, DateTime<Utc>)> {
    let mut fields = bytes.split(|byte| *byte == 0);
    let commit_oid = utf8_field(fields.next(), "commit oid")?.to_string();
    let author = utf8_field(fields.next(), "author")?.to_string();
    let timestamp = utf8_field(fields.next(), "author time")?
        .trim()
        .parse::<i64>()
        .context("invalid git author timestamp")?;
    let date = Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .context("invalid git author timestamp")?;

    Ok((commit_oid, author, date))
}

fn utf8_field<'a>(field: Option<&'a [u8]>, label: &'static str) -> Result<&'a str> {
    let field = field.with_context(|| format!("missing {label} from git log output"))?;
    std::str::from_utf8(field).with_context(|| format!("invalid utf-8 in {label}"))
}

fn parse_unified_diff(bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    let text = String::from_utf8_lossy(bytes);
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for line in text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }

        if let Some(value) = line.strip_prefix('+') {
            if added.len() < MAX_DIFF_LINES_PER_KIND {
                added.push(value.to_string());
            }
        } else if let Some(value) = line.strip_prefix('-')
            && removed.len() < MAX_DIFF_LINES_PER_KIND
        {
            removed.push(value.to_string());
        }
    }

    (added, removed)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command as ProcessCommand,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("reporeel-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git -C {} {} failed: {}",
            repo.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(repo: &Path, message: &str, secs: i64) {
        let date = format!("@{secs} +0000");
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(repo)
            .arg("commit")
            .arg("-q")
            .arg("-m")
            .arg(message)
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn recent_file_diff_reads_latest_added_and_removed_lines() {
        let repo = temp_test_dir("recent-diff");
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "Ada"]);
        git(&repo, &["config", "user.email", "ada@example.com"]);

        fs::write(repo.join("src.txt"), "old\nkeep\n").unwrap();
        git(&repo, &["add", "src.txt"]);
        commit(&repo, "initial", 1_700_000_000);

        fs::write(repo.join("src.txt"), "new\nkeep\n").unwrap();
        git(&repo, &["add", "src.txt"]);
        commit(&repo, "modify", 1_700_000_100);

        let diff = recent_file_diff(&repo, "src.txt").unwrap().unwrap();

        assert_eq!(diff.author, "Ada");
        assert_eq!(diff.date.timestamp(), 1_700_000_100);
        assert_eq!(diff.added, vec!["new"]);
        assert_eq!(diff.removed, vec!["old"]);
        fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn recent_file_diff_returns_none_for_unknown_file() {
        let repo = temp_test_dir("recent-diff-none");
        git(&repo, &["init", "-q"]);

        let diff = recent_file_diff(&repo, "missing.txt").unwrap();

        assert!(diff.is_none());
        fs::remove_dir_all(repo).ok();
    }
}
