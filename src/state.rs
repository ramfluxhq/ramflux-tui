// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use crate::DEFAULT_ACCOUNT_ID;

/// Top-level TUI panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Panel {
    Conversations,
    Messages,
    Objects,
    Contacts,
    Groups,
    Approvals,
}

impl Panel {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Conversations => "Conversations",
            Self::Messages => "Messages",
            Self::Objects => "Objects",
            Self::Contacts => "Contacts",
            Self::Groups => "Groups",
            Self::Approvals => "Approvals",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Conversations => Self::Messages,
            Self::Messages => Self::Objects,
            Self::Objects => Self::Contacts,
            Self::Contacts => Self::Groups,
            Self::Groups => Self::Approvals,
            Self::Approvals => Self::Conversations,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Conversations => 0,
            Self::Messages => 1,
            Self::Objects => 2,
            Self::Contacts => 3,
            Self::Groups => 4,
            Self::Approvals => 5,
        }
    }
}

/// Input events handled by the TUI state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiInput {
    Tab,
    Up,
    Down,
    Enter,
    EnterCompose,
    ExitCompose,
    Char(char),
    Backspace,
    Quit,
}

/// Input handling mode for the message composer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Compose,
}

/// Conversation list row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationRow {
    pub id: String,
    pub title: String,
    pub last_message: String,
    pub unread: usize,
    pub status: String,
    pub recipient_device_id: Option<String>,
    pub target_delivery_id: Option<String>,
}

/// Message row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageRow {
    pub id: String,
    pub sender: String,
    pub body: String,
    pub status: String,
    pub attachments: Vec<AttachmentRow>,
    pub receipts: Vec<MessageReceiptRow>,
}

/// Attachment row shown inside the message/object surfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentRow {
    pub message_id: String,
    pub object_id: String,
    pub status: String,
    pub plaintext_base64: Option<String>,
}

/// Per-peer message receipt shown in the message surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageReceiptRow {
    pub device_id: String,
    pub state: String,
}

/// Transfer row shown in the object panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectTransferRow {
    pub object_id: String,
    pub direction: String,
    pub state: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub percent: u32,
    pub last_error: Option<String>,
    pub relay_endpoint: Option<String>,
    pub relay_service_key_base64: Option<String>,
}

/// Attachment queued for the next direct message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAttachment {
    pub object_id: String,
    pub plaintext_base64: String,
    pub chunk_size: usize,
    pub relay_endpoint: String,
    pub relay_service_key_base64: Option<String>,
}

/// Contact row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactRow {
    pub link_id: String,
    pub requester: String,
    pub target: String,
    pub state: String,
    pub safety_number: Vec<String>,
    pub fingerprint_hex: Option<String>,
    pub verification_state: String,
}

/// Local account device row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceRow {
    pub device_id: String,
    pub device_epoch: u64,
    pub target_delivery_id: String,
    pub is_local: bool,
}

/// Group row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupRow {
    pub id: String,
    pub members: Vec<String>,
    pub disappearing_ttl_secs: Option<i64>,
    pub mute_until: Option<i64>,
}

/// Pending approval row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRow {
    pub id: String,
    pub tool: String,
    pub risk: String,
    pub mode: String,
    pub confirmation_mode: String,
    pub status: String,
    pub surface: Option<ramflux_sync::A2uiSurface>,
    pub rendered_surface: Option<ramflux_sync::RenderedSurface>,
}

/// Immutable-ish view model rendered by ratatui.
#[derive(Clone, Debug)]
pub struct TuiState {
    pub account_id: String,
    pub local_device_id: Option<String>,
    pub selected_panel: Panel,
    pub selected_conversation: usize,
    pub selected_message: usize,
    pub selected_object_transfer: usize,
    pub selected_contact: usize,
    pub selected_group: usize,
    pub input: String,
    pub input_mode: InputMode,
    pub conversations: Vec<ConversationRow>,
    pub messages: Vec<MessageRow>,
    pub object_transfers: Vec<ObjectTransferRow>,
    pub pending_attachments: Vec<PendingAttachment>,
    pub contacts: Vec<ContactRow>,
    pub devices: Vec<DeviceRow>,
    pub groups: Vec<GroupRow>,
    pub approvals: Vec<ApprovalRow>,
    pub selected_approval: usize,
    pub status_message: Option<String>,
    pub(crate) next_message_id: u64,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            account_id: DEFAULT_ACCOUNT_ID.to_owned(),
            local_device_id: None,
            selected_panel: Panel::Conversations,
            selected_conversation: 0,
            selected_message: 0,
            selected_object_transfer: 0,
            selected_contact: 0,
            selected_group: 0,
            input: String::new(),
            input_mode: InputMode::Normal,
            conversations: Vec::new(),
            messages: Vec::new(),
            object_transfers: Vec::new(),
            pending_attachments: Vec::new(),
            contacts: Vec::new(),
            devices: Vec::new(),
            groups: Vec::new(),
            approvals: Vec::new(),
            selected_approval: 0,
            status_message: None,
            next_message_id: 1,
        }
    }
}
