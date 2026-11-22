// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use crate::{ApprovalRow, ContactRow, ConversationRow, DeviceRow, GroupRow, ObjectTransferRow};

pub(crate) fn parse_transfer(response: &serde_json::Value) -> Option<ObjectTransferRow> {
    let transfer = response.get("transfer").filter(|value| value.is_object()).unwrap_or(response);
    let object_id = transfer.get("object_id").or_else(|| response.get("id"))?.as_str()?;
    Some(ObjectTransferRow {
        object_id: object_id.to_owned(),
        direction: string_field(transfer, "direction", "unknown"),
        state: transfer
            .get("state")
            .or_else(|| response.get("transfer_state"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        done_bytes: transfer.get("done_bytes").and_then(serde_json::Value::as_u64).unwrap_or(0),
        total_bytes: transfer
            .get("total_bytes")
            .or_else(|| response.get("size"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        percent: transfer
            .get("percent")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        last_error: transfer
            .get("last_error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        relay_endpoint: None,
        relay_service_key_base64: None,
    })
}

/// Parses the real conversations projection returned by `conversation.list`.
///
/// The storage schema has no peer/display-name column, so the conversation id
/// doubles as the title; it also has no reader-free unread count, so `unread`
/// stays 0 here and is only advanced live as deliveries arrive.
pub(crate) fn parse_conversations(response: &serde_json::Value) -> Vec<ConversationRow> {
    response
        .get("conversations")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|conversation| {
            let id = string_field(conversation, "conversation_id", "conversation");
            let is_archived = conversation
                .get("is_archived")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            ConversationRow {
                title: id.clone(),
                id,
                last_message: String::new(),
                unread: 0,
                status: if is_archived { "archived".to_owned() } else { "synced".to_owned() },
                recipient_device_id: None,
                target_delivery_id: None,
            }
        })
        .collect()
}

pub(crate) fn parse_contacts(response: &serde_json::Value) -> Vec<ContactRow> {
    response
        .get("contacts")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|contact| ContactRow {
            link_id: string_field(contact, "link_id", "contact"),
            requester: string_field(contact, "requester_id", "requester"),
            target: string_field(contact, "target_id", "target"),
            state: string_field(contact, "state", "accepted"),
            safety_number: contact
                .get("safety_number")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            fingerprint_hex: contact
                .get("fingerprint_hex")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            verification_state: string_field(contact, "verification_state", "unverified"),
        })
        .collect()
}

pub(crate) fn parse_groups(response: &serde_json::Value) -> Vec<GroupRow> {
    response
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|group| GroupRow {
            id: string_field(group, "group_id", "group"),
            members: group
                .get("members")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            disappearing_ttl_secs: group
                .get("disappearing_ttl_secs")
                .or_else(|| group.get("ttl_secs"))
                .and_then(serde_json::Value::as_i64),
            mute_until: group.get("mute_until").and_then(serde_json::Value::as_i64),
        })
        .collect()
}

pub(crate) fn parse_devices(response: &serde_json::Value) -> Vec<DeviceRow> {
    response
        .get("devices")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|device| DeviceRow {
            device_id: string_field(device, "device_id", "device"),
            device_epoch: device
                .get("device_epoch")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1),
            target_delivery_id: string_field(device, "target_delivery_id", "target"),
            is_local: device.get("is_local").and_then(serde_json::Value::as_bool).unwrap_or(false),
        })
        .collect()
}

pub(crate) fn parse_approvals(response: &serde_json::Value) -> Vec<ApprovalRow> {
    response
        .get("approvals")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(parse_approval_row)
        .collect()
}

pub(crate) fn parse_approval_row(approval: &serde_json::Value) -> ApprovalRow {
    let confirmation_mode = string_field(approval, "confirmation_mode", "remote_app");
    ApprovalRow {
        id: string_field(approval, "approval_id", "approval"),
        tool: format!(
            "{}/{}",
            string_field(approval, "server_id", "server"),
            string_field(approval, "tool_name", "tool")
        ),
        risk: string_field(approval, "risk_level", "unknown"),
        mode: confirmation_mode.clone(),
        confirmation_mode,
        status: string_field(approval, "status", "pending"),
    }
}

pub(crate) fn string_field(value: &serde_json::Value, field: &str, default: &str) -> String {
    value.get(field).and_then(serde_json::Value::as_str).unwrap_or(default).to_owned()
}
