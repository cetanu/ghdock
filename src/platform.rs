use std::{
    io::{self, Write},
    process::Command,
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub(crate) type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(crate) fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode().context("enabling raw terminal mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("entering alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("creating terminal")
}

pub(crate) fn restore_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode().context("disabling raw terminal mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leaving alternate screen")?;
    terminal.show_cursor().context("showing cursor")?;
    Ok(())
}

pub(crate) fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    command
        .arg(url)
        .spawn()
        .context("opening pull request in browser")?;
    Ok(())
}

pub(crate) fn alert_user(message: &str) {
    let _ = write!(io::stdout(), "\x07");
    let _ = io::stdout().flush();

    #[cfg(target_os = "macos")]
    {
        let escaped = message
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ");
        let script = format!("display notification \"{}\" with title \"ghdock\"", escaped);
        let _ = Command::new("osascript").args(["-e", &script]).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .args(["ghdock", message])
            .spawn();
    }
}
