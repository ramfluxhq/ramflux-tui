// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Account-scoped cursors and read-side refreshes driven by the daemon journal.
use crate::read_model::generated_code;
use crate::{TuiApp, TuiBus, TuiError};
impl TuiApp {
    pub(crate) async fn subscribe_stream<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        self.subscribe_once(bus, true).await
    }

    async fn subscribe_once<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        recover: bool,
    ) -> Result<(), TuiError> {
        let response = self.request_subscription(bus).await;
        if recover && response.as_ref().is_err_and(is_resync_required) {
            let account = self.state.account_id.clone();
            self.state.messages.clear();
            self.stream_cursors.remove(&account);
            self.refresh_messages(bus).await?;
            let retry = self.request_subscription(bus).await;
            return self.finish_subscription(retry);
        }
        self.finish_subscription(response)
    }

    async fn request_subscription<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<serde_json::Value, TuiError> {
        let account = self.state.account_id.clone();
        let cursor = self.stream_cursors.get(&account).copied();
        let topics = stream_topics();
        bus.request(
            Some(account),
            "stream",
            "bus.subscribe",
            serde_json::json!({"topics":topics,"since_seq":cursor,"attended_frontend":true}),
        )
        .await
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
        if event.method == "message.transient" {
            return Ok(());
        }
        if event.method == "stream.lagged" {
            self.follow_message_changes(bus, message_id_hint(event)).await?;
            self.refresh_cards(bus).await?;
            return self.subscribe_once(bus, false).await;
        }
        if let Some(seq) = event.seq
            && self.stream_cursors.get(&self.state.account_id).is_some_and(|cursor| seq <= *cursor)
        {
            return Ok(());
        }
        let mut resynced = false;
        match event.method.as_str() {
            "message.schema.updated"
            | "state.changed"
            | "message.received"
            | "message.updated"
            | "group.message.received" => {
                resynced = self.follow_message_changes(bus, message_id_hint(event)).await?;
            }
            "conversation.changed" | "task.changed" => {
                self.refresh_conversations(bus).await?;
                resynced = self.follow_message_changes(bus, message_id_hint(event)).await?;
            }
            "surface.changed" => self.refresh_cards(bus).await?,
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
                resynced = self.follow_message_changes(bus, message_id_hint(event)).await?;
            }
            _ => {}
        }
        if matches!(
            event.method.as_str(),
            "message.schema.updated"
                | "state.changed"
                | "message.received"
                | "message.updated"
                | "group.message.received"
                | "conversation.changed"
                | "task.changed"
        ) {
            self.refresh_cards(bus).await?;
        }
        if !resynced && let Some(seq) = event.seq {
            let account = self.state.account_id.clone();
            let ahead = self.stream_cursors.get(&account).is_none_or(|cursor| seq > *cursor);
            if ahead {
                self.stream_cursors.insert(account, seq);
            }
        }
        Ok(())
    }

    fn finish_subscription(
        &mut self,
        response: Result<serde_json::Value, TuiError>,
    ) -> Result<(), TuiError> {
        let response = match response {
            Err(error) if is_schema_reset(&error) => {
                self.state.status_message = Some("schema_reset_required".to_owned());
                return Ok(());
            }
            other => other?,
        };
        let account = self.state.account_id.clone();
        if !self.stream_cursors.contains_key(&account)
            && let Some(head) = response.get("head_seq").and_then(serde_json::Value::as_u64)
        {
            self.stream_cursors.insert(account, head);
        }
        Ok(())
    }
}

fn message_id_hint(event: &ramflux_sdk::LocalBusFrame) -> Option<&str> {
    event.body.get("message_id").and_then(serde_json::Value::as_str).filter(|id| !id.is_empty())
}

fn is_resync_required(error: &TuiError) -> bool {
    generated_code(error) == Some(ramflux_sdk::bindings::GeneratedErrorCode::ResyncRequired.name())
}

fn is_schema_reset(error: &TuiError) -> bool {
    generated_code(error)
        == Some(ramflux_sdk::bindings::GeneratedErrorCode::SchemaResetRequired.name())
}

fn stream_topics() -> serde_json::Value {
    serde_json::json!([
        "message.schema.updated",
        "state.changed",
        "message.transient",
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
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    #[derive(Default)]
    struct Bus {
        calls: Vec<(String, serde_json::Value)>,
        /// Typed `resync_required` once, when `since_seq` is a number.
        typed_resync: bool,
        /// Text error that merely contains the words `resync_required`.
        text_resync: bool,
        /// Typed refusal of the next `message.read` that carries `after_sequence`.
        change_refusal: Option<String>,
    }
    #[async_trait]
    impl TuiBus for Bus {
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
            _: Option<String>,
            _: &str,
            method: &str,
            body: serde_json::Value,
            _: String,
        ) -> Result<serde_json::Value, TuiError> {
            self.calls.push((method.to_owned(), body.clone()));
            let conversation =
                body.get("conversation_id").and_then(serde_json::Value::as_str).unwrap_or("thread");
            Ok(match method {
                "bus.subscribe" => {
                    if self.typed_resync && body["since_seq"].is_u64() {
                        self.typed_resync = false;
                        return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBusRefused {
                            code: ramflux_sdk::bindings::GeneratedErrorCode::ResyncRequired
                                .name()
                                .to_owned(),
                            message: "cursor pruned".to_owned(),
                            body: Box::new(
                                serde_json::json!({"cursor": 999, "floor": 1, "head": 10}),
                            ),
                        }));
                    }
                    if self.text_resync && body["since_seq"].is_u64() {
                        return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                            "daemon text says resync_required".to_owned(),
                        )));
                    }
                    serde_json::json!({"subscribed":true,"head_seq":10,"floor_seq":1})
                }
                "surface.list" => serde_json::json!({"surfaces":[],"next_cursor":null}),
                "conversation.list" => {
                    serde_json::json!({"conversations":[{"conversation_id":"thread","unread_count":1}]})
                }
                "message.read"
                    if let Some(id) = body
                        .get("message_id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty()) =>
                {
                    crate::read_model::sample_detail(conversation, id, "single content")
                }
                "message.read" if body.get("after_sequence").is_some() => {
                    if let Some(code) = self.change_refusal.take() {
                        return Err(typed_refusal(code));
                    }
                    let after =
                        body.get("after_sequence").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    crate::read_model::sample_changes(conversation, after)
                }
                "message.read" => {
                    crate::read_model::sample_page(conversation, "message", "read content")
                }
                _ => serde_json::json!({}),
            })
        }
        async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
            std::future::pending().await
        }
    }
    fn typed_refusal(code: String) -> TuiError {
        TuiError::Sdk(ramflux_sdk::SdkError::LocalBusRefused {
            code,
            message: "change refused".to_owned(),
            body: Box::new(serde_json::json!({})),
        })
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
        let read =
            bus.calls.iter().find(|(method, _)| method == "message.read").ok_or_else(|| {
                TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing read".to_owned()))
            })?;
        assert_eq!(read.1["read_model"], true);
        let subscribe =
            bus.calls.iter().find(|(method, _)| method == "bus.subscribe").ok_or_else(|| {
                TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("missing subscribe".to_owned()))
            })?;
        assert_eq!(subscribe.1["since_seq"], 10);
        assert!(
            bus.calls
                .iter()
                .all(|(method, _)| method != "message.receive" && method != "message.ack")
        );
        assert_eq!(app.state.messages[0].body, "read content");
        assert_eq!(app.state.messages[0].device_sequence, "00000000000000000007");
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
        assert!(bus.calls.iter().any(|(method, body)| {
            method == "message.read"
                && body.get("after_sequence").is_some()
                && body.get("read_model").is_none()
        }));
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
        app.apply_stream_event(&mut bus, &event("stream.lagged", None)).await?;
        let subscriptions =
            bus.calls.iter().filter(|(method, _)| method == "bus.subscribe").collect::<Vec<_>>();
        assert_eq!(subscriptions[0].1["since_seq"], 10);
        assert_eq!(subscriptions[1].1["since_seq"], 11);
        assert!(subscriptions.iter().all(|(_, body)| !body["since_seq"].is_null()));
        assert_eq!(app.stream_cursors["bob"], 11);
        assert_eq!(app.state.messages[0].body, "read content");
        Ok(())
    }

    #[tokio::test]
    async fn typed_resync_drops_the_cache_and_reloads_the_page() -> Result<(), TuiError> {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        app.state
            .messages
            .insert(0, crate::MessageRow::local_send("stale".to_owned(), "stale body".to_owned()));
        app.stream_cursors.insert("bob".to_owned(), 999);
        bus.typed_resync = true;
        app.subscribe_stream(&mut bus).await?;
        assert!(app.state.messages.iter().all(|message| message.id != "stale"));
        assert_eq!(app.state.messages[0].body, "read content");
        assert_eq!(app.stream_cursors["bob"], 10);
        let subscriptions =
            bus.calls.iter().filter(|(method, _)| method == "bus.subscribe").collect::<Vec<_>>();
        assert_eq!(
            subscriptions.last().map(|(_, body)| &body["since_seq"]),
            Some(&serde_json::json!(10))
        );
        Ok(())
    }

    #[tokio::test]
    async fn text_resync_required_does_not_drop_the_cache() -> Result<(), TuiError> {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        app.state
            .messages
            .insert(0, crate::MessageRow::local_send("kept".to_owned(), "kept body".to_owned()));
        bus.text_resync = true;
        let error = match app.subscribe_stream(&mut bus).await {
            Ok(()) => {
                return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                    "text error was treated as success".to_owned(),
                )));
            }
            Err(error) => error,
        };
        assert!(crate::read_model::generated_code(&error).is_none());
        assert!(
            app.state
                .messages
                .iter()
                .any(|message| message.id == "kept" && message.body == "kept body")
        );
        Ok(())
    }

    #[tokio::test]
    async fn typed_change_errors_keep_the_cursor_and_the_rows() -> Result<(), TuiError> {
        let codes = [
            ramflux_sdk::bindings::GeneratedErrorCode::TargetNotFound.name(),
            ramflux_sdk::bindings::GeneratedErrorCode::SchemaResetRequired.name(),
            ramflux_sdk::bindings::GeneratedErrorCode::Internal.name(),
            ramflux_sdk::bindings::GeneratedErrorCode::PayloadTooLarge.name(),
        ];
        for code in codes {
            let mut app = TuiApp::new("bob");
            let mut bus = Bus::default();
            app.refresh_all(&mut bus).await?;
            let before = app.state.messages.len();
            app.state
                .messages
                .push(crate::MessageRow::local_send("keep".to_owned(), "stay".to_owned()));
            bus.change_refusal = Some(code.to_owned());
            let error = match app
                .apply_stream_event(&mut bus, &event("message.received", Some(11)))
                .await
            {
                Ok(()) => {
                    return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                        "typed change error was swallowed".to_owned(),
                    )));
                }
                Err(error) => error,
            };
            assert_eq!(crate::read_model::generated_code(&error), Some(code));
            assert_eq!(app.stream_cursors.get("bob").copied(), Some(10));
            assert_eq!(app.state.messages.len(), before + 1);
            assert!(app.state.messages.iter().any(|message| message.id == "keep"));
            assert!(bus.calls.iter().all(|(_, body)| body.get("message_id").is_none()));
            assert_eq!(app.state.status_message.as_deref(), Some(code));
        }
        Ok(())
    }

    #[tokio::test]
    async fn payload_too_large_reads_one_message_and_keeps_the_cursor() -> Result<(), TuiError> {
        let mut app = TuiApp::new("bob");
        let mut bus = Bus::default();
        app.refresh_all(&mut bus).await?;
        bus.calls.clear();
        bus.change_refusal =
            Some(ramflux_sdk::bindings::GeneratedErrorCode::PayloadTooLarge.name().to_owned());
        let mut frame = event("message.schema.updated", Some(11));
        frame.body = serde_json::json!({"message_id": "msg_big"});
        let error = match app.apply_stream_event(&mut bus, &frame).await {
            Ok(()) => {
                return Err(TuiError::Sdk(ramflux_sdk::SdkError::LocalBus(
                    "payload_too_large was swallowed".to_owned(),
                )));
            }
            Err(error) => error,
        };
        assert_eq!(
            crate::read_model::generated_code(&error),
            Some(ramflux_sdk::bindings::GeneratedErrorCode::PayloadTooLarge.name())
        );
        assert_eq!(app.stream_cursors.get("bob").copied(), Some(10));
        assert!(bus.calls.iter().any(|(method, body)| {
            method == "message.read"
                && body.get("message_id").and_then(serde_json::Value::as_str) == Some("msg_big")
                && body.get("after_sequence").is_none()
        }));
        assert!(
            app.state
                .messages
                .iter()
                .any(|message| { message.id == "msg_big" && message.body == "single content" })
        );
        assert_eq!(
            app.state.status_message.as_deref(),
            Some(ramflux_sdk::bindings::GeneratedErrorCode::PayloadTooLarge.name())
        );
        Ok(())
    }
}
