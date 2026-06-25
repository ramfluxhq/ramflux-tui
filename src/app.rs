// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};

use crate::a2ui_render::{approval_a2ui_suffix, dispatch_a2ui_approval_action};
use crate::parsing::{
    parse_approval_row, parse_approvals, parse_contacts, parse_groups, parse_messages,
};
use crate::{
    ConversationRow, DEFAULT_CONVERSATION_ID, DEFAULT_TARGET_DELIVERY_ID, InputMode, MessageRow,
    Panel, TuiBus, TuiError, TuiInput, TuiState,
};

/// TUI application controller.
#[derive(Clone, Debug, Default)]
pub struct TuiApp {
    pub state: TuiState,
    should_quit: bool,
}

impl TuiApp {
    /// Creates an app for one local account.
    #[must_use]
    pub fn new(account_id: impl Into<String>) -> Self {
        Self {
            state: TuiState { account_id: account_id.into(), ..TuiState::default() },
            should_quit: false,
        }
    }

    /// Returns true after the user requests exit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Loads initial projections from the local bus.
    ///
    /// # Errors
    /// Returns an error when any local bus projection request fails.
    pub async fn refresh_all<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        self.open_subscription(bus).await?;
        self.refresh_messages(bus).await?;
        self.refresh_contacts(bus).await?;
        self.refresh_groups(bus).await?;
        self.refresh_approvals(bus).await?;
        self.ensure_default_conversation();
        Ok(())
    }

    /// Opens local bus subscriptions used by live refresh.
    ///
    /// # Errors
    /// Returns an error when subscription registration fails.
    pub async fn open_subscription<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "subscription",
            "subscription.open",
            serde_json::json!({
                "topics": ["gateway.deliver", "conversation.updated", "mcp.approval.request"],
                "attended_frontend": true,
            }),
        )
        .await?;
        Ok(())
    }

    /// Handles one UI input event.
    ///
    /// # Errors
    /// Returns an error when an input-triggered bus operation fails.
    pub async fn handle_input<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        input: TuiInput,
    ) -> Result<(), TuiError> {
        if self.state.input_mode == InputMode::Compose {
            return self.handle_compose_input(bus, input).await;
        }
        match input {
            TuiInput::Tab => self.state.selected_panel = self.state.selected_panel.next(),
            TuiInput::Up => self.move_selection_up(),
            TuiInput::Down => self.move_selection_down(),
            TuiInput::Enter => self.submit_current_message(bus).await?,
            TuiInput::EnterCompose if self.state.selected_panel == Panel::Messages => {
                self.state.input_mode = InputMode::Compose;
            }
            TuiInput::EnterCompose | TuiInput::ExitCompose => {}
            TuiInput::Char('a') if self.state.selected_panel == Panel::Approvals => {
                self.decide_selected_approval(bus, "grant.approve").await?;
            }
            TuiInput::Char('d') if self.state.selected_panel == Panel::Approvals => {
                self.decide_selected_approval(bus, "grant.deny").await?;
            }
            TuiInput::Char('s') if self.state.selected_panel == Panel::Contacts => {
                self.refresh_selected_contact_safety(bus).await?;
            }
            TuiInput::Char('v') if self.state.selected_panel == Panel::Contacts => {
                self.verify_selected_contact(bus).await?;
            }
            TuiInput::Char('x') if self.state.selected_panel == Panel::Messages => {
                self.delete_selected_message(bus).await?;
            }
            TuiInput::Char('l') if self.state.selected_panel == Panel::Messages => {
                self.mark_selected_message_delivered(bus).await?;
            }
            TuiInput::Char('r') if self.state.selected_panel == Panel::Messages => {
                self.mark_selected_message_read(bus).await?;
            }
            TuiInput::Char('m') if self.state.selected_panel == Panel::Groups => {
                self.mute_selected_group(bus, i64::MAX).await?;
            }
            TuiInput::Char('u') if self.state.selected_panel == Panel::Groups => {
                self.unmute_selected_group(bus).await?;
            }
            TuiInput::Char('e') if self.state.selected_panel == Panel::Groups => {
                self.set_selected_group_disappearing(bus, 3_600).await?;
            }
            TuiInput::Char(value) if self.state.selected_panel == Panel::Messages => {
                self.state.input.push(value);
            }
            TuiInput::Char(_value) => {}
            TuiInput::Backspace => {
                self.state.input.pop();
            }
            TuiInput::Quit => self.should_quit = true,
        }
        Ok(())
    }

    async fn handle_compose_input<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        input: TuiInput,
    ) -> Result<(), TuiError> {
        match input {
            TuiInput::Char(value) => self.state.input.push(value),
            TuiInput::EnterCompose => self.state.input.push('i'),
            TuiInput::Backspace => {
                self.state.input.pop();
            }
            TuiInput::ExitCompose => self.state.input_mode = InputMode::Normal,
            TuiInput::Enter => {
                self.submit_current_message(bus).await?;
                self.state.input_mode = InputMode::Normal;
            }
            TuiInput::Quit => self.state.input.push('q'),
            TuiInput::Tab | TuiInput::Up | TuiInput::Down => {}
        }
        Ok(())
    }

    /// Applies one local bus subscription event to the current view model.
    ///
    /// # Errors
    /// Returns an error when event contents cannot be parsed.
    pub fn handle_bus_event(&mut self, event: &ramflux_sdk::LocalBusFrame) -> Result<(), TuiError> {
        if event.method == "gateway.deliver" || event.method == "conversation.updated" {
            for entry in event
                .body
                .get("entries")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let envelope_id = entry
                    .pointer("/envelope/envelope_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("event");
                let sender = entry
                    .pointer("/envelope/source_principal_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("peer");
                self.push_message(MessageRow {
                    id: envelope_id.to_owned(),
                    sender: sender.to_owned(),
                    body: "[new encrypted message]".to_owned(),
                    status: "delivered".to_owned(),
                });
            }
        } else if event.method == "mcp.approval.request" {
            let row = parse_approval_row(&event.body);
            if let Some(existing) = self.state.approvals.iter_mut().find(|item| item.id == row.id) {
                *existing = row;
            } else {
                self.state.approvals.push(row);
            }
        }
        Ok(())
    }

    /// Renders the current UI.
    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
            .split(area);
        let titles = [
            Panel::Conversations.title(),
            Panel::Messages.title(),
            Panel::Contacts.title(),
            Panel::Groups.title(),
            Panel::Approvals.title(),
        ]
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
        frame.render_widget(
            Tabs::new(titles)
                .select(self.state.selected_panel.index())
                .block(Block::default().title("rf tui").borders(Borders::ALL))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            rows[0],
        );
        self.render_body(frame, rows[1]);
        let input_title = match self.state.input_mode {
            InputMode::Compose => "Message input [COMPOSE]",
            InputMode::Normal => "Message input [NORMAL i=compose]",
        };
        let input_text = self.state.status_message.as_ref().map_or_else(
            || self.state.input.clone(),
            |status| format!("{}\n{status}", self.state.input),
        );
        frame.render_widget(
            Paragraph::new(input_text)
                .block(Block::default().title(input_title).borders(Borders::ALL)),
            rows[2],
        );
    }

    fn render_body(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        match self.state.selected_panel {
            Panel::Conversations => self.render_conversations(frame, area),
            Panel::Messages => self.render_messages(frame, area),
            Panel::Contacts => self.render_contacts(frame, area),
            Panel::Groups => self.render_groups(frame, area),
            Panel::Approvals => self.render_approvals(frame, area),
        }
    }

    fn render_conversations(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let items = self
            .state
            .conversations
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let marker = if index == self.state.selected_conversation { "> " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::raw(row.title.as_str()),
                    Span::raw("  "),
                    Span::raw(row.last_message.as_str()),
                    Span::raw(format!("  unread={} {}", row.unread, row.status)),
                ]))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(Block::default().title("Conversations").borders(Borders::ALL)),
            area,
        );
    }

    fn render_messages(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let lines = self
            .state
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                let marker = if index == self.state.selected_message { "> " } else { "  " };
                Line::from(format!(
                    "{marker}{} {}: {} ({})",
                    message.id, message.sender, message.body, message.status
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("Messages  l=delivered r=read x=delete")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_contacts(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let items = self
            .state
            .contacts
            .iter()
            .enumerate()
            .map(|(index, contact)| {
                let marker = if index == self.state.selected_contact { "> " } else { "  " };
                let safety = contact.fingerprint_hex.as_deref().unwrap_or("-");
                ListItem::new(format!(
                    "{marker}{} {} -> {} {} verify={} fp={}",
                    contact.link_id,
                    contact.requester,
                    contact.target,
                    contact.state,
                    contact.verification_state,
                    safety
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(
                Block::default().title("Contacts  s=safety-number v=verify").borders(Borders::ALL),
            ),
            area,
        );
    }

    fn render_groups(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let items = self
            .state
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let marker = if index == self.state.selected_group { "> " } else { "  " };
                ListItem::new(format!(
                    "{marker}{} members={} disappearing={:?} mute_until={:?}",
                    group.id,
                    group.members.join(","),
                    group.disappearing_ttl_secs,
                    group.mute_until
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .title("Groups  m=mute u=unmute e=disappearing")
                    .borders(Borders::ALL),
            ),
            area,
        );
    }

    fn render_approvals(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let lines = self
            .state
            .approvals
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if index == self.state.selected_approval { "> " } else { "  " };
                Line::from(format!(
                    "{marker}{} {} risk={} mode={} {} {}{}",
                    item.id,
                    item.tool,
                    item.risk,
                    item.confirmation_mode,
                    item.status,
                    Self::approval_action_hint(item),
                    approval_a2ui_suffix(item)
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title("Approvals  a=approve d=deny").borders(Borders::ALL))
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn approval_action_hint(item: &crate::ApprovalRow) -> &'static str {
        if item.confirmation_mode == "remote_app" {
            "remote_app: App signature required"
        } else {
            "a=approve"
        }
    }

    async fn refresh_messages<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "message.read",
                serde_json::json!({"conversation_id": DEFAULT_CONVERSATION_ID}),
            )
            .await?;
        self.state.messages = parse_messages(&response);
        Ok(())
    }

    async fn refresh_contacts<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "contact",
                "contact.list",
                serde_json::json!({}),
            )
            .await?;
        self.state.contacts = parse_contacts(&response);
        Ok(())
    }

    async fn refresh_groups<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "group",
                "group.list",
                serde_json::json!({}),
            )
            .await?;
        self.state.groups = parse_groups(&response);
        Ok(())
    }

    async fn refresh_approvals<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "mcp",
                "mcp.approval.list",
                serde_json::json!({}),
            )
            .await?;
        self.state.approvals = parse_approvals(&response);
        Ok(())
    }

    async fn decide_selected_approval<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        method: &str,
    ) -> Result<(), TuiError> {
        let Some(approval) = self.state.approvals.get(self.state.selected_approval).cloned() else {
            return Ok(());
        };
        if method == "grant.approve" {
            if approval.confirmation_mode == "remote_app" {
                self.state.status_message =
                    Some("This approval requires App-side signing (remote_app)".to_owned());
                return Ok(());
            }
            dispatch_a2ui_approval_action(bus, &self.state.account_id, &approval).await?;
        }
        bus.request(
            Some(self.state.account_id.clone()),
            "grant",
            method,
            serde_json::json!({"approval_id": approval.id}),
        )
        .await?;
        self.state.status_message = None;
        self.refresh_approvals(bus).await
    }

    async fn submit_current_message<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        if self.state.selected_panel != Panel::Messages || self.state.input.is_empty() {
            return Ok(());
        }
        let body = std::mem::take(&mut self.state.input);
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX));
        let message_id = format!("tui_msg_{}", self.state.next_message_id);
        let envelope_id = format!("tui_env_{}", self.state.next_message_id);
        self.state.next_message_id = self.state.next_message_id.saturating_add(1);
        let conversation = self.selected_conversation();
        let conversation_id = conversation.map_or(DEFAULT_CONVERSATION_ID, |row| row.id.as_str());
        let recipient_device_id =
            conversation.and_then(|row| row.recipient_device_id.as_ref()).cloned();
        let target_delivery_id = conversation
            .and_then(|row| row.target_delivery_id.as_ref())
            .cloned()
            .unwrap_or_else(|| DEFAULT_TARGET_DELIVERY_ID.to_owned());
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.submit",
            serde_json::json!({
                "conversation_id": conversation_id,
                "message_id": message_id,
                "envelope_id": envelope_id,
                "source_principal_id": self.state.account_id,
                "sender_id": self.state.account_id,
                "recipient_device_id": recipient_device_id,
                "target_delivery_id": target_delivery_id,
                "encrypted_body_base64": "",
                "plaintext_body_base64": ramflux_protocol::encode_base64url(body.as_bytes()),
                "created_at": created_at,
                "ttl": 3_600_u32,
            }),
        )
        .await?;
        self.push_message(MessageRow {
            id: message_id,
            sender: "me".to_owned(),
            body,
            status: "sending".to_owned(),
        });
        Ok(())
    }

    /// Refreshes the safety number for the selected contact.
    ///
    /// # Errors
    /// Returns an error when the local bus safety-number request fails.
    pub async fn refresh_selected_contact_safety<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(contact) = self.state.contacts.get(self.state.selected_contact).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "contact",
                "contact.safety_number",
                serde_json::json!({"contact_identity_commitment": contact.target}),
            )
            .await?;
        self.apply_contact_safety_response(&response);
        Ok(())
    }

    /// Marks the selected contact as verified after an out-of-band safety-number check.
    ///
    /// # Errors
    /// Returns an error when the local bus verification request fails.
    pub async fn verify_selected_contact<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(contact) = self.state.contacts.get(self.state.selected_contact).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "contact",
                "contact.verify",
                serde_json::json!({"contact_identity_commitment": contact.target}),
            )
            .await?;
        self.apply_contact_safety_response(&response);
        Ok(())
    }

    /// Deletes the selected message from the local projection.
    ///
    /// # Errors
    /// Returns an error when the local bus delete request fails.
    pub async fn delete_selected_message<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(message) = self.state.messages.get(self.state.selected_message).cloned() else {
            return Ok(());
        };
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.delete",
            serde_json::json!({
                "conversation_id": DEFAULT_CONVERSATION_ID,
                "message_id": message.id,
            }),
        )
        .await?;
        if let Some(row) = self.state.messages.get_mut(self.state.selected_message) {
            "deleted".clone_into(&mut row.status);
        }
        Ok(())
    }

    /// Marks the selected message delivered in the local projection.
    ///
    /// # Errors
    /// Returns an error when the local bus receipt request fails.
    pub async fn mark_selected_message_delivered<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(message) = self.state.messages.get(self.state.selected_message).cloned() else {
            return Ok(());
        };
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.receipt.delivered",
            serde_json::json!({
                "conversation_id": DEFAULT_CONVERSATION_ID,
                "message_id": message.id,
                "receiver_device_id": self.state.account_id,
            }),
        )
        .await?;
        if let Some(row) = self.state.messages.get_mut(self.state.selected_message) {
            "delivered".clone_into(&mut row.status);
        }
        Ok(())
    }

    /// Marks the selected message read in the local projection.
    ///
    /// # Errors
    /// Returns an error when the local bus receipt request fails.
    pub async fn mark_selected_message_read<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(message) = self.state.messages.get(self.state.selected_message).cloned() else {
            return Ok(());
        };
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.receipt.read",
            serde_json::json!({
                "conversation_id": DEFAULT_CONVERSATION_ID,
                "message_id": message.id,
                "reader_id": self.state.account_id,
            }),
        )
        .await?;
        if let Some(row) = self.state.messages.get_mut(self.state.selected_message) {
            "read".clone_into(&mut row.status);
        }
        Ok(())
    }

    /// Removes a member from the selected group.
    ///
    /// # Errors
    /// Returns an error when the local bus group governance request fails.
    pub async fn remove_group_member<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        member_id: &str,
        actor_id: &str,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "group",
                "group.member.remove",
                serde_json::json!({
                    "group_id": group.id,
                    "actor_id": actor_id,
                    "member_id": member_id,
                }),
            )
            .await?;
        if let Some(row) = self.state.groups.get_mut(self.state.selected_group) {
            row.members = response
                .get("members")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect();
        }
        Ok(())
    }

    /// Sets a disappearing-message policy for the selected group conversation.
    ///
    /// # Errors
    /// Returns an error when the local bus conversation request fails.
    pub async fn set_selected_group_disappearing<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        ttl_secs: i64,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "conversation.disappearing.set",
                serde_json::json!({
                    "conversation_id": group.id,
                    "ttl_secs": ttl_secs,
                }),
            )
            .await?;
        if let Some(row) = self.state.groups.get_mut(self.state.selected_group) {
            row.disappearing_ttl_secs =
                response.get("ttl_secs").and_then(serde_json::Value::as_i64);
        }
        Ok(())
    }

    /// Mutes the selected group conversation.
    ///
    /// # Errors
    /// Returns an error when the local bus conversation request fails.
    pub async fn mute_selected_group<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        mute_until: i64,
    ) -> Result<(), TuiError> {
        self.set_selected_group_mute(bus, Some(mute_until), false).await
    }

    /// Unmutes the selected group conversation.
    ///
    /// # Errors
    /// Returns an error when the local bus conversation request fails.
    pub async fn unmute_selected_group<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        self.set_selected_group_mute(bus, None, true).await
    }

    async fn set_selected_group_mute<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        mute_until: Option<i64>,
        unmute: bool,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "conversation.mute",
                serde_json::json!({
                    "conversation_id": group.id,
                    "mute_until": mute_until,
                    "unmute": unmute,
                }),
            )
            .await?;
        if let Some(row) = self.state.groups.get_mut(self.state.selected_group) {
            row.mute_until = response.get("mute_until").and_then(serde_json::Value::as_i64);
        }
        Ok(())
    }

    fn apply_contact_safety_response(&mut self, response: &serde_json::Value) {
        let Some(contact_id) =
            response.get("contact_identity_commitment").and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some(contact) = self
            .state
            .contacts
            .iter_mut()
            .find(|contact| contact.target == contact_id || contact.requester == contact_id)
        else {
            return;
        };
        contact.safety_number = response
            .get("safety_number")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect();
        contact.fingerprint_hex =
            response.get("fingerprint_hex").and_then(serde_json::Value::as_str).map(str::to_owned);
        response
            .get("verification_state")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unverified")
            .clone_into(&mut contact.verification_state);
    }

    fn ensure_default_conversation(&mut self) {
        if !self.state.conversations.is_empty() {
            return;
        }
        let last_message =
            self.state.messages.last().map_or_else(String::new, |message| message.body.clone());
        self.state.conversations.push(ConversationRow {
            id: DEFAULT_CONVERSATION_ID.to_owned(),
            title: "Default DM".to_owned(),
            last_message,
            unread: 0,
            status: "synced".to_owned(),
            recipient_device_id: None,
            target_delivery_id: None,
        });
    }

    fn selected_conversation(&self) -> Option<&ConversationRow> {
        self.state.conversations.get(self.state.selected_conversation)
    }

    fn push_message(&mut self, message: MessageRow) {
        self.state.messages.push(message);
        let last_message =
            self.state.messages.last().map_or_else(String::new, |message| message.body.clone());
        if let Some(conversation) = self.state.conversations.first_mut() {
            conversation.last_message = last_message;
            conversation.unread = conversation.unread.saturating_add(1);
            "new".clone_into(&mut conversation.status);
        } else {
            self.ensure_default_conversation();
        }
    }

    fn move_selection_up(&mut self) {
        match self.state.selected_panel {
            Panel::Messages => {
                self.state.selected_message = self.state.selected_message.saturating_sub(1);
            }
            Panel::Contacts => {
                self.state.selected_contact = self.state.selected_contact.saturating_sub(1);
            }
            Panel::Groups => {
                self.state.selected_group = self.state.selected_group.saturating_sub(1);
            }
            Panel::Approvals => {
                self.state.selected_approval = self.state.selected_approval.saturating_sub(1);
            }
            Panel::Conversations => {
                self.state.selected_conversation =
                    self.state.selected_conversation.saturating_sub(1);
            }
        }
    }

    fn move_selection_down(&mut self) {
        match self.state.selected_panel {
            Panel::Messages if self.state.selected_message + 1 < self.state.messages.len() => {
                self.state.selected_message += 1;
            }
            Panel::Contacts if self.state.selected_contact + 1 < self.state.contacts.len() => {
                self.state.selected_contact += 1;
            }
            Panel::Groups if self.state.selected_group + 1 < self.state.groups.len() => {
                self.state.selected_group += 1;
            }
            Panel::Approvals if self.state.selected_approval + 1 < self.state.approvals.len() => {
                self.state.selected_approval += 1;
            }
            _ if self.state.selected_conversation + 1 < self.state.conversations.len() => {
                self.state.selected_conversation += 1;
            }
            _ => {}
        }
    }
}
