// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

#![cfg(test)]

use async_trait::async_trait;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::VecDeque;

use crate::parsing::parse_approvals;
use crate::*;

#[derive(Default)]
struct MockBus {
    requests: Vec<MockRequest>,
    events: VecDeque<ramflux_sdk::LocalBusFrame>,
    /// Response returned for `conversation.list`. Defaults to JSON `null`,
    /// which `parse_conversations` reads as "no real conversations" so the
    /// synthetic Default DM fallback still applies.
    conversation_list: serde_json::Value,
    revoke_response: Option<serde_json::Value>,
    /// `(method, generated code)` refused once the request has been recorded.
    typed_refusal: Option<(String, String)>,
    /// `conversation.send` omits `message_id` and keeps `client_message_id`.
    omit_send_event_id: bool,
}

#[derive(Clone, Debug)]
struct MockRequest {
    account_id: Option<String>,
    sdk_api: String,
    method: String,
    body: serde_json::Value,
    operation_id: Option<String>,
}

#[async_trait]
impl TuiBus for MockBus {
    async fn request(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        self.request_operation(account_id, sdk_api, method, body, String::new()).await
    }

    #[allow(clippy::too_many_lines)]
    async fn request_operation(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
        operation_id: String,
    ) -> Result<serde_json::Value, TuiError> {
        self.requests.push(MockRequest {
            account_id,
            sdk_api: sdk_api.to_owned(),
            method: method.to_owned(),
            body: body.clone(),
            operation_id: if operation_id.is_empty() { None } else { Some(operation_id) },
        });
        if let Some((refused_method, code)) = self.typed_refusal.clone()
            && refused_method == method
        {
            return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBusRefused {
                code,
                message: "refused".to_owned(),
                body: Box::new(serde_json::json!({})),
            }));
        }
        let conversation = body
            .get("conversation_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(crate::DEFAULT_CONVERSATION_ID);
        Ok(match method {
            "message.read" if body.get("after_sequence").is_some() => {
                let after =
                    body.get("after_sequence").and_then(serde_json::Value::as_u64).unwrap_or(0);
                crate::read_model::sample_changes(conversation, after)
            }
            "message.read" => {
                crate::read_model::sample_page(conversation, "msg_tui_1", "hello from bus")
            }
            "bus.subscribe" => {
                serde_json::json!({"subscribed": true, "head_seq": 10, "floor_seq": 1})
            }
            "contact.list" => serde_json::json!({
                "contacts": [{
                    "link_id": "friend_tui_1",
                    "requester_id": "alice",
                    "target_id": "bob",
                    "state": "accepted",
                    "verification_state": "unverified"
                }]
            }),
            "contact.safety_number" => serde_json::json!({
                "contact_identity_commitment": "bob",
                "safety_number": ["12345", "67890"],
                "fingerprint_hex": "f00d",
                "verification_state": "unverified"
            }),
            "contact.verify" => serde_json::json!({
                "contact_identity_commitment": "bob",
                "safety_number": ["12345", "67890"],
                "fingerprint_hex": "f00d",
                "verification_state": "verified"
            }),
            "contact.add" | "contact.accept" => serde_json::json!({
                "link_id": body.get("link_id").cloned().unwrap_or(serde_json::Value::Null),
                "requester_id": body.get("requester_id").cloned().unwrap_or(serde_json::Value::Null),
                "target_id": body.get("target_id").cloned().unwrap_or(serde_json::Value::Null),
                "state": "accepted",
            }),
            "device.list" => serde_json::json!({
                "principal_id": "principal_tui_alice",
                "local_device_id": "device_tui_alice_b",
                "devices": [{
                    "device_id": "device_tui_alice_a",
                    "device_epoch": 1,
                    "target_delivery_id": "target_tui_alice_a",
                    "is_local": false
                }, {
                    "device_id": "device_tui_alice_b",
                    "device_epoch": 2,
                    "target_delivery_id": "target_tui_alice_b",
                    "is_local": true
                }]
            }),
            "device.revoke" => self.revoke_response.clone().unwrap_or_else(|| {
                serde_json::json!({
                    "device_id": body.get("device_id").cloned().unwrap_or(serde_json::Value::Null),
                    "revoked": true,
                    "device_set_epoch":3
                })
            }),
            "group.list" => serde_json::json!({
                "groups": [{
                    "group_id": "group_tui_1",
                    "members": ["alice", "bob"],
                    "ttl_secs": 60,
                    "mute_until": null
                }]
            }),
            "mcp.approval.list" => serde_json::json!({
                "approvals": mock_approvals()
            }),
            "grant.approve" | "grant.deny" => serde_json::json!({"ok": true}),
            "conversation.send" => {
                let mut response = serde_json::json!({
                    "client_message_id": "client_sent",
                    "conversation_id": conversation,
                    "delivery_state": "submitted",
                    "deliveries": []
                });
                if !self.omit_send_event_id {
                    response["message_id"] = serde_json::json!("msg_sent");
                }
                response
            }
            "message.receive" => serde_json::json!({
                "entries": [],
                "decrypted_messages": [{
                    "message_id": "env_tui_attach_1",
                    "sender_id": "bob",
                    "plaintext_body_base64": ramflux_protocol::encode_base64url(b"attachment received"),
                    "attachments": [{
                        "object_id": "object_tui_attach_1",
                        "plaintext_base64": ramflux_protocol::encode_base64url(b"attachment bytes")
                    }]
                }]
            }),
            "object.put" => mock_transfer("object_tui_put", "upload", "in_progress", 512, 1024, 50),
            "object.get" => serde_json::json!({
                "object_id": "object_tui_get",
                "plaintext_base64": ramflux_protocol::encode_base64url(b"object bytes")
            }),
            "object.transfer.status" => {
                let object_id = body
                    .get("object_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("object_tui_status");
                let direction =
                    body.get("direction").and_then(serde_json::Value::as_str).unwrap_or("upload");
                mock_transfer(object_id, direction, "in_progress", 512, 1024, 50)
            }
            "object.transfer.resume" => {
                let object_id = body
                    .get("object_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("object_tui_resume");
                let direction =
                    body.get("direction").and_then(serde_json::Value::as_str).unwrap_or("upload");
                mock_transfer(object_id, direction, "complete", 1024, 1024, 100)
            }
            "message.delete" => serde_json::json!({"deleted": true}),
            "message.receipt.delivered" | "message.receipt.read" => {
                serde_json::json!({"scope": "local_projection"})
            }
            "group.member.remove" => serde_json::json!({
                "group_id": "group_tui_1",
                "members": ["alice"]
            }),
            "conversation.disappearing.set" => serde_json::json!({
                "conversation_id": "group_tui_1",
                "ttl_secs": 30
            }),
            "conversation.mute" => serde_json::json!({
                "conversation_id": "group_tui_1",
                "mute_until": body.get("mute_until").cloned().unwrap_or(serde_json::Value::Null)
            }),
            "conversation.list" => self.conversation_list.clone(),
            "subscription.open" => serde_json::json!({"subscribed": true}),
            "account.status" => serde_json::json!({
                "local_account_id": "alice_account",
                "principal_id": "principal_tui_alice",
                "device_id": "device_tui_alice",
                "target_delivery_id": "target_tui_alice",
            }),
            "account.switch" => serde_json::json!({
                "active_account_id": self
                    .requests
                    .last()
                    .and_then(|request| request.account_id.clone())
                    .unwrap_or_else(|| "unknown".to_owned())
            }),
            _ => serde_json::json!({}),
        })
    }

    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
        self.events
            .pop_front()
            .ok_or_else(|| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("no event".to_owned())))
    }
}

fn mock_transfer(
    object_id: &str,
    direction: &str,
    state: &str,
    done_bytes: u64,
    total_bytes: u64,
    percent: u32,
) -> serde_json::Value {
    serde_json::json!({
        "transfer": {
            "transfer_id": format!("{direction}:{object_id}:mock"),
            "object_id": object_id,
            "manifest_hash": "mock_manifest",
            "direction": direction,
            "state": state,
            "total_bytes": total_bytes,
            "done_bytes": done_bytes,
            "total_chunks": 2,
            "completed_chunks": if percent == 100 { 2 } else { 1 },
            "next_chunk_index": if percent == 100 { serde_json::Value::Null } else { serde_json::json!(1) },
            "percent": percent,
            "last_error": serde_json::Value::Null,
            "updated_at": 1
        }
    })
}

fn mock_approvals() -> serde_json::Value {
    serde_json::json!([{
        "approval_id": "approval_tui_1",
        "server_id": "srv",
        "tool_name": "echo",
        "risk_level": "low",
        "confirmation_mode": "attended_local",
        "status": "pending"
    }])
}

#[tokio::test]
async fn renders_conversations_messages_contacts_and_groups() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;

    let mut terminal = Terminal::new(TestBackend::new(100, 28))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);

    assert!(buffer.contains("Default DM"));
    app.state.selected_panel = Panel::Messages;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("hello from bus"));
    app.state.selected_panel = Panel::Contacts;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("friend_tui_1"));
    assert!(buffer.contains("unverified"));

    app.state.selected_panel = Panel::Groups;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("group_tui_1"));
    assert!(buffer.contains("alice,bob"));
    app.state.selected_panel = Panel::Approvals;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("approval_tui_1"));
    assert!(buffer.contains("attended_local"));
    assert!(buffer.contains("a=approve"), "attended approval must retain its keyboard action");
    Ok(())
}

#[tokio::test]
async fn conversations_panel_uses_real_projection_over_synthetic_default() -> Result<(), TuiError> {
    let mut bus = MockBus {
        conversation_list: serde_json::json!({
            "conversations": [
                {
                    "conversation_id": "conv_real_1",
                    "message_count": 3,
                    "last_message_id": "msg_real_1c",
                    "last_activity_at": 1_900_000_100,
                    "is_archived": false,
                    "pin_order": null
                },
                {
                    "conversation_id": "conv_real_2",
                    "message_count": 0,
                    "last_message_id": null,
                    "last_activity_at": null,
                    "is_archived": true,
                    "pin_order": null
                }
            ]
        }),
        ..MockBus::default()
    };
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;

    assert_eq!(app.state.conversations.len(), 2);
    assert_eq!(app.state.conversations[0].id, "conv_real_1");
    assert_eq!(app.state.conversations[1].id, "conv_real_2");
    // The archived flag from storage flows through to the row status.
    assert_eq!(app.state.conversations[1].status, "archived");
    assert!(app.state.conversations.iter().all(|row| row.title != "Default DM"));

    // refresh_messages must follow the real selected conversation.
    let read =
        bus.requests.iter().find(|request| request.method == "message.read").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing read".to_owned()))
        })?;
    assert_eq!(read.body["conversation_id"], "conv_real_1");
    assert_eq!(read.body["read_model"], true);

    let mut terminal = Terminal::new(TestBackend::new(100, 28))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("conv_real_1"));
    assert!(!buffer.contains("Default DM"));
    Ok(())
}

#[tokio::test]
async fn contact_panel_loads_safety_number_and_marks_verified() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Contacts;

    app.handle_input(&mut bus, TuiInput::Char('S')).await?;
    assert_eq!(app.state.contacts[0].fingerprint_hex.as_deref(), Some("f00d"));
    assert_eq!(app.state.contacts[0].verification_state, "unverified");

    app.handle_input(&mut bus, TuiInput::Char('V')).await?;
    assert_eq!(app.state.contacts[0].verification_state, "verified");
    assert!(bus.requests.iter().any(|request| request.method == "contact.verify"));
    Ok(())
}

#[tokio::test]
async fn contacts_panel_adds_contact_with_commitments_and_switches_account() -> Result<(), TuiError>
{
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Contacts;

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    for value in "add friend_tui_2 alice_commitment bob_commitment".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;
    assert_eq!(app.state.input_mode, InputMode::Normal);
    let add =
        bus.requests.iter().find(|request| request.method == "contact.add").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing add".to_owned()))
        })?;
    assert_eq!(add.account_id.as_deref(), Some("alice_account"));
    assert_eq!(add.body["requester_id"], "alice_commitment");
    assert_eq!(add.body["target_id"], "bob_commitment");
    assert_eq!(app.state.status_message.as_deref(), Some("contact added: friend_tui_2"));

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    for value in "switch bob_account".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;
    let switch =
        bus.requests.iter().find(|request| request.method == "account.switch").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing switch".to_owned())),
        )?;
    assert_eq!(switch.account_id.as_deref(), Some("bob_account"));
    assert_eq!(app.state.account_id, "bob_account");
    Ok(())
}

#[tokio::test]
async fn message_panel_surfaces_receipts_and_delete_actions() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.set_local_device_id("alice_device_tui");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;
    app.state.conversations[0].recipient_device_id = Some("bob_device_tui".to_owned());
    app.state.conversations[0].target_delivery_id = Some("target_bob_tui".to_owned());

    app.handle_input(&mut bus, TuiInput::Char('l')).await?;
    app.handle_input(&mut bus, TuiInput::Char('r')).await?;
    app.handle_input(&mut bus, TuiInput::Char('x')).await?;

    assert!(bus.requests.iter().any(|request| request.method == "message.receipt.delivered"));
    assert!(bus.requests.iter().any(|request| request.method == "message.receipt.read"));
    let delivered = bus
        .requests
        .iter()
        .find(|request| request.method == "message.receipt.delivered")
        .ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing delivered".to_owned()))
        })?;
    assert_eq!(delivered.body["receiver_device_id"], "alice_device_tui");
    assert_eq!(delivered.body["recipient_device_id"], "bob_device_tui");
    assert_eq!(delivered.body["target_delivery_id"], "target_bob_tui");
    let read =
        bus.requests.iter().find(|request| request.method == "message.receipt.read").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing read".to_owned())),
        )?;
    assert_eq!(read.body["reader_id"], "alice_device_tui");
    assert_eq!(read.body["recipient_device_id"], "bob_device_tui");
    assert_eq!(read.body["target_delivery_id"], "target_bob_tui");
    assert!(bus.requests.iter().any(|request| request.method == "message.delete"));
    assert_eq!(app.state.messages[0].status, "deleted");
    Ok(())
}

#[tokio::test]
async fn group_panel_surfaces_governance_actions() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;

    app.remove_group_member(&mut bus, "bob", "alice").await?;
    app.set_selected_group_disappearing(&mut bus, 30).await?;
    app.mute_selected_group(&mut bus, 1_760_000_600).await?;
    app.unmute_selected_group(&mut bus).await?;

    assert!(bus.requests.iter().any(|request| request.method == "group.member.remove"));
    assert!(bus.requests.iter().any(|request| request.method == "conversation.disappearing.set"));
    assert!(bus.requests.iter().any(|request| request.method == "conversation.mute"));
    assert_eq!(app.state.groups[0].members, vec!["alice"]);
    assert_eq!(app.state.groups[0].disappearing_ttl_secs, Some(30));
    Ok(())
}

#[tokio::test]
async fn contacts_panel_surfaces_device_list_and_revoke() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");

    app.refresh_devices(&mut bus).await?;
    app.revoke_device(&mut bus, "device_tui_alice_b").await?;

    assert!(app.state.devices.iter().any(|device| {
        device.device_id == "device_tui_alice_b" && device.is_local && device.device_epoch == 2
    }));
    let revoke =
        bus.requests.iter().find(|request| request.method == "device.revoke").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing revoke".to_owned()))
        })?;
    assert_eq!(revoke.body["device_id"], "device_tui_alice_b");
    assert_eq!(
        app.state.status_message.as_deref(),
        Some("Revocation submitted for device_tui_alice_b (signed device-set epoch 3)")
    );
    assert!(bus.requests.iter().any(|request| request.method == "device.list"));
    Ok(())
}

#[tokio::test]
async fn contacts_panel_surfaces_own_device_sync_export() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");

    app.export_device_sync(
        &mut bus,
        "device_tui_alice_b",
        "http://127.0.0.1:18084",
        Some("relay-key".to_owned()),
    )
    .await?;

    let export =
        bus.requests.iter().find(|request| request.method == "device.sync.export").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing sync export".to_owned())),
        )?;
    assert_eq!(export.body["target_device_id"], "device_tui_alice_b");
    assert_eq!(export.body["relay_endpoint"], "http://127.0.0.1:18084");
    Ok(())
}

#[tokio::test]
async fn enter_in_message_panel_submits_plaintext_over_bus() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;
    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    for value in "typed via tui".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    let submit =
        bus.requests.iter().find(|request| request.method == "conversation.send").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing send".to_owned())),
        )?;
    assert_eq!(submit.sdk_api, "conversation");
    assert_eq!(submit.body["text"], "typed via tui");
    assert_eq!(submit.body["media"], serde_json::json!([]));
    assert!(submit.body.get("plaintext_body_base64").is_none());
    assert!(submit.body.get("attachments").is_none());
    assert!(submit.operation_id.as_ref().is_some_and(|id| !id.is_empty() && id.len() <= 256));
    assert!(app.state.messages.iter().any(|message| message.body == "typed via tui"));
    assert_eq!(app.state.messages.first().map(|message| message.id.as_str()), Some("msg_sent"));
    assert!(app.state.messages.iter().all(|message| message.id != "client_sent"));
    let stored = crate::read_model::parse_detail(&crate::read_model::sample_detail(
        crate::DEFAULT_CONVERSATION_ID,
        "msg_sent",
        "from the store",
    ))?;
    app.apply_read_model_row(stored);
    let reconciled = app.state.messages.iter().filter(|message| message.id == "msg_sent").count();
    assert_eq!(reconciled, 1);
    assert_eq!(
        app.state.messages.first().map(|message| message.body.as_str()),
        Some("from the store")
    );
    assert_eq!(app.state.messages.first().map(|message| message.status.as_str()), Some("content"));
    Ok(())
}

#[tokio::test]
async fn compose_mode_submits_shortcut_characters_as_plaintext() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    assert_eq!(app.state.input_mode, InputMode::Compose);
    for value in "relax xray lol".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    assert_eq!(app.state.input_mode, InputMode::Normal);
    assert!(!bus.requests.iter().any(|request| request.method == "message.delete"));
    assert!(!bus.requests.iter().any(|request| request.method == "message.receipt.delivered"));
    assert!(!bus.requests.iter().any(|request| request.method == "message.receipt.read"));
    let submit =
        bus.requests.iter().find(|request| request.method == "conversation.send").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing send".to_owned())),
        )?;
    assert_eq!(submit.body["text"], "relax xray lol");
    assert!(submit.body.get("plaintext_body_base64").is_none());
    Ok(())
}

#[tokio::test]
async fn compose_mode_keeps_keyboard_mapped_q_and_i_as_plaintext() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    let q_input = key_to_input(crossterm::event::KeyCode::Char('q')).ok_or_else(|| {
        TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing q input mapping".to_owned()))
    })?;
    let i_input = key_to_input(crossterm::event::KeyCode::Char('i')).ok_or_else(|| {
        TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing i input mapping".to_owned()))
    })?;
    app.handle_input(&mut bus, q_input).await?;
    app.handle_input(&mut bus, i_input).await?;

    assert_eq!(app.state.input, "qi");
    assert_eq!(app.state.input_mode, InputMode::Compose);
    assert!(!app.should_quit());
    Ok(())
}

#[tokio::test]
async fn compose_mode_on_objects_preserves_command_letters() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Objects;

    // Enter Compose so command letters become literal text. In Normal mode 'r'
    // and 's' are the resume/status shortcuts and would otherwise be eaten.
    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    assert_eq!(app.state.input_mode, InputMode::Compose);
    for value in "put ./foo.rs id".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    assert_eq!(app.state.input, "put ./foo.rs id");
    Ok(())
}

#[tokio::test]
async fn compose_enter_dispatches_object_command_and_resets_mode() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Objects;

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    // "status" + an object id carrying 'r'/'s' proves the letters survive to dispatch.
    for value in "status object_resource".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    assert_eq!(app.state.input_mode, InputMode::Normal);
    assert_eq!(app.state.input, "");
    assert!(bus.requests.iter().any(|request| {
        request.method == "object.transfer.status" && request.body["object_id"] == "object_resource"
    }));
    // The message submit path must not fire for the Objects panel.
    assert!(!bus.requests.iter().any(|request| {
        request.method == "message.submit" || request.method == "conversation.send"
    }));
    Ok(())
}

#[tokio::test]
async fn input_title_reflects_compose_mode() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;

    let mut terminal = Terminal::new(TestBackend::new(100, 28))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    assert!(buffer_text(&terminal).contains("NORMAL i=compose"));

    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    assert!(buffer_text(&terminal).contains("COMPOSE"));
    Ok(())
}

#[test]
fn gateway_deliver_event_refreshes_message_view() -> Result<(), TuiError> {
    let mut app = TuiApp::new("alice_account");
    app.state.conversations.push(ConversationRow {
        id: DEFAULT_CONVERSATION_ID.to_owned(),
        title: "Default DM".to_owned(),
        last_message: String::new(),
        unread: 0,
        status: "synced".to_owned(),
        recipient_device_id: None,
        target_delivery_id: None,
    });
    let event = ramflux_sdk::LocalBusFrame {
        bus_protocol: "ramflux.local_bus.v1".to_owned(),
        frame_id: "frame_evt_test".to_owned(),
        kind: ramflux_sdk::LocalBusFrameKind::Event,
        request_id: "req_test".to_owned(),
        account_id: Some("alice_account".to_owned()),
        sdk_api: "gateway".to_owned(),
        method: "gateway.deliver".to_owned(),
        body: serde_json::json!({
            "entries": [{
                "inbox_seq": 1,
                "envelope": {
                    "envelope_id": "env_tui_event_1",
                    "source_principal_id": "bob"
                }
            }]
        }),
        trace_id: None,
        seq: None,
        at: None,
        deadline_ms: None,
        operation_id: None,
        ok: None,
        error: None,
    };

    app.handle_bus_event(&event)?;
    assert_eq!(app.state.messages, Vec::new());
    assert_eq!(app.state.conversations[0].unread, 0);
    Ok(())
}

#[tokio::test]
async fn message_submit_uses_selected_conversation_recipient_for_bootstrap() -> Result<(), TuiError>
{
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.state.conversations.push(ConversationRow {
        id: "conv_bootstrap".to_owned(),
        title: "Bob".to_owned(),
        last_message: String::new(),
        unread: 0,
        status: "synced".to_owned(),
        recipient_device_id: Some("bob_device_tui".to_owned()),
        target_delivery_id: Some("target_tui_bob".to_owned()),
    });
    app.state.selected_panel = Panel::Messages;
    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    for value in "hello bob".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    let submit =
        bus.requests.iter().find(|request| request.method == "conversation.send").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing send".to_owned())),
        )?;
    assert_eq!(submit.body["conversation_id"], "conv_bootstrap");
    assert_eq!(submit.body["text"], "hello bob");
    assert_eq!(submit.body["media"], serde_json::json!([]));
    assert!(submit.operation_id.as_ref().is_some_and(|id| !id.is_empty()));
    assert!(submit.body.get("source_principal_id").is_none());
    assert!(submit.body.get("plaintext_body_base64").is_none());
    Ok(())
}

#[tokio::test]
async fn message_panel_submits_queued_attachment_over_bus() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;
    app.queue_attachment_bytes_for_relay(
        "object_tui_attach_send",
        b"secret attachment bytes",
        "http://relay.test",
        Some("relay_key_tui".to_owned()),
    );
    app.handle_input(&mut bus, TuiInput::EnterCompose).await?;
    for value in "body with attachment".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    let submit =
        bus.requests.iter().rfind(|request| request.method == "conversation.send").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing send".to_owned())),
        )?;
    assert_eq!(submit.body["text"], "body with attachment");
    assert_eq!(submit.body["media"], serde_json::json!([]));
    assert!(submit.body.get("attachments").is_none());
    assert!(submit.body.get("plaintext_body_base64").is_none());
    let encoded = serde_json::to_string(&submit.body)
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    assert!(!encoded.contains("secret attachment bytes"));
    assert!(!encoded.contains(&ramflux_protocol::encode_base64url(b"secret attachment bytes")));
    assert_eq!(app.state.pending_attachments.len(), 1);
    assert!(app.state.messages.iter().all(|message| message.attachments.is_empty()));
    assert_eq!(
        app.state.status_message.as_deref(),
        Some("queued attachment bytes were not sent; open the object panel to send the file")
    );
    Ok(())
}

#[tokio::test]
async fn message_receive_auto_fetches_and_renders_attachment() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("bob_account");
    app.receive_messages_with_attachments(&mut bus).await?;

    assert!(!bus.requests.iter().any(|request| request.method == "message.receive"));
    let read =
        bus.requests.iter().find(|request| request.method == "message.read").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing read".to_owned()))
        })?;
    assert_eq!(read.body["read_model"], true);
    let message =
        app.state.messages.iter().find(|message| message.id == "msg_tui_1").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing message".to_owned()))
        })?;
    assert_eq!(message.body, "hello from bus");
    assert_eq!(message.attachments, Vec::new());
    assert!(message.device_sequence.chars().all(|ch| ch.is_ascii_digit()));
    assert_eq!(message.device_sequence.len(), 20);
    Ok(())
}

#[tokio::test]
async fn object_panel_renders_transfer_progress_and_resumes() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_object_status(&mut bus, "object_tui_status", Some("upload")).await?;
    if let Some(row) = app.state.object_transfers.first_mut() {
        row.relay_endpoint = Some("http://relay.test".to_owned());
        row.relay_service_key_base64 = Some("relay_key_tui".to_owned());
    }
    app.state.selected_panel = Panel::Objects;

    let mut terminal = Terminal::new(TestBackend::new(120, 28))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("object_tui_status"));
    assert!(buffer.contains("in_progress"));
    assert!(buffer.contains("512/1024 50%"));

    app.handle_input(&mut bus, TuiInput::Char('r')).await?;
    assert!(bus.requests.iter().any(|request| {
        request.method == "object.transfer.resume"
            && request.body["object_id"] == "object_tui_status"
            && request.body["relay_endpoint"] == "http://relay.test"
    }));
    assert_eq!(app.state.object_transfers[0].state, "complete");
    assert_eq!(app.state.object_transfers[0].percent, 100);
    Ok(())
}

#[test]
fn mcp_approval_event_enters_approval_panel() -> Result<(), TuiError> {
    let mut app = TuiApp::new("alice_account");
    let event = ramflux_sdk::LocalBusFrame {
        bus_protocol: "ramflux.local_bus.v1".to_owned(),
        frame_id: "frame_approval_test".to_owned(),
        kind: ramflux_sdk::LocalBusFrameKind::Event,
        request_id: "req_test".to_owned(),
        account_id: Some("alice_account".to_owned()),
        sdk_api: "mcp".to_owned(),
        method: "mcp.approval.request".to_owned(),
        body: serde_json::json!({
            "approval_id": "approval_tui_event",
            "server_id": "srv",
            "tool_name": "echo",
            "risk_level": "low",
            "confirmation_mode": "attended_local",
            "status": "pending"
        }),
        trace_id: None,
        seq: None,
        at: None,
        deadline_ms: None,
        operation_id: None,
        ok: None,
        error: None,
    };

    app.handle_bus_event(&event)?;
    assert_eq!(app.state.approvals.len(), 1);
    assert_eq!(app.state.approvals[0].id, "approval_tui_event");
    Ok(())
}

#[tokio::test]
async fn approval_panel_approves_pending_request_over_bus() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Approvals;
    app.handle_input(&mut bus, TuiInput::Char('a')).await?;
    let approved = bus.requests.iter().any(|request| {
        request.method == "grant.approve"
            && request.body.get("approval_id").and_then(serde_json::Value::as_str)
                == Some("approval_tui_1")
    });
    assert!(approved);
    Ok(())
}

#[tokio::test]
async fn remote_app_approval_is_visible_and_not_locally_approved() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.approvals = parse_approvals(&serde_json::json!({
        "approvals": [{
            "approval_id": "approval_remote_tui",
            "server_id": "srv",
            "tool_name": "shell",
            "risk_level": "high",
            "confirmation_mode": "remote_app",
            "status": "pending"
        }]
    }));
    app.state.selected_panel = Panel::Approvals;

    let mut terminal = Terminal::new(TestBackend::new(120, 28))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    terminal
        .draw(|frame| app.render(frame))
        .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?;
    let buffer = buffer_text(&terminal);
    assert!(buffer.contains("remote_app"));
    assert!(buffer.contains("remote_app:"));
    assert!(buffer.contains("App"));

    app.handle_input(&mut bus, TuiInput::Char('a')).await?;
    assert_eq!(app.state.status_message.as_deref(), Some("该审批需 App 端签名授权(remote_app)"));
    assert!(!bus.requests.iter().any(|request| request.method == "grant.approve"));
    Ok(())
}

/// A bus whose every request fails, so any input that touches the bus errors.
struct FailingBus;

#[async_trait]
impl TuiBus for FailingBus {
    async fn request(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        self.request_operation(account_id, sdk_api, method, body, String::new()).await
    }

    async fn request_operation(
        &mut self,
        _account_id: Option<String>,
        _sdk_api: &str,
        _method: &str,
        _body: serde_json::Value,
        _operation_id: String,
    ) -> Result<serde_json::Value, TuiError> {
        Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("boom".to_owned())))
    }

    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
        Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("no event".to_owned())))
    }
}

#[tokio::test]
async fn dispatch_input_surfaces_error_without_quitting() {
    let mut bus = FailingBus;
    let mut app = TuiApp::new("alice");
    // Drive an input path that issues a bus request: composing a message on the
    // Messages panel and pressing Enter calls the bus, which fails here.
    app.state.selected_panel = Panel::Messages;
    app.state.input = "hi".to_owned();
    assert!(app.state.status_message.is_none());

    app.dispatch_input(&mut bus, TuiInput::Enter).await;

    assert!(!app.should_quit(), "event loop must keep running after an input error");
    assert!(
        app.state.status_message.as_deref().is_some_and(|status| status.starts_with("error:")),
        "error should be surfaced to the status line: {:?}",
        app.state.status_message
    );
}

#[tokio::test]
async fn empty_message_submit_surfaces_status_instead_of_silent_noop() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.state.selected_panel = Panel::Messages;
    assert_eq!(app.state.input, "");

    // Empty input must give visible feedback rather than silently returning Ok.
    app.dispatch_input(&mut bus, TuiInput::Enter).await;

    assert_eq!(app.state.status_message.as_deref(), Some("nothing to send"));
    assert!(!bus.requests.iter().any(|request| {
        request.method == "message.submit" || request.method == "conversation.send"
    }));
    Ok(())
}

#[tokio::test]
async fn decide_approval_with_no_selection_surfaces_status() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.state.selected_panel = Panel::Approvals;
    assert_eq!(app.state.approvals, Vec::<state::ApprovalRow>::new());

    app.handle_input(&mut bus, TuiInput::Char('a')).await?;

    assert_eq!(app.state.status_message.as_deref(), Some("no approval selected"));
    assert!(!bus.requests.iter().any(|request| request.method == "grant.approve"));
    Ok(())
}

#[tokio::test]
async fn verify_refuses_blind_then_allows_after_safety_number() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Contacts;
    assert_eq!(app.state.contacts[0].safety_number, Vec::<String>::new());

    // Blind verify is refused while no safety number has been fetched.
    app.verify_selected_contact(&mut bus).await?;
    assert!(!bus.requests.iter().any(|request| request.method == "contact.verify"));
    assert_eq!(app.state.contacts[0].verification_state, "unverified");
    assert!(
        app.state.status_message.as_deref().is_some_and(|status| status.contains("press S")),
        "refusal hint missing: {:?}",
        app.state.status_message
    );

    // Once the safety number is loaded, verify is allowed to proceed.
    app.refresh_selected_contact_safety(&mut bus).await?;
    assert_ne!(app.state.contacts[0].safety_number, Vec::<String>::new());
    app.verify_selected_contact(&mut bus).await?;
    assert!(bus.requests.iter().any(|request| request.method == "contact.verify"));
    assert_eq!(app.state.contacts[0].verification_state, "verified");
    assert_eq!(app.state.status_message.as_deref(), Some("contact verified"));
    Ok(())
}

#[tokio::test]
async fn decrypted_message_replaces_delivery_placeholder() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("bob_account");
    app.state.conversations.push(ConversationRow {
        id: DEFAULT_CONVERSATION_ID.to_owned(),
        title: "Default DM".to_owned(),
        last_message: String::new(),
        unread: 0,
        status: "synced".to_owned(),
        recipient_device_id: None,
        target_delivery_id: None,
    });

    // A live delivery pushes a placeholder keyed by envelope_id.
    let event = ramflux_sdk::LocalBusFrame {
        bus_protocol: "ramflux.local_bus.v1".to_owned(),
        frame_id: "frame_evt_placeholder".to_owned(),
        kind: ramflux_sdk::LocalBusFrameKind::Event,
        request_id: "req_placeholder".to_owned(),
        account_id: Some("bob_account".to_owned()),
        sdk_api: "gateway".to_owned(),
        method: "gateway.deliver".to_owned(),
        body: serde_json::json!({
            "entries": [{
                "inbox_seq": 1,
                "envelope": {
                    "envelope_id": "env_tui_placeholder_1",
                    "source_principal_id": "alice"
                }
            }]
        }),
        trace_id: None,
        seq: None,
        at: None,
        deadline_ms: None,
        operation_id: None,
        ok: None,
        error: None,
    };
    app.handle_bus_event(&event)?;
    assert_eq!(app.state.conversations[0].unread, 0);
    assert_eq!(app.state.messages, Vec::new());

    app.receive_messages_with_attachments(&mut bus).await?;

    assert!(!bus.requests.iter().any(|request| request.method == "message.receive"));
    assert_eq!(app.state.messages.len(), 1);
    assert_eq!(app.state.messages[0].id, "msg_tui_1");
    assert_eq!(app.state.messages[0].body, "hello from bus");
    assert!(app.state.messages.iter().all(|message| message.body != "[new encrypted message]"));
    Ok(())
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect()
}

#[test]
fn task_messages_render_read_model_text_without_actor_kind_marker()
-> Result<(), Box<dyn std::error::Error>> {
    let sniffed = serde_json::json!({"messages":[{"message_id":"task-message","sender_id":"agent-device","content_type":"task","actor_kind_verified":false,"body_utf8":"Task done (actor kind unverified)"}]});
    assert!(crate::read_model::parse_page(&sniffed).is_err());
    let page = crate::read_model::sample_page("conv", "task-message", "Task done");
    let mut app = TuiApp::new("human");
    app.state.selected_panel = Panel::Messages;
    app.state.messages = crate::read_model::parse_page(&page)?.messages;
    assert_eq!(app.state.messages.len(), 1);
    let mut terminal = Terminal::new(TestBackend::new(120, 28))?;
    terminal.draw(|frame| app.render(frame))?;
    let rendered = buffer_text(&terminal);
    assert!(rendered.contains("Task done"));
    assert!(!rendered.contains("actor kind unverified"));
    Ok(())
}

#[test]
fn target_not_found_removes_only_that_row() {
    let mut app = TuiApp::new("alice");
    app.state.messages.push(MessageRow::local_send("keep".to_owned(), "stay".to_owned()));
    app.state.messages.push(MessageRow::local_send("drop".to_owned(), "gone".to_owned()));
    app.apply_typed_refusal(
        ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name(),
        Some("drop"),
    );
    assert_eq!(app.state.messages.len(), 1);
    assert_eq!(app.state.messages[0].id, "keep");
    assert_eq!(app.state.messages[0].body, "stay");
}

#[test]
fn tombstone_change_replaces_the_cached_body() -> Result<(), TuiError> {
    let mut app = TuiApp::new("alice");
    let page = crate::read_model::sample_page("conv", "msg", "hello");
    let parsed = crate::read_model::parse_page(&page)?;
    let original = parsed
        .messages
        .into_iter()
        .next()
        .ok_or_else(|| TuiError::ReadModel("missing sample row".to_owned()))?;
    app.apply_read_model_row(original);
    let mut tombstone = crate::read_model::sample_message("msg", "conv", "hello");
    tombstone["state"] = serde_json::json!("tombstone");
    tombstone["kind"] = serde_json::json!("tombstone");
    tombstone["erasure"] = serde_json::json!({"scope": "local", "tombstone_id": "tomb"});
    let page = serde_json::json!({
        "schema": crate::read_model::READ_MODEL_SCHEMA,
        "conversation_id": "conv",
        "messages": [tombstone],
        "snapshot_sequence": 12
    });
    let replacement = crate::read_model::parse_page(&page)?
        .messages
        .into_iter()
        .next()
        .ok_or_else(|| TuiError::ReadModel("missing tombstone row".to_owned()))?;
    app.state.conversations.push(ConversationRow {
        id: "conv".to_owned(),
        title: "Conv".to_owned(),
        last_message: "hello".to_owned(),
        unread: 0,
        status: "synced".to_owned(),
        recipient_device_id: None,
        target_delivery_id: None,
    });
    app.state.conversations.push(ConversationRow {
        id: "other".to_owned(),
        title: "Other".to_owned(),
        last_message: "hello".to_owned(),
        unread: 0,
        status: "synced".to_owned(),
        recipient_device_id: None,
        target_delivery_id: None,
    });
    app.apply_read_model_row(replacement);
    assert_eq!(app.state.messages.len(), 1);
    assert_eq!(app.state.messages[0].body, "");
    assert_eq!(app.state.messages[0].state, "tombstone");
    assert_eq!(app.state.messages[0].attachments, Vec::new());
    assert_eq!(app.state.conversations[0].last_message, "");
    assert_eq!(app.state.conversations[1].last_message, "");
    Ok(())
}

#[test]
fn message_cache_drops_the_oldest_row_past_the_named_cap() {
    let mut app = TuiApp::new("alice");
    let cap = crate::app::MAX_CACHED_MESSAGES;
    for index in 0..cap {
        app.apply_read_model_row(MessageRow::local_send(
            format!("old-{index}"),
            format!("body-{index}"),
        ));
    }
    app.apply_read_model_row(MessageRow::local_send("newest".to_owned(), "fresh".to_owned()));
    assert_eq!(app.state.messages.len(), cap);
    assert_eq!(app.state.messages.first().map(|message| message.id.as_str()), Some("newest"));
    assert!(app.state.messages.iter().all(|message| message.id != "old-0"));
}

#[tokio::test]
async fn delete_target_not_found_removes_only_the_requested_id() -> Result<(), TuiError> {
    let mut bus = MockBus {
        typed_refusal: Some((
            "message.delete".to_owned(),
            ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name().to_owned(),
        )),
        ..MockBus::default()
    };
    let mut app = TuiApp::new("alice");
    app.state.messages.push(MessageRow::local_send("keep".to_owned(), "stay".to_owned()));
    app.state.messages.push(MessageRow::local_send("drop".to_owned(), "gone".to_owned()));
    app.state.selected_message = 1;
    let error = match app.delete_selected_message(&mut bus).await {
        Ok(()) => {
            return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                "target_not_found was swallowed".to_owned(),
            )));
        }
        Err(error) => error,
    };
    assert_eq!(
        crate::read_model::generated_code(&error),
        Some(ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name())
    );
    assert_eq!(app.state.messages.len(), 1);
    assert_eq!(app.state.messages[0].id, "keep");
    let delete =
        bus.requests.iter().find(|request| request.method == "message.delete").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing delete".to_owned())),
        )?;
    assert_eq!(delete.body["message_id"], "drop");
    Ok(())
}

#[tokio::test]
async fn switching_conversation_reloads_the_page_and_resets_the_cursor() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice");
    app.state.selected_panel = Panel::Conversations;
    for id in ["conv-a", "conv-b"] {
        app.state.conversations.push(ConversationRow {
            id: id.to_owned(),
            title: id.to_owned(),
            last_message: String::new(),
            unread: 0,
            status: "synced".to_owned(),
            recipient_device_id: None,
            target_delivery_id: None,
        });
    }
    app.stream_cursors.insert("alice".to_owned(), 40);
    app.handle_input(&mut bus, TuiInput::Up).await?;
    assert_eq!(app.stream_cursors.get("alice").copied(), Some(40));
    assert!(bus.requests.iter().all(|request| request.method != "message.read"));
    app.handle_input(&mut bus, TuiInput::Down).await?;
    let read =
        bus.requests.iter().find(|request| request.method == "message.read").ok_or_else(|| {
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing page read".to_owned()))
        })?;
    assert_eq!(read.body["conversation_id"], "conv-b");
    assert_eq!(read.body["read_model"], true);
    assert!(read.body.get("after_sequence").is_none());
    assert_eq!(app.stream_cursors.get("alice").copied(), Some(10));
    Ok(())
}

#[tokio::test]
async fn send_without_event_id_does_not_insert_a_client_message_row() -> Result<(), TuiError> {
    let mut bus = MockBus { omit_send_event_id: true, ..MockBus::default() };
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;
    app.state.input = "orphan".to_owned();
    app.handle_input(&mut bus, TuiInput::Enter).await?;
    assert!(app.state.messages.iter().all(|message| message.id != "client_sent"));
    assert!(app.state.messages.iter().all(|message| message.body != "orphan"));
    Ok(())
}

#[tokio::test]
async fn revoke_requires_matching_device_and_signed_authority_epoch_before_success() {
    for response in [
        serde_json::json!({"device_id":"device","revoked":true}),
        serde_json::json!({"device_id":"other","revoked":true,"device_set_epoch":3}),
        serde_json::json!({"device_id":"device","revoked":false,"device_set_epoch":3}),
    ] {
        let mut bus = MockBus { revoke_response: Some(response), ..MockBus::default() };
        let mut app = TuiApp::new("account");
        assert!(app.revoke_device(&mut bus, "device").await.is_err());
        assert!(app.state.status_message.is_none());
        assert!(!bus.requests.iter().any(|r| r.method == "device.list"));
    }
}
