// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use crate::i18n::{self, Key, Locale};

/// TUI error type.
#[derive(Debug)]
pub enum TuiError {
    Sdk(ramflux_sdk::SdkError),
    Json(serde_json::Error),
    ReadModel(String),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (key, detail) = match self {
            Self::Sdk(error) => (Key::SdkError, error.to_string()),
            Self::Json(error) => (Key::JsonError, error.to_string()),
            Self::ReadModel(detail) => (Key::ReadModelError, detail.clone()),
        };
        let prefix = i18n::text(Locale::En, key);
        write!(formatter, "{prefix} {detail}") // ui-allow: format skeleton
    }
}

impl std::error::Error for TuiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sdk(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::ReadModel(_) => None,
        }
    }
}

impl From<ramflux_sdk::SdkError> for TuiError {
    fn from(error: ramflux_sdk::SdkError) -> Self {
        Self::Sdk(error)
    }
}

impl From<serde_json::Error> for TuiError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
