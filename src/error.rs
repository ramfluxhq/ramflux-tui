// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

/// TUI error type.
#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    #[error("SDK error: {0}")]
    Sdk(#[from] ramflux_sdk::SdkError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
