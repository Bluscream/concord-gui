use std::collections::HashMap;

use crate::{
    config::TranslationOptions,
    discord::{
        AppCommand, MessageState, TranslationTarget,
        ids::{Id, marker::MessageMarker},
    },
};

use super::{
    DashboardState,
    composer::{ComposerInputSnapshot, ComposerTranslationPlan},
};

#[derive(Debug, Default)]
pub(super) struct TranslationUiState {
    options: TranslationOptions,
    entries: HashMap<Id<MessageMarker>, MessageTranslation>,
    composer_request: Option<ComposerTranslationRequest>,
    composer_draft: Option<ComposerTranslationDraft>,
    next_request_id: u64,
}

#[derive(Debug)]
struct ComposerTranslationRequest {
    request_id: u64,
    plan: ComposerTranslationPlan,
}

#[derive(Debug)]
struct ComposerTranslationDraft {
    original: ComposerInputSnapshot,
    translated: ComposerInputSnapshot,
    translated_source: String,
    active: ComposerTranslationBuffer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComposerTranslationBuffer {
    Original,
    Translated,
}

#[derive(Debug)]
struct MessageTranslation {
    source: String,
    visible: bool,
    status: MessageTranslationStatus,
}

#[derive(Debug)]
enum MessageTranslationStatus {
    Loading { request_id: u64 },
    Ready { text: String },
    Failed { message: String },
}

pub(in crate::tui) enum MessageTranslationDisplay<'a> {
    Loading,
    Ready(&'a str),
    Failed(&'a str),
}

impl TranslationUiState {
    pub(super) fn set_options(&mut self, options: TranslationOptions) {
        if self.options != options {
            self.entries.clear();
            self.composer_request = None;
            self.composer_draft = None;
        }
        self.options = options;
    }

    pub(super) const fn options(&self) -> &TranslationOptions {
        &self.options
    }

    fn configured_message_target(&self) -> Option<&str> {
        self.configured_target(self.options.message_target_language.as_deref())
    }

    fn configured_composer_target(&self) -> Option<&str> {
        self.configured_target(self.options.composer_target_language.as_deref())
    }

    fn configured_target<'a>(&self, target_language: Option<&'a str>) -> Option<&'a str> {
        self.options.provider?;
        let target_language = target_language
            .map(str::trim)
            .filter(|language| !language.is_empty())?;
        Some(target_language)
    }

    fn message_disabled_reason(&self) -> Option<String> {
        self.disabled_reason(
            self.options.message_target_language.as_deref(),
            "message_target_language",
        )
    }

    fn composer_disabled_reason(&self) -> Option<String> {
        self.disabled_reason(
            self.options.composer_target_language.as_deref(),
            "composer_target_language",
        )
    }

    fn disabled_reason(&self, target_language: Option<&str>, setting: &str) -> Option<String> {
        if self.options.provider.is_none() {
            return Some("translation provider not configured".to_owned());
        }
        target_language
            .map(str::trim)
            .filter(|language| !language.is_empty())
            .is_none()
            .then(|| format!("translation {setting} not configured"))
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.next_request_id
    }
}

impl DashboardState {
    pub(super) fn translation_disabled_reason(&self) -> Option<String> {
        self.translations.message_disabled_reason()
    }

    pub(super) fn toggle_selected_message_translation(&mut self) -> Option<AppCommand> {
        let message = self.selected_message_state()?;
        let message_id = message.id;
        let content = message.copyable_content()?;
        if content.trim().is_empty() {
            return None;
        }
        let target_language = self.translations.configured_message_target()?;
        let target_language = target_language.to_owned();

        if let Some(entry) = self.translations.entries.get_mut(&message_id)
            && entry.source == content
            && !matches!(entry.status, MessageTranslationStatus::Failed { .. })
        {
            entry.visible = !entry.visible;
            self.clear_message_row_content_metrics_cache();
            return None;
        }

        let request_id = self.translations.next_request_id();
        self.translations.entries.insert(
            message_id,
            MessageTranslation {
                source: content.clone(),
                visible: true,
                status: MessageTranslationStatus::Loading { request_id },
            },
        );
        self.clear_message_row_content_metrics_cache();
        Some(AppCommand::Translate {
            request_id,
            target: TranslationTarget::Message(message_id),
            target_language,
            content,
        })
    }

    pub(in crate::tui) fn translate_composer_input(&mut self) -> Option<AppCommand> {
        if self.toggle_composer_translation_to_original() {
            return None;
        }
        if let Some(reason) = self.translations.composer_disabled_reason() {
            self.show_error_toast(reason, std::time::Instant::now());
            return None;
        }

        let content = self.composer_input().to_owned();
        if content.trim().is_empty() {
            self.show_error_toast("composer is empty", std::time::Instant::now());
            return None;
        }
        if self.restore_cached_composer_translation(&content) {
            return None;
        }
        if self
            .translations
            .composer_request
            .as_ref()
            .is_some_and(|request| request.plan.source_text() == content)
        {
            return None;
        }

        let request_id = self.translations.next_request_id();
        let target_language = self
            .translations
            .configured_composer_target()
            .expect("composer translation target was validated");
        let target_language = target_language.to_owned();
        let original = self.composer_input_snapshot();
        let plan = match original.clone().into_translation_plan(request_id) {
            Ok(plan) => plan,
            Err(message) => {
                self.show_error_toast(message, std::time::Instant::now());
                return None;
            }
        };
        if let Some(draft) = self.translations.composer_draft.as_mut() {
            draft.original = original;
        }
        let provider_text = plan.provider_text().to_owned();
        self.translations.composer_request = Some(ComposerTranslationRequest { request_id, plan });
        Some(AppCommand::Translate {
            request_id,
            target: TranslationTarget::Composer,
            target_language,
            content: provider_text,
        })
    }

    pub(super) fn apply_translation_completed(
        &mut self,
        request_id: u64,
        target: TranslationTarget,
        translated_text: &str,
    ) {
        match target {
            TranslationTarget::Message(message_id) => {
                self.apply_message_translation_completed(request_id, message_id, translated_text);
            }
            TranslationTarget::Composer => {
                self.apply_composer_translation_completed(request_id, translated_text);
            }
        }
    }

    pub(super) fn apply_translation_failed(
        &mut self,
        request_id: u64,
        target: TranslationTarget,
        message: &str,
    ) {
        match target {
            TranslationTarget::Message(message_id) => {
                self.apply_message_translation_failed(request_id, message_id, message);
            }
            TranslationTarget::Composer => {
                self.apply_composer_translation_failed(request_id, message);
            }
        }
    }

    pub(super) fn cancel_composer_translation(&mut self) {
        self.cancel_pending_composer_translation_request();
        self.translations.composer_draft = None;
    }

    pub(super) fn cancel_pending_composer_translation_request(&mut self) {
        let Some(request) = self.translations.composer_request.take() else {
            return;
        };
        self.enqueue_pending_command(AppCommand::CancelComposerTranslation {
            request_id: request.request_id,
        });
    }

    pub(in crate::tui) fn composer_translation_original(&self) -> Option<&str> {
        self.translations
            .composer_draft
            .as_ref()
            .filter(|draft| draft.active == ComposerTranslationBuffer::Translated)
            .map(|draft| draft.original.value())
    }

    pub(in crate::tui) fn composer_translation_pending(&self) -> bool {
        self.is_composing()
            && self
                .translations
                .composer_request
                .as_ref()
                .is_some_and(|request| request.plan.source_text() == self.composer_input())
    }

    fn toggle_composer_translation_to_original(&mut self) -> bool {
        if !self
            .translations
            .composer_draft
            .as_ref()
            .is_some_and(|draft| draft.active == ComposerTranslationBuffer::Translated)
        {
            return false;
        }

        let current = self.composer_input_snapshot();
        let restore = {
            let draft = self
                .translations
                .composer_draft
                .as_mut()
                .expect("composer translation draft exists");
            draft.translated = current;
            draft.active = ComposerTranslationBuffer::Original;
            draft.original.clone()
        };
        self.restore_composer_input_snapshot(&restore);
        true
    }

    fn restore_cached_composer_translation(&mut self, source: &str) -> bool {
        if !self
            .translations
            .composer_draft
            .as_ref()
            .is_some_and(|draft| {
                draft.active == ComposerTranslationBuffer::Original
                    && draft.translated_source == source
            })
        {
            return false;
        }

        let current = self.composer_input_snapshot();
        let translated = {
            let draft = self
                .translations
                .composer_draft
                .as_mut()
                .expect("matching composer translation draft exists");
            draft.original = current;
            draft.active = ComposerTranslationBuffer::Translated;
            draft.translated.clone()
        };
        self.restore_composer_input_snapshot(&translated);
        true
    }

    fn apply_message_translation_completed(
        &mut self,
        request_id: u64,
        message_id: Id<MessageMarker>,
        translated_text: &str,
    ) {
        let Some(entry) = self.translations.entries.get_mut(&message_id) else {
            return;
        };
        if !matches!(
            entry.status,
            MessageTranslationStatus::Loading {
                request_id: active_request_id
            } if active_request_id == request_id
        ) {
            return;
        }
        entry.status = MessageTranslationStatus::Ready {
            text: translated_text.to_owned(),
        };
        self.clear_message_row_content_metrics_cache();
    }

    fn apply_message_translation_failed(
        &mut self,
        request_id: u64,
        message_id: Id<MessageMarker>,
        message: &str,
    ) {
        let Some(entry) = self.translations.entries.get_mut(&message_id) else {
            return;
        };
        if !matches!(
            entry.status,
            MessageTranslationStatus::Loading {
                request_id: active_request_id
            } if active_request_id == request_id
        ) {
            return;
        }
        entry.status = MessageTranslationStatus::Failed {
            message: message.to_owned(),
        };
        self.clear_message_row_content_metrics_cache();
    }

    fn apply_composer_translation_completed(&mut self, request_id: u64, translated_text: &str) {
        let Some(request) = self.translations.composer_request.as_ref() else {
            return;
        };
        if request.request_id != request_id {
            return;
        }
        let current = self.composer_input_snapshot();
        let request = self
            .translations
            .composer_request
            .take()
            .expect("matching composer translation request exists");
        if !self.is_composing() || !request.plan.matches_source(&current) {
            return;
        }

        let translated = match request.plan.translated_snapshot(translated_text) {
            Ok(translated) => translated,
            Err(message) => {
                self.show_error_toast(
                    format!("Composer translation failed: {message}"),
                    std::time::Instant::now(),
                );
                return;
            }
        };
        let translated_source = request.plan.source_text().to_owned();
        self.translations.composer_draft = Some(ComposerTranslationDraft {
            original: current,
            translated: translated.clone(),
            translated_source,
            active: ComposerTranslationBuffer::Translated,
        });
        self.restore_composer_input_snapshot(&translated);
        self.show_success_toast("Composer translated", std::time::Instant::now());
    }

    fn apply_composer_translation_failed(&mut self, request_id: u64, message: &str) {
        let Some(request) = self.translations.composer_request.as_ref() else {
            return;
        };
        if request.request_id != request_id {
            return;
        }
        let should_report =
            self.is_composing() && self.composer_input() == request.plan.source_text();
        self.translations.composer_request = None;
        if should_report {
            self.show_error_toast(
                format!("Composer translation failed: {message}"),
                std::time::Instant::now(),
            );
        }
    }

    pub(in crate::tui) fn message_translation_display(
        &self,
        message: &MessageState,
    ) -> Option<MessageTranslationDisplay<'_>> {
        let entry = self.translations.entries.get(&message.id)?;
        if !entry.visible || message.copyable_content().as_deref() != Some(entry.source.as_str()) {
            return None;
        }

        Some(match &entry.status {
            MessageTranslationStatus::Loading { .. } => MessageTranslationDisplay::Loading,
            MessageTranslationStatus::Ready { text } => {
                MessageTranslationDisplay::Ready(text.as_str())
            }
            MessageTranslationStatus::Failed { message } => {
                MessageTranslationDisplay::Failed(message.as_str())
            }
        })
    }
}
