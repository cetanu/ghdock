use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::domain::{Pull, Snapshot};

pub(crate) struct App {
    pulls: Vec<Pull>,
    previous: HashMap<String, String>,
    selected: usize,
    pub(crate) interval: Duration,
    show_closed: bool,
    next_poll: Instant,
    last_fetch: Option<Instant>,
    alert: Option<String>,
    error: Option<String>,
    loading: bool,
    show_help: bool,
    first_snapshot: bool,
}

impl App {
    pub(crate) fn new(interval: Duration, show_closed: bool) -> Self {
        Self {
            pulls: Vec::new(),
            previous: HashMap::new(),
            selected: 0,
            interval,
            show_closed,
            next_poll: Instant::now(),
            last_fetch: None,
            alert: None,
            error: None,
            loading: true,
            show_help: false,
            first_snapshot: true,
        }
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: Snapshot, next_delay: Duration) -> bool {
        let mut changes = Vec::new();
        let mut current = HashMap::new();
        let pulls = snapshot
            .pulls
            .into_iter()
            .filter(|pull| self.show_closed || pull.state != "closed")
            .filter(|pull| !pull.approved_by_me)
            .collect::<Vec<_>>();
        for pull in &pulls {
            let key = pull.key();
            let fingerprint = pull.fingerprint();
            if !self.first_snapshot && pull.counts_as_update() {
                match self.previous.get(&key) {
                    None => changes.push(format!("{}#{} — new item", pull.repo, pull.number)),
                    Some(previous) if previous != &fingerprint => changes.push(format!(
                        "{}#{} — {}",
                        pull.repo,
                        pull.number,
                        pull.activity_label()
                    )),
                    _ => {}
                }
            }
            current.insert(key, fingerprint);
        }

        self.previous = current;
        self.pulls = pulls;
        self.last_fetch = Some(snapshot.fetched_at);
        self.next_poll = Instant::now() + next_delay;
        self.loading = false;
        self.error = None;
        self.selected = self.selected.min(self.pulls.len().saturating_sub(1));
        self.first_snapshot = false;
        if changes.is_empty() {
            false
        } else {
            self.alert = Some(changes.join("  •  "));
            true
        }
    }

    pub(crate) fn set_error(&mut self, error: String, next_delay: Duration) {
        self.loading = false;
        self.error = Some(error);
        self.next_poll = Instant::now() + next_delay;
    }

    pub(crate) fn start_poll(&mut self) {
        self.loading = true;
        self.error = None;
    }

    pub(crate) fn start_refresh(&mut self) {
        self.loading = true;
        self.error = None;
        self.next_poll = Instant::now() + self.interval;
    }

    pub(crate) fn select_next(&mut self) {
        if !self.pulls.is_empty() {
            self.selected = (self.selected + 1).min(self.pulls.len() - 1);
        }
    }

    pub(crate) fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(crate) fn pulls(&self) -> &[Pull] {
        &self.pulls
    }
    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }
    pub(crate) fn selected_pull(&self) -> Option<&Pull> {
        self.pulls.get(self.selected)
    }
    pub(crate) fn next_poll(&self) -> Instant {
        self.next_poll
    }
    pub(crate) fn last_fetch(&self) -> Option<Instant> {
        self.last_fetch
    }
    pub(crate) fn alert(&self) -> Option<&str> {
        self.alert.as_deref()
    }
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }
    pub(crate) fn is_help_visible(&self) -> bool {
        self.show_help
    }
    pub(crate) fn set_help_visible(&mut self, visible: bool) {
        self.show_help = visible;
    }
    pub(crate) fn clear_alert(&mut self) {
        self.alert = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(state: &str, reason: &str) -> Pull {
        Pull {
            repo: "acme/widget".into(),
            number: 42,
            title: "Improve widget".into(),
            state: state.into(),
            draft: false,
            author: "octocat".into(),
            url: "https://github.com/acme/widget/pull/42".into(),
            reason: reason.into(),
            unread: true,
            updated_at: "2026-01-01T00:00:00Z".into(),
            updated_by_me: false,
            approved_by_me: false,
            comments: 1,
            review_comments: 0,
            commits: 2,
            additions: 3,
            deletions: 1,
            changed_files: 1,
        }
    }

    fn snapshot(pulls: Vec<Pull>) -> Snapshot {
        Snapshot {
            pulls,
            fetched_at: Instant::now(),
        }
    }

    #[test]
    fn first_snapshot_is_silent_and_second_changed_snapshot_alerts() {
        let mut app = App::new(Duration::from_secs(60), true);
        assert!(!app.apply_snapshot(
            snapshot(vec![pull("open", "review_requested")]),
            Duration::from_secs(60)
        ));
        let mut changed = pull("closed", "state_change");
        changed.updated_at = "2026-01-01T01:00:00Z".into();
        assert!(app.apply_snapshot(snapshot(vec![changed]), Duration::from_secs(60)));
        assert!(app.alert().unwrap().contains("state changed"));
    }

    #[test]
    fn closed_pulls_are_hidden_by_default() {
        let mut app = App::new(Duration::from_secs(60), false);
        assert!(!app.apply_snapshot(
            snapshot(vec![pull("closed", "state_change")]),
            Duration::from_secs(60)
        ));
        assert!(app.pulls().is_empty());
    }

    #[test]
    fn closed_pulls_can_be_shown() {
        let mut app = App::new(Duration::from_secs(60), true);
        assert!(!app.apply_snapshot(
            snapshot(vec![pull("closed", "state_change")]),
            Duration::from_secs(60)
        ));
        assert_eq!(app.pulls().len(), 1);
    }

    #[test]
    fn approved_pulls_are_hidden() {
        let mut app = App::new(Duration::from_secs(60), true);
        let mut approved = pull("open", "review_requested");
        approved.approved_by_me = true;

        assert!(!app.apply_snapshot(snapshot(vec![approved]), Duration::from_secs(60)));
        assert!(app.pulls().is_empty());
    }

    #[test]
    fn my_activity_does_not_alert_but_other_activity_does() {
        let mut app = App::new(Duration::from_secs(60), true);
        assert!(!app.apply_snapshot(
            snapshot(vec![pull("open", "review_requested")]),
            Duration::from_secs(60)
        ));

        let mut mine = pull("open", "comment");
        mine.updated_at = "2026-01-01T01:00:00Z".into();
        mine.updated_by_me = true;
        assert!(!app.apply_snapshot(snapshot(vec![mine]), Duration::from_secs(60)));

        let mut theirs = pull("open", "comment");
        theirs.updated_at = "2026-01-01T02:00:00Z".into();
        assert!(app.apply_snapshot(snapshot(vec![theirs]), Duration::from_secs(60)));
    }

    #[test]
    fn polling_marks_the_app_as_loading_until_the_snapshot_arrives() {
        let mut app = App::new(Duration::from_secs(60), true);
        app.apply_snapshot(snapshot(Vec::new()), Duration::from_secs(60));

        app.start_poll();

        assert!(app.is_loading());
    }
}
