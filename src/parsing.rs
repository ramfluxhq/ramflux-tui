// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use crate::a2ui_render::render_a2ui_for_approval;
use crate::{
    ApprovalRow, AttachmentRow, ContactRow, DeviceRow, GroupRow, MessageReceiptRow, MessageRow,
    ObjectTransferRow,
};

pub(crate) fn parse_messages(response: &serde_json::Value) -> Vec<MessageRow> {
    let mut rows = response
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|message| MessageRow {
            id: string_field(message, "message_id", "message"),
            sender: string_field(message, "sender_id", "peer"),
            body: string_field(message, "body_utf8", "[encrypted message]"),
            status: "sent".to_owned(),
            attachments: parse_attachment_rows(
                &string_field(message, "message_id", "message"),
                message.get("attachments"),
            ),
            receipts: parse_receipt_rows(message.get("receipts")),
        })
        .collect::<Vec<_>>();
    rows.extend(parse_decrypted_messages(response));
    rows
}

pub(crate) fn parse_decrypted_messages(response: &serde_json::Value) -> Vec<MessageRow> {
    response
        .get("decrypted_messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|message| {
            let id = string_field(message, "message_id", "message");
            let body = message
                .get("plaintext_body_base64")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| ramflux_protocol::decode_base64url(value).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_else(|| "[encrypted message]".to_owned());
            MessageRow {
                id: id.clone(),
                sender: string_field(message, "sender_id", "peer"),
                body,
                status: "received".to_owned(),
                attachments: parse_attachment_rows(&id, message.get("attachments")),
                receipts: parse_receipt_rows(message.get("receipts")),
            }
        })
        .collect()
}

fn parse_receipt_rows(receipts: Option<&serde_json::Value>) -> Vec<MessageReceiptRow> {
    receipts
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|receipt| MessageReceiptRow {
            device_id: string_field(receipt, "device_id", "device"),
            state: string_field(receipt, "state", "delivered"),
        })
        .collect()
}

pub(crate) fn parse_transfer(response: &serde_json::Value) -> Option<ObjectTransferRow> {
    let transfer = response.get("transfer")?;
    Some(ObjectTransferRow {
        object_id: string_field(transfer, "object_id", "object"),
        direction: string_field(transfer, "direction", "unknown"),
        state: string_field(transfer, "state", "unknown"),
        done_bytes: transfer.get("done_bytes").and_then(serde_json::Value::as_u64).unwrap_or(0),
        total_bytes: transfer.get("total_bytes").and_then(serde_json::Value::as_u64).unwrap_or(0),
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

fn parse_attachment_rows(
    message_id: &str,
    attachments: Option<&serde_json::Value>,
) -> Vec<AttachmentRow> {
    attachments
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|attachment| AttachmentRow {
            message_id: message_id.to_owned(),
            object_id: string_field(attachment, "object_id", "object"),
            status: if attachment.get("plaintext_base64").is_some() {
                "decrypted".to_owned()
            } else {
                string_field(attachment, "status", "referenced")
            },
            plaintext_base64: attachment
                .get("plaintext_base64")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
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
    let surface = approval
        .pointer("/details/surface")
        .or_else(|| approval.get("surface"))
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    let rendered_surface = surface.as_ref().and_then(render_a2ui_for_approval);
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
        surface,
        rendered_surface,
    }
}

pub(crate) fn string_field(value: &serde_json::Value, field: &str, default: &str) -> String {
    value.get(field).and_then(serde_json::Value::as_str).unwrap_or(default).to_owned()
}
