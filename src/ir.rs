use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoId(String);

impl RepoId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub email: String,
}

impl Author {
    pub fn normalized(name: impl AsRef<str>, email: impl AsRef<str>) -> Self {
        Self {
            name: name.as_ref().trim().to_string(),
            email: email.as_ref().trim().to_ascii_lowercase(),
        }
    }

    pub fn identity_key(&self) -> &str {
        &self.email
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub repo: RepoId,
    pub commit_oid: String,
    pub author: Author,
    pub author_time: DateTime<Utc>,
    pub commit_time: DateTime<Utc>,
    pub changes: Vec<FileChange>,
    pub message: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Add,
    Modify,
    Delete,
    Rename { from: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimelineTrack {
    pub git_events: Vec<HistoryEvent>,
    pub captions: Vec<Caption>,
    pub markers: Vec<Marker>,
    pub chapters: Vec<Chapter>,
}

impl TimelineTrack {
    pub fn from_git_events(git_events: Vec<HistoryEvent>) -> Self {
        Self {
            git_events,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caption {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub time: DateTime<Utc>,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub title: String,
    pub summary: Option<String>,
}
