// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Account-scoped cursors and read-side refreshes driven by the daemon journal.
use crate::{TuiApp, TuiBus, TuiError};
impl TuiApp {
    /// Enables the one-release compatibility mode for daemons without a journal.
    pub const fn set_legacy_poll(&mut self, enabled: bool) {
        self.legacy_poll = enabled;
    }

    pub(crate) async fn subscribe_stream<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let account = self.state.account_id.clone();
        let cursor = self.stream_cursors.get(&account).copied();
        let topics = serde_json::json!([
            "message.received",
            "message.updated",
            "group.message.received",
            "group.changed",
            "conversation.changed",
            "contact.changed",
            "safety.changed",
            "object.transfer.progress",
            "object.changed",
            "account.state",
            "node.health",
            "sync.status",
            "mcp.approval.request",
            "mcp.approval.granted",
            "mcp.approval.denied",
            "surface.changed",
            "task.changed"
        ]);
        let response = bus
            .request(
                Some(account.clone()),
                "stream",
                "bus.subscribe",
                serde_json::json!({"topics":topics,"since_seq":cursor,"attended_frontend":true}),
            )
            .await;
        let response = match response {
            Err(error) if error.to_string().contains("resync_required") => {
                // Subscribe first, then load snapshots: concurrent writes remain queued for replay.
                let response = bus
                    .request(
                        Some(account.clone()),
                        "stream",
                        "bus.subscribe",
                        serde_json::json!({"topics":topics,"attended_frontend":true}),
                    )
                    .await?;
                self.stream_cursors.remove(&account);
                response
            }
            other => other?,
        };
        if !self.stream_cursors.contains_key(&account)
            && let Some(head) = response.get("head_seq").and_then(serde_json::Value::as_u64)
        {
            self.stream_cursors.insert(account, head);
        }
        Ok(())
    }

    /// Applies metadata invalidations through content read methods, committing the cursor last.
    ///
    /// # Errors
    /// Leaves the cursor unchanged if a snapshot/content read fails, so reconnect replays the event.
    pub async fn apply_stream_event<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        event: &ramflux_sdk::LocalBusFrame,
    ) -> Result<(), TuiError> {
        if event.account_id.as_deref() != Some(self.state.account_id.as_str()) {
            return Ok(());
        }
        if self.legacy_poll {
            return self.handle_bus_event(event);
        }
        if event.method == "stream.lagged" {
            return self.refresh_all(bus).await;
        }
        if let Some(seq) = event.seq
            && self.stream_cursors.get(&self.state.account_id).is_some_and(|cursor| seq <= *cursor)
        {
            return Ok(());
        }
        match event.method.as_str() {
            "message.received"
            | "message.updated"
            | "group.message.received"
            | "conversation.changed"
            | "task.changed" => {
                self.refresh_conversations(bus).await?;
                self.refresh_messages(bus).await?;
            }
            "group.changed" => self.refresh_groups(bus).await?,
            "contact.changed" | "safety.changed" => self.refresh_contacts(bus).await?,
            "mcp.approval.request" | "mcp.approval.granted" | "mcp.approval.denied" => {
                self.refresh_approvals(bus).await?;
            }
            "object.transfer.progress" | "object.changed" => {
                if let Some(mut row) = crate::parsing::parse_transfer(&event.body) {
                    if let Some(existing) = self
                        .state
                        .object_transfers
                        .iter_mut()
                        .find(|value| value.object_id == row.object_id)
                    {
                        row.relay_endpoint.clone_from(&existing.relay_endpoint);
                        row.relay_service_key_base64.clone_from(&existing.relay_service_key_base64);
                        *existing = row;
                    } else {
                        self.state.object_transfers.push(row);
                    }
                }
                self.refresh_messages(bus).await?;
            }
            _ => {}
        }
        if let Some(seq) = event.seq {
            self.stream_cursors.insert(self.state.account_id.clone(), seq);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    #[derive(Default)]
    struct Bus {
        calls: Vec<(String, serde_json::Value)>,
        resync: bool,
    }
    #[async_trait]
    impl TuiBus for Bus {
        async fn request(
            &mut self,
            _: Option<String>,
            _: &str,
            method: &str,
            body: serde_json::Value,
        ) -> Result<serde_json::Value, TuiError> {
            self.calls.push((method.to_owned(), body.clone()));
            Ok(match method {
                "bus.subscribe" => {
                    if self.resync && body["since_seq"].is_u64() {
                        self.resync = false;
                        return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                            "resync_required".to_owned(),
                        )));
                    }
                    serde_json::json!({"subscribed":true,"head_seq":10,"floor_seq":1})
                }
                "conversation.list" => {
                    serde_json::json!({"conversations":[{"conversation_id":"thread","unread_count":1}]})
                }
                "message.read" => {
                    serde_json::json!({"messages":[{"message_id":"message","sender_id":"peer","body_utf8":"read content"}]})
                }
                _ => serde_json::json!({}),
            })
        }
        async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
            std::future::pending().await
        }
    }
    fn event(topic: &str, seq: Option<u64>) -> ramflux_sdk::LocalBusFrame {
        ramflux_sdk::LocalBusFrame {
            kind: ramflux_sdk::LocalBusFrameKind::Event,
            seq,
            at: Some(1),
            ..ramflux_sdk::LocalBusFrame::request(
                "event",
                Some("bob".to_owned()),
                "stream",
                topic,
                serde_json::json!({}),
            )
        }
    }
    #[tokio::test]
    async fn remote_attachment_directory_row_keeps_its_identity_and_state() -> Result<(), TuiError>
    {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        let mut frame = event("object.changed", Some(11));
        frame.body = serde_json::json!({"id":"deferred-attachment","transfer":null,
            "transfer_state":"not_fetched","size":1234});
        app.apply_stream_event(&mut bus, &frame).await?;
        assert_eq!(app.state.object_transfers.len(), 1);
        let row = &app.state.object_transfers[0];
        assert_eq!(row.object_id, "deferred-attachment");
        assert_eq!(row.state, "not_fetched");
        assert_eq!(row.total_bytes, 1234);
        Ok(())
    }
    #[tokio::test]
    async fn default_refresh_uses_cursor_subscription_and_content_reads_without_receive_polling()
    -> Result<(), TuiError> {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        assert_eq!(bus.calls[0].0, "bus.subscribe");
        assert!(
            bus.calls
                .iter()
                .all(|(method, _)| method != "message.receive" && method != "message.ack")
        );
        assert_eq!(app.state.messages[0].body, "read content");
        app.apply_stream_event(&mut bus, &event("message.received", Some(11))).await?;
        assert_eq!(app.stream_cursors["bob"], 11);
        let calls = bus.calls.len();
        app.apply_stream_event(&mut bus, &event("message.received", Some(11))).await?;
        assert_eq!(bus.calls.len(), calls);
        Ok(())
    }
    #[tokio::test]
    async fn task_changes_reload_message_fallback_without_driving_receive() -> Result<(), TuiError>
    {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        bus.calls.clear();
        app.apply_stream_event(&mut bus, &event("task.changed", Some(11))).await?;
        assert!(bus.calls.iter().any(|(method, _)| method == "message.read"));
        assert!(bus.calls.iter().any(|(method, _)| method == "conversation.list"));
        assert!(
            bus.calls
                .iter()
                .all(|(method, _)| method != "message.receive" && method != "message.ack")
        );
        assert_eq!(app.stream_cursors["bob"], 11);
        assert_eq!(app.state.messages[0].body, "read content");
        Ok(())
    }
    #[tokio::test]
    async fn lagged_resubscribes_and_resync_reloads_snapshots() -> Result<(), TuiError> {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        app.apply_stream_event(&mut bus, &event("message.updated", Some(11))).await?;
        bus.resync = true;
        app.apply_stream_event(&mut bus, &event("stream.lagged", None)).await?;
        let subscriptions =
            bus.calls.iter().filter(|(method, _)| method == "bus.subscribe").collect::<Vec<_>>();
        assert_eq!(subscriptions[1].1["since_seq"], 11);
        assert!(subscriptions[2].1["since_seq"].is_null());
        assert_eq!(app.stream_cursors["bob"], 10);
        assert_eq!(app.state.messages[0].body, "read content");
        Ok(())
    }
}
