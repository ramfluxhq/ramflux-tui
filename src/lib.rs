// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

mod app;
mod bus;
mod error;
mod parsing;
mod read_model;
mod state;
mod stream;
#[cfg(test)]
mod tests;

/// Shared product version and build commit, for example `0.2.0 (build abcdef123456)`.
pub const VERSION: &str =
    concat!(env!("CARGO_PKG_VERSION"), " (build ", env!("RAMFLUX_TUI_BUILD"), ")");

pub use app::TuiApp;
pub use bus::{SdkLocalBus, TuiBus};
pub use error::TuiError;
pub use state::{
    ApprovalRow, AttachmentRow, ContactRow, ConversationRow, DeviceRow, GroupRow, InputMode,
    MessageReceiptRow, MessageRow, ObjectTransferRow, Panel, PendingAttachment, TuiInput, TuiState,
};

/// Converts one terminal key code into a TUI input event.
#[must_use]
pub fn key_to_input(code: crossterm::event::KeyCode) -> Option<TuiInput> {
    match code {
        crossterm::event::KeyCode::Tab => Some(TuiInput::Tab),
        crossterm::event::KeyCode::Up => Some(TuiInput::Up),
        crossterm::event::KeyCode::Down => Some(TuiInput::Down),
        crossterm::event::KeyCode::Enter => Some(TuiInput::Enter),
        crossterm::event::KeyCode::Esc => Some(TuiInput::ExitCompose),
        crossterm::event::KeyCode::Backspace => Some(TuiInput::Backspace),
        crossterm::event::KeyCode::Char('q') => Some(TuiInput::Quit),
        crossterm::event::KeyCode::Char('i') => Some(TuiInput::EnterCompose),
        crossterm::event::KeyCode::Char(value) => Some(TuiInput::Char(value)),
        _ => None,
    }
}

/// Default account used by the first private TUI milestone.
pub const DEFAULT_ACCOUNT_ID: &str = "default";
pub(crate) const DEFAULT_CONVERSATION_ID: &str = "conv_tui_default";

#[cfg(test)]
mod version_tests {
    #[test]
    fn version_starts_with_the_package_version_and_names_the_build() {
        assert!(super::VERSION.starts_with(env!("CARGO_PKG_VERSION")));
        assert!(super::VERSION.contains(" (build "));
    }
}
