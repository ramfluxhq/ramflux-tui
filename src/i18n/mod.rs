// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! English and Simplified Chinese catalogs.
//!
//! A typed key keeps the two catalogs aligned. Templates, plurals, numbers and
//! dates are filled here. Call sites pass structured values and do not build
//! sentences themselves. No extra crate: a new dependency needs a separate
//! approval, and the account screens already used this shape.

mod en;
mod key;
mod zh;

use std::borrow::Cow;
use std::cell::Cell;

pub(crate) use key::Key;

/// English product version line, including the build commit.
pub const VERSION: &str =
    concat!(env!("CARGO_PKG_VERSION"), " (build ", env!("RAMFLUX_TUI_BUILD"), ")");

/// Language selected for the interface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    En,
    ZhCn,
}

/// Plural category for integers. English uses one/other. Chinese uses other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Category {
    One,
    Other,
}

/// A value substituted into a catalog template.
#[derive(Clone, Copy)]
pub(crate) enum Arg<'a> {
    Text(&'a str),
    Count(u64),
    Int(i64),
    Secs(i64),
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

thread_local! {
    static ACTIVE: Cell<Locale> = const { Cell::new(Locale::En) };
}

/// Restores the parse locale when dropped. Hold it only across synchronous code.
#[must_use]
pub(crate) struct LocaleGuard(Locale);

impl LocaleGuard {
    /// Sets the locale used by parsers that cannot take a parameter on every helper.
    pub(crate) fn enter(locale: Locale) -> Self {
        let previous = ACTIVE.with(|cell| cell.replace(locale));
        Self(previous)
    }
}

impl Drop for LocaleGuard {
    fn drop(&mut self) {
        let previous = self.0;
        ACTIVE.with(|cell| cell.set(previous));
    }
}

pub(crate) fn active() -> Locale {
    ACTIVE.with(Cell::get)
}

/// Reads an explicit language tag. Unknown values stay English.
#[must_use]
pub fn locale_from_env(value: &str) -> Locale {
    interpret(value).unwrap_or(Locale::En)
}

/// Resolves the interface language.
///
/// An explicit tag (`--lang`, then `RAMFLUX_TUI_LANG`) wins. Otherwise the
/// first set value among `LC_ALL`, `LC_MESSAGES` and `LANG` is used. Anything
/// this client does not translate, including Traditional Chinese, is English.
#[must_use]
pub fn resolve_locale(cli: Option<&str>) -> Locale {
    resolve_with(cli, |name| std::env::var(name).ok())
}

fn resolve_with(cli: Option<&str>, mut env: impl FnMut(&str) -> Option<String>) -> Locale {
    if let Some(value) = cli.map(str::trim).filter(|value| !value.is_empty()) {
        return locale_from_env(value);
    }
    if let Some(value) = env("RAMFLUX_TUI_LANG") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return locale_from_env(trimmed);
        }
    }
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = env(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return interpret(trimmed).unwrap_or(Locale::En);
            }
        }
    }
    Locale::En
}

fn interpret(value: &str) -> Option<Locale> {
    let head = value.trim().split(['.', '@']).next().unwrap_or("").trim();
    if head.is_empty() || head.eq_ignore_ascii_case("c") || head.eq_ignore_ascii_case("posix") {
        return None;
    }
    let tag = head.replace('_', "-");
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or("").to_ascii_lowercase();
    let qualifiers = parts.map(str::to_ascii_lowercase).collect::<Vec<_>>();
    match language.as_str() {
        "en" => Some(Locale::En),
        "zh" if qualifiers
            .iter()
            .any(|part| matches!(part.as_str(), "tw" | "hk" | "mo" | "hant")) =>
        {
            None
        }
        "zh" => Some(Locale::ZhCn),
        _ => None,
    }
}

/// One static catalog sentence. The key must not contain placeholders.
#[must_use]
pub(crate) fn text(locale: Locale, key: Key) -> &'static str {
    if !key::ALL.contains(&key) {
        return "";
    }
    template(locale, key, Category::Other)
}

/// Fills one catalog template. `{name}` is replaced from `fields`.
#[must_use]
pub(crate) fn say(locale: Locale, key: Key, fields: &[(&'static str, Arg<'_>)]) -> String {
    let category = fields
        .iter()
        .find_map(|(name, arg)| (*name == "count").then(|| category_of(locale, arg)))
        .unwrap_or(Category::Other);
    fill(locale, template(locale, key, category), fields)
}

/// Recovery-word prompt. The position is a grouped number.
#[must_use]
pub(crate) fn word_prompt(locale: Locale, position: u8) -> String {
    say(locale, Key::WordPrompt, &[("position", Arg::Count(u64::from(position)))])
}

/// Numbered recovery phrase. Each line keeps a two-column index.
#[must_use]
pub(crate) fn numbered_phrase(locale: Locale, phrase: &str) -> String {
    let mut out = String::new();
    for (index, word) in phrase.split_whitespace().enumerate() {
        let position = pad_position(locale, index.saturating_add(1));
        let line = say(
            locale,
            Key::PhraseLine,
            &[("position", Arg::Text(position.as_str())), ("word", Arg::Text(word))],
        );
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Known interface tokens stay stable in storage and are translated at render.
#[must_use]
pub(crate) fn token(locale: Locale, value: &str) -> Cow<'_, str> {
    let key = match value {
        "me" => Key::SenderMe,
        "sending" => Key::StatusSending,
        "synced" => Key::StatusSynced,
        "archived" => Key::StatusArchived,
        "referenced" => Key::StatusReferenced,
        "pending" => Key::StatusPending,
        "unknown" => Key::StatusUnknown,
        "unverified" => Key::StatusUnverified,
        "verified" => Key::StatusVerified,
        "deleted" => Key::StatusDeleted,
        "delivered" => Key::StatusDelivered,
        "read" => Key::StatusRead,
        "content" => Key::StatusContent,
        "unsupported" => Key::StatusUnsupported,
        "tombstone" => Key::StatusTombstone,
        "in_progress" => Key::TransferInProgress,
        "complete" => Key::TransferComplete,
        "upload" => Key::DirectionUpload,
        "download" => Key::DirectionDownload,
        "accepted" => Key::StatusAccepted,
        _ => return Cow::Borrowed(value),
    };
    Cow::Borrowed(text(locale, key))
}

/// Groups an integer with the locale's CLDR grouping separator.
#[must_use]
pub(crate) fn format_u64(locale: Locale, value: u64) -> String {
    group_digits(&value.to_string(), separator(locale))
}

/// Groups a signed integer. `i64::MIN` stays decimal.
#[must_use]
pub(crate) fn format_i64(locale: Locale, value: i64) -> String {
    let mut out = String::new();
    if value < 0 {
        out.push('-');
    }
    out.push_str(&group_digits(&value.unsigned_abs().to_string(), separator(locale)));
    out
}

/// Formats a UTC instant. English uses an abbreviated month. Chinese uses numeric fields.
#[must_use]
pub(crate) fn format_datetime(locale: Locale, unix_secs: i64) -> String {
    let Some((year, month, day, hour, minute, second)) = civil_utc(unix_secs) else {
        return format_i64(locale, unix_secs);
    };
    let hour = pad2(hour);
    let minute = pad2(minute);
    let second = pad2(second);
    match locale {
        Locale::En => {
            let name = month_name(month);
            format!("{name} {day}, {year}, {hour}:{minute}:{second} UTC")
        }
        Locale::ZhCn => {
            format!("{year}年{month}月{day}日 {hour}:{minute}:{second} UTC")
        }
    }
}

/// English plural category for an integer count. Chinese is always `other`.
#[must_use]
pub(crate) fn cardinal(locale: Locale, count: u64) -> Category {
    match locale {
        Locale::En if count == 1 => Category::One,
        Locale::En | Locale::ZhCn => Category::Other,
    }
}

/// Command description for the current language.
#[must_use]
pub fn about(locale: Locale) -> &'static str {
    text(locale, Key::About)
}

/// Help line for the `--lang` setting.
#[must_use]
pub fn lang_help(locale: Locale) -> &'static str {
    text(locale, Key::LangHelp)
}

/// Version line for the current language.
#[must_use]
pub fn version_line(locale: Locale) -> String {
    say(
        locale,
        Key::Version,
        &[
            ("version", Arg::Text(env!("CARGO_PKG_VERSION"))),
            ("build", Arg::Text(env!("RAMFLUX_TUI_BUILD"))),
        ],
    )
}

fn template(locale: Locale, key: Key, category: Category) -> &'static str {
    match locale {
        Locale::En => en::template(key, category),
        Locale::ZhCn => zh::template(key, category),
    }
}

fn category_of(locale: Locale, arg: &Arg<'_>) -> Category {
    match arg {
        Arg::Count(count) => cardinal(locale, *count),
        Arg::Int(value) if *value >= 0 => {
            cardinal(locale, u64::try_from(*value).unwrap_or(u64::MAX))
        }
        Arg::Text(_) | Arg::Int(_) | Arg::Secs(_) => Category::Other,
    }
}

fn fill(locale: Locale, template: &str, fields: &[(&str, Arg<'_>)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push('{');
            out.push_str(after);
            return out;
        };
        let name = &after[..end];
        if let Some((_, arg)) = fields.iter().find(|(field, _)| *field == name) {
            out.push_str(&present(locale, arg));
        } else {
            out.push('{');
            out.push_str(name);
            out.push('}');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn present(locale: Locale, arg: &Arg<'_>) -> String {
    match arg {
        Arg::Text(text) => (*text).to_owned(),
        Arg::Count(count) => format_u64(locale, *count),
        Arg::Int(value) => format_i64(locale, *value),
        Arg::Secs(secs) => format_datetime(locale, *secs),
    }
}

fn pad_position(locale: Locale, position: usize) -> String {
    let formatted = format_u64(locale, u64::try_from(position).unwrap_or(u64::MAX));
    if formatted.chars().count() >= 2 {
        formatted
    } else {
        let mut padded = String::from(" ");
        padded.push_str(&formatted);
        padded
    }
}

fn separator(locale: Locale) -> char {
    match locale {
        Locale::En | Locale::ZhCn => ',',
    }
}

fn group_digits(digits: &str, sep: char) -> String {
    let chars = digits.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        let remaining = chars.len() - index;
        if index > 0 && remaining.is_multiple_of(3) {
            out.push(sep);
        }
        out.push(*ch);
    }
    out
}

fn pad2(value: u32) -> String {
    let text = value.to_string();
    if text.len() >= 2 {
        text
    } else {
        let mut padded = String::from("0");
        padded.push_str(&text);
        padded
    }
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "?",
    }
}

fn civil_utc(seconds: i64) -> Option<(i64, u32, u32, u32, u32, u32)> {
    if seconds < 0 {
        return None;
    }
    let seconds = u64::try_from(seconds).ok()?;
    let days = i64::try_from(seconds / 86_400).ok()? + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096)
        .div_euclid(365);
    let mut year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let month_prime = (5 * day_of_year + 2).div_euclid(153);
    let day = u32::try_from(day_of_year - (153 * month_prime + 2).div_euclid(5) + 1).ok()?;
    let month_number = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month_number <= 2);
    let month = u32::try_from(month_number).ok()?;
    let hour = u32::try_from((seconds / 3600) % 24).ok()?;
    let minute = u32::try_from((seconds / 60) % 60).ok()?;
    let second = u32::try_from(seconds % 60).ok()?;
    Some((year, month, day, hour, minute, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_both_catalogs() {
        for key in key::ALL {
            let english = en::template(*key, Category::Other);
            let chinese = zh::template(*key, Category::Other);
            assert!(!english.is_empty(), "missing English for {key:?}");
            assert!(!chinese.is_empty(), "missing Chinese for {key:?}");
            let english_one = en::template(*key, Category::One);
            let chinese_one = zh::template(*key, Category::One);
            assert!(!english_one.is_empty(), "missing English one-form for {key:?}");
            assert!(!chinese_one.is_empty(), "missing Chinese one-form for {key:?}");
        }
    }

    #[test]
    fn english_plural_selects_one_only_for_a_single_item() {
        assert_eq!(cardinal(Locale::En, 1), Category::One);
        assert_eq!(cardinal(Locale::En, 0), Category::Other);
        assert_eq!(cardinal(Locale::En, 2), Category::Other);
        assert_eq!(cardinal(Locale::ZhCn, 1), Category::Other);
        let one = say(Locale::En, Key::FirstCards, &[("count", Arg::Count(1))]);
        let many = say(Locale::En, Key::FirstCards, &[("count", Arg::Count(100))]);
        assert_eq!(one, "Showing the first 1 card in this conversation");
        assert_eq!(many, "Showing the first 100 cards in this conversation");
        let chinese = say(Locale::ZhCn, Key::FirstCards, &[("count", Arg::Count(100))]);
        assert!(chinese.contains("100"));
        assert_ne!(chinese, many);
    }

    #[test]
    fn numbers_dates_and_prompts_use_the_catalog() {
        assert_eq!(format_u64(Locale::En, 1024), "1,024");
        assert_eq!(format_u64(Locale::ZhCn, 1024), "1,024");
        assert_eq!(format_i64(Locale::En, -1200), "-1,200");
        assert_eq!(format_datetime(Locale::En, 0), "Jan 1, 1970, 00:00:00 UTC");
        assert_eq!(format_datetime(Locale::ZhCn, 0), "1970年1月1日 00:00:00 UTC");
        assert_eq!(word_prompt(Locale::En, 1), "Recovery word 1");
        assert_eq!(word_prompt(Locale::ZhCn, 12), "恢复词 12");
        assert_eq!(numbered_phrase(Locale::En, "alpha beta"), " 1. alpha\n 2. beta\n");
        assert_eq!(version_line(Locale::En), VERSION);
    }

    #[test]
    fn explicit_setting_overrides_the_operating_system_locale() {
        let env = |name: &str| (name == "LANG").then(|| "zh_CN.UTF-8".to_owned());
        assert_eq!(resolve_with(None, env), Locale::ZhCn);
        assert_eq!(resolve_with(Some("en"), env), Locale::En);
        assert_eq!(resolve_with(Some("zh-TW"), env), Locale::En);
        let traditional = |name: &str| match name {
            "LC_ALL" => Some("zh_TW.UTF-8".to_owned()),
            "LANG" => Some("zh_CN.UTF-8".to_owned()),
            _ => None,
        };
        assert_eq!(resolve_with(None, traditional), Locale::En);
        let english = |name: &str| (name == "LANG").then(|| "en_US.UTF-8".to_owned());
        assert_eq!(resolve_with(None, english), Locale::En);
        let forced = |name: &str| match name {
            "RAMFLUX_TUI_LANG" => Some("zh-CN".to_owned()),
            "LANG" => Some("en_US.UTF-8".to_owned()),
            _ => None,
        };
        assert_eq!(resolve_with(None, forced), Locale::ZhCn);
    }

    #[test]
    fn hard_coded_interface_strings_fail_the_guard() -> Result<(), String> {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output = std::process::Command::new("python3")
            .arg("-B")
            .arg(manifest.join("scripts/check-ui-strings.py"))
            .current_dir(&manifest)
            .output()
            .map_err(|error| format!("test: python3 did not start: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            Ok(())
        } else {
            Err(format!("hard-coded user-facing strings:\n{stdout}{stderr}"))
        }
    }
}
