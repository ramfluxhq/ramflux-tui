// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Bounded terminal projection. Unsupported declarations never become executable controls.
use super::{Card, Cards};
use crate::i18n::{self, Arg, Key, Locale, LocaleGuard};
use ramflux_protocol::JsonPointer;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub(super) struct Row {
    pub text: String,
    pub control: Option<Value>,
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

pub(super) fn safe(text: &str) -> String {
    // Use the same terminal-safe Unicode policy as protocol text fallbacks.
    let quoted = ramflux_protocol::text_safety::quote_fallback_field(text);
    quoted.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(&quoted).to_owned()
}

impl Card {
    pub(super) fn value(&self, value: &Value) -> Option<Value> {
        if let Some(path) = value.get("@path").and_then(Value::as_str) {
            let pointer = JsonPointer::parse(path).ok()?;
            return self.drafts.get(path).cloned().or_else(|| {
                self.state.as_ref()?.content().and_then(|s| pointer.get(&s.data_model)).cloned()
            });
        }
        if value.as_object().is_some_and(|o| o.keys().any(|key| key.starts_with('@'))) {
            return None;
        }
        Some(value.clone())
    }

    fn text(&self, value: Option<&Value>) -> String {
        let locale = i18n::active();
        match value.and_then(|v| self.value(v)) {
            Some(Value::String(text)) => safe(&text),
            Some(Value::Number(number)) => shown_number(locale, &number),
            Some(Value::Bool(value)) => {
                i18n::text(locale, if value { Key::BoolTrue } else { Key::BoolFalse }).to_owned()
            }
            _ => i18n::text(locale, Key::UnsupportedValue).to_owned(),
        }
    }

    fn label(&self, component: &Value) -> String {
        component
            .get("child")
            .and_then(Value::as_str)
            .and_then(|id| self.component(id))
            .map_or_else(
                || i18n::text(i18n::active(), Key::ButtonLabel).to_owned(),
                |child| self.text(child.get("text")),
            )
    }

    fn component(&self, id: &str) -> Option<&Value> {
        self.state
            .as_ref()?
            .content()?
            .components
            .iter()
            .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
    }

    pub(super) fn rows(&self, locale: Locale) -> Vec<Row> {
        let _guard = LocaleGuard::enter(locale);
        let mut rows = Vec::new();
        if self.state.is_some() {
            self.walk("root", 0, &mut rows);
        }
        if rows.is_empty() {
            rows.push(Row { text: safe(&self.fallback), control: None });
        }
        rows
    }

    fn choices(&self, component: &Value) -> String {
        let selected = component.get("value").and_then(|v| self.value(v));
        component.get("options").and_then(Value::as_array).map_or_else(
            || i18n::text(i18n::active(), Key::ChoicesUnavailable).to_owned(),
            |options| {
                options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        let checked = selected
                            .as_ref()
                            .and_then(Value::as_array)
                            .is_some_and(|a| option.get("value").is_some_and(|v| a.contains(v)));
                        let mark = if checked { "[x]" } else { "[ ]" };
                        let label = self.text(option.get("label"));
                        let position = i18n::format_u64(
                            i18n::active(),
                            u64::try_from(index.saturating_add(1)).unwrap_or(u64::MAX),
                        );
                        i18n::say(
                            i18n::active(),
                            Key::ChoiceItem,
                            &[
                                ("index", Arg::Text(position.as_str())),
                                ("mark", Arg::Text(mark)),
                                ("label", Arg::Text(label.as_str())),
                            ],
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ")
            },
        )
    }

    fn walk(&self, id: &str, depth: usize, rows: &mut Vec<Row>) {
        if depth >= 16 || rows.len() >= 256 {
            return;
        }
        let Some(component) = self.component(id) else {
            return;
        };
        let kind = component.get("component").and_then(Value::as_str).unwrap_or("");
        let mut control = None;
        let text = match kind {
            "Text" => self.text(component.get("text")),
            "Divider" => i18n::text(i18n::active(), Key::Divider).to_owned(),
            "Row" | "Column" | "List" | "Card" => String::new(),
            "Button" if component.pointer("/action/event").is_some() => {
                control = Some(component.clone());
                i18n::say(
                    i18n::active(),
                    Key::ButtonChrome,
                    &[("label", Arg::Text(&self.label(component)))],
                )
            }
            "Button" => i18n::say(
                i18n::active(),
                Key::LinkUnavailable,
                &[("label", Arg::Text(&self.label(component)))],
            ),
            "TextField" | "DateTimeInput" | "Slider" | "CheckBox" | "ChoicePicker" => {
                if component.pointer("/value/@path").is_some() {
                    control = Some(component.clone());
                }
                let label = component.get("label").map_or_else(|| safe(id), |v| self.text(Some(v)));
                let value = if kind == "ChoicePicker" {
                    self.choices(component)
                } else {
                    self.text(component.get("value"))
                };
                i18n::say(
                    i18n::active(),
                    Key::FieldLine,
                    &[("label", Arg::Text(label.as_str())), ("value", Arg::Text(value.as_str()))],
                )
            }
            _ => i18n::say(
                i18n::active(),
                Key::UnsupportedComponent,
                &[("kind", Arg::Text(&safe(kind)))],
            ),
        };
        let supported = matches!(kind, "Row" | "Column" | "List" | "Card");
        if !text.is_empty() {
            if self.unavailable.is_some() {
                control = None;
            }
            rows.push(Row { text, control });
        }
        if supported {
            if let Some(child) = component.get("child").and_then(Value::as_str) {
                self.walk(child, depth + 1, rows);
            }
            if let Some(children) = component.get("children") {
                if let Some(children) = children.as_array() {
                    for child in children.iter().filter_map(Value::as_str) {
                        self.walk(child, depth + 1, rows);
                    }
                } else {
                    rows.push(Row {
                        text: i18n::text(i18n::active(), Key::RepeatedUnavailable).to_owned(),
                        control: None,
                    });
                }
            }
        }
    }
}

impl Cards {
    pub(crate) fn summary(&self, locale: Locale) -> Vec<ratatui::text::Line<'static>> {
        let mut lines = Vec::new();
        for card in &self.rows {
            let fallback = safe(&card.fallback);
            lines.push(ratatui::text::Line::from(i18n::say(
                locale,
                Key::CardSummary,
                &[("fallback", Arg::Text(fallback.as_str()))],
            )));
        }
        if let Some(notice) = &self.notice {
            lines.push(ratatui::text::Line::from(notice.clone()));
        }
        lines
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, locale: Locale) {
        let Some(card) = self.rows.get(self.selected) else {
            return;
        };
        let fallback = safe(&card.fallback);
        let index = i18n::format_u64(
            locale,
            u64::try_from(self.selected.saturating_add(1)).unwrap_or(u64::MAX),
        );
        let total = i18n::format_u64(locale, u64::try_from(self.rows.len()).unwrap_or(u64::MAX));
        let mut lines = vec![i18n::say(
            locale,
            Key::CardHeader,
            &[
                ("index", Arg::Text(index.as_str())),
                ("total", Arg::Text(total.as_str())),
                ("fallback", Arg::Text(fallback.as_str())),
            ],
        )];
        if let Some(reason) = &card.unavailable {
            lines.push(reason.clone());
        }
        let mut control = 0;
        let mut selected_line = 0;
        for row in card.rows(locale) {
            let marker = if row.control.is_some() {
                let selected = control == self.control;
                control += 1;
                if selected {
                    selected_line = lines.len();
                    "> "
                } else {
                    "  "
                }
            } else {
                "  "
            };
            lines.push(format!("{marker}{}", row.text));
        }
        if let Some(text) = &self.editing {
            let shown = safe(text);
            lines.push(i18n::say(locale, Key::EditLine, &[("text", Arg::Text(shown.as_str()))]));
            selected_line = lines.len();
        }
        if let Some(result) = &card.result {
            lines.push(result.clone());
        }
        if let Some(notice) = &self.notice {
            lines.push(notice.clone());
        }
        let height = usize::from(area.height.saturating_sub(2));
        let offset = selected_line.saturating_sub(height.saturating_sub(3));
        frame.render_widget(
            Paragraph::new(lines.into_iter().map(ratatui::text::Line::from).collect::<Vec<_>>())
                .scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0))
                .block(
                    Block::default()
                        .title(i18n::text(locale, Key::CardChromeTitle))
                        .borders(Borders::ALL),
                ),
            area,
        );
    }
}
