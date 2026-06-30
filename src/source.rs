use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};

use crate::ir::{Author, ChangeKind, FileChange, HistoryEvent, RepoId};

const COMMIT_START: u8 = 0x1e;

pub trait HistorySource {
    fn events(&self) -> Result<Vec<HistoryEvent>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLogSource {
    pub repo_path: PathBuf,
    pub range: Option<RefRange>,
}

impl GitLogSource {
    pub fn new(repo_path: impl Into<PathBuf>, range: Option<RefRange>) -> Self {
        Self {
            repo_path: repo_path.into(),
            range,
        }
    }
}

impl HistorySource for GitLogSource {
    fn events(&self) -> Result<Vec<HistoryEvent>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_path)
            .arg("log")
            .arg("--reverse")
            .arg("--date=unix")
            .arg("-z")
            .arg("--decorate=short")
            .arg("--pretty=format:%x1e%H%x00%an%x00%ae%x00%at%x00%ct%x00%B%x00%D%x00")
            .args(self.range.iter().filter_map(RefRange::to_git_arg))
            .arg("--raw")
            .arg("--numstat")
            .output()
            .with_context(|| format!("failed to run git log in {}", self.repo_path.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("does not have any commits yet") {
                return Ok(Vec::new());
            }

            // Translate git's raw fatal messages into actionable hints.
            if stderr.contains("not a git repository") {
                bail!(
                    "{} is not a git repository.\n  hint: run reporeel inside a git repo, or pass --repo <path>",
                    self.repo_path.display()
                );
            }
            if stderr.contains("not a directory") || stderr.contains("Not a directory") {
                bail!(
                    "repository path is not a directory: {}\n  hint: --repo must point to a directory, not a file",
                    self.repo_path.display()
                );
            }
            if stderr.contains("No such file or directory") || stderr.contains("No such directory")
            {
                bail!(
                    "repository path does not exist: {}\n  hint: check the --repo path",
                    self.repo_path.display()
                );
            }
            if stderr.contains("ambiguous argument") {
                let git_stderr = stderr.lines().next().unwrap_or_default().trim();
                if let Some(git_ref) = unresolved_ref_for_error(self.range.as_ref(), git_stderr) {
                    bail!(
                        "git ref '{git_ref}' not found. --from/--to take refs like HEAD~30, v1.0, or a branch name. git said: {git_stderr}"
                    );
                } else {
                    bail!(
                        "git could not resolve the --from/--to range.\n  hint: --from/--to take git refs like HEAD~30, v1.0, or a branch name, not dates; check that the ref exists with git log <ref>.\n  git said: {git_stderr}"
                    );
                }
            }

            bail!(
                "git log failed in {}: {}",
                self.repo_path.display(),
                stderr.trim()
            );
        }

        let repo_id = RepoId::new(repo_namespace(&self.repo_path));
        parse_git_log_output(&output.stdout, repo_id).map_err(Into::into)
    }
}

fn unresolved_ref_for_error<'a>(range: Option<&'a RefRange>, stderr: &str) -> Option<&'a str> {
    let range = range?;
    if let Some(from) = range.from.as_deref()
        && stderr.contains(from)
    {
        return Some(from);
    }
    if let Some(to) = range.to.as_deref()
        && stderr.contains(to)
    {
        return Some(to);
    }
    range.from.as_deref().or(range.to.as_deref())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl RefRange {
    pub fn to_git_arg(&self) -> Option<OsString> {
        match (self.from.as_deref(), self.to.as_deref()) {
            (Some(from), Some(to)) => Some(format!("{from}..{to}").into()),
            (Some(from), None) => Some(format!("{from}..HEAD").into()),
            (None, Some(to)) => Some(to.into()),
            (None, None) => None,
        }
    }
}

#[derive(Debug)]
pub enum GitLogParseError {
    MissingField {
        field: &'static str,
    },
    Utf8 {
        field: &'static str,
        source: std::str::Utf8Error,
    },
    Timestamp {
        field: &'static str,
        value: String,
    },
}

impl std::fmt::Display for GitLogParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "commit record is missing {field}"),
            Self::Utf8 { field, .. } => write!(f, "invalid utf-8 in {field}"),
            Self::Timestamp { field, value } => {
                write!(f, "invalid unix timestamp in {field}: {value}")
            }
        }
    }
}

impl std::error::Error for GitLogParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Utf8 { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn parse_git_log_output(
    bytes: &[u8],
    repo: RepoId,
) -> std::result::Result<Vec<HistoryEvent>, GitLogParseError> {
    bytes
        .split(|byte| *byte == COMMIT_START)
        .filter(|chunk| !chunk.trim_ascii().is_empty())
        .map(|chunk| parse_commit(chunk, repo.clone()))
        .collect()
}

fn parse_commit(chunk: &[u8], repo: RepoId) -> std::result::Result<HistoryEvent, GitLogParseError> {
    let mut fields = chunk.splitn(8, |byte| *byte == 0);
    let commit_oid = utf8_field(fields.next(), "commit_oid")?.to_string();
    let author_name = utf8_field(fields.next(), "author_name")?;
    let author_email = utf8_field(fields.next(), "author_email")?;
    let author_time = parse_unix_time(utf8_field(fields.next(), "author_time")?, "author_time")?;
    let commit_time = parse_unix_time(utf8_field(fields.next(), "commit_time")?, "commit_time")?;
    let message = utf8_field(fields.next(), "message")?.trim_end().to_string();
    let tags = parse_tags(utf8_field(fields.next(), "tags")?);
    let file_block = fields.next().unwrap_or_default();

    Ok(HistoryEvent {
        repo,
        commit_oid,
        author: Author::normalized(author_name, author_email),
        author_time,
        commit_time,
        changes: parse_file_changes(file_block),
        message,
        tags,
    })
}

fn utf8_field<'a>(
    field: Option<&'a [u8]>,
    name: &'static str,
) -> std::result::Result<&'a str, GitLogParseError> {
    let field = field.ok_or(GitLogParseError::MissingField { field: name })?;
    std::str::from_utf8(field).map_err(|source| GitLogParseError::Utf8 {
        field: name,
        source,
    })
}

fn parse_unix_time(
    value: &str,
    field: &'static str,
) -> std::result::Result<DateTime<Utc>, GitLogParseError> {
    let secs = value
        .trim()
        .parse::<i64>()
        .map_err(|_| GitLogParseError::Timestamp {
            field,
            value: value.to_string(),
        })?;
    Utc.timestamp_opt(secs, 0)
        .single()
        .ok_or_else(|| GitLogParseError::Timestamp {
            field,
            value: value.to_string(),
        })
}

fn parse_tags(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|part| part.trim().strip_prefix("tag: "))
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_file_changes(block: &[u8]) -> Vec<FileChange> {
    let tokens: Vec<&[u8]> = block
        .split(|byte| *byte == 0)
        .map(trim_ascii_noise)
        .filter(|token| !token.is_empty())
        .collect();

    let mut numstats = HashMap::new();
    for token in &tokens {
        if let Some((path, added, deleted)) = parse_numstat(token) {
            numstats.insert(path, (added, deleted));
        }
    }

    let mut changes = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if parse_numstat(token).is_some() {
            index += 1;
            continue;
        }

        if let Some((status, path)) = parse_inline_status(token) {
            push_status_change(&mut changes, &numstats, status, path);
            index += 1;
            continue;
        }

        if let Some(status) = parse_raw_status_token(token).or_else(|| parse_status_token(token)) {
            if status.starts_with('R') {
                if index + 2 < tokens.len() {
                    let old_path = path_from_bytes(tokens[index + 1]);
                    let new_path = path_from_bytes(tokens[index + 2]);
                    let delete_stats = numstats.get(&new_path).copied().unwrap_or_default();
                    let add_stats = numstats.get(&old_path).copied().unwrap_or_default();
                    push_change(&mut changes, ChangeKind::Delete, old_path, delete_stats);
                    push_change(&mut changes, ChangeKind::Add, new_path, add_stats);
                    index += 3;
                    continue;
                }
            } else if index + 1 < tokens.len() {
                let path = path_from_bytes(tokens[index + 1]);
                push_status_change(&mut changes, &numstats, status, path);
                index += 2;
                continue;
            }
        }

        index += 1;
    }

    if changes.is_empty() {
        for (path, stats) in numstats {
            push_change(&mut changes, ChangeKind::Modify, path, stats);
        }
    }

    changes
}

fn parse_numstat(token: &[u8]) -> Option<(PathBuf, u32, u32)> {
    let mut parts = token.splitn(3, |byte| *byte == b'\t');
    let added = parse_line_count(parts.next()?)?;
    let deleted = parse_line_count(parts.next()?)?;
    let path = parts.next()?;
    Some((path_from_bytes(path), added, deleted))
}

fn parse_line_count(bytes: &[u8]) -> Option<u32> {
    if bytes == b"-" {
        return Some(0);
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

fn parse_inline_status(token: &[u8]) -> Option<(&str, PathBuf)> {
    let (status, path) = token.split_once_byte(b'\t')?;
    let status = parse_status_token(status)?;
    Some((status, path_from_bytes(path)))
}

fn parse_status_token(token: &[u8]) -> Option<&str> {
    let status = std::str::from_utf8(token).ok()?;
    let mut chars = status.chars();
    let first = chars.next()?;
    let is_known = matches!(first, 'A' | 'M' | 'D' | 'R' | 'C' | 'T');
    if is_known && chars.all(|ch| ch.is_ascii_digit()) {
        Some(status)
    } else {
        None
    }
}

fn parse_raw_status_token(token: &[u8]) -> Option<&str> {
    if !token.starts_with(b":") {
        return None;
    }

    let raw = std::str::from_utf8(token).ok()?;
    parse_status_token(raw.rsplit_once(' ')?.1.as_bytes())
}

fn push_status_change(
    changes: &mut Vec<FileChange>,
    numstats: &HashMap<PathBuf, (u32, u32)>,
    status: &str,
    path: PathBuf,
) {
    let kind = match status.as_bytes()[0] {
        b'A' => ChangeKind::Add,
        b'D' => ChangeKind::Delete,
        _ => ChangeKind::Modify,
    };
    push_change(
        changes,
        kind,
        path.clone(),
        numstats.get(&path).copied().unwrap_or_default(),
    );
}

fn push_change(changes: &mut Vec<FileChange>, kind: ChangeKind, path: PathBuf, stats: (u32, u32)) {
    changes.push(FileChange {
        path,
        kind,
        lines_added: stats.0,
        lines_deleted: stats.1,
    });
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn trim_ascii_noise(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b'\n' | b'\r')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn repo_namespace(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

trait ByteSplitOnce {
    fn split_once_byte(&self, needle: u8) -> Option<(&[u8], &[u8])>;
}

impl ByteSplitOnce for [u8] {
    fn split_once_byte(&self, needle: u8) -> Option<(&[u8], &[u8])> {
        let index = self.iter().position(|byte| *byte == needle)?;
        Some((&self[..index], &self[index + 1..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ChangeKind;

    fn fixture_repo() -> RepoId {
        RepoId::new("fixture")
    }

    #[test]
    fn parses_single_commit_with_numstat_and_status() {
        let input = b"\x1eabc123\0Ada\0ADA@EXAMPLE.COM\x001700000000\x001700000005\0Initial import\n\0tag: v0.1\0\n12\t3\tsrc/main.rs\0A\0src/main.rs\0";

        let events = parse_git_log_output(input, fixture_repo()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].commit_oid, "abc123");
        assert_eq!(events[0].author.email, "ada@example.com");
        assert_eq!(events[0].message, "Initial import");
        assert_eq!(events[0].tags, vec!["v0.1"]);
        assert_eq!(events[0].changes.len(), 1);
        assert_eq!(events[0].changes[0].kind, ChangeKind::Add);
        assert_eq!(events[0].changes[0].lines_added, 12);
        assert_eq!(events[0].changes[0].lines_deleted, 3);
    }

    #[test]
    fn parses_multiple_commits_and_delete_status() {
        let input = b"\x1efirst\0Ada\0ada@example.com\x001700000000\x001700000000\0one\0\0\n1\t0\tREADME.md\0A\0README.md\0\x1esecond\0Lin\0lin@example.com\x001700000100\x001700000110\0two\0\0\n0\t8\told.rs\0D\0old.rs\0";

        let events = parse_git_log_output(input, fixture_repo()).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].changes[0].kind, ChangeKind::Add);
        assert_eq!(events[1].changes[0].kind, ChangeKind::Delete);
        assert_eq!(events[1].changes[0].lines_deleted, 8);
    }

    #[test]
    fn parses_raw_status_with_numstat() {
        let input = b"\x1eraw\0Ada\0ada@example.com\x001700000000\x001700000000\0raw diff\0\0\n:000000 100644 0000000 ce01362 A\0README.md\x001\t0\tREADME.md\0";

        let events = parse_git_log_output(input, fixture_repo()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].changes.len(), 1);
        assert_eq!(events[0].changes[0].kind, ChangeKind::Add);
        assert_eq!(events[0].changes[0].lines_added, 1);
    }

    #[test]
    fn treats_rename_status_as_delete_plus_add() {
        let input = b"\x1erename\0Ada\0ada@example.com\x001700000000\x001700000000\0rename file\0\0\n2\t1\tnew.rs\0R100\0old.rs\0new.rs\0";

        let events = parse_git_log_output(input, fixture_repo()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].changes.len(), 2);
        assert_eq!(events[0].changes[0].kind, ChangeKind::Delete);
        assert_eq!(events[0].changes[0].path, PathBuf::from("old.rs"));
        assert_eq!(events[0].changes[1].kind, ChangeKind::Add);
        assert_eq!(events[0].changes[1].path, PathBuf::from("new.rs"));
    }
}
