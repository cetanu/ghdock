use std::{collections::HashMap, process::Command, time::Instant};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::domain::{MAX_NOTIFICATIONS, Pull, Snapshot};

const API_VERSION: &str = "2022-11-28";

pub(crate) struct GithubClient {
    client: Client,
    token: String,
    username: String,
}

impl GithubClient {
    pub(crate) fn from_gh_cli() -> Result<Self> {
        let output = Command::new("gh")
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .context("could not run gh; install GitHub CLI and run gh auth login")?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "GitHub CLI is not authenticated; run gh auth login{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({detail})")
                }
            );
        }

        let token = String::from_utf8(output.stdout)
            .context("GitHub CLI returned a non-UTF-8 token")?
            .trim()
            .to_string();
        if token.is_empty() {
            anyhow::bail!("GitHub CLI returned an empty token; run gh auth login");
        }
        Self::new(token)
    }

    fn new(token: String) -> Result<Self> {
        let client = Client::builder()
            .user_agent("ghdock/0.1")
            .build()
            .context("could not build HTTP client")?;
        let user: User = client
            .get("https://api.github.com/user")
            .headers(Self::headers_for(&token))
            .send()
            .context("requesting the authenticated GitHub user")?
            .error_for_status()
            .context("GitHub rejected the authenticated-user request")?
            .json()
            .context("decoding the authenticated GitHub user")?;
        Ok(Self {
            client,
            token,
            username: user.login,
        })
    }

    pub(crate) fn fetch_snapshot(&self, show_closed: bool) -> Result<Snapshot> {
        let notifications: Vec<Notification> = self
            .client
            .get("https://api.github.com/notifications")
            .query(&[
                ("all", "true"),
                ("participating", "false"),
                ("per_page", "50"),
            ])
            .headers(self.headers())
            .send()
            .context("requesting GitHub notifications")?
            .error_for_status()
            .context("GitHub rejected the notifications request")?
            .json()
            .context("decoding GitHub notifications")?;

        let query = if show_closed {
            format!("is:pr involves:{}", self.username)
        } else {
            format!("is:pr is:open involves:{}", self.username)
        };
        let search: SearchResponse = self
            .client
            .get("https://api.github.com/search/issues")
            .query(&[("q", query), ("per_page", "100".into())])
            .headers(self.headers())
            .send()
            .context("searching pull requests involving the authenticated user")?
            .error_for_status()
            .context("GitHub rejected the pull request search request")?
            .json()
            .context("decoding pull request search results")?;

        let mut notification_by_key = notifications
            .into_iter()
            .take(MAX_NOTIFICATIONS)
            .filter(|notification| notification.subject.kind == "PullRequest")
            .filter_map(|notification| {
                let number = notification
                    .subject
                    .url
                    .as_deref()
                    .and_then(pull_number_from_api_url)?;
                Some((
                    format!("{}#{number}", notification.repository.full_name),
                    notification,
                ))
            })
            .collect::<HashMap<_, _>>();
        let mut candidates = Vec::new();
        let mut seen = HashMap::new();

        for item in search.items {
            let Some(repository) = repository_from_api_url(&item.repository_url) else {
                continue;
            };
            let key = format!("{repository}#{}", item.number);
            if seen.insert(key, ()).is_none() {
                candidates.push((repository, item.number));
            }
        }
        for key in notification_by_key.keys() {
            if seen.insert(key.clone(), ()).is_none() {
                let Some((repository, number)) = key.rsplit_once('#') else {
                    continue;
                };
                let Ok(number) = number.parse() else {
                    continue;
                };
                candidates.push((repository.to_string(), number));
            }
        }

        let mut pulls = Vec::new();
        for (repository, number) in candidates {
            let key = format!("{repository}#{number}");
            let notification = notification_by_key.remove(&key);
            let pr_api_url = format!("https://api.github.com/repos/{repository}/pulls/{number}");

            let pr: PullRequest = self
                .client
                .get(pr_api_url)
                .headers(self.headers())
                .send()
                .with_context(|| format!("requesting {repository}#{number}"))?
                .error_for_status()
                .context("GitHub rejected a pull request request")?
                .json()
                .context("decoding pull request details")?;

            let reviews = self.pull_reviews(&repository, pr.number)?;
            let latest_reviews = latest_reviews(&reviews);
            let (approved_by_me, review_status) = review_summary(&latest_reviews, &self.username);
            let (checks_passed, checks_total) =
                self.pull_checks(&repository, pr.head.as_ref().map(|head| head.sha.as_str()))?;
            let notification_reason = notification.as_ref().map(|item| item.reason.as_str());
            let reason = notification_reason
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| derive_reason(&pr, &self.username));
            let updated_by_me = notification
                .as_ref()
                .map(|item| self.latest_activity_by_me(&repository, pr.number, &item.updated_at))
                .transpose()?
                .unwrap_or(false);
            let is_author = pr
                .user
                .as_ref()
                .is_some_and(|user| user.login == self.username);
            let review_requested = pr
                .requested_reviewers
                .iter()
                .any(|reviewer| reviewer.login == self.username)
                || notification_reason == Some("review_requested");
            let team_review_requested =
                !pr.requested_teams.is_empty() && notification_reason == Some("team_mention");
            let assigned_to_me = pr
                .assignees
                .iter()
                .any(|assignee| assignee.login == self.username);

            pulls.push(Pull {
                repo: repository,
                number: pr.number,
                title: pr.title,
                state: pr.state,
                draft: pr.draft.unwrap_or(false),
                author: pr
                    .user
                    .map(|user| user.login)
                    .unwrap_or_else(|| "unknown".into()),
                url: pr.html_url,
                reason,
                unread: notification.as_ref().is_some_and(|item| item.unread),
                updated_at: pr.updated_at,
                updated_by_me,
                approved_by_me,
                ready_to_merge: ready_to_merge(
                    &latest_reviews,
                    pr.mergeable_state.as_deref(),
                    checks_passed,
                    checks_total,
                ),
                is_author,
                review_requested,
                team_review_requested,
                assigned_to_me,
                review_status,
                checks_passed,
                checks_total,
                comments: pr.comments,
                review_comments: pr.review_comments,
                commits: pr.commits,
                additions: pr.additions,
                deletions: pr.deletions,
                changed_files: pr.changed_files,
            });
        }

        pulls.sort_by(|left, right| {
            left.section()
                .rank()
                .cmp(&right.section().rank())
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        Ok(Snapshot {
            pulls,
            fetched_at: Instant::now(),
        })
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        Self::headers_for(&self.token)
    }

    fn headers_for(token: &str) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "application/vnd.github+json"
                .parse()
                .expect("static header"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            API_VERSION.parse().expect("static header"),
        );
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("token header"),
        );
        headers
    }

    fn pull_reviews(&self, repository: &str, number: u64) -> Result<Vec<Review>> {
        self.client
            .get(format!(
                "https://api.github.com/repos/{repository}/pulls/{number}/reviews"
            ))
            .query(&[("per_page", "100")])
            .headers(self.headers())
            .send()
            .with_context(|| format!("requesting reviews for {repository}#{number}"))?
            .error_for_status()
            .context("GitHub rejected the pull request reviews request")?
            .json()
            .context("decoding pull request reviews")
    }

    fn pull_checks(&self, repository: &str, sha: Option<&str>) -> Result<(u64, u64)> {
        let Some(sha) = sha else {
            return Ok((0, 0));
        };
        let response: CheckRunsResponse = self
            .client
            .get(format!(
                "https://api.github.com/repos/{repository}/commits/{sha}/check-runs"
            ))
            .query(&[("per_page", "100")])
            .headers(self.headers())
            .send()
            .with_context(|| format!("requesting checks for {repository}@{sha}"))?
            .error_for_status()
            .context("GitHub rejected the pull request checks request")?
            .json()
            .context("decoding pull request checks")?;
        let total = response.check_runs.len() as u64;
        let passed = response
            .check_runs
            .iter()
            .filter(|check| {
                matches!(
                    check.conclusion.as_deref(),
                    Some("success" | "skipped" | "neutral")
                )
            })
            .count() as u64;
        Ok((passed, total))
    }

    fn latest_activity_by_me(
        &self,
        repository: &str,
        number: u64,
        notification_updated_at: &str,
    ) -> Result<bool> {
        let events: Vec<TimelineEvent> = self
            .client
            .get(format!(
                "https://api.github.com/repos/{repository}/issues/{number}/timeline"
            ))
            .query(&[("per_page", "100")])
            .headers(self.headers())
            .send()
            .with_context(|| format!("requesting activity for {repository}#{number}"))?
            .error_for_status()
            .context("GitHub rejected the pull request activity request")?
            .json()
            .context("decoding pull request activity")?;

        let latest = events
            .into_iter()
            .filter_map(|event| {
                let timestamp = event
                    .created_at
                    .or(event.submitted_at)
                    .or(event.updated_at)?;
                if timestamp.as_str() > notification_updated_at {
                    return None;
                }
                let login = event.actor.or(event.user).map(|user| user.login)?;
                Some((timestamp, login))
            })
            .max_by(|left, right| left.0.cmp(&right.0));

        Ok(latest.is_some_and(|(_, login)| login == self.username))
    }
}

#[derive(Debug, Deserialize)]
struct Notification {
    repository: Repository,
    subject: Subject,
    reason: String,
    unread: bool,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct Repository {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct Subject {
    #[allow(dead_code)]
    title: String,
    url: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    repository_url: String,
    number: u64,
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    number: u64,
    title: String,
    state: String,
    draft: Option<bool>,
    html_url: String,
    user: Option<User>,
    updated_at: String,
    mergeable_state: Option<String>,
    #[serde(default)]
    assignees: Vec<User>,
    #[serde(default)]
    requested_reviewers: Vec<User>,
    #[serde(default)]
    requested_teams: Vec<Team>,
    head: Option<Head>,
    comments: u64,
    review_comments: u64,
    commits: u64,
    additions: u64,
    deletions: u64,
    changed_files: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct Review {
    user: Option<User>,
    state: String,
    submitted_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Team {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    slug: String,
}

#[derive(Debug, Deserialize)]
struct Head {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TimelineEvent {
    actor: Option<User>,
    user: Option<User>,
    created_at: Option<String>,
    submitted_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct User {
    login: String,
}

fn ready_to_merge(
    reviews: &[Review],
    mergeable_state: Option<&str>,
    checks_passed: u64,
    checks_total: u64,
) -> bool {
    reviews
        .iter()
        .any(|review| review.state.eq_ignore_ascii_case("approved"))
        && !reviews
            .iter()
            .any(|review| review.state.eq_ignore_ascii_case("changes_requested"))
        && mergeable_state == Some("clean")
        && checks_total == checks_passed
}

fn latest_reviews(reviews: &[Review]) -> Vec<Review> {
    let mut latest = HashMap::<String, Review>::new();
    for review in reviews {
        let Some(user) = review.user.as_ref() else {
            continue;
        };
        let replace = latest
            .get(&user.login)
            .and_then(|current| current.submitted_at.as_ref())
            .is_none_or(|current| {
                review
                    .submitted_at
                    .as_ref()
                    .is_some_and(|next| next > current)
            });
        if replace {
            latest.insert(user.login.clone(), review.clone());
        }
    }
    latest.into_values().collect()
}

fn review_summary(reviews: &[Review], username: &str) -> (bool, String) {
    let approved_by_me = reviews.iter().any(|review| {
        review.state.eq_ignore_ascii_case("approved")
            && review
                .user
                .as_ref()
                .is_some_and(|user| user.login == username)
    });
    let status = if reviews
        .iter()
        .any(|review| review.state.eq_ignore_ascii_case("changes_requested"))
    {
        "changes_requested"
    } else if reviews
        .iter()
        .any(|review| review.state.eq_ignore_ascii_case("approved"))
    {
        "approved"
    } else {
        "pending"
    };
    (approved_by_me, status.into())
}

fn derive_reason(pr: &PullRequest, username: &str) -> String {
    if pr
        .requested_reviewers
        .iter()
        .any(|reviewer| reviewer.login == username)
    {
        "review_requested".into()
    } else if pr
        .assignees
        .iter()
        .any(|assignee| assignee.login == username)
    {
        "assign".into()
    } else if pr.user.as_ref().is_some_and(|user| user.login == username) {
        "author".into()
    } else {
        "subscribed".into()
    }
}

fn repository_from_api_url(url: &str) -> Option<String> {
    url.strip_prefix("https://api.github.com/repos/")
        .filter(|path| path.split('/').count() == 2)
        .map(ToOwned::to_owned)
}

fn pull_number_from_api_url(url: &str) -> Option<u64> {
    let mut parts = url
        .strip_prefix("https://api.github.com/repos/")?
        .split('/');
    parts.next()?;
    parts.next()?;
    let resource = parts.next()?;
    let number = parts.next()?;
    if parts.next().is_some() || (resource != "issues" && resource != "pulls") {
        return None;
    }
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(state: &str) -> Review {
        Review {
            user: None,
            state: state.into(),
            submitted_at: None,
        }
    }

    #[test]
    fn ready_to_merge_requires_approval_without_requested_changes_and_clean_checks() {
        assert!(ready_to_merge(&[review("APPROVED")], Some("clean"), 0, 0));
        assert!(!ready_to_merge(&[], Some("clean"), 0, 0));
        assert!(!ready_to_merge(
            &[review("APPROVED"), review("CHANGES_REQUESTED")],
            Some("clean"),
            0,
            0
        ));
        assert!(!ready_to_merge(
            &[review("APPROVED")],
            Some("unstable"),
            0,
            0
        ));
        assert!(!ready_to_merge(&[review("APPROVED")], None, 0, 0));
    }

    #[test]
    fn extracts_issue_subject_numbers() {
        assert_eq!(
            pull_number_from_api_url("https://api.github.com/repos/acme/widget/issues/42"),
            Some(42)
        );
    }

    #[test]
    fn ignores_non_pull_subject_urls() {
        assert_eq!(
            pull_number_from_api_url(
                "https://api.github.com/repos/acme/widget/issues/42/comments/7"
            ),
            None
        );
        assert_eq!(
            pull_number_from_api_url("https://github.com/acme/widget/pull/42"),
            None
        );
    }
}
