// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Consumer for `ramflux.message.read_model.v1`.
//!
//! Pages and change batches are the only message render source. Device sequence stays a 20-digit
//! decimal string, and the hybrid clock stays the split `hlc_ms` / `hlc_logical` pair.

use ramflux_protocol::message::{self as message, Content};
use serde_json::{Map, Value};

use crate::{AttachmentRow, MessageReceiptRow, MessageRow, TuiError};

/// Contract name carried by every read-model response.
pub(crate) const READ_MODEL_SCHEMA: &str = "ramflux.message.read_model.v1";
/// Neutral line for a kind this build cannot render.
pub(crate) const UNSUPPORTED_MESSAGE: &str = "This message needs a newer version";
/// Exclusive upper bound for `hlc_ms` (2^53).
const HLC_MS_LIMIT: u64 = 1 << 53;
/// Logical counter width of the hybrid clock.
const HLC_LOGICAL_LIMIT: u64 = 1 << 16;
/// Largest row count accepted from one response.
const MAX_ROWS: usize = 100;

const PAGE_FIELDS: &[&str] =
    &["schema", "conversation_id", "messages", "next_before", "snapshot_sequence"];
const CHANGE_FIELDS: &[&str] =
    &["schema", "conversation_id", "changes", "next_after", "more", "snapshot_sequence"];
const VIEW_FIELDS: &[&str] = &[
    "id",
    "conversation_id",
    "author",
    "device_sequence",
    "hlc_ms",
    "hlc_logical",
    "group_id",
    "journal_sequence",
    "is_own",
    "state",
    "kind",
    "text",
    "content",
    "unsupported",
    "erasure",
    "revision",
    "edited",
    "reactions",
    "receipts",
];

type Object = Map<String, Value>;

/// Newest-first page.
#[derive(Debug)]
pub(crate) struct ReadModelPage {
    pub conversation_id: String,
    pub messages: Vec<MessageRow>,
    pub next_before: Option<Value>,
    pub snapshot_sequence: u64,
}

/// Oldest-first change batch.
#[derive(Debug)]
pub(crate) struct ReadModelChanges {
    pub conversation_id: String,
    pub changes: Vec<MessageRow>,
    pub next_after: u64,
    pub more: bool,
}

struct ViewParts {
    id: String,
    conversation_id: String,
    author_anchor: Option<String>,
    device_sequence: String,
    hlc_ms: u64,
    hlc_logical: u64,
    journal_sequence: u64,
    is_own: bool,
    state: String,
    text: Option<String>,
    content: Option<Content>,
    erasure_scope: Option<String>,
    receipts: Vec<MessageReceiptRow>,
}

/// Wire name of a typed generated refusal, when the error preserved it.
///
/// A text `LocalBus` error is not a generated code, even when its sentence contains a code name.
#[must_use]
pub(crate) fn generated_code(error: &TuiError) -> Option<&str> {
    match error {
        TuiError::Sdk(ramflux_sdk::SdkError::LocalBusRefused { code, .. }) => Some(code.as_str()),
        TuiError::Sdk(ramflux_sdk::SdkError::GeneratedApi(inner)) => Some(inner.name),
        _ => None,
    }
}

/// Parses a rendered page.
///
/// # Errors
/// Rejects a missing schema, an unknown field, a numeric device sequence, or a combined `hlc`.
pub(crate) fn parse_page(value: &Value) -> Result<ReadModelPage, TuiError> {
    let map = expect_object(value, "page")?;
    reject_unknown(map, PAGE_FIELDS)?;
    require_schema(&req_string(map, "schema")?)?;
    let conversation_id = req_string(map, "conversation_id")?;
    let messages = parse_rows(field(map, "messages")?, "page")?;
    Ok(ReadModelPage {
        conversation_id,
        messages,
        next_before: optional_object(map, "next_before")?,
        snapshot_sequence: req_u64(map, "snapshot_sequence")?,
    })
}

/// Parses one change batch.
///
/// # Errors
/// Rejects the same malformed rows as [`parse_page`].
pub(crate) fn parse_changes(value: &Value) -> Result<ReadModelChanges, TuiError> {
    let map = expect_object(value, "changes")?;
    reject_unknown(map, CHANGE_FIELDS)?;
    require_schema(&req_string(map, "schema")?)?;
    let conversation_id = req_string(map, "conversation_id")?;
    let changes = parse_rows(field(map, "changes")?, "change batch")?;
    // `snapshot_sequence` is the account journal head. The follow cursor is `next_after`.
    req_u64(map, "snapshot_sequence")?;
    Ok(ReadModelChanges {
        conversation_id,
        changes,
        next_after: req_u64(map, "next_after")?,
        more: req_bool(map, "more")?,
    })
}

/// `EventIdentity` cursors keep `device_sequence` as a string. A JSON number is not the read model.
///
/// # Errors
/// Returns an error when the cursor encodes the device sequence as a number.
pub(crate) fn reject_numeric_cursor_sequence(cursor: Option<&Value>) -> Result<(), TuiError> {
    let Some(object) = cursor.and_then(Value::as_object) else {
        return Ok(());
    };
    for key in ["deviceSequence", "device_sequence"] {
        if object.get(key).is_some_and(Value::is_number) {
            return Err(invalid("next_before device sequence must be a string"));
        }
    }
    Ok(())
}

fn require_schema(schema: &str) -> Result<(), TuiError> {
    if schema == READ_MODEL_SCHEMA {
        Ok(())
    } else {
        Err(invalid("schema is not ramflux.message.read_model.v1"))
    }
}

fn invalid(message: impl AsRef<str>) -> TuiError {
    TuiError::ReadModel(message.as_ref().to_owned())
}

fn expect_object<'a>(value: &'a Value, what: &str) -> Result<&'a Object, TuiError> {
    value.as_object().ok_or_else(|| invalid(format!("{what} must be an object")))
}

fn reject_unknown(map: &Object, allowed: &[&str]) -> Result<(), TuiError> {
    for key in map.keys() {
        if !allowed.iter().any(|name| *name == key) {
            return Err(invalid(format!("unknown field {key}")));
        }
    }
    Ok(())
}

fn field<'a>(map: &'a Object, key: &str) -> Result<&'a Value, TuiError> {
    map.get(key).ok_or_else(|| invalid(format!("missing field {key}")))
}

fn req_string(map: &Object, key: &str) -> Result<String, TuiError> {
    field(map, key)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid(format!("{key} must be a string")))
}

fn req_u64(map: &Object, key: &str) -> Result<u64, TuiError> {
    field(map, key)?.as_u64().ok_or_else(|| invalid(format!("{key} must be an integer")))
}

fn req_u32(map: &Object, key: &str) -> Result<u32, TuiError> {
    let value = req_u64(map, key)?;
    u32::try_from(value).map_err(|_| invalid(format!("{key} must fit in 32 bits")))
}

fn req_bool(map: &Object, key: &str) -> Result<bool, TuiError> {
    field(map, key)?.as_bool().ok_or_else(|| invalid(format!("{key} must be a boolean")))
}

fn opt_string(map: &Object, key: &str) -> Result<Option<String>, TuiError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(invalid(format!("{key} must be a string"))),
    }
}

fn optional_object(map: &Object, key: &str) -> Result<Option<Value>, TuiError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value @ Value::Object(_)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(format!("{key} must be an object"))),
    }
}

fn opt_object<'a>(map: &'a Object, key: &str) -> Result<Option<&'a Object>, TuiError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => Ok(Some(object)),
        Some(_) => Err(invalid(format!("{key} must be an object"))),
    }
}

fn parse_rows(value: &Value, what: &str) -> Result<Vec<MessageRow>, TuiError> {
    let Some(items) = value.as_array() else {
        return Err(invalid(format!("{what} messages must be an array")));
    };
    if items.len() > MAX_ROWS {
        return Err(invalid(format!("{what} has more than {MAX_ROWS} messages")));
    }
    let mut rows = Vec::new();
    for item in items {
        rows.push(parse_row(item)?);
    }
    Ok(rows)
}

fn parse_row(value: &Value) -> Result<MessageRow, TuiError> {
    let map = expect_object(value, "message")?;
    reject_unknown(map, VIEW_FIELDS)?;
    let parts = ViewParts {
        id: req_string(map, "id")?,
        conversation_id: req_string(map, "conversation_id")?,
        author_anchor: parse_author_anchor(map)?,
        device_sequence: parse_device_sequence(field(map, "device_sequence")?)?,
        hlc_ms: req_u64(map, "hlc_ms")?,
        hlc_logical: req_u64(map, "hlc_logical")?,
        journal_sequence: req_u64(map, "journal_sequence")?,
        is_own: req_bool(map, "is_own")?,
        state: req_string(map, "state")?,
        text: opt_string(map, "text")?,
        content: opt_content(map)?,
        erasure_scope: parse_erasure_scope(map)?,
        receipts: parse_receipts(map)?,
    };
    validate_side_fields(map)?;
    project_row(parts)
}

fn validate_side_fields(map: &Object) -> Result<(), TuiError> {
    let _kind = req_string(map, "kind")?;
    let _revision = req_u64(map, "revision")?;
    let _edited = req_bool(map, "edited")?;
    validate_group_id(map)?;
    validate_unsupported(map)?;
    validate_reactions(map)
}

fn parse_device_sequence(value: &Value) -> Result<String, TuiError> {
    let Some(text) = value.as_str() else {
        return Err(invalid("device_sequence must be a string, not a number"));
    };
    if text.len() == 20 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(text.to_owned())
    } else {
        Err(invalid("device_sequence must be a 20-digit decimal string"))
    }
}

fn parse_author_anchor(map: &Object) -> Result<Option<String>, TuiError> {
    let Some(author) = opt_object(map, "author")? else {
        return Ok(None);
    };
    reject_unknown(author, &["account_anchor", "device_id", "device_epoch"])?;
    let anchor = req_string(author, "account_anchor")?;
    let _device_id = req_string(author, "device_id")?;
    let _device_epoch = req_u64(author, "device_epoch")?;
    Ok(Some(anchor))
}

fn validate_group_id(map: &Object) -> Result<(), TuiError> {
    match map.get("group_id") {
        None | Some(Value::Null | Value::String(_)) => Ok(()),
        Some(_) => Err(invalid("group_id must be a string")),
    }
}

fn opt_content(map: &Object) -> Result<Option<Content>, TuiError> {
    match map.get("content") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| invalid(format!("content: {error}"))),
    }
}

fn validate_unsupported(map: &Object) -> Result<(), TuiError> {
    let Some(placeholder) = opt_object(map, "unsupported")? else {
        return Ok(());
    };
    reject_unknown(placeholder, &["layer", "kind", "raw_length", "reason"])?;
    let _layer = req_u32(placeholder, "layer")?;
    let _kind = req_u32(placeholder, "kind")?;
    let _raw_length = req_u32(placeholder, "raw_length")?;
    let _reason = req_string(placeholder, "reason")?;
    Ok(())
}

fn parse_erasure_scope(map: &Object) -> Result<Option<String>, TuiError> {
    let Some(erasure) = opt_object(map, "erasure")? else {
        return Ok(None);
    };
    reject_unknown(erasure, &["scope", "tombstone_id"])?;
    let scope = req_string(erasure, "scope")?;
    let _tombstone_id = req_string(erasure, "tombstone_id")?;
    Ok(Some(scope))
}

fn validate_reactions(map: &Object) -> Result<(), TuiError> {
    let Some(value) = map.get("reactions") else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid("reactions must be an array"));
    };
    for item in items {
        let reaction = expect_object(item, "reaction")?;
        reject_unknown(reaction, &["token", "count"])?;
        let _token = req_string(reaction, "token")?;
        let _count = req_u32(reaction, "count")?;
    }
    Ok(())
}

fn parse_receipts(map: &Object) -> Result<Vec<MessageReceiptRow>, TuiError> {
    let Some(value) = map.get("receipts") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid("receipts must be an array"));
    };
    let mut receipts = Vec::new();
    for item in items {
        receipts.push(parse_receipt(item)?);
    }
    Ok(receipts)
}

fn parse_receipt(value: &Value) -> Result<MessageReceiptRow, TuiError> {
    let receipt = expect_object(value, "receipt")?;
    reject_unknown(
        receipt,
        &["device_id", "principal_commitment", "state", "delivered_at", "read_at"],
    )?;
    let device_id = req_string(receipt, "device_id")?;
    let state = req_string(receipt, "state")?;
    let _principal = opt_string(receipt, "principal_commitment")?;
    validate_optional_i64(receipt, "delivered_at")?;
    validate_optional_i64(receipt, "read_at")?;
    Ok(MessageReceiptRow { device_id, state })
}

fn validate_optional_i64(map: &Object, key: &str) -> Result<(), TuiError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(value) if value.as_i64().is_some() => Ok(()),
        Some(_) => Err(invalid(format!("{key} must be an integer"))),
    }
}

fn project_row(view: ViewParts) -> Result<MessageRow, TuiError> {
    if view.hlc_ms >= HLC_MS_LIMIT {
        return Err(invalid("hlc_ms must be below 2^53"));
    }
    if view.hlc_logical >= HLC_LOGICAL_LIMIT {
        return Err(invalid("hlc_logical must fit in 16 bits"));
    }
    if view.id.is_empty() || view.conversation_id.is_empty() {
        return Err(invalid("message identity is empty"));
    }
    let sender = if view.is_own {
        "me".to_owned()
    } else {
        match view.author_anchor.as_deref() {
            Some(anchor) if !anchor.is_empty() => anchor.to_owned(),
            _ => "unknown".to_owned(),
        }
    };
    let (body, status, attachments, receipts) = render_body(&view)?;
    Ok(MessageRow {
        id: view.id,
        sender,
        body,
        status,
        attachments,
        receipts,
        device_sequence: view.device_sequence,
        hlc_ms: view.hlc_ms,
        hlc_logical: view.hlc_logical,
        state: view.state,
        journal_sequence: view.journal_sequence,
    })
}

fn render_body(
    view: &ViewParts,
) -> Result<(String, String, Vec<AttachmentRow>, Vec<MessageReceiptRow>), TuiError> {
    match view.state.as_str() {
        "tombstone" => {
            let status = view
                .erasure_scope
                .clone()
                .filter(|scope| !scope.is_empty())
                .unwrap_or_else(|| "tombstone".to_owned());
            Ok((String::new(), status, Vec::new(), Vec::new()))
        }
        "unsupported" => {
            Ok((UNSUPPORTED_MESSAGE.to_owned(), "unsupported".to_owned(), Vec::new(), Vec::new()))
        }
        "content" => {
            let attachments = view
                .content
                .as_ref()
                .map(|content| referenced_media(&view.id, content))
                .unwrap_or_default();
            Ok((
                view.text.clone().unwrap_or_default(),
                "content".to_owned(),
                attachments,
                view.receipts.clone(),
            ))
        }
        _ => Err(invalid("message state is not content, unsupported, or tombstone")),
    }
}

fn referenced_media(message_id: &str, content: &Content) -> Vec<AttachmentRow> {
    let Some(message::content::Value::Message(body)) = &content.value else {
        return Vec::new();
    };
    body.media
        .iter()
        .filter(|media| !media.object_id.is_empty())
        .map(|media| AttachmentRow {
            message_id: message_id.to_owned(),
            object_id: media.object_id.clone(),
            status: "referenced".to_owned(),
            plaintext_base64: None,
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn sample_message(id: &str, conversation_id: &str, text: &str) -> Value {
    serde_json::json!({
        "id": id,
        "conversation_id": conversation_id,
        "author": {"account_anchor": "alice", "device_id": "device-alice", "device_epoch": 1},
        "device_sequence": "00000000000000000007",
        "hlc_ms": 1_700_000_000_000_u64,
        "hlc_logical": 4,
        "journal_sequence": 9,
        "is_own": false,
        "state": "content",
        "kind": "message",
        "text": text,
        "revision": 0,
        "edited": false,
        "reactions": [],
        "receipts": []
    })
}

#[cfg(test)]
pub(crate) fn sample_page(conversation_id: &str, id: &str, text: &str) -> Value {
    serde_json::json!({
        "schema": READ_MODEL_SCHEMA,
        "conversation_id": conversation_id,
        "messages": [sample_message(id, conversation_id, text)],
        "next_before": null,
        "snapshot_sequence": 10
    })
}

#[cfg(test)]
pub(crate) fn sample_changes(conversation_id: &str, next_after: u64) -> Value {
    serde_json::json!({
        "schema": READ_MODEL_SCHEMA,
        "conversation_id": conversation_id,
        "changes": [],
        "next_after": next_after,
        "more": false,
        "snapshot_sequence": next_after
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_message() -> Content {
        Content {
            value: Some(message::content::Value::Message(message::Message {
                text: "你好 🌟".to_owned(),
                erasure_nonce: vec![0x00, 0xff, 0x10],
                ..message::Message::default()
            })),
        }
    }

    #[test]
    fn binding_round_trip_preserves_presence_integers_unicode_and_bytes() -> Result<(), TuiError> {
        let content = content_message();
        let encoded = serde_json::to_value(&content)?;
        let decoded: Content = serde_json::from_value(encoded.clone())?;
        assert_eq!(decoded, content);
        let raw = serde_json::to_string(&content)?;
        assert!(raw.contains("你好"));
        assert!(!raw.contains("replyTo") && !raw.contains("reply_to"));

        let identity = message::EventIdentity {
            id: vec![1, 2, 3, 4],
            conversation_id: "conv".to_owned(),
            author: None,
            device_sequence: u64::MAX,
            hlc: (1_700_000_000_000_u64 << 16) | 7,
            group_id: None,
            group_epoch: None,
        };
        let identity_json = serde_json::to_value(&identity)?;
        let identity_back: message::EventIdentity = serde_json::from_value(identity_json.clone())?;
        assert_eq!(identity_back.device_sequence, u64::MAX);
        assert_eq!(identity_back.hlc, identity.hlc);
        assert!(identity_back.group_id.is_none());
        assert!(identity_json.get("groupId").is_none() && identity_json.get("group_id").is_none());
        let sequence = identity_json
            .get("deviceSequence")
            .or_else(|| identity_json.get("device_sequence"))
            .ok_or_else(|| TuiError::ReadModel("device sequence field missing".to_owned()))?;
        assert!(sequence.is_string(), "protocol device sequence must not be a JSON number");

        let request = message::MessageRequest {
            request_id: "req-1".to_owned(),
            account_id: "acct".to_owned(),
            action: Some(message::message_request::Action::Send(message::SendMessage {
                conversation_id: "conv".to_owned(),
                content: Some(content.clone()),
                operation_id: b"op-\x00\xff".to_vec(),
                reply_to: None,
            })),
        };
        let request_back: message::MessageRequest =
            serde_json::from_value(serde_json::to_value(&request)?)?;
        assert_eq!(request_back, request);

        let update = message::MessageUpdate {
            account_id: "acct".to_owned(),
            journal_sequence: 11,
            identity: Some(identity),
            state: Some(message::message_update::State::Content(content.clone())),
        };
        let update_back: message::MessageUpdate =
            serde_json::from_value(serde_json::to_value(&update)?)?;
        assert_eq!(update_back, update);

        let mut view = sample_message("msg-bind", "conv", "你好 🌟");
        view["content"] = encoded;
        view["device_sequence"] = Value::String("18446744073709551615".to_owned());
        view["hlc_ms"] = serde_json::json!(1_700_000_000_000_u64);
        view["hlc_logical"] = serde_json::json!(7);
        let cursor = identity_json.clone();
        let page = serde_json::json!({
            "schema": READ_MODEL_SCHEMA,
            "conversation_id": "conv",
            "messages": [view],
            "next_before": identity_json,
            "snapshot_sequence": 11
        });
        let parsed = parse_page(&page)?;
        assert_eq!(parsed.messages[0].body, "你好 🌟");
        assert_eq!(parsed.messages[0].device_sequence, "18446744073709551615");
        assert_eq!(parsed.messages[0].hlc_ms, 1_700_000_000_000);
        assert_eq!(parsed.messages[0].hlc_logical, 7);
        assert_eq!(parsed.next_before.as_ref(), Some(&cursor));
        Ok(())
    }

    #[test]
    fn rejects_numeric_device_sequence_combined_hlc_and_body_sniff() -> Result<(), TuiError> {
        let mut numeric = sample_message("msg", "conv", "hidden");
        numeric["device_sequence"] = serde_json::json!(7);
        let page = serde_json::json!({
            "schema": READ_MODEL_SCHEMA,
            "conversation_id": "conv",
            "messages": [numeric],
            "snapshot_sequence": 1
        });
        assert!(parse_page(&page).is_err());

        let mut combined = sample_message("msg", "conv", "hidden");
        let object = combined
            .as_object_mut()
            .ok_or_else(|| TuiError::ReadModel("sample message is not an object".to_owned()))?;
        object.remove("hlc_ms");
        object.remove("hlc_logical");
        object.insert("hlc".to_owned(), serde_json::json!(99));
        let page = serde_json::json!({
            "schema": READ_MODEL_SCHEMA,
            "conversation_id": "conv",
            "messages": [combined],
            "snapshot_sequence": 1
        });
        assert!(parse_page(&page).is_err());

        let sniffed = serde_json::json!({
            "messages": [{
                "message_id": "old",
                "sender_id": "peer",
                "body_utf8": "old body"
            }]
        });
        assert!(parse_page(&sniffed).is_err());
        Ok(())
    }

    #[test]
    fn tombstone_drops_stray_text_and_unsupported_does_not_guess() -> Result<(), TuiError> {
        let mut tombstone = sample_message("msg-old", "conv", "secret old body");
        tombstone["state"] = serde_json::json!("tombstone");
        tombstone["kind"] = serde_json::json!("tombstone");
        tombstone["text"] = serde_json::json!("secret old body");
        tombstone["erasure"] = serde_json::json!({"scope": "everyone", "tombstone_id": "tomb"});
        tombstone["receipts"] = serde_json::json!([{"device_id": "device", "state": "read"}]);
        let page = serde_json::json!({
            "schema": READ_MODEL_SCHEMA,
            "conversation_id": "conv",
            "messages": [tombstone],
            "snapshot_sequence": 4
        });
        let parsed = parse_page(&page)?;
        assert_eq!(parsed.messages[0].body, "");
        assert_eq!(parsed.messages[0].attachments, Vec::new());
        assert_eq!(parsed.messages[0].receipts, Vec::new());
        assert_eq!(parsed.messages[0].status, "everyone");
        assert_eq!(parsed.messages[0].state, "tombstone");

        let mut unsupported = sample_message("msg-new", "conv", "guess this body");
        unsupported["state"] = serde_json::json!("unsupported");
        unsupported["kind"] = serde_json::json!("unsupported");
        unsupported["text"] = serde_json::json!("guess this body");
        unsupported["unsupported"] =
            serde_json::json!({"layer": 9, "kind": 4, "raw_length": 12, "reason": "layer"});
        let page = serde_json::json!({
            "schema": READ_MODEL_SCHEMA,
            "conversation_id": "conv",
            "messages": [unsupported],
            "snapshot_sequence": 5
        });
        let parsed = parse_page(&page)?;
        assert_eq!(parsed.messages[0].body, UNSUPPORTED_MESSAGE);
        assert!(!parsed.messages[0].body.contains("guess"));
        Ok(())
    }

    #[test]
    fn text_local_bus_error_is_not_a_generated_resync_code() {
        let text =
            TuiError::Sdk(ramflux_sdk::SdkError::LocalBus("please resync_required now".to_owned()));
        assert!(generated_code(&text).is_none());
        let typed = TuiError::Sdk(ramflux_sdk::SdkError::LocalBusRefused {
            code: ramflux_sdk::bindings::GeneratedErrorCode::ResyncRequired.name().to_owned(),
            message: "cursor pruned".to_owned(),
            body: Box::new(serde_json::json!({"cursor": 9, "floor": 1, "head": 4})),
        });
        assert_eq!(
            generated_code(&typed),
            Some(ramflux_sdk::bindings::GeneratedErrorCode::ResyncRequired.name())
        );
    }
}
