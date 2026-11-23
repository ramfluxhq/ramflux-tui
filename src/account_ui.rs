// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Account creation, unlock, code rotation and recovery.
//! Switch and idle never open these screens. Explicit lock does.

use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use zeroize::{Zeroize, Zeroizing};

use super::TuiApp;
use crate::i18n::{self, Key, Locale};
use crate::{InputMode, TuiBus, TuiError, TuiInput};

const NAME_LIMIT: usize = 128;
const WORD_LIMIT: usize = 32;
const PHRASE_LIMIT: usize = 512;
const ANCHOR_LIMIT: usize = 256;

pub(super) struct AccountUi {
    mode: Mode,
}

impl Default for AccountUi {
    fn default() -> Self {
        Self { mode: Mode::Closed }
    }
}

impl AccountUi {
    pub(super) const fn is_open(&self) -> bool {
        !matches!(self.mode, Mode::Closed)
    }
}

enum Mode {
    Closed,
    Unlock { entry: Zeroizing<String> },
    Create(Box<Create>),
    Rotate(Rotate),
    Recover(Box<Recover>),
}

#[derive(Clone, Copy)]
enum CreateStep {
    Name,
    Phrase,
    Word,
    Code,
    Repeat,
}

struct Create {
    client: Option<ramflux_sdk::LocalBusCreationClient>,
    name: String,
    phrase: Option<Zeroizing<String>>,
    positions: [u8; 3],
    words: [Zeroizing<String>; 3],
    word_index: u8,
    code: Zeroizing<String>,
    draft: Zeroizing<String>,
    step: CreateStep,
}

impl Create {
    fn new() -> Self {
        Self {
            client: None,
            name: String::new(),
            phrase: None,
            positions: [0, 0, 0],
            words: std::array::from_fn(|_| Zeroizing::new(String::new())),
            word_index: 0,
            code: Zeroizing::new(String::new()),
            draft: Zeroizing::new(String::new()),
            step: CreateStep::Name,
        }
    }
}

#[derive(Clone, Copy)]
enum RotateStep {
    Old,
    New,
    Repeat,
}

struct Rotate {
    old: Zeroizing<String>,
    new_code: Zeroizing<String>,
    draft: Zeroizing<String>,
    step: RotateStep,
}

impl Rotate {
    fn new() -> Self {
        Self {
            old: Zeroizing::new(String::new()),
            new_code: Zeroizing::new(String::new()),
            draft: Zeroizing::new(String::new()),
            step: RotateStep::Old,
        }
    }
}

#[derive(Clone, Copy)]
enum RecoverStep {
    Anchor,
    Phrase,
    Code,
}

struct Recover {
    client: Option<ramflux_sdk::LocalBusRecoveryClient>,
    anchor: String,
    warning: String,
    typed: String,
    phrase: Zeroizing<String>,
    code: Zeroizing<String>,
    draft: Zeroizing<String>,
    step: RecoverStep,
}

impl TuiApp {
    pub(super) async fn handle_account_input<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
        input: TuiInput,
    ) -> Result<(), TuiError> {
        match input {
            TuiInput::ExitCompose => self.cancel_account().await,
            TuiInput::Backspace => self.account_backspace(),
            TuiInput::Char(value) => self.account_char(value),
            TuiInput::Enter => {
                if let Err(error) = self.account_enter(bus).await {
                    self.note_daemon(&error);
                }
            }
            TuiInput::Tab
            | TuiInput::Up
            | TuiInput::Down
            | TuiInput::EnterCompose
            | TuiInput::Quit => {}
        }
        Ok(())
    }

    pub(super) fn begin_create(&mut self) {
        if self.socket.is_none() {
            self.note(Key::MissingSocket);
            return;
        }
        self.open_account(Mode::Create(Box::new(Create::new())));
    }

    pub(super) fn begin_rotate(&mut self) {
        self.open_account(Mode::Rotate(Rotate::new()));
    }

    pub(super) async fn begin_recover(&mut self) -> Result<(), TuiError> {
        let Some(socket) = self.socket.clone() else {
            self.note(Key::MissingSocket);
            return Ok(());
        };
        let account = self.state.account_id.clone();
        let mut client = match ramflux_sdk::LocalBusRecoveryClient::connect(&socket).await {
            Ok(client) => client,
            Err(error) => {
                self.note_daemon(&error.into());
                return Ok(());
            }
        };
        let pending = match client.begin(&account).await {
            Ok(pending) => pending,
            Err(error) => {
                let _ = client.cancel().await;
                self.note_daemon(&error.into());
                return Ok(());
            }
        };
        self.open_account(Mode::Recover(Box::new(Recover {
            client: Some(client),
            anchor: pending.account_anchor,
            warning: pending.warning,
            typed: String::new(),
            phrase: Zeroizing::new(String::new()),
            code: Zeroizing::new(String::new()),
            draft: Zeroizing::new(String::new()),
            step: RecoverStep::Anchor,
        })));
        Ok(())
    }

    pub(super) async fn lock_account<B: TuiBus + Send>(
        &mut self,
        bus: &mut B,
    ) -> Result<(), TuiError> {
        let account = self.state.account_id.clone();
        if let Err(error) =
            bus.request(Some(account), "account", "account.lock", serde_json::json!({})).await
        {
            self.note_daemon(&error);
            return Ok(());
        }
        self.open_account(Mode::Unlock { entry: Zeroizing::new(String::new()) });
        self.note(Key::Locked);
        Ok(())
    }

    pub(super) fn render_account(&self, frame: &mut Frame<'_>, area: Rect) {
        let locale = self.state.locale;
        let title = self.account_title(locale);
        let mut body = self.account_body(locale);
        if let Some(status) = &self.state.status_message {
            let _ = write!(body, "\n{status}");
        }
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .block(Block::default().title(title).borders(Borders::ALL)),
            area,
        );
    }

    pub(super) fn render_account_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let locale = self.state.locale;
        let title = self.account_hint(locale);
        frame.render_widget(
            Paragraph::new(self.account_echo())
                .block(Block::default().title(title).borders(Borders::ALL)),
            area,
        );
    }

    async fn cancel_account(&mut self) {
        match std::mem::replace(&mut self.account_ui.mode, Mode::Closed) {
            Mode::Create(mut create) => {
                if let Some(client) = create.client.as_mut() {
                    let _ = client.cancel().await;
                }
            }
            Mode::Recover(mut recover) => {
                if let Some(client) = recover.client.as_mut() {
                    let _ = client.cancel().await;
                }
            }
            Mode::Closed | Mode::Unlock { .. } | Mode::Rotate(_) => {}
        }
        self.note(Key::Cancelled);
    }

    fn account_char(&mut self, value: char) {
        let outcome = match &mut self.account_ui.mode {
            Mode::Unlock { entry } => push_digit(entry, value),
            Mode::Create(create) => push_create(create, value),
            Mode::Rotate(rotate) => push_digit(&mut rotate.draft, value),
            Mode::Recover(recover) => push_recover(recover, value),
            Mode::Closed => None,
        };
        match outcome {
            Some(key) => self.note(key),
            None => self.state.status_message = None,
        }
    }

    fn account_backspace(&mut self) {
        match &mut self.account_ui.mode {
            Mode::Unlock { entry } => {
                entry.pop();
            }
            Mode::Create(create) => match create.step {
                CreateStep::Name => {
                    create.name.pop();
                }
                CreateStep::Phrase => {}
                CreateStep::Word | CreateStep::Code | CreateStep::Repeat => {
                    create.draft.pop();
                }
            },
            Mode::Rotate(rotate) => {
                rotate.draft.pop();
            }
            Mode::Recover(recover) => match recover.step {
                RecoverStep::Anchor => {
                    recover.typed.pop();
                }
                RecoverStep::Phrase | RecoverStep::Code => {
                    recover.draft.pop();
                }
            },
            Mode::Closed => {}
        }
        self.state.status_message = None;
    }

    async fn account_enter<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        match &self.account_ui.mode {
            Mode::Unlock { .. } => self.submit_unlock(bus).await,
            Mode::Create(_) => self.submit_create(bus).await,
            Mode::Rotate(_) => self.submit_rotate(bus).await,
            Mode::Recover(_) => self.submit_recover(bus).await,
            Mode::Closed => Ok(()),
        }
    }

    async fn submit_unlock<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let code = match &self.account_ui.mode {
            Mode::Unlock { entry } => entry.clone(),
            _ => return Ok(()),
        };
        if !six_digits(&code) {
            self.note(Key::NotSixDigits);
            return Ok(());
        }
        let account = self.state.account_id.clone();
        let result = bus
            .request(
                Some(account),
                "account",
                "account.unlock",
                serde_json::json!({ "passphrase": code.as_str() }),
            )
            .await;
        if let Mode::Unlock { entry } = &mut self.account_ui.mode {
            entry.zeroize();
        }
        result?;
        self.account_ui.mode = Mode::Closed;
        self.note(Key::Unlocked);
        if let Err(error) = self.refresh_all(bus).await {
            self.note_daemon(&error);
        }
        Ok(())
    }

    async fn submit_create<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let step = match &self.account_ui.mode {
            Mode::Create(create) => create.step,
            _ => return Ok(()),
        };
        match step {
            CreateStep::Name => self.submit_create_name().await,
            CreateStep::Phrase => self.hide_phrase().await,
            CreateStep::Word => {
                self.commit_word();
                Ok(())
            }
            CreateStep::Code => {
                self.commit_new_code();
                Ok(())
            }
            CreateStep::Repeat => self.confirm_create(bus).await,
        }
    }

    async fn submit_create_name(&mut self) -> Result<(), TuiError> {
        let Mode::Create(create) = &self.account_ui.mode else {
            return Ok(());
        };
        let name = create.name.trim().to_owned();
        if name.is_empty() {
            self.note(Key::NameRequired);
            return Ok(());
        }
        if name.len() > NAME_LIMIT {
            self.note(Key::NameTooLong);
            return Ok(());
        }
        let Some(socket) = self.socket.clone() else {
            self.note(Key::MissingSocket);
            self.account_ui.mode = Mode::Closed;
            return Ok(());
        };
        let mut client = ramflux_sdk::LocalBusCreationClient::connect(&socket).await?;
        if let Err(error) = client.begin(&name, ramflux_protocol::PrincipalKind::Human).await {
            let _ = client.cancel().await;
            return Err(error.into());
        }
        let phrase = match client.show_once().await {
            Ok(phrase) => phrase,
            Err(error) => {
                let _ = client.cancel().await;
                return Err(error.into());
            }
        };
        if let Mode::Create(create) = &mut self.account_ui.mode {
            create.name = name;
            create.client = Some(client);
            create.phrase = Some(phrase);
            create.step = CreateStep::Phrase;
        }
        self.state.status_message = None;
        Ok(())
    }

    async fn hide_phrase(&mut self) -> Result<(), TuiError> {
        let positions = {
            let Mode::Create(create) = &mut self.account_ui.mode else {
                return Ok(());
            };
            create.phrase = None;
            let Some(client) = create.client.as_mut() else {
                return Ok(());
            };
            client.hidden_challenge().await?
        };
        if let Mode::Create(create) = &mut self.account_ui.mode {
            create.positions = positions;
            create.word_index = 0;
            create.draft.zeroize();
            create.step = CreateStep::Word;
        }
        self.state.status_message = None;
        Ok(())
    }

    fn commit_word(&mut self) {
        let empty =
            matches!(&self.account_ui.mode, Mode::Create(create) if create.draft.is_empty());
        if empty {
            self.note(Key::WordRequired);
            return;
        }
        let Mode::Create(create) = &mut self.account_ui.mode else {
            return;
        };
        let word = std::mem::take(&mut create.draft);
        let index = usize::from(create.word_index);
        if let Some(slot) = create.words.get_mut(index) {
            *slot = word;
        }
        create.word_index = create.word_index.saturating_add(1);
        if create.word_index >= 3 {
            create.step = CreateStep::Code;
        }
        self.state.status_message = None;
    }

    fn commit_new_code(&mut self) {
        let admitted = match &self.account_ui.mode {
            Mode::Create(create) => admit_new(create.draft.as_str()),
            _ => return,
        };
        if let Err(key) = admitted {
            self.note(key);
            return;
        }
        let Mode::Create(create) = &mut self.account_ui.mode else {
            return;
        };
        create.code = std::mem::take(&mut create.draft);
        create.step = CreateStep::Repeat;
        self.state.status_message = None;
    }

    async fn confirm_create<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let compared = match &self.account_ui.mode {
            Mode::Create(create) => {
                if create.draft.as_str() == create.code.as_str() {
                    Some(admit_new(create.draft.as_str()))
                } else {
                    None
                }
            }
            _ => return Ok(()),
        };
        let Some(admitted) = compared else {
            self.clear_create_draft();
            self.note(Key::CodeMismatch);
            return Ok(());
        };
        if let Err(key) = admitted {
            self.note(key);
            return Ok(());
        }
        let mut create = match std::mem::replace(&mut self.account_ui.mode, Mode::Closed) {
            Mode::Create(create) => create,
            other => {
                self.account_ui.mode = other;
                return Ok(());
            }
        };
        let Some(mut client) = create.client.take() else {
            return Ok(());
        };
        let outcome = client.confirm(&create.words, &create.code).await;
        create.words.zeroize();
        create.code.zeroize();
        create.draft.zeroize();
        match outcome {
            Ok(created) => {
                self.state.account_id.clone_from(&created.account_id);
                self.note(Key::Created);
                if let Err(error) = self.refresh_all(bus).await {
                    self.note_daemon(&error);
                }
                Ok(())
            }
            Err(error) => {
                let _ = client.cancel().await;
                Err(error.into())
            }
        }
    }

    fn clear_create_draft(&mut self) {
        if let Mode::Create(create) = &mut self.account_ui.mode {
            create.draft.zeroize();
        }
    }

    async fn submit_rotate<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let step = match &self.account_ui.mode {
            Mode::Rotate(rotate) => rotate.step,
            _ => return Ok(()),
        };
        match step {
            RotateStep::Old => {
                let ok = matches!(&self.account_ui.mode, Mode::Rotate(rotate) if six_digits(&rotate.draft));
                if !ok {
                    self.note(Key::NotSixDigits);
                    return Ok(());
                }
                let Mode::Rotate(rotate) = &mut self.account_ui.mode else {
                    return Ok(());
                };
                rotate.old = std::mem::take(&mut rotate.draft);
                rotate.step = RotateStep::New;
                self.state.status_message = None;
                Ok(())
            }
            RotateStep::New => {
                let admitted = match &self.account_ui.mode {
                    Mode::Rotate(rotate) => admit_new(rotate.draft.as_str()),
                    _ => return Ok(()),
                };
                if let Err(key) = admitted {
                    self.note(key);
                    return Ok(());
                }
                let Mode::Rotate(rotate) = &mut self.account_ui.mode else {
                    return Ok(());
                };
                rotate.new_code = std::mem::take(&mut rotate.draft);
                rotate.step = RotateStep::Repeat;
                self.state.status_message = None;
                Ok(())
            }
            RotateStep::Repeat => self.confirm_rotate(bus).await,
        }
    }

    async fn confirm_rotate<B: TuiBus + Send>(&mut self, bus: &mut B) -> Result<(), TuiError> {
        let codes = match &self.account_ui.mode {
            Mode::Rotate(rotate) if rotate.draft.as_str() == rotate.new_code.as_str() => {
                Some((rotate.old.clone(), rotate.new_code.clone()))
            }
            Mode::Rotate(_) => None,
            _ => return Ok(()),
        };
        let Some((old_code, new_code)) = codes else {
            if let Mode::Rotate(rotate) = &mut self.account_ui.mode {
                rotate.draft.zeroize();
            }
            self.note(Key::CodeMismatch);
            return Ok(());
        };
        let account = self.state.account_id.clone();
        let result = bus
            .request(
                Some(account),
                "account",
                "account.passphrase.rotate",
                serde_json::json!({
                    "old_passphrase": old_code.as_str(),
                    "new_passphrase": new_code.as_str(),
                }),
            )
            .await;
        self.account_ui.mode = Mode::Closed;
        if let Err(error) = result {
            self.account_ui.mode = Mode::Rotate(Rotate::new());
            return Err(error);
        }
        self.note(Key::Rotated);
        Ok(())
    }

    async fn submit_recover<B: TuiBus + Send>(&mut self, _bus: &mut B) -> Result<(), TuiError> {
        let step = match &self.account_ui.mode {
            Mode::Recover(recover) => recover.step,
            _ => return Ok(()),
        };
        match step {
            RecoverStep::Anchor => {
                let matches_anchor = matches!(
                    &self.account_ui.mode,
                    Mode::Recover(recover) if recover.typed == recover.anchor
                );
                if !matches_anchor {
                    if let Mode::Recover(recover) = &mut self.account_ui.mode {
                        recover.typed.clear();
                    }
                    self.note(Key::AnchorMismatch);
                    return Ok(());
                }
                if let Mode::Recover(recover) = &mut self.account_ui.mode {
                    recover.step = RecoverStep::Phrase;
                    recover.draft.zeroize();
                }
                self.state.status_message = None;
                Ok(())
            }
            RecoverStep::Phrase => {
                let empty = matches!(&self.account_ui.mode, Mode::Recover(recover) if recover.draft.is_empty());
                if empty {
                    self.note(Key::WordRequired);
                    return Ok(());
                }
                if let Mode::Recover(recover) = &mut self.account_ui.mode {
                    recover.phrase = std::mem::take(&mut recover.draft);
                    recover.step = RecoverStep::Code;
                }
                self.state.status_message = None;
                Ok(())
            }
            RecoverStep::Code => self.confirm_recover().await,
        }
    }

    async fn confirm_recover(&mut self) -> Result<(), TuiError> {
        let ready =
            matches!(&self.account_ui.mode, Mode::Recover(recover) if six_digits(&recover.draft));
        if !ready {
            self.note(Key::NotSixDigits);
            return Ok(());
        }
        let mut recover = match std::mem::replace(&mut self.account_ui.mode, Mode::Closed) {
            Mode::Recover(recover) => recover,
            other => {
                self.account_ui.mode = other;
                return Ok(());
            }
        };
        let Some(mut client) = recover.client.take() else {
            return Ok(());
        };
        recover.code = std::mem::take(&mut recover.draft);
        let outcome = client.confirm(&recover.phrase, &recover.code, &recover.anchor).await;
        recover.phrase.zeroize();
        recover.code.zeroize();
        match outcome {
            Ok(created) => {
                self.state.account_id.clone_from(&created.account_id);
                self.note(Key::Recovered);
                Ok(())
            }
            Err(error) => {
                let _ = client.cancel().await;
                Err(error.into())
            }
        }
    }

    fn open_account(&mut self, mode: Mode) {
        self.cards.open = false;
        self.state.input.clear();
        self.state.input_mode = InputMode::Normal;
        self.state.status_message = None;
        self.account_ui.mode = mode;
    }

    fn note(&mut self, key: Key) {
        self.state.status_message = Some(i18n::text(self.state.locale, key).to_owned());
    }

    fn note_daemon(&mut self, error: &TuiError) {
        let raw = error.to_string();
        let key = if raw.contains("not_six_digits") {
            Key::NotSixDigits
        } else if raw.contains("trivial_code") {
            Key::TrivialCode
        } else {
            Key::DaemonRefused
        };
        self.note(key);
    }

    fn account_title(&self, locale: Locale) -> String {
        let key = match &self.account_ui.mode {
            Mode::Unlock { .. } | Mode::Closed => Key::UnlockTitle,
            Mode::Create(create) => match create.step {
                CreateStep::Name => Key::CreateNameTitle,
                CreateStep::Phrase => Key::CreatePhraseTitle,
                CreateStep::Word => {
                    let position =
                        create.positions.get(usize::from(create.word_index)).copied().unwrap_or(0);
                    return i18n::word_prompt(locale, position);
                }
                CreateStep::Code => Key::CreateCodeTitle,
                CreateStep::Repeat => Key::CreateRepeatTitle,
            },
            Mode::Rotate(rotate) => match rotate.step {
                RotateStep::Old => Key::RotateOldTitle,
                RotateStep::New => Key::RotateNewTitle,
                RotateStep::Repeat => Key::RotateRepeatTitle,
            },
            Mode::Recover(recover) => match recover.step {
                RecoverStep::Anchor => Key::RecoverAnchorTitle,
                RecoverStep::Phrase => Key::RecoverPhraseTitle,
                RecoverStep::Code => Key::RecoverCodeTitle,
            },
        };
        i18n::text(locale, key).to_owned()
    }

    fn account_body(&self, locale: Locale) -> String {
        match &self.account_ui.mode {
            Mode::Create(create) => create
                .phrase
                .as_ref()
                .map(|phrase| i18n::numbered_phrase(locale, phrase))
                .unwrap_or_default(),
            Mode::Recover(recover) if matches!(recover.step, RecoverStep::Anchor) => i18n::say(
                locale,
                Key::RecoverWarning,
                &[
                    ("label", i18n::Arg::Text(i18n::text(locale, Key::WarningLabel))),
                    ("warning", i18n::Arg::Text(recover.warning.as_str())),
                    ("anchor", i18n::Arg::Text(recover.anchor.as_str())),
                ],
            ),
            Mode::Unlock { .. } | Mode::Rotate(_) | Mode::Recover(_) | Mode::Closed => {
                String::new()
            }
        }
    }

    fn account_hint(&self, locale: Locale) -> &'static str {
        let hide = matches!(
            &self.account_ui.mode,
            Mode::Create(create) if matches!(create.step, CreateStep::Phrase)
        );
        i18n::text(locale, if hide { Key::HintHidePhrase } else { Key::HintConfirm })
    }

    fn account_echo(&self) -> String {
        match &self.account_ui.mode {
            Mode::Unlock { entry } => mask(entry.len()),
            Mode::Create(create) => match create.step {
                CreateStep::Name => create.name.clone(),
                CreateStep::Phrase => String::new(),
                CreateStep::Word | CreateStep::Code | CreateStep::Repeat => {
                    mask(create.draft.len())
                }
            },
            Mode::Rotate(rotate) => mask(rotate.draft.len()),
            Mode::Recover(recover) => match recover.step {
                RecoverStep::Anchor => recover.typed.clone(),
                RecoverStep::Phrase => mask_phrase(&recover.draft),
                RecoverStep::Code => mask(recover.draft.len()),
            },
            Mode::Closed => String::new(),
        }
    }
}

fn push_create(create: &mut Create, value: char) -> Option<Key> {
    match create.step {
        CreateStep::Name => push_name(&mut create.name, value),
        CreateStep::Phrase => None,
        CreateStep::Word => push_word(&mut create.draft, value),
        CreateStep::Code | CreateStep::Repeat => push_digit(&mut create.draft, value),
    }
}

fn push_recover(recover: &mut Recover, value: char) -> Option<Key> {
    match recover.step {
        RecoverStep::Anchor => {
            if push_visible(&mut recover.typed, value, ANCHOR_LIMIT) {
                None
            } else {
                Some(Key::NameTooLong)
            }
        }
        RecoverStep::Phrase => push_phrase(&mut recover.draft, value),
        RecoverStep::Code => push_digit(&mut recover.draft, value),
    }
}

fn push_name(target: &mut String, value: char) -> Option<Key> {
    if value.is_control() {
        return None;
    }
    if !push_visible(target, value, NAME_LIMIT) {
        return Some(Key::NameTooLong);
    }
    None
}

fn push_visible(target: &mut String, value: char, limit: usize) -> bool {
    if value.is_control() {
        return false;
    }
    if target.len().saturating_add(value.len_utf8()) > limit {
        return false;
    }
    target.push(value);
    true
}

fn push_word(draft: &mut Zeroizing<String>, value: char) -> Option<Key> {
    if !value.is_ascii_alphabetic() || draft.len() >= WORD_LIMIT {
        return Some(Key::WordRequired);
    }
    draft.push(value.to_ascii_lowercase());
    None
}

fn push_phrase(draft: &mut Zeroizing<String>, value: char) -> Option<Key> {
    if draft.len() >= PHRASE_LIMIT {
        return Some(Key::WordRequired);
    }
    if value.is_ascii_alphabetic() {
        draft.push(value.to_ascii_lowercase());
        return None;
    }
    if value == ' ' && !draft.is_empty() && !draft.ends_with(' ') {
        draft.push(' ');
        return None;
    }
    Some(Key::WordRequired)
}

fn push_digit(entry: &mut Zeroizing<String>, value: char) -> Option<Key> {
    if !value.is_ascii_digit() {
        return Some(Key::DigitsOnly);
    }
    if entry.len() >= ramflux_sdk::ACCOUNT_CODE_DIGITS {
        return Some(Key::NotSixDigits);
    }
    entry.push(value);
    None
}

fn six_digits(code: &str) -> bool {
    code.len() == ramflux_sdk::ACCOUNT_CODE_DIGITS && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn admit_new(code: &str) -> Result<(), Key> {
    ramflux_sdk::check_new_passphrase(code).map_err(|error| match error {
        ramflux_sdk::PassphrasePolicyError::TrivialCode => Key::TrivialCode,
        ramflux_sdk::PassphrasePolicyError::NotSixDigits
        | ramflux_sdk::PassphrasePolicyError::TooShort { .. }
        | ramflux_sdk::PassphrasePolicyError::TooCommon => Key::NotSixDigits,
    })
}

fn mask(len: usize) -> String {
    "*".repeat(len)
}

fn mask_phrase(value: &str) -> String {
    value.chars().map(|ch| if ch == ' ' { ' ' } else { '*' }).collect()
}
