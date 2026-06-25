// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain
use crate::a2ui_render::render_a2ui_for_approval;
use crate::{ApprovalRow, ContactRow, GroupRow, MessageRow};

pub(crate) fn parse_messages(response: &serde_json::Value) -> Vec<MessageRow> {
    response
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|message| MessageRow {
            id: string_field(message, "message_id", "message"),
            sender: string_field(message, "sender_id", "peer"),
            body: string_field(message, "body_utf8", "[encrypted message]"),
            status: "sent".to_owned(),
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
