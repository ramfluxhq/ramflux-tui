// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Catalog keys. One variant is one message, in both languages.

macro_rules! keys {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum Key {
            $($name),*
        }

        pub(crate) const ALL: &[Key] = &[$(Key::$name),*];
    };
}

keys! {
    ChooseAccount, ChooseAccountHint, UnlockHint, HintKeys, HintConfirm, HintHidePhrase, UnlockTitle, CreateNameTitle, CreatePhraseTitle,
    CreateCodeTitle, CreateRepeatTitle, RotateOldTitle, RotateNewTitle, RotateRepeatTitle,
    RecoverAnchorTitle, RecoverPhraseTitle, RecoverCodeTitle, WarningLabel, DigitsOnly,
    NotSixDigits, TrivialCode, CodeMismatch, NameRequired, NameTooLong, WordRequired,
    AnchorMismatch, MissingSocket, DaemonRefused, Locked, Unlocked, Created, Rotated, Recovered,
    Cancelled, WordPrompt, PhraseLine, RecoverWarning,
    About, LangHelp, Version, SdkError, JsonError, ReadModelError, TitleBar, InputCompose, InputNormal,
    PanelConversations, PanelMessages, PanelObjects, PanelContacts, PanelGroups, PanelApprovals,
    MessagesTitle, ObjectsTitle, ContactsTitle, GroupsTitle, ApprovalsTitle, CardChromeTitle,
    Divider, UnreadSuffix, MessageLine, ObjectLine, ObjectError, AttachmentsSuffix, ReceiptsSuffix,
    ContactLine, DeviceLine, GroupLine, ApprovalLine, ApprovalApprove, ApprovalRemoteHint,
    CardSummary, CardHeader, EditLine, ButtonChrome, LinkUnavailable, UnsupportedComponent,
    UnsupportedValue, ChoicesUnavailable, RepeatedUnavailable, ChoiceItem, FieldLine,
    StatusError, NoCards, NoApproval, RemoteAppApproval, NothingToSend, AttachRefused,
    AttachmentNotSent, OpIdTooLong, EpochMissing, RevokeMismatch, RevocationSubmitted,
    QueuedAttachments, ObjectUsage, DownloadedObject, NoTransfer, MissingRelay, NoContact,
    BlindVerify, ContactVerified, UnknownCommand, NoCommand, ContactAdded, ContactAccepted,
    ActiveAccount, NoMessage, NoGroup, DefaultDm, ChangeBatchLimit, ChangeBatchWhat,
    ConversationMismatch,
    MissingRelayEnv, MissingArg,
    UnexpectedArg, StreamRefreshFailed, Reconnecting, SchemaReset, ContactAddLink,
    ContactAddRequester, ContactAddTarget, ContactAddCommand, ContactAcceptLink,
    ContactAcceptRequester, ContactAcceptTarget, ContactAcceptCommand, AccountSwitchId,
    AccountSwitchCommand,
    EnterWholeNumber, MissingOptions, EnterOptionNumbers, OptionOutOfRange, ChooseOne,
    InputBindingUnavailable, CardActionsUnavailable, CardStateUnavailable, CardSnapshotUnavailable,
    ButtonNoEvent, UnsupportedContext, MissingCardHash, ControlUnavailable, CheckboxUnavailable,
    ChoiceHint, CardChanged, ActionSubmitted, ActionRefused, NotAccepted, ActionSendFailed,
    CardPrefixed, SurfaceMissing, SurfaceScopeMismatch, CardUnavailable, TextFallback,
    CardContentUnavailable, MissingSnapshot, SnapshotIdentity, InvalidSnapshot, MissingVersion,
    SnapshotHash, CardClosed, CardOutOfSync, ReceiptVerified, ReceiptPending, CardsLoadFailed,
    InvalidSurfaceList, SurfacePageTooLarge, DuplicateSurface, CursorDidNotAdvance, FirstCards,
    BoolTrue, BoolFalse, ButtonLabel,
    CursorSequenceString, SchemaMismatch, MustBeObject, RetiredField, MissingField, MustBeString,
    MustBeInteger, MustFit32, MustBeBoolean, MessagesMustBeArray, TooManyMessages,
    DeviceSequenceNotNumber, DeviceSequenceDigits, GroupIdString, ContentError, ReactionsArray,
    ReceiptsArray, HlcMsLimit, HlcLogicalLimit, EmptyIdentity, BadMessageState, UnsupportedMessage,
    SenderMe, StatusSending, StatusSynced, StatusArchived, StatusReferenced, StatusPending,
    StatusUnknown, StatusUnverified, StatusVerified, StatusDeleted, StatusDelivered, StatusRead,
    StatusContent, StatusUnsupported, StatusTombstone, TransferInProgress, TransferComplete,
    DirectionUpload, DirectionDownload, NoneValue, StatusAccepted,
}
