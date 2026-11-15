// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use async_trait::async_trait;
use std::path::Path;

use crate::TuiError;

/// Local-bus surface consumed by the private UI.
// `async_trait` emits `#[must_use]` on the generated boxed-future methods, which
// clippy >= 1.99 reports as `double_must_use` at this trait definition.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait TuiBus {
    /// Sends a request to the local SDK bus.
    ///
    /// # Errors
    /// Returns an error when the bus request fails.
    async fn request(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError>;

    /// Sends a request and keeps a daemon refusal's code instead of flattening it to text.
    ///
    /// `operation_id` is required by generated mutating methods such as `conversation.send`.
    ///
    /// # Errors
    /// Returns an error when the bus request fails. Daemon refusals stay
    /// [`ramflux_sdk::SdkError::LocalBusRefused`].
    async fn request_operation(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
        operation_id: String,
    ) -> Result<serde_json::Value, TuiError>;

    /// Reads one subscription event from the local SDK bus.
    ///
    /// # Errors
    /// Returns an error when the bus event cannot be read.
    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError>;
}

/// Production bus adapter over the open SDK `LocalBusClient`.
pub struct SdkLocalBus {
    client: ramflux_sdk::LocalBusClient,
    next_request: u64,
}

impl SdkLocalBus {
    /// # Errors
    /// Returns an error when the daemon socket cannot be opened.
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, TuiError> {
        Ok(Self {
            client: ramflux_sdk::LocalBusClient::connect(socket_path).await?,
            next_request: 1,
        })
    }

    fn frame(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
        operation_id: Option<String>,
    ) -> ramflux_sdk::LocalBusFrame {
        let request_id = self.next_request;
        self.next_request = self.next_request.saturating_add(1);
        let mut frame = ramflux_sdk::LocalBusFrame::request(
            format!("tui_{request_id}"),
            account_id,
            sdk_api,
            method,
            body,
        );
        frame.operation_id = operation_id;
        frame
    }
}

#[async_trait]
impl TuiBus for SdkLocalBus {
    async fn request(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        let frame = self.frame(account_id, sdk_api, method, body, None);
        Ok(self.client.request_frame_structured(frame).await?)
    }

    async fn request_operation(
        &mut self,
        account_id: Option<String>,
        sdk_api: &str,
        method: &str,
        body: serde_json::Value,
        operation_id: String,
    ) -> Result<serde_json::Value, TuiError> {
        let frame = self.frame(account_id, sdk_api, method, body, Some(operation_id));
        Ok(self.client.request_frame_structured(frame).await?)
    }

    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
        Ok(self.client.next_event().await?)
    }
}
