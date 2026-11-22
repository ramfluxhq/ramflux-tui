// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Account-screen copy. English and zh-CN are the only catalogs.

/// Language for account creation, unlock, rotation and recovery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    En,
    ZhCn,
}

impl Locale {
    /// Switches between the two catalogs.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::En => Self::ZhCn,
            Self::ZhCn => Self::En,
        }
    }
}

/// Reads `RAMFLUX_TUI_LANG`. Unknown values stay English.
#[must_use]
pub fn locale_from_env(value: &str) -> Locale {
    match value {
        "zh-CN" | "zh_CN" | "zh" => Locale::ZhCn,
        _ => Locale::En,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Key {
    HintKeys,
    HintConfirm,
    HintHidePhrase,
    UnlockTitle,
    CreateNameTitle,
    CreatePhraseTitle,
    CreateCodeTitle,
    CreateRepeatTitle,
    RotateOldTitle,
    RotateNewTitle,
    RotateRepeatTitle,
    RecoverAnchorTitle,
    RecoverPhraseTitle,
    RecoverCodeTitle,
    WarningLabel,
    DigitsOnly,
    NotSixDigits,
    TrivialCode,
    CodeMismatch,
    NameRequired,
    NameTooLong,
    WordRequired,
    AnchorMismatch,
    MissingSocket,
    DaemonRefused,
    Locked,
    Unlocked,
    Created,
    Rotated,
    Recovered,
    Cancelled,
}

pub(crate) fn text(locale: Locale, key: Key) -> &'static str {
    match locale {
        Locale::En => english(key),
        Locale::ZhCn => chinese(key),
    }
}

pub(crate) fn word_prompt(locale: Locale, position: u8) -> String {
    match locale {
        Locale::En => format!("Recovery word {position}"),
        Locale::ZhCn => format!("恢复词 {position}"),
    }
}

fn english(key: Key) -> &'static str {
    match key {
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
    }
}

fn chinese(key: Key) -> &'static str {
    match key {
        Key::HintKeys => "L 锁定  N 新建  P 修改口令  Y 恢复  G 语言",
        Key::HintConfirm => "回车确认。Esc 取消。",
        Key::HintHidePhrase => "请抄下这些词。回车后隐藏。",
        Key::UnlockTitle | Key::RecoverCodeTitle => "账户口令",
        Key::CreateNameTitle => "显示名称",
        Key::CreatePhraseTitle | Key::RecoverPhraseTitle => "恢复短语",
        Key::CreateCodeTitle | Key::RotateNewTitle => "新账户口令",
        Key::CreateRepeatTitle => "再次输入账户口令",
        Key::RotateOldTitle => "当前账户口令",
        Key::RotateRepeatTitle => "再次输入新账户口令",
        Key::RecoverAnchorTitle => "输入账户指纹",
        Key::WarningLabel => "警告",
        Key::DigitsOnly => "账户口令只能是数字。",
        Key::NotSixDigits => "账户口令必须是 6 位数字。",
        Key::TrivialCode => "这个口令太容易猜到。请另选一个。",
        Key::CodeMismatch => "两次输入的账户口令不一致。",
        Key::NameRequired => "需要显示名称。",
        Key::NameTooLong => "显示名称太长。",
        Key::WordRequired => "请输入恢复词。",
        Key::AnchorMismatch => "指纹不一致。",
        Key::MissingSocket => "这个界面需要守护进程套接字。",
        Key::DaemonRefused => "守护进程拒绝了账户请求。",
        Key::Locked => "账户已锁定。请输入账户口令。",
        Key::Unlocked => "账户已解锁。",
        Key::Created => "账户已创建。",
        Key::Rotated => "账户口令已更改。",
        Key::Recovered => "账户已恢复。",
        Key::Cancelled => "已取消。",
    }
}
