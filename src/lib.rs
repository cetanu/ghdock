mod app;
mod config;
mod controller;
mod domain;
mod github;
mod platform;
mod poller;
mod ui;

use std::io::{self, IsTerminal};

use anyhow::Result;

pub fn run() -> Result<()> {
    if !io::stdout().is_terminal() {
        anyhow::bail!("ghdock needs an interactive terminal");
    }

    let config = config::Config::from_env();
    let github = github::GithubClient::from_gh_cli()?;
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let (refresh_tx, refresh_rx) = std::sync::mpsc::channel();

    poller::spawn(
        github,
        worker_tx,
        refresh_rx,
        config.poll_interval,
        config.max_poll_interval,
        config.show_closed,
    );

    let mut terminal = platform::setup_terminal()?;
    let result = controller::run(
        &mut terminal,
        worker_rx,
        refresh_tx,
        config.poll_interval,
        config.show_closed,
    );
    platform::restore_terminal(&mut terminal)?;
    result
}
