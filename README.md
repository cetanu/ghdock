# ghdock

ghdock is a Ratatui dashboard for the pull requests in your GitHub inbox. It combines GitHub notifications with pull requests involving your account, enriches them with their current review, check, and comment counts, and alerts when a new item or a tracked detail changes.

## Setup

ghdock gets its GitHub credentials from the GitHub CLI. Authenticate once with a classic token that has the notifications scope:

    gh auth login
    cargo run

The default poll interval is 60 seconds. GitHub recommends at least that interval for notification polling; override it with:

    GHDOCK_POLL_INTERVAL=120 cargo run

Closed pull requests are hidden by default. Show them with:

    GHDOCK_SHOW_CLOSED=true cargo run

Pull requests you have approved are removed from the inbox. Updates caused by your own activity are not treated as changes for alerts. The inbox is grouped by pull-request state.

When consecutive syncs have no visible changes, the poll interval doubles up to 15 minutes, then resets to the base interval after a change. Configure the cap with:

    GHDOCK_MAX_POLL_INTERVAL=1800 cargo run

## Controls

- Up / Down or j / k: move through pull requests
- Enter or o: open the selected pull request in the browser
- r: refresh immediately
- ?: show help
- q / Esc: quit

Changes produce a terminal bell. macOS also receives an osascript desktop notification, and Linux uses notify-send when it is installed.

GitHub notifications provide activity labels such as review requests, comments, mentions, assignments, and state changes. GitHub pull-request search supplies the broader set of authored, assigned, subscribed, and review-related pull requests shown in the web inbox. The initial sync is silent; alerts begin on the next sync to avoid treating the existing inbox as new work.

## Structure

The binary is a thin composition root. The application is split into modules with one-way dependencies:

- `github`: GitHub CLI authentication, API requests, and response mapping
- `poller`: background polling, change detection, and backoff scheduling
- `domain`: pull-request data and polling rules
- `app`: user-facing state, selection, filtering, and alerts
- `controller`: terminal event loop and input handling
- `ui`: Ratatui rendering only
- `platform`: terminal setup, browser opening, and desktop notifications
- `config`: environment-backed runtime configuration
