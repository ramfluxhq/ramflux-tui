// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Conversation-scoped native surface presentation and input.
mod input;
mod read;
mod render;

pub(crate) use read::{Card, Cards, invalid, protocol, string};
