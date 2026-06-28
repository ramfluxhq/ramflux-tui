// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};
use std::fmt::Write as _;

use crate::a2ui_render::{approval_a2ui_suffix, dispatch_a2ui_approval_action};
use crate::parsing::{
    parse_approval_row, parse_approvals, parse_contacts, parse_decrypted_messages, parse_devices,
    parse_groups, parse_messages, parse_transfer,
};
use crate::{
    ConversationRow, DEFAULT_CONVERSATION_ID, DEFAULT_TARGET_DELIVERY_ID, InputMode, MessageRow,
    Panel, PendingAttachment, TuiBus, TuiError, TuiInput, TuiState,
};

const DEFAULT_OBJECT_CHUNK_SIZE: usize = 64 * 1024;
const RELAY_URL_ENV: &str = "RAMFLUX_TUI_RELAY_URL";
const RELAY_KEY_ENV: &str = "RAMFLUX_TUI_RELAY_SERVICE_KEY_BASE64";

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

    /// Sets the local device id used as the sender id for direct messages.
    pub fn set_local_device_id(&mut self, device_id: impl Into<String>) {
        self.state.local_device_id = Some(device_id.into());
    }

    /// Loads initial projections from the local bus.
    ///
    /// # Errors
    /// Returns an error when any local bus projection request fails.
    pub async fn refresh_all<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        self.open_subscription(bus).await?;
        self.refresh_messages(bus).await?;
        self.receive_messages_with_attachments(bus).await?;
        self.refresh_contacts(bus).await?;
        self.refresh_devices(bus).await?;
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

    /// Handles one UI input event, surfacing any error into the status line.
    ///
    /// Unlike [`Self::handle_input`], this never returns the error to the caller:
    /// the event loop must keep running, so a failed input only sets
    /// `status_message` and leaves `should_quit` untouched.
    pub async fn dispatch_input<B: TuiBus + Send>(&mut self, bus: &mut B, input: TuiInput) {
        if let Err(error) = self.handle_input(bus, input).await {
            self.state.status_message = Some(format!("error: {error}"));
        }
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
            TuiInput::Enter if self.state.selected_panel == Panel::Objects => {
                self.submit_object_command(bus).await?;
            }
            TuiInput::Enter if self.state.selected_panel == Panel::Contacts => {
                self.submit_contact_command(bus).await?;
            }
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
            TuiInput::Char('r') if self.state.selected_panel == Panel::Objects => {
                self.resume_selected_transfer(bus).await?;
            }
            TuiInput::Char('s') if self.state.selected_panel == Panel::Objects => {
                self.refresh_selected_transfer_status(bus).await?;
            }
            TuiInput::Char('S') if self.state.selected_panel == Panel::Contacts => {
                self.refresh_selected_contact_safety(bus).await?;
            }
            TuiInput::Char('V') if self.state.selected_panel == Panel::Contacts => {
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
            TuiInput::Char(value) if self.state.selected_panel == Panel::Objects => {
                self.state.input.push(value);
            }
            TuiInput::Char(value) if self.state.selected_panel == Panel::Contacts => {
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
                    attachments: Vec::new(),
                    receipts: Vec::new(),
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
            Panel::Objects.title(),
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
            Panel::Objects => self.render_objects(frame, area),
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
                    "{marker}{} {}: {} ({}){}",
                    message.id,
                    message.sender,
                    message.body,
                    message.status,
                    Self::message_suffix(message)
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("Messages  /attach <path> l=delivered r=read x=delete")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_objects(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let lines = self
            .state
            .object_transfers
            .iter()
            .enumerate()
            .map(|(index, transfer)| {
                let marker = if index == self.state.selected_object_transfer { "> " } else { "  " };
                let error = transfer
                    .last_error
                    .as_ref()
                    .map_or_else(String::new, |value| format!(" error={value}"));
                Line::from(format!(
                    "{marker}{} {} {} {}/{} {}%{}",
                    transfer.object_id,
                    transfer.direction,
                    transfer.state,
                    transfer.done_bytes,
                    transfer.total_bytes,
                    transfer.percent,
                    error
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("Objects  put/get/status/resume commands  s=status r=resume")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn message_suffix(message: &MessageRow) -> String {
        let mut suffix = String::new();
        if !message.attachments.is_empty() {
            let summary = message
                .attachments
                .iter()
                .map(|attachment| format!("{}:{}", attachment.object_id, attachment.status))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(suffix, " attachments=[{summary}]");
        }
        if !message.receipts.is_empty() {
            let summary = message
                .receipts
                .iter()
                .map(|receipt| format!("{}:{}", receipt.device_id, receipt.state))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(suffix, " receipts=[{summary}]");
        }
        suffix
    }

    fn pending_attachments_json(attachments: &[PendingAttachment]) -> serde_json::Value {
        serde_json::Value::Array(
            attachments
                .iter()
                .map(|attachment| {
                    serde_json::json!({
                        "object_id": attachment.object_id,
                        "plaintext_base64": attachment.plaintext_base64,
                        "chunk_size": attachment.chunk_size,
                        "relay_endpoint": attachment.relay_endpoint,
                        "relay_service_key_base64": attachment.relay_service_key_base64,
                    })
                })
                .collect(),
        )
    }

    fn render_contacts(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let mut items = self
            .state
            .contacts
            .iter()
            .enumerate()
            .map(|(index, contact)| {
                let marker = if index == self.state.selected_contact { "> " } else { "  " };
                let safety = contact.fingerprint_hex.as_deref().unwrap_or("-");
                let safety_number = if contact.safety_number.is_empty() {
                    "-".to_owned()
                } else {
                    contact.safety_number.join(" ")
                };
                ListItem::new(format!(
                    "{marker}{} {} -> {} {} verify={} fp={} sn={safety_number}",
                    contact.link_id,
                    contact.requester,
                    contact.target,
                    contact.state,
                    contact.verification_state,
                    safety
                ))
            })
            .collect::<Vec<_>>();
        items.extend(self.state.devices.iter().map(|device| {
            let marker = if device.is_local { "* " } else { "  " };
            ListItem::new(format!(
                "{marker}device {} epoch={} target={}",
                device.device_id, device.device_epoch, device.target_delivery_id
            ))
        }));
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .title("Contacts/Devices  add/switch/accept + Enter  S=safety V=verify")
                    .borders(Borders::ALL),
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
            "remote_app: 需 App 签名"
        } else {
            "a=approve"
        }
    }

    async fn refresh_messages<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "message.read",
                serde_json::json!({"conversation_id": conversation_id}),
            )
            .await?;
        self.state.messages = parse_messages(&response);
        Ok(())
    }

    /// Receives gateway messages and asks the SDK to auto-fetch/decrypt attachments.
    ///
    /// # Errors
    /// Returns an error when the local bus receive request fails.
    pub async fn receive_messages_with_attachments<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        self.receive_messages_with_attachment_relay_key(bus, Self::relay_service_key_base64()).await
    }

    /// Receives gateway messages and auto-fetches/decrypts attachments with an explicit relay key.
    ///
    /// # Errors
    /// Returns an error when the local bus receive request fails.
    pub async fn receive_messages_with_attachment_relay_key<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        relay_service_key_base64: Option<String>,
    ) -> Result<(), TuiError> {
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "message.receive",
                serde_json::json!({
                    "limit": 100,
                    "conversation_id": conversation_id,
                    "auto_fetch_attachments": true,
                    "relay_service_key_base64": relay_service_key_base64,
                }),
            )
            .await?;
        for message in parse_decrypted_messages(&response) {
            self.upsert_message(message);
        }
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

    /// Refreshes local account devices for the contacts/devices panel.
    ///
    /// # Errors
    /// Returns an error when the local bus device list request fails.
    pub async fn refresh_devices<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "device",
                "device.list",
                serde_json::json!({}),
            )
            .await?;
        self.state.devices = parse_devices(&response);
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
                self.state.status_message = Some("该审批需 App 端签名授权(remote_app)".to_owned());
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
        if let Some(path) = body.strip_prefix("/attach ") {
            self.queue_attachment_path(path.trim())?;
            return Ok(());
        }
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX));
        let message_id = format!("tui_msg_{}", self.state.next_message_id);
        let envelope_id = format!("tui_env_{}", self.state.next_message_id);
        self.state.next_message_id = self.state.next_message_id.saturating_add(1);
        let conversation = self.selected_conversation();
        let conversation_id =
            conversation.map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let recipient_device_id =
            conversation.and_then(|row| row.recipient_device_id.as_ref()).cloned();
        let target_delivery_id = conversation
            .and_then(|row| row.target_delivery_id.as_ref())
            .cloned()
            .unwrap_or_else(|| DEFAULT_TARGET_DELIVERY_ID.to_owned());
        let account_status = self.account_status(bus).await?;
        let source_principal_id = account_status
            .get("principal_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(self.state.account_id.as_str())
            .to_owned();
        let sender_id = self
            .state
            .local_device_id
            .clone()
            .or_else(|| {
                account_status
                    .get("device_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| self.state.account_id.clone());
        let attachments = std::mem::take(&mut self.state.pending_attachments);
        let attachment_payload = Self::pending_attachments_json(&attachments);
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.submit",
            serde_json::json!({
                "conversation_id": conversation_id,
                "message_id": message_id,
                "envelope_id": envelope_id,
                "source_principal_id": source_principal_id,
                "sender_id": sender_id,
                "recipient_device_id": recipient_device_id,
                "target_delivery_id": target_delivery_id,
                "encrypted_body_base64": "",
                "plaintext_body_base64": ramflux_protocol::encode_base64url(body.as_bytes()),
                "created_at": created_at,
                "ttl": 3_600_u32,
                "attachments": attachment_payload,
            }),
        )
        .await?;
        self.push_message(MessageRow {
            id: message_id,
            sender: "me".to_owned(),
            body,
            status: "sending".to_owned(),
            attachments: attachments
                .into_iter()
                .map(|attachment| crate::AttachmentRow {
                    message_id: envelope_id.clone(),
                    object_id: attachment.object_id,
                    status: "uploading".to_owned(),
                    plaintext_base64: None,
                })
                .collect(),
            receipts: Vec::new(),
        });
        Ok(())
    }

    async fn account_status<B: TuiBus + Send>(
        &self,
        bus: &mut B,
    ) -> Result<serde_json::Value, TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "account",
            "account.status",
            serde_json::json!({}),
        )
        .await
    }

    /// Activates a restored device through the SDK manifest-publish path.
    ///
    /// # Errors
    /// Returns an error when the local bus device activation request fails.
    pub async fn activate_device<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        device_id: &str,
        target_delivery_id: &str,
        device_seed: [u8; 32],
        device_epoch: Option<u64>,
    ) -> Result<(), TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "device",
            "device.activate",
            serde_json::json!({
                "device_id": device_id,
                "target_delivery_id": target_delivery_id,
                "device_seed": device_seed,
                "device_epoch": device_epoch,
            }),
        )
        .await?;
        self.refresh_devices(bus).await?;
        Ok(())
    }

    /// Revokes one device through the root-authorized manifest update path.
    ///
    /// # Errors
    /// Returns an error when the local bus device revoke request fails.
    pub async fn revoke_device<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        device_id: &str,
    ) -> Result<(), TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "device",
            "device.revoke",
            serde_json::json!({ "device_id": device_id }),
        )
        .await?;
        self.refresh_devices(bus).await?;
        Ok(())
    }

    /// Exports an E2EE own-device history sync snapshot for one target device.
    ///
    /// # Errors
    /// Returns an error when the local bus own-device sync export request fails.
    pub async fn export_device_sync<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        target_device_id: &str,
        relay_endpoint: &str,
        relay_service_key_base64: Option<String>,
    ) -> Result<serde_json::Value, TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "device",
            "device.sync.export",
            serde_json::json!({
                "target_device_id": target_device_id,
                "relay_endpoint": relay_endpoint,
                "relay_service_key_base64": relay_service_key_base64,
            }),
        )
        .await
    }

    /// Imports an E2EE own-device history sync envelope.
    ///
    /// # Errors
    /// Returns an error when the local bus own-device sync import request fails.
    pub async fn import_device_sync<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        envelope: serde_json::Value,
        relay_service_key_base64: Option<String>,
    ) -> Result<serde_json::Value, TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "device",
            "device.sync.import",
            serde_json::json!({
                "envelope": envelope,
                "relay_service_key_base64": relay_service_key_base64,
            }),
        )
        .await
    }

    /// Queues one in-memory attachment for the next direct message.
    ///
    /// # Errors
    /// Returns an error when relay configuration is missing.
    pub fn queue_attachment_bytes(
        &mut self,
        object_id: impl Into<String>,
        plaintext: &[u8],
    ) -> Result<(), TuiError> {
        let relay_endpoint = Self::relay_endpoint()?;
        self.queue_attachment_bytes_for_relay(
            object_id,
            plaintext,
            relay_endpoint,
            Self::relay_service_key_base64(),
        );
        Ok(())
    }

    /// Queues one in-memory attachment with explicit relay settings.
    pub fn queue_attachment_bytes_for_relay(
        &mut self,
        object_id: impl Into<String>,
        plaintext: &[u8],
        relay_endpoint: impl Into<String>,
        relay_service_key_base64: Option<String>,
    ) {
        self.state.pending_attachments.push(PendingAttachment {
            object_id: object_id.into(),
            plaintext_base64: ramflux_protocol::encode_base64url(plaintext),
            chunk_size: DEFAULT_OBJECT_CHUNK_SIZE,
            relay_endpoint: relay_endpoint.into(),
            relay_service_key_base64,
        });
        self.state.status_message =
            Some(format!("queued attachment count={}", self.state.pending_attachments.len()));
    }

    /// Queues a file attachment for the next direct message.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read or relay configuration is missing.
    pub fn queue_attachment_path(&mut self, path: &str) -> Result<(), TuiError> {
        let bytes = std::fs::read(path)
            .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
        let object_id =
            std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).map_or_else(
                || format!("attachment:{}", self.state.pending_attachments.len()),
                |name| format!("attachment:{name}:{}", self.state.next_message_id),
            );
        self.queue_attachment_bytes(object_id, &bytes)
    }

    async fn submit_object_command<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let command = std::mem::take(&mut self.state.input);
        let parts = command.split_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            ["put", path, object_id] => self.put_object_path(bus, object_id, path).await?,
            ["get", object_id, _out] => self.get_object(bus, object_id).await?,
            ["status", object_id] => self.refresh_object_status(bus, object_id, None).await?,
            ["status", object_id, direction] => {
                self.refresh_object_status(bus, object_id, Some(direction)).await?;
            }
            ["resume", object_id, direction] => {
                self.resume_transfer(bus, object_id, direction).await?;
            }
            _ => {
                self.state.status_message =
                    Some("object command: put <path> <object_id> | get <object_id> <out> | status <object_id> [direction] | resume <object_id> <direction>".to_owned());
            }
        }
        Ok(())
    }

    /// Uploads one object through the SDK object relay path.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read or the local bus request fails.
    pub async fn put_object_path<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        object_id: &str,
        path: &str,
    ) -> Result<(), TuiError> {
        let bytes = std::fs::read(path)
            .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "object",
                "object.put",
                serde_json::json!({
                    "object_id": object_id,
                    "plaintext_base64": ramflux_protocol::encode_base64url(&bytes),
                    "chunk_size": DEFAULT_OBJECT_CHUNK_SIZE,
                    "relay_endpoint": Self::relay_endpoint().ok(),
                    "relay_service_key_base64": Self::relay_service_key_base64(),
                }),
            )
            .await?;
        self.apply_transfer_response(&response);
        Ok(())
    }

    /// Downloads one object through the SDK object relay path.
    ///
    /// # Errors
    /// Returns an error when the local bus request fails.
    pub async fn get_object<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        object_id: &str,
    ) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "object",
                "object.get",
                serde_json::json!({
                    "object_id": object_id,
                    "relay_endpoint": Self::relay_endpoint().ok(),
                    "relay_service_key_base64": Self::relay_service_key_base64(),
                    "relay_ack": true,
                }),
            )
            .await?;
        self.refresh_object_status(bus, object_id, Some("download")).await?;
        if response.get("plaintext_base64").is_some() {
            self.state.status_message = Some(format!("downloaded object {object_id}"));
        }
        Ok(())
    }

    /// Refreshes one object transfer status.
    ///
    /// # Errors
    /// Returns an error when the local bus request fails.
    pub async fn refresh_object_status<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        object_id: &str,
        direction: Option<&str>,
    ) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "object",
                "object.transfer.status",
                serde_json::json!({"object_id": object_id, "direction": direction}),
            )
            .await?;
        self.apply_transfer_response(&response);
        Ok(())
    }

    /// Refreshes the selected object transfer status.
    ///
    /// # Errors
    /// Returns an error when the local bus request fails.
    pub async fn refresh_selected_transfer_status<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(row) =
            self.state.object_transfers.get(self.state.selected_object_transfer).cloned()
        else {
            return Ok(());
        };
        self.refresh_object_status(bus, &row.object_id, Some(&row.direction)).await
    }

    /// Resumes the selected object transfer.
    ///
    /// # Errors
    /// Returns an error when the local bus request fails.
    pub async fn resume_selected_transfer<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let Some(row) =
            self.state.object_transfers.get(self.state.selected_object_transfer).cloned()
        else {
            return Ok(());
        };
        self.resume_transfer_with_relay(
            bus,
            &row.object_id,
            &row.direction,
            row.relay_endpoint.as_deref().ok_or_else(|| {
                TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                    "selected transfer is missing relay endpoint".to_owned(),
                ))
            })?,
            row.relay_service_key_base64.clone(),
        )
        .await
    }

    /// Resumes one object transfer through the SDK object relay path.
    ///
    /// # Errors
    /// Returns an error when the local bus request fails.
    pub async fn resume_transfer<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        object_id: &str,
        direction: &str,
    ) -> Result<(), TuiError> {
        self.resume_transfer_with_relay(
            bus,
            object_id,
            direction,
            &Self::relay_endpoint()?,
            Self::relay_service_key_base64(),
        )
        .await
    }

    async fn resume_transfer_with_relay<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        object_id: &str,
        direction: &str,
        relay_endpoint: &str,
        relay_service_key_base64: Option<String>,
    ) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "object",
                "object.transfer.resume",
                serde_json::json!({
                    "object_id": object_id,
                    "direction": direction,
                    "relay_endpoint": relay_endpoint,
                    "relay_service_key_base64": relay_service_key_base64,
                }),
            )
            .await?;
        self.apply_transfer_response(&response);
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

    /// Executes a contact/account command from the contacts panel input.
    ///
    /// # Errors
    /// Returns an error when the command is malformed or the local bus request fails.
    pub async fn submit_contact_command<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let command = std::mem::take(&mut self.state.input);
        let mut parts = command.split_whitespace();
        match parts.next() {
            Some("add") => {
                let link_id = required_contact_arg(parts.next(), "contact add link_id")?;
                let requester_id =
                    required_contact_arg(parts.next(), "contact add requester commitment")?;
                let target_id =
                    required_contact_arg(parts.next(), "contact add target commitment")?;
                ensure_no_extra_contact_arg(parts.next(), "contact add")?;
                self.add_contact(bus, link_id, requester_id, target_id).await?;
            }
            Some("accept") => {
                let link_id = required_contact_arg(parts.next(), "contact accept link_id")?;
                let requester_id =
                    required_contact_arg(parts.next(), "contact accept requester commitment")?;
                let target_id =
                    required_contact_arg(parts.next(), "contact accept target commitment")?;
                ensure_no_extra_contact_arg(parts.next(), "contact accept")?;
                self.accept_contact(bus, link_id, requester_id, target_id).await?;
            }
            Some("switch") => {
                let account_id = required_contact_arg(parts.next(), "account switch account_id")?;
                ensure_no_extra_contact_arg(parts.next(), "account switch")?;
                self.switch_account(bus, account_id).await?;
            }
            Some(other) => {
                return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(format!(
                    "unknown contacts command: {other}"
                ))));
            }
            None => {}
        }
        Ok(())
    }

    /// Adds a local trusted contact using principal commitments.
    ///
    /// # Errors
    /// Returns an error when the local bus contact.add request fails.
    pub async fn add_contact<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        link_id: &str,
        requester_id: &str,
        target_id: &str,
    ) -> Result<(), TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "contact",
            "contact.add",
            serde_json::json!({
                "link_id": link_id,
                "requester_id": requester_id,
                "target_id": target_id,
            }),
        )
        .await?;
        self.refresh_contacts(bus).await?;
        self.state.status_message = Some(format!("contact added: {link_id}"));
        Ok(())
    }

    /// Accepts a contact request using principal commitments.
    ///
    /// # Errors
    /// Returns an error when the local bus contact.accept request fails.
    pub async fn accept_contact<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        link_id: &str,
        requester_id: &str,
        target_id: &str,
    ) -> Result<(), TuiError> {
        bus.request(
            Some(self.state.account_id.clone()),
            "contact",
            "contact.accept",
            serde_json::json!({
                "link_id": link_id,
                "requester_id": requester_id,
                "target_id": target_id,
            }),
        )
        .await?;
        self.refresh_contacts(bus).await?;
        self.state.status_message = Some(format!("contact accepted: {link_id}"));
        Ok(())
    }

    /// Switches the active local bus account and refreshes TUI projections.
    ///
    /// # Errors
    /// Returns an error when account.switch or the follow-up refresh fails.
    pub async fn switch_account<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        account_id: &str,
    ) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(account_id.to_owned()),
                "account",
                "account.switch",
                serde_json::json!({}),
            )
            .await?;
        let active_account_id = response
            .get("active_account_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(account_id);
        active_account_id.clone_into(&mut self.state.account_id);
        self.state.selected_conversation = 0;
        self.state.selected_message = 0;
        self.state.selected_contact = 0;
        self.state.selected_group = 0;
        self.state.status_message = Some(format!("active account: {active_account_id}"));
        self.refresh_all(bus).await?;
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
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.delete",
            serde_json::json!({
                "conversation_id": conversation_id,
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
        let conversation = self.selected_conversation();
        let conversation_id =
            conversation.map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let recipient_device_id =
            conversation.and_then(|row| row.recipient_device_id.as_ref()).cloned();
        let target_delivery_id =
            conversation.and_then(|row| row.target_delivery_id.as_ref()).cloned();
        let receiver_device_id =
            self.state.local_device_id.clone().unwrap_or_else(|| self.state.account_id.clone());
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.receipt.delivered",
            serde_json::json!({
                "conversation_id": conversation_id,
                "message_id": message.id,
                "receiver_device_id": receiver_device_id,
                "recipient_device_id": recipient_device_id,
                "target_delivery_id": target_delivery_id,
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
        let conversation = self.selected_conversation();
        let conversation_id =
            conversation.map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let recipient_device_id =
            conversation.and_then(|row| row.recipient_device_id.as_ref()).cloned();
        let target_delivery_id =
            conversation.and_then(|row| row.target_delivery_id.as_ref()).cloned();
        let reader_id =
            self.state.local_device_id.clone().unwrap_or_else(|| self.state.account_id.clone());
        bus.request(
            Some(self.state.account_id.clone()),
            "message",
            "message.receipt.read",
            serde_json::json!({
                "conversation_id": conversation_id,
                "message_id": message.id,
                "reader_id": reader_id,
                "recipient_device_id": recipient_device_id,
                "target_delivery_id": target_delivery_id,
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

    /// Kicks a member from the selected group through a signed control event.
    ///
    /// # Errors
    /// Returns an error when the local bus group governance request fails.
    pub async fn kick_group_member<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        member_id: &str,
        actor_id: &str,
    ) -> Result<(), TuiError> {
        self.group_member_control(bus, "group.member.kick", member_id, actor_id).await
    }

    /// Bans a member from the selected group through a signed control event.
    ///
    /// # Errors
    /// Returns an error when the local bus group governance request fails.
    pub async fn ban_group_member<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        member_id: &str,
        actor_id: &str,
    ) -> Result<(), TuiError> {
        self.group_member_control(bus, "group.member.ban", member_id, actor_id).await
    }

    /// Invites a device to the selected group through a signed control event.
    ///
    /// # Errors
    /// Returns an error when the local bus group invite request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn invite_group_member<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        actor_id: &str,
        invitee_id: &str,
        invitee_signing_public_key: &str,
        target_delivery_id: &str,
        expires_at: i64,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        bus.request(
            Some(self.state.account_id.clone()),
            "group",
            "group.invite.create",
            serde_json::json!({
                "group_id": group.id,
                "actor_id": actor_id,
                "invitee_id": invitee_id,
                "invitee_signing_public_key": invitee_signing_public_key,
                "target_delivery_id": target_delivery_id,
                "role": "member",
                "expires_at": expires_at,
            }),
        )
        .await?;
        Ok(())
    }

    /// Accepts a pending group invite through a signed control event.
    ///
    /// # Errors
    /// Returns an error when the local bus group invite accept request fails.
    pub async fn accept_group_invite<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        actor_id: &str,
        invite_id: &str,
        target_delivery_id: Option<&str>,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "group",
                "group.invite.accept",
                serde_json::json!({
                    "group_id": group.id,
                    "actor_id": actor_id,
                    "invite_id": invite_id,
                    "target_delivery_id": target_delivery_id,
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

    async fn group_member_control<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        method: &str,
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
                method,
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

    /// Sets a member role in the selected group.
    ///
    /// # Errors
    /// Returns an error when the signed group governance request fails.
    pub async fn set_group_member_role<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        member_id: &str,
        actor_id: &str,
        role: &str,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "group",
                "group.role.set",
                serde_json::json!({
                    "group_id": group.id,
                    "actor_id": actor_id,
                    "member_id": member_id,
                    "role": role,
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

    /// Tombstones the selected group message through a signed control event.
    ///
    /// # Errors
    /// Returns an error when the local bus group message delete request fails.
    pub async fn delete_selected_group_message<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        actor_id: &str,
    ) -> Result<(), TuiError> {
        let Some(group) = self.state.groups.get(self.state.selected_group).cloned() else {
            return Ok(());
        };
        let Some(message) = self.state.messages.get(self.state.selected_message).cloned() else {
            return Ok(());
        };
        bus.request(
            Some(self.state.account_id.clone()),
            "group",
            "group.message.delete",
            serde_json::json!({
                "group_id": group.id,
                "actor_id": actor_id,
                "message_id": message.id,
                "delete_scope": "group_tombstone",
            }),
        )
        .await?;
        if let Some(row) = self.state.messages.get_mut(self.state.selected_message) {
            "deleted".clone_into(&mut row.status);
            row.body.clear();
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

    fn upsert_message(&mut self, message: MessageRow) {
        if let Some(existing) = self.state.messages.iter_mut().find(|row| row.id == message.id) {
            *existing = message;
        } else {
            self.push_message(message);
        }
    }

    fn apply_transfer_response(&mut self, response: &serde_json::Value) {
        let Some(mut transfer) = parse_transfer(response) else {
            return;
        };
        if transfer.relay_endpoint.is_none() {
            transfer.relay_endpoint = Self::relay_endpoint().ok();
        }
        if transfer.relay_service_key_base64.is_none() {
            transfer.relay_service_key_base64 = Self::relay_service_key_base64();
        }
        if let Some(existing) =
            self.state.object_transfers.iter_mut().find(|row| {
                row.object_id == transfer.object_id && row.direction == transfer.direction
            })
        {
            *existing = transfer;
        } else {
            self.state.object_transfers.push(transfer);
        }
    }

    fn relay_endpoint() -> Result<String, TuiError> {
        std::env::var(RELAY_URL_ENV).map_err(|_| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(format!(
                "missing {RELAY_URL_ENV} for relay object transfer"
            )))
        })
    }

    fn relay_service_key_base64() -> Option<String> {
        std::env::var(RELAY_KEY_ENV).ok().filter(|value| !value.is_empty())
    }

    fn move_selection_up(&mut self) {
        match self.state.selected_panel {
            Panel::Messages => {
                self.state.selected_message = self.state.selected_message.saturating_sub(1);
            }
            Panel::Objects => {
                self.state.selected_object_transfer =
                    self.state.selected_object_transfer.saturating_sub(1);
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
            Panel::Objects
                if self.state.selected_object_transfer + 1 < self.state.object_transfers.len() =>
            {
                self.state.selected_object_transfer += 1;
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

fn required_contact_arg<'a>(value: Option<&'a str>, label: &str) -> Result<&'a str, TuiError> {
    value.ok_or_else(|| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(format!("missing {label}"))))
}

fn ensure_no_extra_contact_arg(value: Option<&str>, command: &str) -> Result<(), TuiError> {
    if let Some(extra) = value {
        return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(format!(
            "unexpected {command} argument: {extra}"
        ))));
    }
    Ok(())
}
