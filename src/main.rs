// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use clap::{CommandFactory, FromArgMatches, Parser};
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
#[command(name = "rf-tui")]
struct Cli {
    #[arg(long, default_value = DEFAULT_SOCKET)]
    socket: PathBuf,
    #[arg(long, default_value = ramflux_cli_pro::DEFAULT_ACCOUNT_ID)]
    account: String,
    #[arg(long)]
    lang: Option<String>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), TuiError> {
    let locale = ramflux_cli_pro::resolve_locale(explicit_lang().as_deref());
    // clap keeps this string for the process and accepts only a static version.
    let version: &'static str = Box::leak(ramflux_cli_pro::version_line(locale).into_boxed_str());
    let matches = match Cli::command()
        .about(ramflux_cli_pro::about(locale))
        .version(version)
        .mut_arg("lang", |arg| arg.help(ramflux_cli_pro::lang_help(locale)))
        .try_get_matches()
    {
        Ok(matches) => matches,
        Err(error) => error.exit(),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let mut bus = SdkLocalBus::connect(&cli.socket).await?;
    let mut app = TuiApp::new(cli.account);
    app.set_socket(&cli.socket);
    app.apply_locale(cli.lang.as_deref());
    app.start_session(&mut bus).await?;

    let mut terminal = setup_terminal()?;
    let result = run_terminal_loop(&mut terminal, &mut app, &mut bus, &cli.socket).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_terminal_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut TuiApp,
    bus: &mut SdkLocalBus,
    socket: &std::path::Path,
) -> Result<(), TuiError> {
    while !app.should_quit() {
        match tokio::time::timeout(Duration::from_millis(1), bus.next_event()).await {
            Ok(Ok(event)) => {
                if let Err(error) = app.apply_stream_event(bus, &event).await {
                    app.note_stream_failure(&error);
                    reconnect(app, bus, socket).await;
                }
            }
            Ok(Err(error)) => {
                app.note_reconnecting(&error);
                reconnect(app, bus, socket).await;
            }
            Err(_) => {}
        }
        terminal.draw(|frame| app.render(frame)).map_err(|error| io_error(&error))?;
        if event::poll(Duration::from_millis(100)).map_err(|error| io_error(&error))?
            && let Event::Key(key) = event::read().map_err(|error| io_error(&error))?
            && key.kind == KeyEventKind::Press
            && let Some(input) =
                if app.captures_text() { account_key(key.code) } else { key_to_input(key.code) }
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

fn account_key(code: crossterm::event::KeyCode) -> Option<ramflux_cli_pro::TuiInput> {
    match code {
        crossterm::event::KeyCode::Enter => Some(ramflux_cli_pro::TuiInput::Enter),
        crossterm::event::KeyCode::Esc => Some(ramflux_cli_pro::TuiInput::ExitCompose),
        crossterm::event::KeyCode::Backspace => Some(ramflux_cli_pro::TuiInput::Backspace),
        crossterm::event::KeyCode::Char(value) => Some(ramflux_cli_pro::TuiInput::Char(value)),
        _ => None,
    }
}

fn explicit_lang() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--lang=") {
            // ui-allow: command syntax
            return Some(value.to_owned());
        }
        if arg == "--lang" {
            return args.next();
        }
        if arg == "--" {
            break;
        }
    }
    None
}

fn io_error(error: &io::Error) -> TuiError {
    TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string()))
}

async fn reconnect(app: &mut TuiApp, bus: &mut SdkLocalBus, socket: &std::path::Path) {
    tokio::time::sleep(Duration::from_millis(250)).await;
    if let Ok(connected) = SdkLocalBus::connect(socket).await {
        *bus = connected;
        if app.captures_text() {
            return;
        }
        if let Err(error) = app.refresh_all(bus).await {
            app.note_reconnecting(&error);
        }
    }
}
