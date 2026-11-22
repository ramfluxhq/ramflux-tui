// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Native surface reads and conversation-scoped terminal sessions.
mod input;
mod render;

use ramflux_protocol::{SurfaceSnapshot, SurfaceState, canonical_json_bytes};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::{TuiApp, TuiBus, TuiError};

#[derive(Clone, Debug, Default)]
pub(crate) struct Cards {
    pub rows: Vec<Card>,
    pub selected: usize,
    pub open: bool,
    pub control: usize,
    pub editing: Option<String>,
    pub notice: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Card {
    reference: String,
    conversation: String,
    author: String,
    fallback: String,
    state: Option<SurfaceState>,
    unavailable: Option<String>,
    drafts: BTreeMap<String, Value>,
    result: Option<String>,
}

pub(super) fn invalid(message: impl Into<String>) -> TuiError {
    TuiError::ReadModel(format!("card: {}", message.into()))
}

pub(super) fn protocol(error: impl std::fmt::Display) -> TuiError {
    invalid(error.to_string())
}

pub(super) fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, TuiError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("missing {key}")))
}

impl Card {
    fn parse(value: &Value, conversation: &str, reference: &str) -> Result<Self, TuiError> {
        if string(value, "conversation_id")? != conversation
            || string(value, "surface_ref")? != reference
        {
            return Err(invalid("surface scope mismatch"));
        }
        let mut card = Self {
            reference: reference.into(),
            conversation: conversation.into(),
            author: string(value, "author_principal_id")?.into(),
            fallback: value
                .get("fallback_text")
                .and_then(Value::as_str)
                .unwrap_or("Card unavailable")
                .into(),
            state: None,
            unavailable: None,
            drafts: BTreeMap::new(),
            result: None,
        };
        // A locally authored snapshot may be readable for planning without renderer authority.
        if value.get("render_allowed").and_then(Value::as_bool) != Some(true) {
            card.unavailable = Some("Text fallback: this card is not from your own agent".into());
            return Ok(card);
        }
        if value.get("snapshot").is_none_or(Value::is_null) {
            card.unavailable = Some("Card content unavailable".into());
            return Ok(card);
        }
        let snapshot: SurfaceSnapshot = serde_json::from_value(
            value.get("snapshot").cloned().ok_or_else(|| invalid("missing snapshot"))?,
        )?;
        card.fallback.clone_from(&snapshot.fallback_text);
        if snapshot.surface_id != string(value, "surface_id")? {
            return Err(invalid("snapshot identity mismatch"));
        }
        let mut checkpoint = serde_json::to_value(&snapshot)?;
        checkpoint.as_object_mut().ok_or_else(|| invalid("invalid snapshot"))?.extend([
            ("conversationId".into(), json!(conversation)),
            (
                "surfaceVersion".into(),
                value.get("version").cloned().ok_or_else(|| invalid("missing version"))?,
            ),
        ]);
        let state = SurfaceState::from_checkpoint_bytes(
            &canonical_json_bytes(&checkpoint).map_err(protocol)?,
        )
        .map_err(protocol)?;
        if state.hash() != value.get("surface_hash").and_then(Value::as_str) {
            return Err(invalid("snapshot hash mismatch"));
        }
        if state.is_closed() || value.get("closed").and_then(Value::as_bool) != Some(false) {
            card.unavailable = Some("Card closed".into());
        } else if value.get("out_of_sync").and_then(Value::as_bool) != Some(false) {
            card.unavailable = Some("Card out of sync; actions unavailable".into());
        }
        card.result =
            value.get("actions").and_then(Value::as_array).and_then(|actions| actions.last()).map(
                |action| {
                    if action.get("receipt_verified").and_then(Value::as_bool) == Some(true) {
                        "Action receipt verified".into()
                    } else {
                        "Action recorded; receipt pending".into()
                    }
                },
            );
        card.state = Some(state);
        Ok(card)
    }

    fn same_revision(&self, other: &Self) -> bool {
        self.reference == other.reference
            && self.author == other.author
            && self.conversation == other.conversation
            && self.unavailable == other.unavailable
            && self.state.as_ref().map(|s| (s.version(), s.hash()))
                == other.state.as_ref().map(|s| (s.version(), s.hash()))
    }
}

impl TuiApp {
    pub(crate) async fn refresh_cards<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let conversation = self
            .state
            .conversations
            .get(self.state.selected_conversation)
            .map(|row| row.id.clone());
        let Some(conversation) = conversation else {
            self.cards = Cards::default();
            return Ok(());
        };
        // Clear first: a failed read must never leave another conversation's controls active.
        let previous = std::mem::take(&mut self.cards);
        let result = self.read_cards(bus, &conversation).await;
        match result {
            Ok(mut cards) => {
                let selected_ref =
                    previous.rows.get(previous.selected).map(|c| c.reference.as_str());
                for card in &mut cards.rows {
                    if let Some(old) = previous.rows.iter().find(|old| old.same_revision(card)) {
                        card.drafts.clone_from(&old.drafts);
                        if card.result.is_none() {
                            card.result.clone_from(&old.result);
                        }
                    }
                }
                cards.selected = cards
                    .rows
                    .iter()
                    .position(|c| Some(c.reference.as_str()) == selected_ref)
                    .unwrap_or(0);
                cards.open = previous.open && !cards.rows.is_empty();
                cards.control = previous.control;
                // A changed revision discards edits, even while a field is being entered.
                if cards
                    .rows
                    .get(cards.selected)
                    .zip(previous.rows.get(previous.selected))
                    .is_some_and(|(a, b)| a.same_revision(b))
                {
                    cards.editing = previous.editing;
                }
                self.cards = cards;
                Ok(())
            }
            Err(error) => {
                self.cards.notice = Some("Cards could not be loaded; press c to retry".into());
                Err(error)
            }
        }
    }

    async fn read_cards<B: TuiBus + Send>(
        &self,
        bus: &mut B,
        conversation: &str,
    ) -> Result<Cards, TuiError> {
        let mut cards = Cards::default();
        let mut cursor = Value::Null;
        // At most 100 explicit content reads; pagination never makes an unbounded loop.
        for _ in 0..10 {
            let page = bus
                .request(
                    Some(self.state.account_id.clone()),
                    "surface",
                    "surface.list",
                    json!({"conversation_id":conversation,"cursor":cursor,"limit":10}),
                )
                .await?;
            let rows = page
                .get("surfaces")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("invalid surface list"))?;
            if rows.len() > 10 {
                return Err(invalid("surface page too large"));
            }
            for row in rows {
                let reference = string(row, "surface_ref")?;
                if cards.rows.iter().any(|card| card.reference == reference) {
                    return Err(invalid("duplicate surface reference"));
                }
                let value = bus
                    .request(
                        Some(self.state.account_id.clone()),
                        "surface",
                        "surface.get",
                        json!({"surface_ref":reference}),
                    )
                    .await?;
                if !value.is_null() {
                    cards.rows.push(Card::parse(&value, conversation, reference)?);
                }
            }
            let next = page.get("next_cursor").cloned().unwrap_or(Value::Null);
            if next.is_null() {
                return Ok(cards);
            }
            if !next.is_string() || next == cursor {
                return Err(invalid("surface cursor did not advance"));
            }
            cursor = next;
        }
        cards.notice = Some("Showing the first 100 cards in this conversation".into());
        Ok(cards)
    }
}
