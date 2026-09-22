use std::{
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::App,
    platform::{self, AppTerminal},
    poller::{Command, Message},
    ui,
};

pub(crate) fn run(
    terminal: &mut AppTerminal,
    worker_rx: Receiver<Message>,
    refresh_tx: Sender<Command>,
    interval: Duration,
    show_closed: bool,
) -> Result<()> {
    let mut app = App::new(interval, show_closed);
    let mut last_alert = Instant::now();

    loop {
        while let Ok(message) = worker_rx.try_recv() {
            match message {
                Message::Snapshot {
                    result: Ok(snapshot),
                    next_delay,
                } => {
                    if app.apply_snapshot(snapshot, next_delay)
                        && let Some(alert) = app.alert()
                    {
                        platform::alert_user(alert);
                        last_alert = Instant::now();
                    }
                }
                Message::Snapshot {
                    result: Err(error),
                    next_delay,
                } => app.set_error(error, next_delay),
            }
        }

        terminal.draw(|frame| ui::draw(frame, &app))?;
        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && handle_key(&mut app, key, &refresh_tx)?
        {
            return Ok(());
        }
        if app.alert().is_some() && last_alert.elapsed() > Duration::from_secs(12) {
            app.clear_alert();
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent, refresh_tx: &Sender<Command>) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }
    if app.is_help_visible() {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
            app.set_help_visible(false);
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Char('?') => app.set_help_visible(true),
        KeyCode::Char('r') => {
            app.start_refresh();
            refresh_tx
                .send(Command::Refresh)
                .context("refresh worker has stopped")?;
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            if let Some(pull) = app.selected_pull() {
                platform::open_url(&pull.url)?;
            }
        }
        _ => {}
    }
    Ok(false)
}
