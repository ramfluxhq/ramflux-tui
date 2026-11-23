// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Local input drafts and typed, version-bound action submission.
use super::read::phrase;
use super::{Card, invalid, protocol, string};
use crate::i18n::{self, Arg, Key, Locale};
use crate::{TuiApp, TuiBus, TuiError, TuiInput};
use ramflux_protocol::{ACTION_SCHEMA, Action, ActionFrame, ActionMessage, SurfaceAction};
use serde_json::{Value, json};

impl Card {
    fn control(&self, locale: Locale, index: usize) -> Option<Value> {
        self.rows(locale).into_iter().filter_map(|row| row.control).nth(index)
    }

    fn edit(&mut self, locale: Locale, component: &Value, text: &str) -> Result<(), TuiError> {
        let kind = string(locale, component, "component")?;
        let value = match kind {
            "Slider" => json!(
                text.parse::<i64>()
                    .map_err(|_| { invalid(locale, phrase(locale, Key::EnterWholeNumber)) })?
            ),
            "ChoicePicker" => {
                let choices = component
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(locale, phrase(locale, Key::MissingOptions)))?;
                let mut selected = Vec::new();
                for number in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let index =
                        number.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).ok_or_else(
                            || invalid(locale, phrase(locale, Key::EnterOptionNumbers)),
                        )?;
                    let value =
                        choices.get(index).and_then(|c| c.get("value")).cloned().ok_or_else(
                            || invalid(locale, phrase(locale, Key::OptionOutOfRange)),
                        )?;
                    if !selected.contains(&value) {
                        selected.push(value);
                    }
                }
                if component.get("variant").and_then(Value::as_str) != Some("multipleSelection")
                    && selected.len() > 1
                {
                    return Err(invalid(locale, phrase(locale, Key::ChooseOne)));
                }
                Value::Array(selected)
            }
            _ => Value::String(text.into()),
        };
        let path = component
            .pointer("/value/@path")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(locale, phrase(locale, Key::InputBindingUnavailable)))?;
        self.drafts.insert(path.into(), value);
        Ok(())
    }

    fn action_request(
        &self,
        locale: Locale,
        component: &Value,
    ) -> Result<ramflux_sdk::SurfaceSendRequest, TuiError> {
        if self.unavailable.is_some() {
            return Err(invalid(locale, phrase(locale, Key::CardActionsUnavailable)));
        }
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| invalid(locale, phrase(locale, Key::CardStateUnavailable)))?;
        let snapshot = state
            .content()
            .ok_or_else(|| invalid(locale, phrase(locale, Key::CardSnapshotUnavailable)))?;
        let event = component
            .pointer("/action/event")
            .ok_or_else(|| invalid(locale, phrase(locale, Key::ButtonNoEvent)))?;
        let mut context = serde_json::Map::new();
        if let Some(declared) = event.get("context").and_then(Value::as_object) {
            for (key, value) in declared {
                context.insert(
                    key.clone(),
                    self.value(value)
                        .ok_or_else(|| invalid(locale, phrase(locale, Key::UnsupportedContext)))?,
                );
            }
        }
        let mut frame = ActionFrame {
            schema: ACTION_SCHEMA.into(),
            conversation_id: self.conversation.clone(),
            surface_version: state.version(),
            surface_hash: state
                .hash()
                .ok_or_else(|| invalid(locale, phrase(locale, Key::MissingCardHash)))?
                .into(),
            source_instance: Vec::new(),
            args_hash: String::new(),
            task_binding: None,
            a2ui: ActionMessage {
                version: "1.0".into(),
                action: Action {
                    name: string(locale, event, "name")?.into(),
                    surface_id: snapshot.surface_id.clone(),
                    source_component_id: string(locale, component, "id")?.into(),
                    timestamp: timestamp(locale)?,
                    context,
                    user_message: event
                        .get("userMessage")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    metadata: None,
                },
            },
        };
        frame.args_hash = frame.arguments_hash().map_err(|error| protocol(locale, error))?;
        let wire = frame.wire_bytes().map_err(|error| protocol(locale, error))?;
        let action =
            SurfaceAction::from_wire_bytes(&wire).map_err(|error| protocol(locale, error))?;
        // The shared validator enforces input ownership, form scope, types, ranges and checks.
        state.validate_action(&action).map_err(|error| protocol(locale, error))?;
        Ok(ramflux_sdk::SurfaceSendRequest::action(&self.author, &wire)?)
    }
}

impl TuiApp {
    pub(crate) async fn handle_card_input<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        input: &TuiInput,
    ) -> Result<(), TuiError> {
        if self.cards.editing.is_some() {
            return self.edit_card_input(input);
        }
        match input {
            TuiInput::ExitCompose | TuiInput::Quit => self.cards.open = false,
            TuiInput::Up => self.cards.control = self.cards.control.saturating_sub(1),
            TuiInput::Down => {
                let count = self.cards.rows.get(self.cards.selected).map_or(0, |c| {
                    c.rows(self.state.locale).iter().filter(|r| r.control.is_some()).count()
                });
                if self.cards.control + 1 < count {
                    self.cards.control += 1;
                }
            }
            TuiInput::Char('[') => {
                self.cards.selected = self.cards.selected.saturating_sub(1);
                self.cards.control = 0;
            }
            TuiInput::Char(']') => {
                if self.cards.selected + 1 < self.cards.rows.len() {
                    self.cards.selected += 1;
                }
                self.cards.control = 0;
            }
            TuiInput::Char('c') => self.refresh_cards(bus).await?,
            TuiInput::Enter => self.activate_card_control(bus).await?,
            _ => {}
        }
        Ok(())
    }

    fn edit_card_input(&mut self, input: &TuiInput) -> Result<(), TuiError> {
        let Some(text) = self.cards.editing.as_mut() else {
            return Ok(());
        };
        match input {
            TuiInput::ExitCompose => self.cards.editing = None,
            TuiInput::Char(c) if text.len() + c.len_utf8() <= 4096 => text.push(*c),
            TuiInput::EnterCompose if text.len() < 4096 => text.push('i'),
            TuiInput::Quit if text.len() < 4096 => text.push('q'),
            TuiInput::Backspace => {
                text.pop();
            }
            TuiInput::Enter => {
                let Some(card) = self.cards.rows.get_mut(self.cards.selected) else {
                    return Ok(());
                };
                let locale = self.state.locale;
                let component = card
                    .control(locale, self.cards.control)
                    .ok_or_else(|| invalid(locale, phrase(locale, Key::ControlUnavailable)))?;
                card.edit(locale, &component, text)?;
                self.cards.editing = None;
            }
            _ => {}
        }
        Ok(())
    }

    async fn activate_card_control<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(card) = self.cards.rows.get_mut(self.cards.selected) else {
            return Ok(());
        };
        let locale = self.state.locale;
        let Some(component) = card.control(locale, self.cards.control) else {
            return Ok(());
        };
        match string(locale, &component, "component")? {
            "Button" => {
                let shown = card.clone();
                self.submit_card_action(bus, &shown, &component).await?;
            }
            "CheckBox" => {
                let path = component
                    .pointer("/value/@path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(locale, phrase(locale, Key::InputBindingUnavailable)))?;
                let value = component
                    .get("value")
                    .and_then(|v| card.value(v))
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| invalid(locale, phrase(locale, Key::CheckboxUnavailable)))?;
                card.drafts.insert(path.into(), json!(!value));
            }
            "ChoicePicker" => {
                self.cards.editing = Some(String::new());
                self.cards.notice = Some(phrase(locale, Key::ChoiceHint));
            }
            _ => {
                self.cards.editing =
                    Some(component.get("value").and_then(|v| card.value(v)).map_or_else(
                        String::new,
                        |v| match v {
                            Value::String(s) => s,
                            Value::Number(n) => shown_number(locale, &n),
                            _ => String::new(),
                        },
                    ));
            }
        }
        Ok(())
    }
    async fn submit_card_action<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        shown: &Card,
        component: &Value,
    ) -> Result<(), TuiError> {
        // Content authority may change without a new surface version. Fetch it at activation.
        let read = bus
            .request(
                Some(self.state.account_id.clone()),
                "surface",
                "surface.get",
                json!({"surface_ref":shown.reference}),
            )
            .await;
        let value = match read {
            Ok(value) => value,
            Err(error) => {
                self.cards = super::Cards::default();
                return Err(error);
            }
        };
        let locale = self.state.locale;
        let fresh = Card::parse(locale, &value, &shown.conversation, &shown.reference)?;
        if !shown.same_revision(&fresh) {
            self.refresh_cards(bus).await?;
            return Err(invalid(locale, phrase(locale, Key::CardChanged)));
        }
        let request = shown.action_request(locale, component)?;
        let reference = shown.reference.clone();
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "surface",
                "surface.action",
                serde_json::to_value(&request)?,
            )
            .await;
        // Always re-read authority/state after sending, including a stale-state refusal.
        let reload = self.refresh_cards(bus).await;
        let result = match &response {
            Ok(body) if body.get("accepted").and_then(Value::as_bool) == Some(true) => {
                phrase(self.state.locale, Key::ActionSubmitted)
            }
            Ok(body) => {
                let locale = self.state.locale;
                let reason = body
                    .get("reason_code")
                    .and_then(Value::as_str)
                    .map_or_else(|| phrase(locale, Key::NotAccepted), super::render::safe);
                i18n::say(locale, Key::ActionRefused, &[("reason", Arg::Text(reason.as_str()))])
            }
            Err(_) => phrase(self.state.locale, Key::ActionSendFailed),
        };
        self.state.status_message = Some(result.clone());
        if let Some(card) = self.cards.rows.iter_mut().find(|c| c.reference == reference) {
            card.result = Some(result);
        }
        response?;
        reload?;
        Ok(())
    }
}

fn shown_number(locale: Locale, number: &serde_json::Number) -> String {
    if let Some(value) = number.as_i64() {
        return i18n::format_i64(locale, value);
    }
    if let Some(value) = number.as_u64() {
        return i18n::format_u64(locale, value);
    }
    number.to_string()
}

fn timestamp(locale: Locale) -> Result<String, TuiError> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| protocol(locale, error))?
        .as_secs();
    // Gregorian civil date from days since Unix epoch; integer arithmetic, UTC only.
    let days = i64::try_from(seconds / 86_400).map_err(|error| protocol(locale, error))? + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    ))
}
