use std::{env, time::Duration};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
const DEFAULT_MAX_POLL_INTERVAL_SECS: u64 = 900;

pub(crate) struct Config {
    pub(crate) poll_interval: Duration,
    pub(crate) max_poll_interval: Duration,
    pub(crate) show_closed: bool,
}

impl Config {
    pub(crate) fn from_env() -> Self {
        let poll_seconds = env::var("GHDOCK_POLL_INTERVAL")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECS)
            .max(10);
        let max_poll_seconds = env::var("GHDOCK_MAX_POLL_INTERVAL")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_POLL_INTERVAL_SECS)
            .max(poll_seconds);

        Self {
            poll_interval: Duration::from_secs(poll_seconds),
            max_poll_interval: Duration::from_secs(max_poll_seconds),
            show_closed: env::var("GHDOCK_SHOW_CLOSED")
                .ok()
                .map(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                })
                .unwrap_or(false),
        }
    }
}
