use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use crate::{
    domain::{Snapshot, next_poll_delay, snapshot_signature},
    github::GithubClient,
};

pub(crate) enum Command {
    Refresh,
}

pub(crate) enum Message {
    Snapshot {
        result: Result<Snapshot, String>,
        next_delay: Duration,
    },
}

pub(crate) fn spawn(
    client: GithubClient,
    messages: Sender<Message>,
    commands: Receiver<Command>,
    interval: Duration,
    max_interval: Duration,
    show_closed: bool,
) {
    thread::spawn(move || {
        let mut next_delay = interval;
        let mut previous = None;
        loop {
            let result = client
                .fetch_snapshot()
                .map_err(|error| format!("{error:#}"));
            let changed = match &result {
                Ok(snapshot) => {
                    let current = snapshot_signature(snapshot, show_closed);
                    let changed = previous.as_ref().map(|old| old != &current).unwrap_or(true);
                    previous = Some(current);
                    changed
                }
                Err(_) => false,
            };
            next_delay = next_poll_delay(next_delay, changed, interval, max_interval);
            if messages
                .send(Message::Snapshot { result, next_delay })
                .is_err()
            {
                return;
            }
            match commands.recv_timeout(next_delay) {
                Ok(Command::Refresh) => next_delay = interval,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}
