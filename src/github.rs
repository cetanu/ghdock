use std::{process::Command, time::Instant};

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

    pub(crate) fn fetch_snapshot(&self) -> Result<Snapshot> {
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

        let mut pulls = Vec::new();
        for notification in notifications.into_iter().take(MAX_NOTIFICATIONS) {
            if notification.subject.kind != "PullRequest" {
                continue;
            }
            let Some(pr_url) = notification.subject.url.as_deref() else {
                continue;
            };
            let Some(pr_api_url) = pull_api_url(pr_url) else {
                continue;
            };

            let pr: PullRequest = self
                .client
                .get(pr_api_url)
                .headers(self.headers())
                .send()
                .with_context(|| {
                    format!(
                        "requesting {}#{}",
                        notification.repository.full_name, notification.subject.title
                    )
                })?
                .error_for_status()
                .context("GitHub rejected a pull request request")?
                .json()
                .context("decoding pull request details")?;

            let reviews = self.pull_reviews(&notification.repository.full_name, pr.number)?;
            let approved_by_me = reviews.iter().any(|review| {
                review.state.eq_ignore_ascii_case("approved")
                    && review
                        .user
                        .as_ref()
                        .is_some_and(|user| user.login == self.username)
            });
            let updated_by_me = self.latest_activity_by_me(
                &notification.repository.full_name,
                pr.number,
                &notification.updated_at,
            )?;

            pulls.push(Pull {
                repo: notification.repository.full_name,
                number: pr.number,
                title: if pr.title.is_empty() {
                    notification.subject.title
                } else {
                    pr.title
                },
                state: pr.state,
                draft: pr.draft.unwrap_or(false),
                author: pr
                    .user
                    .map(|user| user.login)
                    .unwrap_or_else(|| "unknown".into()),
                url: pr.html_url,
                reason: notification.reason,
                unread: notification.unread,
                updated_at: if pr.updated_at.is_empty() {
                    notification.updated_at
                } else {
                    pr.updated_at
                },
                updated_by_me,
                approved_by_me,
                comments: pr.comments,
                review_comments: pr.review_comments,
                commits: pr.commits,
                additions: pr.additions,
                deletions: pr.deletions,
                changed_files: pr.changed_files,
            });
        }

        pulls.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
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
    title: String,
    url: Option<String>,
    #[serde(rename = "type")]
    kind: String,
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
    comments: u64,
    review_comments: u64,
    commits: u64,
    additions: u64,
    deletions: u64,
    changed_files: u64,
}

#[derive(Debug, Deserialize)]
struct Review {
    user: Option<User>,
    state: String,
}

#[derive(Debug, Deserialize)]
struct TimelineEvent {
    actor: Option<User>,
    user: Option<User>,
    created_at: Option<String>,
    submitted_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

fn pull_api_url(subject_url: &str) -> Option<String> {
    let path = subject_url.strip_prefix("https://api.github.com/repos/")?;
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let resource = parts.next()?;
    let number = parts.next()?;
    if parts.next().is_some() || (resource != "issues" && resource != "pulls") {
        return None;
    }
    Some(format!(
        "https://api.github.com/repos/{owner}/{repo}/pulls/{number}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_issue_subject_urls_to_pull_urls() {
        assert_eq!(
            pull_api_url("https://api.github.com/repos/acme/widget/issues/42"),
            Some("https://api.github.com/repos/acme/widget/pulls/42".into())
        );
    }

    #[test]
    fn ignores_non_pull_subject_urls() {
        assert_eq!(
            pull_api_url("https://api.github.com/repos/acme/widget/issues/42/comments/7"),
            None
        );
        assert_eq!(pull_api_url("https://github.com/acme/widget/pull/42"), None);
    }
}
