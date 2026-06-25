// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain
use async_trait::async_trait;
use std::path::Path;

use crate::TuiError;

/// Local-bus surface consumed by the private UI.
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

    /// Reads one subscription event from the local SDK bus.
    ///
    /// # Errors
    /// Returns an error when the bus event cannot be read.
    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError>;
}

/// Production bus adapter over the open SDK `LocalBusClient`.
pub struct SdkLocalBus {
    client: ramflux_sdk::LocalBusClient,
}

impl SdkLocalBus {
    /// # Errors
    /// Returns an error when the daemon socket cannot be opened.
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, TuiError> {
        Ok(Self { client: ramflux_sdk::LocalBusClient::connect(socket_path).await? })
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
        Ok(self.client.request(account_id, sdk_api, method, &body).await?)
    }

    async fn next_event(&mut self) -> Result<ramflux_sdk::LocalBusFrame, TuiError> {
        Ok(self.client.next_event().await?)
    }
}
