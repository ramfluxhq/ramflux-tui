// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ramflux_cli_pro::{SdkLocalBus, TuiApp, TuiBus, TuiError, key_to_input};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_SOCKET: &str = "/tmp/ramflux/rfd.sock";

#[derive(Parser)]
#[command(name = "rf-tui", about = "Private Ramflux TUI over the open local SDK bus")]
struct Cli {
    #[arg(long, default_value = DEFAULT_SOCKET)]
    socket: PathBuf,
    #[arg(long, default_value = ramflux_cli_pro::DEFAULT_ACCOUNT_ID)]
    account: String,
    /// Keep legacy receive polling for compatibility with older daemons.
    #[arg(long)]
    legacy_poll: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), TuiError> {
    let cli = Cli::parse();
    let mut bus = SdkLocalBus::connect(&cli.socket).await?;
    let mut app = TuiApp::new(cli.account);
    app.set_legacy_poll(cli.legacy_poll);
    app.refresh_all(&mut bus).await?;

    let mut terminal = setup_terminal()?;
    let result =
        run_terminal_loop(&mut terminal, &mut app, &mut bus, &cli.socket, cli.legacy_poll).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_terminal_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut TuiApp,
    bus: &mut SdkLocalBus,
    socket: &std::path::Path,
    legacy_poll: bool,
) -> Result<(), TuiError> {
    let mut next_legacy_poll = tokio::time::Instant::now() + Duration::from_secs(1);
    while !app.should_quit() {
        if legacy_poll && tokio::time::Instant::now() >= next_legacy_poll {
            if let Err(error) = app.receive_messages_with_attachments(bus).await {
                app.state.status_message = Some(format!("receive failed: {error}"));
            }
            next_legacy_poll = tokio::time::Instant::now() + Duration::from_secs(1);
        }
        match tokio::time::timeout(Duration::from_millis(1), bus.next_event()).await {
            Ok(Ok(event)) => {
                if let Err(error) = app.apply_stream_event(bus, &event).await {
                    app.state.status_message = Some(format!("stream refresh failed: {error}"));
                    reconnect(app, bus, socket).await;
                }
            }
            Ok(Err(error)) => {
                app.state.status_message = Some(format!("reconnecting: {error}"));
                reconnect(app, bus, socket).await;
            }
            Err(_) => {}
        }
        terminal.draw(|frame| app.render(frame)).map_err(|error| io_error(&error))?;
        if event::poll(Duration::from_millis(100)).map_err(|error| io_error(&error))?
            && let Event::Key(key) = event::read().map_err(|error| io_error(&error))?
            && key.kind == KeyEventKind::Press
            && let Some(input) = key_to_input(key.code)
        {
            app.dispatch_input(bus, input).await;
        }
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, TuiError> {
    enable_raw_mode().map_err(|error| io_error(&error))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|error| io_error(&error))?;
    Terminal::new(CrosstermBackend::new(stdout)).map_err(|error| io_error(&error))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<(), TuiError> {
    disable_raw_mode().map_err(|error| io_error(&error))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|error| io_error(&error))?;
    terminal.show_cursor().map_err(|error| io_error(&error))
}

fn io_error(error: &io::Error) -> TuiError {
    TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string()))
}

async fn reconnect(app: &mut TuiApp, bus: &mut SdkLocalBus, socket: &std::path::Path) {
    tokio::time::sleep(Duration::from_millis(250)).await;
    if let Ok(connected) = SdkLocalBus::connect(socket).await {
        *bus = connected;
        if let Err(error) = app.refresh_all(bus).await {
            app.state.status_message = Some(format!("reconnecting: {error}"));
        }
    }
}
