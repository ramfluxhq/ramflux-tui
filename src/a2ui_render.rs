// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

use std::collections::BTreeSet;

use crate::{ApprovalRow, TuiBus, TuiError};

pub(crate) fn render_a2ui_for_approval(
    surface: &ramflux_sync::A2uiSurface,
) -> Option<ramflux_sync::RenderedSurface> {
    let supported = BTreeSet::from(["ramflux.basic.v1".to_owned()]);
    let permissions = BTreeSet::from([
        "mcp.approve".to_owned(),
        "mcp.deny".to_owned(),
        "message.send".to_owned(),
        "task.stop".to_owned(),
        "task.resume".to_owned(),
    ]);
    ramflux_sync::render_a2ui_surface(surface, &supported, &permissions).ok()
}

pub(crate) fn approval_a2ui_suffix(item: &ApprovalRow) -> String {
    let Some(rendered) = item.rendered_surface.as_ref() else {
        return String::new();
    };
    format!(
        " a2ui_hash={} fallback={} {}",
        rendered.surface_hash, rendered.fallback_used, rendered.semantic_snapshot
    )
}

pub(crate) async fn dispatch_a2ui_approval_action<B: TuiBus + Send>(
    bus: &mut B,
    account_id: &str,
    approval: &ApprovalRow,
) -> Result<(), TuiError> {
    let (Some(surface), Some(rendered)) = (&approval.surface, &approval.rendered_surface) else {
        return Ok(());
    };
    let Some((component_id, permission)) = first_actionable_component(&surface.components) else {
        return Ok(());
    };
    let action = ramflux_sync::A2uiAction {
        surface_id: surface.surface_id.clone(),
        surface_hash: rendered.surface_hash.clone(),
        component_id,
        permission,
        source_device_id: "attended_tui_device".to_owned(),
        target_device_id: "local_mcp_surface".to_owned(),
        created_at: 1_760_000_700,
        nonce: format!("nonce:{}", approval.id),
        signature: format!("attended-local:{}", approval.id),
    };
    bus.request(
        Some(account_id.to_owned()),
        "a2ui",
        "a2ui.action",
        serde_json::json!({
            "surface": surface,
            "action": action,
        }),
    )
    .await?;
    Ok(())
}

fn first_actionable_component(
    components: &[ramflux_sync::A2uiComponent],
) -> Option<(String, String)> {
    for component in components {
        if let Some(permission) = component.action_permission.as_ref() {
            return Some((component.id.clone(), permission.clone()));
        }
        if let Some(action) = first_actionable_component(&component.children) {
            return Some(action);
        }
    }
    None
}
