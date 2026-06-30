use chrono::{DateTime, Utc};

use crate::ir::HistoryEvent;

#[derive(Debug, Clone)]
pub struct TimelinePlayer {
    events: Vec<HistoryEvent>,
    cursor_idx: usize,
    speed: f64,
}

impl TimelinePlayer {
    pub fn new(mut events: Vec<HistoryEvent>) -> Self {
        events.sort_by_key(|event| event.author_time);
        Self {
            events,
            cursor_idx: 0,
            speed: 1.0,
        }
    }

    pub fn events_up_to_cursor(&self) -> &[HistoryEvent] {
        &self.events[..self.cursor_idx]
    }

    pub fn cursor_time(&self) -> Option<DateTime<Utc>> {
        self.events
            .get(self.cursor_idx)
            .map(|event| event.author_time)
    }

    pub fn seek_to_event(&mut self, idx: usize) {
        self.cursor_idx = idx.min(self.events.len());
    }

    pub fn step_event(&mut self, delta: i32) {
        let cursor = self.cursor_idx.saturating_add_signed(delta as isize);
        self.seek_to_event(cursor);
    }

    pub fn total(&self) -> usize {
        self.events.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor_idx
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn set_speed(&mut self, s: f64) {
        self.speed = s;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ir::{Author, ChangeKind, FileChange, HistoryEvent, RepoId};

    fn event(secs: i64) -> HistoryEvent {
        HistoryEvent {
            repo: RepoId::new("fixture"),
            commit_oid: format!("commit-{secs}"),
            author: Author::normalized("Ada", "ada@example.com"),
            author_time: Utc.timestamp_opt(secs, 0).single().unwrap(),
            commit_time: Utc.timestamp_opt(secs, 0).single().unwrap(),
            changes: vec![FileChange {
                path: PathBuf::from("src/main.rs"),
                kind: ChangeKind::Modify,
                lines_added: 1,
                lines_deleted: 0,
            }],
            message: "fixture".to_string(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn new_sorts_events_by_author_time_ascending() {
        let player = TimelinePlayer::new(vec![event(30), event(10), event(20)]);

        let times: Vec<i64> = player
            .events
            .iter()
            .map(|event| event.author_time.timestamp())
            .collect();

        assert_eq!(times, vec![10, 20, 30]);
    }

    #[test]
    fn events_up_to_cursor_returns_expected_slice() {
        let mut player = TimelinePlayer::new(vec![event(10), event(20), event(30)]);

        assert_eq!(player.events_up_to_cursor().len(), 0);

        player.seek_to_event(2);
        assert_eq!(
            player
                .events_up_to_cursor()
                .iter()
                .map(|event| event.author_time.timestamp())
                .collect::<Vec<_>>(),
            vec![10, 20]
        );

        player.seek_to_event(player.total());
        assert_eq!(player.events_up_to_cursor().len(), 3);
    }

    #[test]
    fn step_event_clamps_forward_backward_and_out_of_bounds() {
        let mut player = TimelinePlayer::new(vec![event(10), event(20), event(30)]);

        player.step_event(2);
        assert_eq!(player.cursor(), 2);

        player.step_event(-1);
        assert_eq!(player.cursor(), 1);

        player.step_event(-10);
        assert_eq!(player.cursor(), 0);

        player.step_event(10);
        assert_eq!(player.cursor(), 3);
    }
}
