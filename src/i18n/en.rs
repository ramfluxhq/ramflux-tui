// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! English catalog. Sentences that tests lock stay byte-identical.

use super::{Category, Key};

pub(crate) fn template(key: Key, category: Category) -> &'static str {
    account(key)
        .or_else(|| chrome(key))
        .or_else(|| lines(key))
        .or_else(|| status(key))
        .or_else(|| cards(key))
        .or_else(|| parse_keys(key))
        .or_else(|| tokens(key))
        .unwrap_or_else(|| plural(key, category))
}

fn account(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::HintKeys => "L lock  N new  P change code  Y recover  G language",
        Key::HintConfirm => "Enter confirms. Esc cancels.",
        Key::HintHidePhrase => "Write the words down. Enter hides them.",
        Key::UnlockTitle | Key::RecoverCodeTitle => "Account code",
        Key::CreateNameTitle => "Display name",
        Key::CreatePhraseTitle | Key::RecoverPhraseTitle => "Recovery phrase",
        Key::CreateCodeTitle | Key::RotateNewTitle => "New account code",
        Key::CreateRepeatTitle => "Repeat account code",
        Key::RotateOldTitle => "Current account code",
        Key::RotateRepeatTitle => "Repeat new account code",
        Key::RecoverAnchorTitle => "Type the account fingerprint",
        Key::WarningLabel => "Warning",
        Key::DigitsOnly => "The account code uses digits only.",
        Key::NotSixDigits => "The account code must be exactly 6 digits.",
        Key::TrivialCode => "That code is too easy to guess. Choose another.",
        Key::CodeMismatch => "The account codes did not match.",
        Key::NameRequired => "A display name is required.",
        Key::NameTooLong => "The display name is too long.",
        Key::WordRequired => "Enter the recovery word.",
        Key::AnchorMismatch => "The fingerprint did not match.",
        Key::MissingSocket => "This screen needs the daemon socket.",
        Key::DaemonRefused => "The daemon refused the account request.",
        Key::Locked => "Account locked. Enter the account code.",
        Key::Unlocked => "Account unlocked.",
        Key::Created => "Account created.",
        Key::Rotated => "Account code changed.",
        Key::Recovered => "Account recovered.",
        Key::Cancelled => "Cancelled.",
        Key::WordPrompt => "Recovery word {position}",
        Key::PhraseLine => "{position}. {word}",
        Key::RecoverWarning => "{label}\n{warning}\n\n{anchor}",
        _ => return None,
    })
}

fn chrome(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::About => "Private Ramflux TUI over the open local SDK bus",
        Key::LangHelp => "Interface language: en or zh-CN",
        Key::Version => "{version} (build {build})",
        Key::SdkError => "SDK error:",
        Key::JsonError => "JSON error:",
        Key::ReadModelError => "read model:",
        Key::TitleBar => "rf tui  {hints}",
        Key::InputCompose => "Message input [COMPOSE]",
        Key::InputNormal => "Message input [NORMAL i=compose]",
        Key::PanelConversations => "Conversations",
        Key::PanelMessages => "Messages",
        Key::PanelObjects => "Objects",
        Key::PanelContacts => "Contacts",
        Key::PanelGroups => "Groups",
        Key::PanelApprovals => "Approvals",
        Key::MessagesTitle => "Messages  c=cards  /attach <path> l=delivered r=read x=delete",
        Key::ObjectsTitle => "Objects  i=type put/get/status/resume command  s=status r=resume",
        Key::ContactsTitle => {
            "Contacts/Devices  i=type add/switch/accept then Enter  S=safety V=verify"
        }
        Key::GroupsTitle => "Groups  m=mute u=unmute e=disappearing",
        Key::ApprovalsTitle => "Approvals  a=approve d=deny",
        Key::CardChromeTitle => "Card  ↑↓=select Enter=act [ ]=card Esc=back",
        Key::Divider => "────────────────",
        _ => return None,
    })
}

fn lines(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::UnreadSuffix => "  unread={count} {status}",
        Key::MessageLine => "{marker}{id} {sender}: {body} ({status}){suffix}",
        Key::ObjectLine => "{marker}{id} {direction} {state} {done}/{total} {percent}%{error}",
        Key::ObjectError => " error={value}",
        Key::AttachmentsSuffix => " attachments=[{summary}]",
        Key::ReceiptsSuffix => " receipts=[{summary}]",
        Key::ContactLine => {
            "{marker}{link} {requester} -> {target} {state} verify={verification} fp={fingerprint} sn={safety}"
        }
        Key::DeviceLine => "{marker}device {id} epoch={epoch} target={target}",
        Key::GroupLine => {
            "{marker}{id} members={members} disappearing={disappearing} mute_until={mute}"
        }
        Key::ApprovalLine => "{marker}{id} {tool} risk={risk} mode={mode} {status} {hint}",
        Key::ApprovalApprove => "a=approve",
        Key::ApprovalRemoteHint => "remote_app: needs an app signature",
        Key::CardSummary => "Card: {fallback}  c=open",
        Key::CardHeader => "Card {index}/{total} — {fallback}",
        Key::EditLine => "Edit: {text}  Enter=save Esc=cancel",
        Key::ButtonChrome => "[ {label} ]",
        Key::LinkUnavailable => "[{label}: link/function unavailable in this terminal]",
        Key::UnsupportedComponent => "[Unsupported card component: {kind}]",
        Key::UnsupportedValue => "[Unsupported display value]",
        Key::ChoicesUnavailable => "[Choices unavailable]",
        Key::RepeatedUnavailable => "[Repeated content unavailable in this terminal]",
        Key::ChoiceItem => "{index}. {mark} {label}",
        Key::FieldLine => "{label}: {value}",
        _ => return None,
    })
}

fn status(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::StatusError => "error: {detail}",
        Key::NoCards => "No cards in this conversation",
        Key::NoApproval => "no approval selected",
        Key::RemoteAppApproval => "This approval needs a signature in the app (remote_app).",
        Key::NothingToSend => "nothing to send",
        Key::AttachRefused => {
            "conversation.send does not accept attachment bytes; use the object panel"
        }
        Key::AttachmentNotSent => {
            "queued attachment bytes were not sent; open the object panel to send the file"
        }
        Key::OpIdTooLong => "operation_id is longer than 256 bytes",
        Key::EpochMissing => "signed device-set epoch missing from revoke acknowledgement",
        Key::RevokeMismatch => "device revoke acknowledgement mismatch",
        Key::RevocationSubmitted => {
            "Revocation submitted for {device} (signed device-set epoch {epoch})"
        }
        Key::ObjectUsage => {
            "object command: put <path> <object_id> | get <object_id> <out> | status <object_id> [direction] | resume <object_id> <direction>"
        }
        Key::DownloadedObject => "downloaded object {object_id}",
        Key::NoTransfer => "no object transfer selected",
        Key::MissingRelay => "selected transfer is missing relay endpoint",
        Key::NoContact => "no contact selected",
        Key::BlindVerify => {
            "refusing blind verify: press S to load the safety number, then compare all 60 digits out-of-band before verifying"
        }
        Key::ContactVerified => "contact verified",
        Key::UnknownCommand => "unknown command: {command}",
        Key::NoCommand => "no command entered",
        Key::ContactAdded => "contact added: {link_id}",
        Key::ContactAccepted => "contact accepted: {link_id}",
        Key::ActiveAccount => "active account: {account}",
        Key::NoMessage => "no message selected",
        Key::NoGroup => "no group selected",
        Key::DefaultDm => "Default DM",
        Key::ChangeBatchLimit => "message change batch stopped at the round limit",
        Key::ChangeBatchWhat => "change batch",
        Key::ConversationMismatch => "conversation_id mismatch",
        Key::MissingRelayEnv => "missing {name} for relay object transfer",
        Key::MissingArg => "missing {label}",
        Key::UnexpectedArg => "unexpected {command} argument: {extra}",
        Key::StreamRefreshFailed => "stream refresh failed: {error}",
        Key::Reconnecting => "reconnecting: {error}",
        Key::SchemaReset => "The message schema has to be reset before this client can continue.",
        _ => return None,
    })
}

fn cards(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::ContactAddLink => "contact add link_id",
        Key::ContactAddRequester => "contact add requester commitment",
        Key::ContactAddTarget => "contact add target commitment",
        Key::ContactAddCommand => "contact add",
        Key::ContactAcceptLink => "contact accept link_id",
        Key::ContactAcceptRequester => "contact accept requester commitment",
        Key::ContactAcceptTarget => "contact accept target commitment",
        Key::ContactAcceptCommand => "contact accept",
        Key::AccountSwitchId => "account switch account_id",
        Key::AccountSwitchCommand => "account switch",
        Key::EnterWholeNumber => "enter a whole number",
        Key::MissingOptions => "missing options",
        Key::EnterOptionNumbers => "enter option numbers separated by commas",
        Key::OptionOutOfRange => "option number out of range",
        Key::ChooseOne => "choose one option",
        Key::InputBindingUnavailable => "input binding unavailable",
        Key::CardActionsUnavailable => "card actions unavailable",
        Key::CardStateUnavailable => "card state unavailable",
        Key::CardSnapshotUnavailable => "card snapshot unavailable",
        Key::ButtonNoEvent => "button has no event",
        Key::UnsupportedContext => "unsupported action context",
        Key::MissingCardHash => "missing card hash",
        Key::ControlUnavailable => "control unavailable",
        Key::CheckboxUnavailable => "checkbox value unavailable",
        Key::ChoiceHint => "Enter option numbers in displayed order (1,2); empty clears selection",
        Key::CardChanged => "card changed; review it again before acting",
        Key::ActionSubmitted => "Action submitted; awaiting agent receipt",
        Key::ActionRefused => "Action refused: {reason}",
        Key::NotAccepted => "not accepted",
        Key::ActionSendFailed => {
            "Action send failed; delivery is unknown. Refresh before acting again"
        }
        Key::CardPrefixed => "card: {detail}",
        Key::SurfaceMissing => "missing {key}",
        Key::SurfaceScopeMismatch => "surface scope mismatch",
        Key::CardUnavailable => "Card unavailable",
        Key::TextFallback => "Text fallback: this card is not from your own agent",
        Key::CardContentUnavailable => "Card content unavailable",
        Key::MissingSnapshot => "missing snapshot",
        Key::SnapshotIdentity => "snapshot identity mismatch",
        Key::InvalidSnapshot => "invalid snapshot",
        Key::MissingVersion => "missing version",
        Key::SnapshotHash => "snapshot hash mismatch",
        Key::CardClosed => "Card closed",
        Key::CardOutOfSync => "Card out of sync; actions unavailable",
        Key::ReceiptVerified => "Action receipt verified",
        Key::ReceiptPending => "Action recorded; receipt pending",
        Key::CardsLoadFailed => "Cards could not be loaded; press c to retry",
        Key::InvalidSurfaceList => "invalid surface list",
        Key::SurfacePageTooLarge => "surface page too large",
        Key::DuplicateSurface => "duplicate surface reference",
        Key::CursorDidNotAdvance => "surface cursor did not advance",
        _ => return None,
    })
}

fn parse_keys(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::CursorSequenceString => "next_before device sequence must be a string",
        Key::SchemaMismatch => "schema is not ramflux.message.read_model.v1",
        Key::MustBeObject => "{what} must be an object",
        Key::RetiredField => "retired field {key}",
        Key::MissingField => "missing field {key}",
        Key::MustBeString => "{key} must be a string",
        Key::MustBeInteger => "{key} must be an integer",
        Key::MustFit32 => "{key} must fit in 32 bits",
        Key::MustBeBoolean => "{key} must be a boolean",
        Key::MessagesMustBeArray => "{what} messages must be an array",
        Key::TooManyMessages => "{what} has more than 100 messages",
        Key::DeviceSequenceNotNumber => "device_sequence must be a string, not a number",
        Key::DeviceSequenceDigits => "device_sequence must be a 20-digit decimal string",
        Key::GroupIdString => "group_id must be a string",
        Key::ContentError => "content: {error}",
        Key::ReactionsArray => "reactions must be an array",
        Key::ReceiptsArray => "receipts must be an array",
        Key::HlcMsLimit => "hlc_ms must be below 2^53",
        Key::HlcLogicalLimit => "hlc_logical must fit in 16 bits",
        Key::EmptyIdentity => "message identity is empty",
        Key::BadMessageState => "message state is not content, unsupported, or tombstone",
        Key::UnsupportedMessage => "This message needs a newer version",
        _ => return None,
    })
}

fn tokens(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::BoolTrue => "true",
        Key::BoolFalse => "false",
        Key::ButtonLabel => "Button",
        Key::SenderMe => "me",
        Key::StatusSending => "sending",
        Key::StatusSynced => "synced",
        Key::StatusArchived => "archived",
        Key::StatusReferenced => "referenced",
        Key::StatusPending => "pending",
        Key::StatusUnknown => "unknown",
        Key::StatusUnverified => "unverified",
        Key::StatusVerified => "verified",
        Key::StatusDeleted => "deleted",
        Key::StatusDelivered => "delivered",
        Key::StatusRead => "read",
        Key::StatusContent => "content",
        Key::StatusUnsupported => "unsupported",
        Key::StatusTombstone => "tombstone",
        Key::TransferInProgress => "in_progress",
        Key::TransferComplete => "complete",
        Key::DirectionUpload => "upload",
        Key::DirectionDownload => "download",
        Key::NoneValue => "none",
        Key::StatusAccepted => "accepted",
        _ => return None,
    })
}

fn plural(key: Key, category: Category) -> &'static str {
    match key {
        Key::FirstCards => match category {
            Category::One => "Showing the first {count} card in this conversation",
            Category::Other => "Showing the first {count} cards in this conversation",
        },
        Key::QueuedAttachments => "queued attachment count={count}",
        _ => "",
    }
}
