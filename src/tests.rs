// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain
#![cfg(test)]

use async_trait::async_trait;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::VecDeque;

use crate::a2ui_render::render_a2ui_for_approval;
use crate::parsing::parse_approvals;
use crate::*;

#[derive(Default)]
struct MockBus {
    requests: Vec<MockRequest>,
    events: VecDeque<ramflux_sdk::LocalBusFrame>,
}

#[derive(Clone, Debug)]
struct MockRequest {
    sdk_api: String,
    method: String,
    body: serde_json::Value,
}

#[async_trait]
impl TuiBus for MockBus {
    async fn request(
        &mut self,
        _account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        self.requests.push(MockRequest {
            sdk_api: sdk_api.to_owned(),
            method: method.to_owned(),
            body: body.clone(),
        });
        Ok(match method {
            "message.read" => serde_json::json!({
                "messages": [{
                    "message_id": "msg_tui_1",
                    "sender_id": "alice",
                    "body_utf8": "hello from bus"
                }]
            }),
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
            "a2ui.action" => serde_json::json!({"accepted": true}),
            "grant.approve" | "grant.deny" => serde_json::json!({"ok": true}),
            "message.submit" => serde_json::json!({"submitted": true}),
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
            "subscription.open" => serde_json::json!({"subscribed": true}),
            _ => serde_json::json!({}),
        })
    }

    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
        self.events
            .pop_front()
            .ok_or_else(|| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("no event".to_owned())))
    }
}

fn mock_approvals() -> serde_json::Value {
    serde_json::json!([{
        "approval_id": "approval_tui_1",
        "server_id": "srv",
        "tool_name": "echo",
        "risk_level": "low",
        "confirmation_mode": "attended_local",
        "status": "pending",
        "details": {
            "surface": {
                "surface_id": "surface_tui_approval",
                "catalog": "ramflux.basic.v1",
                "catalog_version": "1",
                "components": [{
                    "id": "approve_button",
                    "component_type": "approval_card",
                    "action_permission": "mcp.approve",
                    "children": []
                }]
            }
        }
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
    assert!(buffer.contains("approval_card"));
    assert!(buffer.contains("a2ui_hash="));
    Ok(())
}

#[tokio::test]
async fn contact_panel_loads_safety_number_and_marks_verified() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Contacts;

    app.handle_input(&mut bus, TuiInput::Char('s')).await?;
    assert_eq!(app.state.contacts[0].fingerprint_hex.as_deref(), Some("f00d"));
    assert_eq!(app.state.contacts[0].verification_state, "unverified");

    app.handle_input(&mut bus, TuiInput::Char('v')).await?;
    assert_eq!(app.state.contacts[0].verification_state, "verified");
    assert!(bus.requests.iter().any(|request| request.method == "contact.verify"));
    Ok(())
}

#[tokio::test]
async fn message_panel_surfaces_receipts_and_delete_actions() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;

    app.handle_input(&mut bus, TuiInput::Char('l')).await?;
    app.handle_input(&mut bus, TuiInput::Char('r')).await?;
    app.handle_input(&mut bus, TuiInput::Char('x')).await?;

    assert!(bus.requests.iter().any(|request| request.method == "message.receipt.delivered"));
    assert!(bus.requests.iter().any(|request| request.method == "message.receipt.read"));
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
async fn enter_in_message_panel_submits_plaintext_over_bus() -> Result<(), TuiError> {
    let mut bus = MockBus::default();
    let mut app = TuiApp::new("alice_account");
    app.refresh_all(&mut bus).await?;
    app.state.selected_panel = Panel::Messages;
    for value in "typed via tui".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    let submit =
        bus.requests.iter().find(|request| request.method == "message.submit").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing submit".to_owned())),
        )?;
    assert_eq!(submit.sdk_api, "message");
    assert_eq!(submit.body["recipient_device_id"], serde_json::Value::Null);
    let plaintext =
        submit.body.get("plaintext_body_base64").and_then(serde_json::Value::as_str).ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing plaintext".to_owned())),
        )?;
    assert_eq!(
        ramflux_protocol::decode_base64url(plaintext)
            .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?,
        b"typed via tui"
    );
    assert!(app.state.messages.iter().any(|message| message.body == "typed via tui"));
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
        bus.requests.iter().find(|request| request.method == "message.submit").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing submit".to_owned())),
        )?;
    let plaintext =
        submit.body.get("plaintext_body_base64").and_then(serde_json::Value::as_str).ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing plaintext".to_owned())),
        )?;
    assert_eq!(
        ramflux_protocol::decode_base64url(plaintext)
            .map_err(|error| TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(error.to_string())))?,
        b"relax xray lol"
    );
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
        ok: None,
        error: None,
    };

    app.handle_bus_event(&event)?;
    assert!(app.state.messages.iter().any(|message| message.id == "env_tui_event_1"));
    assert_eq!(app.state.conversations[0].unread, 1);
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
    for value in "hello bob".chars() {
        app.handle_input(&mut bus, TuiInput::Char(value)).await?;
    }
    app.handle_input(&mut bus, TuiInput::Enter).await?;

    let submit =
        bus.requests.iter().find(|request| request.method == "message.submit").ok_or_else(
            || TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing submit".to_owned())),
        )?;
    assert_eq!(submit.body["conversation_id"], "conv_bootstrap");
    assert_eq!(submit.body["recipient_device_id"], "bob_device_tui");
    assert_eq!(submit.body["target_delivery_id"], "target_tui_bob");
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
    let action = bus.requests.iter().any(|request| {
        request.method == "a2ui.action"
            && request.body.pointer("/action/permission").and_then(serde_json::Value::as_str)
                == Some("mcp.approve")
    });
    assert!(action);
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
    assert_eq!(
        app.state.status_message.as_deref(),
        Some("This approval requires App-side signing (remote_app)")
    );
    assert!(!bus.requests.iter().any(|request| request.method == "grant.approve"));
    Ok(())
}

#[test]
fn a2ui_unknown_component_uses_fallback_renderer() -> Result<(), TuiError> {
    let surface = ramflux_sync::A2uiSurface {
        surface_id: "surface_unknown".to_owned(),
        catalog: "ramflux.basic.v1".to_owned(),
        catalog_version: "1".to_owned(),
        components: vec![ramflux_sync::A2uiComponent {
            id: "mystery".to_owned(),
            component_type: "future_card".to_owned(),
            action_permission: None,
            children: Vec::new(),
        }],
    };
    let rendered = render_a2ui_for_approval(&surface).ok_or_else(|| {
        TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing fallback".to_owned()))
    })?;
    assert!(rendered.fallback_used);
    assert!(rendered.semantic_snapshot.contains("future_card"));
    Ok(())
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect()
}
