use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub(crate) const MAX_NOTIFICATIONS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboxSection {
    NeedsYourReview,
    NeedsTeamReview,
    Drafts,
    Waiting,
    NeedsAction,
    ReadyToMerge,
}

impl InboxSection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NeedsYourReview => "NEEDS YOUR REVIEW",
            Self::NeedsTeamReview => "NEEDS YOUR TEAM'S REVIEW",
            Self::Drafts => "YOUR DRAFTS",
            Self::Waiting => "WAITING FOR REVIEW OR CHECKS",
            Self::NeedsAction => "NEEDS ACTION",
            Self::ReadyToMerge => "READY TO MERGE",
        }
    }

    pub(crate) fn rank(self) -> usize {
        match self {
            Self::NeedsYourReview => 0,
            Self::NeedsTeamReview => 1,
            Self::Drafts => 2,
            Self::Waiting => 3,
            Self::NeedsAction => 4,
            Self::ReadyToMerge => 5,
        }
    }
}

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
    pub(crate) ready_to_merge: bool,
    pub(crate) is_author: bool,
    pub(crate) review_requested: bool,
    pub(crate) team_review_requested: bool,
    pub(crate) assigned_to_me: bool,
    pub(crate) review_status: String,
    pub(crate) checks_passed: u64,
    pub(crate) checks_total: u64,
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
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.state,
            self.draft,
            self.updated_at,
            self.comments,
            self.review_comments,
            self.reason,
            self.title,
            self.ready_to_merge,
            self.review_status,
            self.checks_passed,
            self.checks_total,
            self.section().rank()
        )
    }

    pub(crate) fn section(&self) -> InboxSection {
        if self.review_requested {
            InboxSection::NeedsYourReview
        } else if self.team_review_requested {
            InboxSection::NeedsTeamReview
        } else if self.draft && self.is_author {
            InboxSection::Drafts
        } else if self.review_status == "changes_requested" && self.is_author {
            InboxSection::NeedsAction
        } else if self.ready_to_merge {
            InboxSection::ReadyToMerge
        } else {
            InboxSection::Waiting
        }
    }

    pub(crate) fn review_label(&self) -> &'static str {
        if self.state == "closed" {
            "Closed"
        } else if self.draft {
            "Not ready"
        } else if self.review_status == "changes_requested" {
            "Changes requested"
        } else if self.ready_to_merge {
            "Ready to merge"
        } else {
            "Awaiting approval"
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
            "subscribed" if self.assigned_to_me => "assigned to you".into(),
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
