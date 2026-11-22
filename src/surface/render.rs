// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Bounded terminal projection. Unsupported declarations never become executable controls.
use super::{Card, Cards};
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
        match value.and_then(|v| self.value(v)) {
            Some(Value::String(text)) => safe(&text),
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::Bool(value)) => value.to_string(),
            _ => "[Unsupported display value]".into(),
        }
    }

    fn label(&self, component: &Value) -> String {
        component
            .get("child")
            .and_then(Value::as_str)
            .and_then(|id| self.component(id))
            .map_or_else(|| "Button".into(), |child| self.text(child.get("text")))
    }

    fn component(&self, id: &str) -> Option<&Value> {
        self.state
            .as_ref()?
            .content()?
            .components
            .iter()
            .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
    }

    pub(super) fn rows(&self) -> Vec<Row> {
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
            || "[Choices unavailable]".into(),
            |options| {
                options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        let checked = selected
                            .as_ref()
                            .and_then(Value::as_array)
                            .is_some_and(|a| option.get("value").is_some_and(|v| a.contains(v)));
                        format!(
                            "{}. {} {}",
                            index + 1,
                            if checked { "[x]" } else { "[ ]" },
                            self.text(option.get("label"))
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
            "Divider" => "────────────────".into(),
            "Row" | "Column" | "List" | "Card" => String::new(),
            "Button" if component.pointer("/action/event").is_some() => {
                control = Some(component.clone());
                format!("[ {} ]", self.label(component))
            }
            "Button" => {
                format!("[{}: link/function unavailable in this terminal]", self.label(component))
            }
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
                format!("{label}: {value}")
            }
            _ => format!("[Unsupported card component: {}]", safe(kind)),
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
                        text: "[Repeated content unavailable in this terminal]".into(),
                        control: None,
                    });
                }
            }
        }
    }
}

impl Cards {
    pub(crate) fn summary(&self) -> Vec<ratatui::text::Line<'static>> {
        let mut lines = Vec::new();
        for card in &self.rows {
            lines
                .push(ratatui::text::Line::from(format!("Card: {}  c=open", safe(&card.fallback))));
        }
        if let Some(notice) = &self.notice {
            lines.push(ratatui::text::Line::from(notice.clone()));
        }
        lines
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(card) = self.rows.get(self.selected) else {
            return;
        };
        let mut lines = vec![format!(
            "Card {}/{} — {}",
            self.selected + 1,
            self.rows.len(),
            safe(&card.fallback)
        )];
        if let Some(reason) = &card.unavailable {
            lines.push(reason.clone());
        }
        let mut control = 0;
        let mut selected_line = 0;
        for row in card.rows() {
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
            lines.push(format!("Edit: {}  Enter=save Esc=cancel", safe(text)));
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
                        .title("Card  ↑↓=select Enter=act [ ]=card Esc=back")
                        .borders(Borders::ALL),
                ),
            area,
        );
    }
}
