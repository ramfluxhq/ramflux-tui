// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ramflux_cli_pro::{SdkLocalBus, TuiApp, TuiError, key_to_input};
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
    let mut bus = SdkLocalBus::connect(cli.socket).await?;
    let mut app = TuiApp::new(cli.account);
    app.refresh_all(&mut bus).await?;

    let mut terminal = setup_terminal()?;
    let result = run_terminal_loop(&mut terminal, &mut app, &mut bus).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_terminal_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut TuiApp,
    bus: &mut SdkLocalBus,
) -> Result<(), TuiError> {
    while !app.should_quit() {
        terminal.draw(|frame| app.render(frame)).map_err(|error| io_error(&error))?;
        if event::poll(Duration::from_millis(100)).map_err(|error| io_error(&error))?
            && let Event::Key(key) = event::read().map_err(|error| io_error(&error))?
            && key.kind == KeyEventKind::Press
            && let Some(input) = key_to_input(key.code)
        {
            app.handle_input(bus, input).await?;
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
