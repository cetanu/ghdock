use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub(crate) const MAX_NOTIFICATIONS: usize = 50;

#[derive(Debug, Clone)]
pub(crate) struct Pull {
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) draft: bool,
    pub(crate) author: String,
    pub(crate) url: String,
    pub(crate) reason: String,
    pub(crate) unread: bool,
    pub(crate) updated_at: String,
    pub(crate) updated_by_me: bool,
    pub(crate) approved_by_me: bool,
    pub(crate) comments: u64,
    pub(crate) review_comments: u64,
    pub(crate) commits: u64,
    pub(crate) additions: u64,
    pub(crate) deletions: u64,
    pub(crate) changed_files: u64,
}

impl Pull {
    pub(crate) fn key(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    pub(crate) fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.state,
            self.draft,
            self.updated_at,
            self.comments,
            self.review_comments,
            self.reason,
            self.title
        )
    }

    pub(crate) fn status_label(&self) -> &'static str {
        if self.draft {
            "DRAFT"
        } else {
            match self.state.as_str() {
                "open" => "OPEN",
                "closed" => "CLOSED",
                _ => "UNKNOWN",
            }
        }
    }

    pub(crate) fn activity_label(&self) -> String {
        match self.reason.as_str() {
            "review_requested" => "review requested".into(),
            "state_change" => "state changed".into(),
            "comment" => "new comment".into(),
            "mention" => "mentioned you".into(),
            "assign" => "assigned to you".into(),
            "author" => "your pull request".into(),
            "team_mention" => "team mentioned".into(),
            "subscribed" => "subscribed".into(),
            other => other.replace('_', " "),
        }
    }

    pub(crate) fn counts_as_update(&self) -> bool {
        !self.updated_by_me
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub(crate) pulls: Vec<Pull>,
    pub(crate) fetched_at: Instant,
}

pub(crate) fn snapshot_signature(
    snapshot: &Snapshot,
    show_closed: bool,
) -> HashMap<String, String> {
    snapshot
        .pulls
        .iter()
        .filter(|pull| show_closed || pull.state != "closed")
        .filter(|pull| !pull.approved_by_me)
        .map(|pull| (pull.key(), pull.fingerprint()))
        .collect()
}

pub(crate) fn next_poll_delay(
    current: Duration,
    changed: bool,
    base: Duration,
    cap: Duration,
) -> Duration {
    if changed {
        base
    } else {
        current.checked_mul(2).unwrap_or(cap).min(cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_polls_back_off_to_a_cap() {
        let base = Duration::from_secs(60);
        let cap = Duration::from_secs(300);
        assert_eq!(
            next_poll_delay(base, false, base, cap),
            Duration::from_secs(120)
        );
        assert_eq!(
            next_poll_delay(Duration::from_secs(240), false, base, cap),
            cap
        );
        assert_eq!(next_poll_delay(cap, false, base, cap), cap);
        assert_eq!(next_poll_delay(cap, true, base, cap), base);
    }
}
