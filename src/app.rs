// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

#[path = "account_ui.rs"]
mod account_ui;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};
use std::fmt::Write as _;

use crate::i18n::{self, Key};
use crate::parsing::{
    parse_approval_row, parse_approvals, parse_contacts, parse_conversations, parse_devices,
    parse_groups, parse_transfer,
};
use crate::read_model::{self, generated_code};
use crate::{
    ConversationRow, DEFAULT_CONVERSATION_ID, InputMode, MessageRow, Panel, PendingAttachment,
    TuiBus, TuiError, TuiInput, TuiState,
};

const DEFAULT_OBJECT_CHUNK_SIZE: usize = 64 * 1024;
const RELAY_URL_ENV: &str = "RAMFLUX_TUI_RELAY_URL";
const RELAY_KEY_ENV: &str = "RAMFLUX_TUI_RELAY_SERVICE_KEY_BASE64";
/// Upper bound on consecutive change batches so a stuck `more` flag cannot spin.
const MAX_CHANGE_ROUNDS: u32 = 8;
/// Newest-first cache. Older rows past this cap are dropped.
pub(crate) const MAX_CACHED_MESSAGES: usize = 100;

/// TUI application controller.
pub struct TuiApp {
    pub state: TuiState,
    should_quit: bool,
    pub(crate) cards: crate::surface::Cards,
    pub(crate) stream_cursors: std::collections::BTreeMap<String, u64>,
    account_ui: account_ui::AccountUi,
    socket: Option<std::path::PathBuf>,
}

impl TuiApp {
    /// Creates an app for one local account.
    #[must_use]
    pub fn new(account_id: impl Into<String>) -> Self {
        Self {
            state: TuiState { account_id: account_id.into(), ..TuiState::default() },
            should_quit: false,
            cards: crate::surface::Cards::default(),
            stream_cursors: std::collections::BTreeMap::new(),
            account_ui: account_ui::AccountUi::default(),
            socket: None,
        }
    }

    /// Remembers the daemon socket used by creation and recovery.
    pub fn set_socket(&mut self, socket: impl AsRef<std::path::Path>) {
        self.socket = Some(socket.as_ref().to_path_buf());
    }

    /// Applies `RAMFLUX_TUI_LANG` when it is set. Unset stays English.
    pub fn apply_locale_env(&mut self) {
        if let Ok(value) = std::env::var("RAMFLUX_TUI_LANG") {
            self.state.locale = i18n::locale_from_env(&value);
        }
    }

    /// Account screens capture letters that the normal map treats as commands.
    #[must_use]
    pub const fn captures_text(&self) -> bool {
        self.account_ui.is_open()
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
        self.refresh_conversations(bus).await?;
        self.refresh_messages(bus).await?;
        self.subscribe_stream(bus).await?;
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
        self.subscribe_stream(bus).await
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
        if self.account_ui.is_open() {
            return self.handle_account_input(bus, input).await;
        }
        if self.cards.open {
            return self.handle_card_input(bus, &input).await;
        }
        if self.state.input_mode == InputMode::Compose {
            return self.handle_compose_input(bus, input).await;
        }
        match input {
            TuiInput::Tab => self.state.selected_panel = self.state.selected_panel.next(),
            TuiInput::Up => self.move_conversation_selection(bus, true).await?,
            TuiInput::Down => self.move_conversation_selection(bus, false).await?,
            TuiInput::Enter if self.state.selected_panel == Panel::Objects => {
                self.submit_object_command(bus).await?;
            }
            TuiInput::Enter if self.state.selected_panel == Panel::Contacts => {
                self.submit_contact_command(bus).await?;
            }
            TuiInput::Enter => self.submit_current_message(bus).await?,
            TuiInput::EnterCompose
                if matches!(
                    self.state.selected_panel,
                    Panel::Messages | Panel::Objects | Panel::Contacts
                ) =>
            {
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
            TuiInput::Char('c') if self.state.selected_panel == Panel::Messages => {
                self.refresh_cards(bus).await?;
                self.cards.open = !self.cards.rows.is_empty();
                if !self.cards.open {
                    self.state.status_message = Some("No cards in this conversation".into());
                }
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
            TuiInput::Char('L') => self.lock_account(bus).await?,
            TuiInput::Char('N') => self.begin_create(),
            TuiInput::Char('P') => self.begin_rotate(),
            TuiInput::Char('Y') => self.begin_recover().await?,
            TuiInput::Char('G') => {
                self.state.locale = self.state.locale.toggle();
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
                match self.state.selected_panel {
                    Panel::Objects => self.submit_object_command(bus).await?,
                    Panel::Contacts => self.submit_contact_command(bus).await?,
                    _ => self.submit_current_message(bus).await?,
                }
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
        if event.method == "mcp.approval.request" {
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
                .block(
                    Block::default()
                        .title(format!("rf tui  {}", i18n::text(self.state.locale, Key::HintKeys)))
                        .borders(Borders::ALL),
                )
                .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            rows[0],
        );
        if self.account_ui.is_open() {
            self.render_account(frame, rows[1]);
            self.render_account_footer(frame, rows[2]);
            return;
        }
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
        if self.cards.open {
            self.cards.render(frame, area);
            return;
        }
        let mut lines = self
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
        lines.extend(self.cards.summary());
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("Messages  c=cards  /attach <path> l=delivered r=read x=delete")
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
                        .title("Objects  i=type put/get/status/resume command  s=status r=resume")
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
                    .title(
                        "Contacts/Devices  i=type add/switch/accept then Enter  S=safety V=verify",
                    )
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
                    "{marker}{} {} risk={} mode={} {} {}",
                    item.id,
                    item.tool,
                    item.risk,
                    item.confirmation_mode,
                    item.status,
                    Self::approval_action_hint(item)
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

    /// Loads the real conversation list from storage via the local bus.
    ///
    /// When the account has real conversations they replace the list entirely
    /// (the synthetic "Default DM" row is bypassed). When none exist the list
    /// is left untouched so [`Self::ensure_default_conversation`] can keep a
    /// usable empty-state row.
    ///
    /// # Errors
    /// Returns an error when the local bus conversation list request fails.
    pub(crate) async fn refresh_conversations<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "conversation.list",
                serde_json::json!({}),
            )
            .await?;
        let conversations = parse_conversations(&response);
        if !conversations.is_empty() {
            self.state.conversations = conversations;
            if self.state.selected_conversation >= self.state.conversations.len() {
                self.state.selected_conversation = 0;
            }
        }
        Ok(())
    }

    pub(crate) async fn refresh_messages<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "message.read",
                serde_json::json!({
                    "conversation_id": conversation_id,
                    "read_model": true,
                    "limit": 100
                }),
            )
            .await?;
        let page = read_model::parse_page(&response)?;
        if page.conversation_id != conversation_id {
            return Err(TuiError::ReadModel("conversation_id mismatch".to_owned()));
        }
        read_model::reject_numeric_cursor_sequence(page.next_before.as_ref())?;
        self.state.messages = page.messages;
        self.stream_cursors.insert(self.state.account_id.clone(), page.snapshot_sequence);
        self.refresh_cards(bus).await?;
        Ok(())
    }

    /// Reloads the rendered message page. The volatile receive page is not a render source.
    ///
    /// # Errors
    /// Returns an error when the read-model request fails.
    pub async fn receive_messages_with_attachments<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        self.refresh_messages(bus).await
    }

    pub(crate) async fn refresh_contacts<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
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

    pub(crate) async fn refresh_groups<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
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

    pub(crate) async fn refresh_approvals<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
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
            self.state.status_message = Some("no approval selected".to_owned());
            return Ok(());
        };
        if method == "grant.approve" && approval.confirmation_mode == "remote_app" {
            self.state.status_message = Some("该审批需 App 端签名授权(remote_app)".to_owned());
            return Ok(());
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
            self.state.status_message = Some("nothing to send".to_owned());
            return Ok(());
        }
        let body = std::mem::take(&mut self.state.input);
        if let Some(_path) = body.strip_prefix("/attach ") {
            self.state.status_message = Some(
                "conversation.send does not accept attachment bytes; use the object panel"
                    .to_owned(),
            );
            return Ok(());
        }
        if !self.state.pending_attachments.is_empty() {
            self.state.status_message = Some(
                "queued attachment bytes were not sent; open the object panel to send the file"
                    .to_owned(),
            );
        }
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        let operation_id = format!("tui-{}-{}", self.state.account_id, self.state.next_message_id);
        self.state.next_message_id = self.state.next_message_id.saturating_add(1);
        if operation_id.len() > 256 {
            return Err(TuiError::ReadModel("operation_id is longer than 256 bytes".to_owned()));
        }
        let response = bus
            .request_operation(
                Some(self.state.account_id.clone()),
                "conversation",
                "conversation.send",
                serde_json::json!({
                    "conversation_id": conversation_id,
                    "text": body,
                    "media": []
                }),
                operation_id.clone(),
            )
            .await?;
        // `message_id` is the event id the read model uses. `client_message_id` is not.
        if let Some(event_id) = response
            .get("message_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.apply_read_model_row(MessageRow::local_send(event_id.to_owned(), body));
        }
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
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "device",
                "device.revoke",
                serde_json::json!({ "device_id": device_id }),
            )
            .await?;
        let epoch = response["device_set_epoch"].as_u64().filter(|e| *e > 0).ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                "signed device-set epoch missing from revoke acknowledgement".into(),
            ))
        })?;
        if response["revoked"] != true || response["device_id"] != device_id {
            return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                "device revoke acknowledgement mismatch".into(),
            )));
        }
        self.refresh_devices(bus).await?;
        self.state.status_message =
            Some(format!("Revocation submitted for {device_id} (signed device-set epoch {epoch})"));
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
            self.state.status_message = Some("no object transfer selected".to_owned());
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
            self.state.status_message = Some("no object transfer selected".to_owned());
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
            self.state.status_message = Some("no contact selected".to_owned());
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
            self.state.status_message = Some("no contact selected".to_owned());
            return Ok(());
        };
        if contact.safety_number.is_empty() {
            self.state.status_message = Some(
                "refusing blind verify: press S to load the safety number, then compare all 60 digits out-of-band before verifying"
                    .to_owned(),
            );
            return Ok(());
        }
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "contact",
                "contact.verify",
                serde_json::json!({"contact_identity_commitment": contact.target}),
            )
            .await?;
        self.apply_contact_safety_response(&response);
        self.state.status_message = Some("contact verified".to_owned());
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
                self.state.status_message = Some(format!("unknown command: {other}"));
            }
            None => {
                self.state.status_message = Some("no command entered".to_owned());
            }
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
    /// The request body is empty. Account switch does not ask for an account code.
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
        self.cards = crate::surface::Cards::default();
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
            self.state.status_message = Some("no message selected".to_owned());
            return Ok(());
        };
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        self.request_keeping_miss(
            bus,
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
            self.state.status_message = Some("no message selected".to_owned());
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
        self.request_keeping_miss(
            bus,
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
            self.state.status_message = Some("no message selected".to_owned());
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
        self.request_keeping_miss(
            bus,
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
            self.state.status_message = Some("no group selected".to_owned());
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
            self.state.status_message = Some("no group selected".to_owned());
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
            self.state.messages.first().map_or_else(String::new, |message| message.body.clone());
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

    pub(crate) fn apply_read_model_row(&mut self, message: MessageRow) {
        let tombstone = message.state == "tombstone";
        let previous_body =
            self.state.messages.iter().find(|row| row.id == message.id).map(|row| row.body.clone());
        if let Some(existing) = self.state.messages.iter_mut().find(|row| row.id == message.id) {
            *existing = message;
        } else {
            self.state.messages.insert(0, message);
            if self.state.messages.len() > MAX_CACHED_MESSAGES {
                self.state.messages.truncate(MAX_CACHED_MESSAGES);
            }
        }
        if tombstone && let Some(previous_body) = previous_body.filter(|body| !body.is_empty()) {
            for conversation in &mut self.state.conversations {
                if conversation.last_message == previous_body {
                    conversation.last_message.clear();
                }
            }
        }
        self.sync_conversation_preview();
    }

    fn sync_conversation_preview(&mut self) {
        let preview =
            self.state.messages.first().map(|message| message.body.clone()).unwrap_or_default();
        let conversation_id = self.selected_conversation().map(|row| row.id.clone());
        let Some(conversation_id) = conversation_id else {
            return;
        };
        if let Some(conversation) =
            self.state.conversations.iter_mut().find(|row| row.id == conversation_id)
        {
            conversation.last_message = preview;
        }
    }

    fn remove_requested_message(&mut self, body: &serde_json::Value) {
        let Some(message_id) = body.get("message_id").and_then(serde_json::Value::as_str) else {
            return;
        };
        if message_id.is_empty() {
            return;
        }
        self.state.messages.retain(|row| row.id != message_id);
        self.sync_conversation_preview();
    }

    /// Applies a typed generated refusal. `target_not_found` drops that row and nothing else.
    #[cfg(test)]
    pub(crate) fn apply_typed_refusal(&mut self, name: &str, message_id: Option<&str>) {
        if name == ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name()
            && let Some(id) = message_id
        {
            self.state.messages.retain(|row| row.id != id);
        }
        self.state.status_message = Some(name.to_owned());
    }

    /// Drops the message cache and loads a fresh rendered page.
    ///
    /// # Errors
    /// Returns an error when the page request fails.
    pub(crate) async fn resync_messages<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        self.state.messages.clear();
        self.stream_cursors.remove(&self.state.account_id);
        self.refresh_messages(bus).await
    }

    /// Reads durable changes after the stored cursor and replaces rows with the same id.
    ///
    /// # Errors
    /// Returns an error when a change read fails with a code this client does not handle.
    pub(crate) async fn follow_message_changes<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        message_hint: Option<&str>,
    ) -> Result<bool, TuiError> {
        if !self.stream_cursors.contains_key(&self.state.account_id) {
            self.resync_messages(bus).await?;
            return Ok(true);
        }
        let account = self.state.account_id.clone();
        let conversation_id = self
            .selected_conversation()
            .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone());
        for _ in 0..MAX_CHANGE_ROUNDS {
            let after = self.stream_cursors.get(&account).copied();
            let Some(after) = after else {
                self.resync_messages(bus).await?;
                return Ok(true);
            };
            let body = serde_json::json!({
                "conversation_id": conversation_id,
                "after_sequence": after,
                "limit": 100
            });
            let response = match bus
                .request(Some(account.clone()), "message", "message.read", body.clone())
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    return self.handle_change_error(bus, &body, message_hint, error).await;
                }
            };
            let batch = read_model::parse_changes(&response)?;
            if batch.conversation_id != conversation_id {
                return Err(TuiError::ReadModel("conversation_id mismatch".to_owned()));
            }
            for row in batch.changes {
                self.apply_read_model_row(row);
            }
            self.stream_cursors.insert(account.clone(), batch.next_after);
            if !batch.more {
                return Ok(false);
            }
        }
        self.state.status_message =
            Some("message change batch stopped at the round limit".to_owned());
        Ok(false)
    }

    async fn handle_change_error<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        body: &serde_json::Value,
        message_hint: Option<&str>,
        error: TuiError,
    ) -> Result<bool, TuiError> {
        let name = generated_code(&error).map(str::to_owned);
        let Some(name) = name else {
            return Err(error);
        };
        if name == ramflux_sdk::bindings::GeneratedErrorCode::ResyncRequired.name() {
            self.resync_messages(bus).await?;
            return Ok(true);
        }
        if name == ramflux_sdk::bindings::GeneratedErrorCode::PayloadTooLarge.name() {
            self.read_single_message(bus, body, message_hint).await?;
        } else if name == ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name() {
            self.remove_requested_message(body);
        }
        self.state.status_message = Some(name);
        Err(error)
    }

    async fn read_single_message<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        body: &serde_json::Value,
        message_hint: Option<&str>,
    ) -> Result<(), TuiError> {
        let message_id = body
            .get("message_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| message_hint.filter(|value| !value.is_empty()).map(str::to_owned));
        let Some(message_id) = message_id else {
            return Ok(());
        };
        let conversation_id = body
            .get("conversation_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map_or_else(
                || {
                    self.selected_conversation()
                        .map_or_else(|| DEFAULT_CONVERSATION_ID.to_owned(), |row| row.id.clone())
                },
                str::to_owned,
            );
        let response = bus
            .request(
                Some(self.state.account_id.clone()),
                "message",
                "message.read",
                serde_json::json!({
                    "conversation_id": conversation_id,
                    "message_id": message_id
                }),
            )
            .await?;
        self.apply_read_model_row(read_model::parse_detail(&response)?);
        Ok(())
    }

    async fn request_keeping_miss<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        match bus.request(Some(self.state.account_id.clone()), sdk_api, method, body.clone()).await
        {
            Ok(value) => Ok(value),
            Err(error)
                if generated_code(&error)
                    == Some(ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name()) =>
            {
                self.remove_requested_message(&body);
                self.state.status_message = Some(
                    ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name().to_owned(),
                );
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn move_conversation_selection<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        upward: bool,
    ) -> Result<(), TuiError> {
        let previous = self.selected_conversation().map(|row| row.id.clone());
        if upward {
            self.move_selection_up();
        } else {
            self.move_selection_down();
        }
        if self.state.selected_panel != Panel::Conversations {
            return Ok(());
        }
        let current = self.selected_conversation().map(|row| row.id.clone());
        if current == previous {
            return Ok(());
        }
        self.refresh_messages(bus).await
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
