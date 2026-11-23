// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Catalog selection, lock admission and account-scoped content lifetime.

use super::TuiApp;
use crate::{ContactRow, TuiBus, TuiError, TuiState};

pub(super) struct CatalogAccount {
    pub id: String,
    pub name: String,
    locked: bool,
}

impl TuiApp {
    /// Resolves the account before reading any protected projection.
    ///
    /// # Errors
    /// Returns catalog or projection errors.
    pub async fn start_session<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let accounts = catalog(bus).await?;
        self.clear_content();
        self.content_locked = true;
        if accounts.is_empty() {
            self.begin_create();
            return Ok(());
        }
        if self.state.account_id == crate::DEFAULT_ACCOUNT_ID {
            if accounts.len() > 1 {
                self.show_chooser(accounts);
                return Ok(());
            }
            if let Some(account) = accounts.first() {
                self.state.account_id.clone_from(&account.id);
            }
        }
        self.open_session_account(bus).await
    }

    pub(super) async fn open_session_account<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let accounts = catalog(bus).await?;
        if accounts.iter().find(|row| row.id == self.state.account_id).is_none_or(|row| row.locked)
        {
            self.show_locked();
            return Ok(());
        }
        self.content_locked = false;
        self.load_account_identity(bus).await?;
        self.refresh_all(bus).await
    }

    /// Checks the product lock state even while the terminal receives no events.
    /// Catalog failures hide content until the account is authenticated again.
    pub async fn sync_lock_status<B: TuiBus + Send>(&mut self, bus: &mut B) {
        if self.content_locked {
            return;
        }
        match catalog(bus).await {
            Ok(rows) if rows.iter().any(|row| row.id == self.state.account_id && !row.locked) => {}
            Ok(_) | Err(_) => self.show_locked(),
        }
    }

    pub(super) fn clear_content(&mut self) {
        let previous = std::mem::take(&mut self.state);
        self.state = TuiState {
            account_id: previous.account_id,
            locale: previous.locale,
            selected_panel: previous.selected_panel,
            next_message_id: previous.next_message_id,
            ..TuiState::default()
        };
        self.cards = crate::surface::Cards::default();
        self.stream_cursors.clear();
        self.contact_peers.clear();
    }

    pub(super) async fn load_account_identity<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let status = bus
            .request(
                Some(self.state.account_id.clone()),
                "account",
                "account.status",
                serde_json::json!({}),
            )
            .await?;
        if status.get("state").and_then(serde_json::Value::as_str) == Some("locked") {
            self.show_locked();
            return Ok(());
        }
        self.state.local_device_id =
            status.get("device_id").and_then(serde_json::Value::as_str).map(str::to_owned);
        Ok(())
    }

    pub(super) fn contact_peer<'a>(&'a self, row: &'a ContactRow) -> Result<&'a str, TuiError> {
        self.contact_peers
            .get(&row.link_id)
            .filter(|peer| !peer.is_empty())
            .map(String::as_str)
            .ok_or_else(|| TuiError::ReadModel(self.text(crate::i18n::Key::NoContact).to_owned()))
    }
}

async fn catalog<B: TuiBus + Send>(bus: &mut B) -> Result<Vec<CatalogAccount>, TuiError> {
    let response = bus.request(None, "account", "account.list", serde_json::json!({})).await?;
    let rows = response
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TuiError::ReadModel("account.list.accounts".to_owned()))?;
    rows.iter()
        .map(|row| {
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| TuiError::ReadModel("account.list.id".to_owned()))?;
            let locked = row
                .get("locked")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| TuiError::ReadModel("account.list.locked".to_owned()))?;
            Ok(CatalogAccount {
                id: id.to_owned(),
                name: row
                    .get("display_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                locked,
            })
        })
        .collect()
}
