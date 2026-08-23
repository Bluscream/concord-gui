use crate::tui::keybindings::{OptionsCategoryShortcut, SelectionAction};
use concord::discord::AppCommand;
use concord::discord::{MicrophoneSensitivityDb, VoiceAudioSourceOptions, VoiceVolumePercent};

use super::super::{DashboardState, DisplayOptionGauge, DisplayOptionItem};
use super::{
    ActiveModalPopupKind, ModalPopup, OptionsCategory, OptionsPopupState, SelectablePopupTarget,
};

const DISPLAY_OPTION_COUNT: usize = 9;
const COMPOSER_OPTION_COUNT: usize = 1;
const NOTIFICATION_OPTION_COUNT: usize = 1;
const VOICE_OPTION_COUNT: usize = VoiceOption::ALL.len();
const OPTION_CATEGORY_COUNT: usize = 5;

/// A row of the voice options popup.
///
/// `ALL` is the single source of on-screen order: the row list is built by
/// mapping over it, and every selection index is resolved through it. Adding or
/// moving a row is therefore a change to `ALL` alone, with no match arms to
/// renumber and no row count to keep in sync by hand.
#[derive(Clone, Copy, Eq, PartialEq)]
enum VoiceOption {
    Muted,
    Deafened,
    InputSource,
    OutputSource,
    AllowMicrophoneTransmit,
    PushToTalk,
    PushToTalkShortcut,
    NoiseSuppression,
    MicrophoneSensitivity,
    MicrophoneVolume,
    VoiceVolume,
}

impl VoiceOption {
    const ALL: [Self; 11] = [
        Self::Muted,
        Self::Deafened,
        Self::InputSource,
        Self::OutputSource,
        Self::AllowMicrophoneTransmit,
        Self::PushToTalk,
        Self::PushToTalkShortcut,
        Self::NoiseSuppression,
        Self::MicrophoneSensitivity,
        Self::MicrophoneVolume,
        Self::VoiceVolume,
    ];

    fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

impl DashboardState {
    #[cfg(test)]
    pub fn open_options_popup(&mut self) {
        self.open_options_category(OptionsCategory::Display);
    }

    pub fn open_options_category_picker(&mut self) {
        self.popups
            .set_modal(ModalPopup::Options(OptionsPopupState::default()));
    }

    pub fn open_options_category(&mut self, category: OptionsCategory) {
        if category == OptionsCategory::Voice {
            self.options.next_voice_audio_sources_request_id = self
                .options
                .next_voice_audio_sources_request_id
                .wrapping_add(1)
                .max(1);
            let request_id = self.options.next_voice_audio_sources_request_id;
            self.options.voice_audio_sources_request_id = Some(request_id);
            self.enqueue_pending_command(AppCommand::LoadVoiceAudioSources { request_id });
        }
        if category == OptionsCategory::Connections {
            self.enqueue_pending_command(AppCommand::LoadConnections);
        }
        if category == OptionsCategory::Access {
            // Both lists, because the panel shows both and fetching one on
            // demand would mean a tab that is empty until it is touched.
            self.enqueue_pending_command(AppCommand::LoadAuthSessions);
            self.enqueue_pending_command(AppCommand::LoadAuthorisedApps);
        }
        self.popups
            .set_modal(ModalPopup::Options(OptionsPopupState {
                category: Some(category),
                connections_loading: category == OptionsCategory::Connections,
                access_loading: category == OptionsCategory::Access,
                account_form: concord::discord::AccountForm::new(
                    self.current_user().unwrap_or_default(),
                    "",
                ),
                ..OptionsPopupState::default()
            }));
    }

    pub fn close_options_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::Options) {
            self.popups.clear_modal();
        }
    }

    pub fn move_option_down(&mut self) {
        self.move_selectable_popup(SelectablePopupTarget::Options, SelectionAction::Next);
    }

    pub fn move_option_up(&mut self) {
        self.move_selectable_popup(SelectablePopupTarget::Options, SelectionAction::Previous);
    }

    pub fn selected_option_index(&self) -> Option<usize> {
        self.popups.options_popup().map(|popup| {
            popup
                .selection
                .selected_for_len(self.options_popup_item_count())
        })
    }

    pub fn options_popup_title(&self) -> &'static str {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None => "Options",
            Some(OptionsCategory::Display) => "Display Options",
            Some(OptionsCategory::Composer) => "Composer Options",
            Some(OptionsCategory::Notifications) => "Notification Options",
            Some(OptionsCategory::Voice) => "Voice Options",
            Some(OptionsCategory::Connections) => "Linked Accounts",
            Some(OptionsCategory::Privacy) => "Privacy and Safety",
            Some(OptionsCategory::Access) => "Sessions and Apps",
            Some(OptionsCategory::Account) => "Account Settings",
        }
    }

    pub fn is_options_category_picker_open(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| popup.category.is_none())
    }

    pub(in crate::tui) fn is_capturing_push_to_talk_shortcut(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| popup.capturing_push_to_talk_shortcut)
    }

    pub(in crate::tui) fn cancel_push_to_talk_shortcut_capture(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.capturing_push_to_talk_shortcut = false;
        }
    }

    pub(in crate::tui) fn capture_push_to_talk_shortcut(&mut self, shortcut: String) {
        self.cancel_push_to_talk_shortcut_capture();
        if self.options.voice_options.push_to_talk_shortcut == shortcut {
            return;
        }
        self.options.voice_options.push_to_talk_shortcut = shortcut;
        self.mark_options_changed();
    }

    pub(super) fn options_popup_item_count(&self) -> usize {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None => OPTION_CATEGORY_COUNT,
            Some(OptionsCategory::Display) => DISPLAY_OPTION_COUNT,
            Some(OptionsCategory::Composer) => COMPOSER_OPTION_COUNT,
            Some(OptionsCategory::Notifications) => NOTIFICATION_OPTION_COUNT,
            Some(OptionsCategory::Voice) => VOICE_OPTION_COUNT,
            // At least one, so an empty list still has a row to explain
            // itself rather than rendering as a blank panel.
            Some(OptionsCategory::Connections) => self.connection_rows().len().max(1),
            Some(OptionsCategory::Privacy) => concord::discord::PrivacySetting::ALL.len(),
            // At least one, so an empty account still has a row to explain
            // itself rather than rendering as a blank panel.
            Some(OptionsCategory::Access) => self.access_row_count().max(1),
            // The form's fields, then the two-factor row under them.
            Some(OptionsCategory::Account) => concord::discord::AccountField::ALL.len() + 1,
        }
    }

    pub fn display_option_items(&self) -> Vec<DisplayOptionItem> {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None if self.is_active_modal_popup(ActiveModalPopupKind::Options) => {
                return self.option_category_items();
            }
            Some(OptionsCategory::Connections) => return self.connection_option_items(),
            Some(OptionsCategory::Privacy) => return self.privacy_option_items(),
            Some(OptionsCategory::Access) => return self.access_option_items(),
            Some(OptionsCategory::Account) => return self.account_option_items(),
            Some(OptionsCategory::Display) => return self.display_option_items_for_display(),
            Some(OptionsCategory::Composer) => return self.display_option_items_for_composer(),
            Some(OptionsCategory::Notifications) => {
                return self.display_option_items_for_notifications();
            }
            Some(OptionsCategory::Voice) => return self.display_option_items_for_voice(),
            None => {}
        }

        let mut items = self.display_option_items_for_display();
        items.extend(self.display_option_items_for_composer());
        items.extend(self.display_option_items_for_notifications());
        items.extend(self.display_option_items_for_voice());
        items
    }

    fn option_category_items(&self) -> Vec<DisplayOptionItem> {
        vec![
            DisplayOptionItem {
                label: "Display",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Display.key().to_string()),
                gauge: None,
                effective: true,
                description: "Image, custom emoji, and pane display settings.",
            },
            DisplayOptionItem {
                label: "Composer",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Composer.key().to_string()),
                gauge: None,
                effective: true,
                description: "Message input and send-format settings.",
            },
            DisplayOptionItem {
                label: "Notifications",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Notifications.key().to_string()),
                gauge: None,
                effective: true,
                description: "Desktop notification settings.",
            },
            DisplayOptionItem {
                label: "Voice",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Voice.key().to_string()),
                gauge: None,
                effective: true,
                description: "Mute, deaf, push-to-talk, microphone processing, and volume settings.",
            },
            DisplayOptionItem {
                label: "Linked accounts",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Connections.key().to_string()),
                gauge: None,
                effective: true,
                description: "Accounts linked to your profile, what they show, and unlinking.",
            },
            DisplayOptionItem {
                label: "Privacy and safety",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Privacy.key().to_string()),
                gauge: None,
                effective: true,
                description: "Direct-message scanning and who may send you a friend request.",
            },
            DisplayOptionItem {
                label: "Sessions and apps",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Access.key().to_string()),
                gauge: None,
                effective: true,
                description: "What else is signed in to this account, and which apps have access.",
            },
            DisplayOptionItem {
                label: "Account",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Account.key().to_string()),
                gauge: None,
                effective: true,
                description: "Username, email, password and two-factor authentication.",
            },
        ]
    }

    /// The linked accounts, as loaded.
    fn connection_rows(&self) -> &[concord::discord::Connection] {
        self.popups
            .options_popup()
            .map_or(&[], |popup| popup.connections.as_slice())
    }

    /// Sessions first, then apps: one list, because two panels to hunt through
    /// is the last thing wanted by someone checking after a scare.
    fn access_row_count(&self) -> usize {
        self.popups
            .options_popup()
            .map_or(0, |popup| popup.sessions.len() + popup.apps.len())
    }

    fn access_option_items(&self) -> Vec<DisplayOptionItem> {
        let Some(popup) = self.popups.options_popup() else {
            return Vec::new();
        };
        if popup.sessions.is_empty() && popup.apps.is_empty() {
            return vec![DisplayOptionItem {
                label: if popup.access_loading {
                    "Loading"
                } else {
                    "Nothing else has access"
                },
                enabled: false,
                value: None,
                gauge: None,
                effective: false,
                description: "No other sessions and no authorised applications.",
            }];
        }

        let sessions = popup.sessions.iter().map(|session| DisplayOptionItem {
            label: "Session",
            // Selected for logout, not "active" - every listed session is
            // active, so a tick meaning that would say nothing.
            enabled: popup.session_logout_targets.contains(&session.id_hash),
            value: Some(format!(
                "{} - {}",
                if session.platform.is_empty() {
                    "Unknown platform"
                } else {
                    &session.platform
                },
                session.summary()
            )),
            gauge: None,
            effective: !session.current,
            description: "enter selects it for logout; L logs the selected ones out.",
        });

        let apps = popup.apps.iter().map(|app| DisplayOptionItem {
            label: "Application",
            enabled: false,
            value: Some(format!("{} - {}", app.name, app.summary())),
            gauge: None,
            effective: true,
            description: "r revokes this application's access.",
        });

        sessions.chain(apps).collect()
    }

    /// Mark or unmark the highlighted session for logout.
    ///
    /// Selecting rather than logging out immediately: Discord needs the account
    /// password for this, so it takes a prompt either way, and one prompt for
    /// several sessions beats one per session.
    fn toggle_selected_session_target(&mut self) {
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let Some(session) = popup.sessions.get(index) else {
            return;
        };
        let id_hash = session.id_hash.clone();
        if !popup.session_logout_targets.remove(&id_hash) {
            popup.session_logout_targets.insert(id_hash);
        }
    }

    /// Revoke the highlighted application's access.
    ///
    /// Immediate, unlike a session logout: Discord asks for no password here,
    /// so there is no prompt to batch behind.
    pub fn revoke_selected_authorised_app(&mut self) {
        if !self.is_access_category_open() {
            return;
        }
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        // Apps are listed after sessions, so the row index is offset by them.
        let Some(app_index) = index.checked_sub(popup.sessions.len()) else {
            return;
        };
        if app_index >= popup.apps.len() {
            return;
        }
        let app = popup.apps.remove(app_index);
        self.enqueue_pending_command(AppCommand::RevokeAuthorisedApp {
            id: app.id,
            label: app.name,
        });
    }

    pub fn is_access_category_open(&self) -> bool {
        self.popups.options_popup().and_then(|popup| popup.category)
            == Some(OptionsCategory::Access)
    }

    /// Whether any session is selected, which is what makes a logout possible.
    pub fn has_session_logout_targets(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| !popup.session_logout_targets.is_empty())
    }

    /// Ask for the password that Discord requires to log a session out.
    ///
    /// A prompt rather than anything remembered: the client has nowhere to
    /// keep a password and no reason to, so it is typed each time.
    pub fn start_session_logout(&mut self) {
        if !self.is_access_category_open() || !self.has_session_logout_targets() {
            return;
        }
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.access_password = Some(crate::tui::text_input::TextInputState::masked());
        }
    }

    pub fn is_session_password_prompt_open(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| popup.access_password.is_some())
    }

    pub fn edit_session_password(&mut self, action: crate::tui::text_input::TextEditAction) {
        if let Some(popup) = self.popups.options_popup_mut()
            && let Some(input) = popup.access_password.as_mut()
        {
            input.apply_edit_action(action);
        }
    }

    pub fn insert_session_password_char(&mut self, value: char) {
        if let Some(popup) = self.popups.options_popup_mut()
            && let Some(input) = popup.access_password.as_mut()
        {
            input.insert_char(value);
        }
    }

    pub fn session_password_display(&self) -> Option<String> {
        self.popups
            .options_popup()
            .and_then(|popup| popup.access_password.as_ref())
            .map(crate::tui::text_input::TextInputState::display_value)
    }

    pub fn cancel_session_logout(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.access_password = None;
        }
    }

    /// Take what was typed and send the logout.
    pub fn confirm_session_logout(&mut self) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let Some(input) = popup.access_password.take() else {
            return;
        };
        // Empty is not sent: Discord would reject it, and the round trip would
        // read as a wrong password rather than as an empty one.
        if input.value().is_empty() {
            return;
        }
        self.log_out_selected_sessions(concord::discord::Secret::new(input.value()));
    }

    /// Send the logout for the selected sessions.
    ///
    /// The password is taken, used and dropped in one step - it is never put
    /// back into popup state, so there is no window in which it is held after
    /// the request is queued.
    pub fn log_out_selected_sessions(&mut self, password: concord::discord::Secret) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let id_hashes: Vec<String> = popup.session_logout_targets.iter().cloned().collect();
        popup.session_logout_targets.clear();
        popup.access_password = None;
        if id_hashes.is_empty() {
            return;
        }
        self.enqueue_pending_command(AppCommand::RevokeAuthSessions {
            id_hashes,
            password,
        });
    }

    pub fn set_auth_sessions(&mut self, sessions: Vec<concord::discord::AuthSession>) {
        if let Some(popup) = self.popups.options_popup_mut() {
            // Selections for sessions that no longer exist are dropped: a
            // logout aimed at a gone session would fail the whole request.
            popup
                .session_logout_targets
                .retain(|id_hash| sessions.iter().any(|s| &s.id_hash == id_hash));
            popup.sessions = sessions;
            popup.access_loading = false;
        }
    }

    pub fn set_authorised_apps(&mut self, apps: Vec<concord::discord::AuthorisedApp>) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.apps = apps;
            popup.access_loading = false;
        }
    }

    pub fn mark_access_load_failed(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.access_loading = false;
        }
    }

    /// The two-factor row sits after the form's fields.
    fn totp_row_index(&self) -> usize {
        concord::discord::AccountField::ALL.len()
    }

    fn account_option_items(&self) -> Vec<DisplayOptionItem> {
        let Some(popup) = self.popups.options_popup() else {
            return Vec::new();
        };
        let form = &popup.account_form;

        let mut items: Vec<DisplayOptionItem> = concord::discord::AccountField::ALL
            .into_iter()
            .map(|field| DisplayOptionItem {
                label: field.label(),
                enabled: !form.value(field).is_empty(),
                // Bullets for a credential; the real value never reaches here.
                value: Some(form.display_value(field)),
                gauge: None,
                effective: true,
                description: field.hint(),
            })
            .collect();

        items.push(DisplayOptionItem {
            label: "Two-factor authentication",
            enabled: popup.totp_secret.is_some(),
            value: Some(match &popup.totp_secret {
                // Shown deliberately: enrolment cannot happen unless this
                // reaches the authenticator app.
                Some(secret) => secret.grouped(),
                None => "enter starts enrolment".to_owned(),
            }),
            gauge: None,
            effective: true,
            description: "enter starts or cancels enrolment; type the code and press S to finish.",
        });
        items
    }

    /// Start or cancel two-factor enrolment.
    fn toggle_totp_enrolment(&mut self) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        if popup.totp_secret.is_some() {
            popup.totp_secret = None;
            popup.totp_code.clear();
        } else {
            popup.totp_secret = Some(concord::discord::TotpSecret::generate());
        }
    }

    /// The `otpauth://` URI for the enrolment in progress, for a QR code.
    pub fn totp_enrolment_uri(&self) -> Option<String> {
        let popup = self.popups.options_popup()?;
        let secret = popup.totp_secret.as_ref()?;
        Some(secret.otpauth_uri(self.current_user().unwrap_or("Discord")))
    }

    pub fn totp_code(&self) -> Option<&str> {
        self.popups
            .options_popup()
            .map(|popup| popup.totp_code.as_str())
    }

    pub fn is_account_category_open(&self) -> bool {
        self.popups.options_popup().and_then(|popup| popup.category)
            == Some(OptionsCategory::Account)
    }

    /// Type into whichever field is highlighted.
    pub fn type_account_character(&mut self, character: char) {
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let totp_row = self.totp_row_index();
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        if index == totp_row {
            // Only while enrolling, or typing would fill a code for an
            // enrolment that has not started.
            if popup.totp_secret.is_some() {
                popup.totp_code.push(character);
            }
            return;
        }
        if let Some(field) = concord::discord::AccountField::at(index) {
            popup.account_form.push(field, character);
        }
    }

    pub fn delete_account_character(&mut self) {
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let totp_row = self.totp_row_index();
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        if index == totp_row {
            popup.totp_code.pop();
            return;
        }
        if let Some(field) = concord::discord::AccountField::at(index) {
            popup.account_form.pop(field);
        }
    }

    /// Why the form cannot be submitted, for the line under it.
    pub fn account_form_problem(&self) -> Option<String> {
        self.popups
            .options_popup()
            .and_then(|popup| popup.account_form.problem())
            .map(concord::discord::AccountFormProblem::message)
    }

    /// Send the credential change.
    ///
    /// The form is taken rather than borrowed, so submitting leaves no copy of
    /// three passwords behind in popup state.
    pub fn submit_account_form(&mut self) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        if popup.account_form.problem().is_some() {
            return;
        }
        let form = std::mem::take(&mut popup.account_form);
        if let Some(command) = form.submit() {
            self.enqueue_pending_command(command);
        }
    }

    /// Finish enrolment with the code from the authenticator app.
    pub fn submit_totp_enrolment(&mut self) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let (Some(secret), Some(())) = (
            popup.totp_secret.clone(),
            (!popup.totp_code.is_empty()).then_some(()),
        ) else {
            return;
        };
        // Discord needs the account password here too, and it is the one the
        // form already asks for rather than a second prompt.
        let password = popup
            .account_form
            .value(concord::discord::AccountField::CurrentPassword);
        if password.is_empty() {
            return;
        }
        let password = concord::discord::Secret::new(password);
        let code = std::mem::take(&mut popup.totp_code);
        popup.totp_secret = None;
        self.enqueue_pending_command(AppCommand::EnableTotp {
            secret: secret.as_str().to_owned(),
            code,
            password,
        });
    }

    /// Turn two-factor off with a current code.
    ///
    /// A code rather than a password, which is Discord's rule: the point is to
    /// prove the second factor still works before removing it.
    pub fn disable_totp(&mut self) {
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        // Only when no enrolment is in progress, or this would disable using
        // a code meant for the enrolment being set up.
        if popup.totp_secret.is_some() || popup.totp_code.is_empty() {
            return;
        }
        let code = std::mem::take(&mut popup.totp_code);
        self.enqueue_pending_command(AppCommand::DisableTotp { code });
    }

    /// Fetch the backup codes, or regenerate them.
    ///
    /// Regenerating invalidates the old ones, so it is a separate action
    /// rather than something the fetch does on its own.
    pub fn load_backup_codes(&mut self, regenerate: bool) {
        let Some(popup) = self.popups.options_popup() else {
            return;
        };
        let password = popup
            .account_form
            .value(concord::discord::AccountField::CurrentPassword);
        if password.is_empty() {
            return;
        }
        let password = concord::discord::Secret::new(password);
        self.enqueue_pending_command(AppCommand::LoadBackupCodes {
            password,
            regenerate,
        });
    }

    pub fn set_backup_codes(&mut self, codes: Vec<concord::discord::BackupCode>) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.backup_codes = codes;
        }
    }

    pub fn backup_codes(&self) -> &[concord::discord::BackupCode] {
        self.popups
            .options_popup()
            .map_or(&[], |popup| popup.backup_codes.as_slice())
    }

    /// Open the account settings panel.
    pub fn open_account(&mut self) {
        self.open_options_category(OptionsCategory::Account);
    }

    /// Open the sessions and apps panel.
    pub fn open_access(&mut self) {
        self.open_options_category(OptionsCategory::Access);
    }

    /// Open the linked-accounts panel.
    ///
    /// Named rather than reached through `open_options_category`, so callers
    /// outside this module need no access to the private category enum.
    /// Open the privacy and safety panel.
    pub fn open_privacy(&mut self) {
        self.open_options_category(OptionsCategory::Privacy);
    }

    fn privacy_option_items(&self) -> Vec<DisplayOptionItem> {
        let state = self.discord.privacy_state();
        concord::discord::PrivacySetting::ALL
            .into_iter()
            .map(|setting| {
                let on = setting.is_on(&state);
                DisplayOptionItem {
                    label: setting.label(),
                    enabled: on.unwrap_or(false),
                    value: setting.value(&state).map(str::to_owned),
                    gauge: None,
                    // Not yet received is distinct from off, and a row that
                    // showed them alike would report an unknown setting as
                    // permissive.
                    effective: on.is_some(),
                    description: setting.detail(),
                }
            })
            .collect()
    }

    pub fn open_connections(&mut self) {
        self.open_options_category(OptionsCategory::Connections);
    }

    pub fn is_connections_category_open(&self) -> bool {
        self.popups.options_popup().and_then(|popup| popup.category)
            == Some(OptionsCategory::Connections)
    }

    fn connection_option_items(&self) -> Vec<DisplayOptionItem> {
        let loading = self
            .popups
            .options_popup()
            .is_some_and(|popup| popup.connections_loading);
        let rows = self.connection_rows();
        if rows.is_empty() {
            return vec![DisplayOptionItem {
                label: if loading {
                    "Loading linked accounts"
                } else {
                    "No linked accounts"
                },
                enabled: false,
                value: None,
                gauge: None,
                effective: false,
                // Linking is an OAuth flow through a browser, which would mean
                // handling someone else's credentials. This client does not.
                description: "Link an account on Discord's website; this client can show, change and unlink them.",
            }];
        }

        rows.iter()
            .map(|connection| DisplayOptionItem {
                // The service and username go in the value rather than the
                // label: the label is `&'static str`, and leaking a string per
                // redraw to widen it would be a leak on every frame.
                label: "Linked account",
                enabled: connection.visibility == concord::discord::ConnectionVisibility::Everyone,
                value: Some(format!(
                    "{} - {} - {}",
                    connection.kind,
                    connection.name,
                    connection.summary()
                )),
                gauge: None,
                effective: connection.verified,
                description: "enter changes who sees it, a toggles activity, d unlinks.",
            })
            .collect()
    }

    /// Show or hide the highlighted connection on your profile.
    fn cycle_selected_connection_visibility(&mut self) {
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let Some(connection) = popup.connections.get_mut(index) else {
            return;
        };
        connection.visibility = connection.visibility.toggled();
        let command = AppCommand::ModifyConnection {
            kind: connection.kind.clone(),
            id: connection.id.clone(),
            visibility: connection.visibility,
            show_activity: connection.show_activity,
            label: connection.name.clone(),
        };
        self.enqueue_pending_command(command);
    }

    /// Whether what you do on the highlighted service appears in your presence.
    pub fn toggle_selected_connection_activity(&mut self) {
        if self.popups.options_popup().and_then(|popup| popup.category)
            != Some(OptionsCategory::Connections)
        {
            return;
        }
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        let Some(connection) = popup.connections.get_mut(index) else {
            return;
        };
        connection.show_activity = !connection.show_activity;
        let command = AppCommand::ModifyConnection {
            kind: connection.kind.clone(),
            id: connection.id.clone(),
            visibility: connection.visibility,
            show_activity: connection.show_activity,
            label: connection.name.clone(),
        };
        self.enqueue_pending_command(command);
    }

    /// Unlink the highlighted account.
    pub fn delete_selected_connection(&mut self) {
        if self.popups.options_popup().and_then(|popup| popup.category)
            != Some(OptionsCategory::Connections)
        {
            return;
        }
        let Some(index) = self.selected_option_index() else {
            return;
        };
        let Some(popup) = self.popups.options_popup_mut() else {
            return;
        };
        if index >= popup.connections.len() {
            return;
        }
        let connection = popup.connections.remove(index);
        self.enqueue_pending_command(AppCommand::DeleteConnection {
            kind: connection.kind,
            id: connection.id,
            label: connection.name,
        });
    }

    /// Take the fetched connections.
    pub fn set_connections(&mut self, connections: Vec<concord::discord::Connection>) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.connections = connections;
            popup.connections_loading = false;
        }
    }

    /// The fetch failed; stop saying it is loading, or the panel claims to be
    /// working forever.
    pub fn mark_connections_load_failed(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.connections_loading = false;
        }
    }

    fn display_option_items_for_display(&self) -> Vec<DisplayOptionItem> {
        let options = self.options.display_options;
        vec![
            DisplayOptionItem {
                label: "Disable all image previews",
                enabled: options.disable_image_preview,
                value: None,
                gauge: None,
                effective: options.disable_image_preview,
                description: "Master switch for avatars, images, and custom emoji images.",
            },
            DisplayOptionItem {
                label: "Show avatars",
                enabled: options.show_avatars,
                value: None,
                gauge: None,
                effective: options.avatars_visible(),
                description: "Message and profile avatars.",
            },
            DisplayOptionItem {
                label: "Show images",
                enabled: options.show_images,
                value: None,
                gauge: None,
                effective: options.images_visible(),
                description: "Attachment, embed, and attachment viewer previews.",
            },
            DisplayOptionItem {
                label: "Image preview quality",
                enabled: true,
                value: Some(options.image_preview_quality.label().to_owned()),
                gauge: None,
                effective: options.images_visible(),
                description: "Quality preset for attachment and embed.",
            },
            DisplayOptionItem {
                label: "Attachment viewer quality",
                enabled: true,
                value: Some(options.attachment_viewer_quality.label().to_owned()),
                gauge: None,
                effective: options.images_visible(),
                description: "Quality preset for attachment viewer previews.",
            },
            DisplayOptionItem {
                label: "Show custom emoji images",
                enabled: options.show_custom_emoji,
                value: None,
                gauge: None,
                effective: options.custom_emoji_visible(),
                description: "When off, custom emoji are shown as their emoji id.",
            },
            DisplayOptionItem {
                label: "Circular avatars",
                enabled: options.circular_avatars,
                value: None,
                gauge: None,
                effective: options.avatars_visible() && options.circular_avatars,
                description: "Mask message and profile avatars into a circle.",
            },
            DisplayOptionItem {
                label: "Media playback",
                enabled: options.media_playback,
                value: None,
                gauge: None,
                effective: options.media_playback_enabled(),
                description: "Allow videos to open in the external media player.",
            },
            DisplayOptionItem {
                label: "24-hour time",
                enabled: options.hour_format_24,
                value: None,
                gauge: None,
                effective: options.hour_format_24,
                description: "Use 24-hour time for message timestamps.",
            },
        ]
    }

    fn display_option_items_for_composer(&self) -> Vec<DisplayOptionItem> {
        let options = self.options.composer_options;
        vec![DisplayOptionItem {
            label: "Emojis as links",
            enabled: options.emojis_as_links,
            value: None,
            gauge: None,
            effective: options.emojis_as_links,
            description: "Sends unavailable emojis as a link instead.",
        }]
    }

    fn display_option_items_for_notifications(&self) -> Vec<DisplayOptionItem> {
        vec![
            DisplayOptionItem {
                label: "Desktop notifications",
                enabled: self.options.notification_options.desktop_notifications,
                value: None,
                gauge: None,
                effective: self.options.notification_options.desktop_notifications,
                description: "Show OS notifications for Discord messages that pass notification settings.",
            },
            DisplayOptionItem {
                label: "Notification sounds",
                enabled: self.options.notification_options.notification_sounds,
                value: None,
                gauge: None,
                effective: self.options.notification_options.notification_sounds,
                description: "Play a sound for the same messages. Separate from the popup, so either can be had without the other.",
            },
        ]
    }

    fn display_option_items_for_voice(&self) -> Vec<DisplayOptionItem> {
        VoiceOption::ALL
            .iter()
            .map(|option| self.voice_option_item(*option))
            .collect()
    }

    fn voice_option_item(&self, option: VoiceOption) -> DisplayOptionItem {
        let voice = &self.options.voice_options;
        match option {
            VoiceOption::Muted => DisplayOptionItem {
                label: "Voice muted",
                enabled: voice.self_mute,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::Deafened => DisplayOptionItem {
                label: "Voice deafened",
                enabled: voice.self_deaf,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::InputSource => DisplayOptionItem {
                label: "Input source",
                enabled: true,
                value: Some(self.voice_audio_source_value(|sources| {
                    sources.input_label(voice.input_source.as_deref())
                })),
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "Enter or ←/→ to change.",
            },
            VoiceOption::OutputSource => DisplayOptionItem {
                label: "Output source",
                enabled: true,
                value: Some(self.voice_audio_source_value(|sources| {
                    sources.output_label(voice.output_source.as_deref())
                })),
                gauge: None,
                effective: !voice.self_deaf,
                description: "Enter or ←/→ to change.",
            },
            VoiceOption::AllowMicrophoneTransmit => DisplayOptionItem {
                label: "Allow microphone transmit",
                enabled: voice.allow_microphone_transmit,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::PushToTalk => DisplayOptionItem {
                label: "Push to talk",
                enabled: voice.push_to_talk,
                value: None,
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "Hold the shortcut to transmit.",
            },
            VoiceOption::PushToTalkShortcut => DisplayOptionItem {
                label: "Push-to-talk shortcut",
                enabled: true,
                value: Some(if self.is_capturing_push_to_talk_shortcut() {
                    "Press shortcut (Esc cancels)".to_owned()
                } else {
                    voice.push_to_talk_shortcut.clone()
                }),
                gauge: None,
                effective: voice.push_to_talk,
                description: if self.is_capturing_push_to_talk_shortcut() {
                    ""
                } else {
                    "Enter to record."
                },
            },
            VoiceOption::NoiseSuppression => DisplayOptionItem {
                label: "Noise suppression",
                enabled: voice.noise_suppression,
                value: None,
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "",
            },
            VoiceOption::MicrophoneSensitivity => DisplayOptionItem {
                label: "Microphone sensitivity",
                enabled: true,
                value: Some(voice.microphone_sensitivity.label()),
                gauge: Some(DisplayOptionGauge::new(
                    microphone_sensitivity_percent(voice.microphone_sensitivity),
                    100,
                )),
                effective: voice.allow_microphone_transmit && !voice.push_to_talk,
                description: "Lower dB detects quieter input.",
            },
            VoiceOption::MicrophoneVolume => DisplayOptionItem {
                label: "Microphone volume",
                enabled: true,
                value: Some(voice.microphone_volume.label()),
                gauge: Some(DisplayOptionGauge::new(
                    u16::from(voice.microphone_volume.value()),
                    u16::from(VoiceVolumePercent::maximum()),
                )),
                effective: voice.allow_microphone_transmit,
                description: "",
            },
            VoiceOption::VoiceVolume => DisplayOptionItem {
                label: "Voice volume",
                enabled: true,
                value: Some(voice.voice_output_volume.label()),
                gauge: Some(DisplayOptionGauge::new(
                    u16::from(voice.voice_output_volume.value()),
                    u16::from(VoiceVolumePercent::maximum()),
                )),
                effective: !voice.self_deaf,
                description: "",
            },
        }
    }

    fn voice_audio_source_value(
        &self,
        label: impl FnOnce(&VoiceAudioSourceOptions) -> String,
    ) -> String {
        if self.options.voice_audio_sources_request_id.is_some() {
            return "Loading sources...".to_owned();
        }
        label(&self.options.voice_audio_source_options)
    }

    pub fn toggle_selected_display_option(&mut self) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };
        let Some(category) = self.popups.options_popup().and_then(|popup| popup.category) else {
            self.open_selected_options_category();
            return;
        };

        if category == OptionsCategory::Voice {
            if let Some(option) = VoiceOption::at(selected) {
                self.toggle_voice_option(option);
            }
            return;
        }

        if category == OptionsCategory::Connections {
            self.cycle_selected_connection_visibility();
            return;
        }

        if category == OptionsCategory::Access {
            self.toggle_selected_session_target();
            return;
        }

        if category == OptionsCategory::Account {
            if selected == self.totp_row_index() {
                self.toggle_totp_enrolment();
            }
            return;
        }

        if category == OptionsCategory::Privacy {
            if let Some(setting) = concord::discord::PrivacySetting::at(selected) {
                let edit = setting.toggled(&self.discord.privacy_state());
                self.enqueue_pending_command(AppCommand::ModifyPrivacySettings { edit });
            }
            return;
        }

        let images_visible_before = self.show_images();

        match (category, selected) {
            (OptionsCategory::Display, 0) => {
                self.options.display_options.disable_image_preview =
                    !self.options.display_options.disable_image_preview
            }
            (OptionsCategory::Display, 1) => {
                self.options.display_options.show_avatars =
                    !self.options.display_options.show_avatars
            }
            (OptionsCategory::Display, 2) => {
                self.options.display_options.show_images = !self.options.display_options.show_images
            }
            (OptionsCategory::Display, 3) => {
                self.options.display_options.image_preview_quality =
                    self.options.display_options.image_preview_quality.next()
            }
            (OptionsCategory::Display, 4) => {
                self.options.display_options.attachment_viewer_quality = self
                    .options
                    .display_options
                    .attachment_viewer_quality
                    .next()
            }
            (OptionsCategory::Display, 5) => {
                self.options.display_options.show_custom_emoji =
                    !self.options.display_options.show_custom_emoji
            }
            (OptionsCategory::Display, 6) => {
                self.options.display_options.circular_avatars =
                    !self.options.display_options.circular_avatars
            }
            (OptionsCategory::Display, 7) => {
                self.options.display_options.media_playback =
                    !self.options.display_options.media_playback
            }
            (OptionsCategory::Display, 8) => {
                self.options.display_options.hour_format_24 =
                    !self.options.display_options.hour_format_24
            }
            (OptionsCategory::Composer, 0) => {
                self.options.composer_options.emojis_as_links =
                    !self.options.composer_options.emojis_as_links
            }
            (OptionsCategory::Notifications, 0) => {
                self.options.notification_options.desktop_notifications =
                    !self.options.notification_options.desktop_notifications
            }
            (OptionsCategory::Notifications, 1) => {
                self.options.notification_options.notification_sounds =
                    !self.options.notification_options.notification_sounds
            }
            _ => return,
        }
        if images_visible_before != self.show_images() {
            self.refresh_composer_attachment_previews();
        }
        self.mark_options_changed();
    }

    /// Enter on a voice row. Toggles boolean rows and steps the cycled source
    /// rows forward. The gauge rows respond to left and right only.
    fn toggle_voice_option(&mut self, option: VoiceOption) {
        match option {
            VoiceOption::Muted => {
                self.options.voice_options.self_mute = !self.options.voice_options.self_mute;
                self.mark_options_changed();
                self.queue_current_voice_state_update();
            }
            VoiceOption::Deafened => {
                self.options.voice_options.self_deaf = !self.options.voice_options.self_deaf;
                self.mark_options_changed();
                self.queue_current_voice_state_update();
            }
            VoiceOption::InputSource | VoiceOption::OutputSource => {
                self.adjust_voice_option(option, 1);
            }
            VoiceOption::AllowMicrophoneTransmit => {
                self.options.voice_options.allow_microphone_transmit =
                    !self.options.voice_options.allow_microphone_transmit;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::PushToTalk => {
                self.options.voice_options.push_to_talk = !self.options.voice_options.push_to_talk;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::PushToTalkShortcut => {
                if let Some(popup) = self.popups.options_popup_mut() {
                    popup.capturing_push_to_talk_shortcut = true;
                }
            }
            VoiceOption::NoiseSuppression => {
                self.options.voice_options.noise_suppression =
                    !self.options.voice_options.noise_suppression;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::MicrophoneSensitivity
            | VoiceOption::MicrophoneVolume
            | VoiceOption::VoiceVolume => {}
        }
    }

    pub fn adjust_selected_display_option(&mut self, delta: i8) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };
        if self.popups.options_popup().and_then(|popup| popup.category)
            != Some(OptionsCategory::Voice)
        {
            return;
        }
        if let Some(option) = VoiceOption::at(selected) {
            self.adjust_voice_option(option, delta);
        }
    }

    fn adjust_voice_option(&mut self, option: VoiceOption, delta: i8) {
        match option {
            VoiceOption::InputSource | VoiceOption::OutputSource => {
                // Cycling against a list that is still loading would pick from
                // the previous popup's devices.
                if self.options.voice_audio_sources_request_id.is_some() {
                    return;
                }
                let changed = if option == VoiceOption::InputSource {
                    self.options
                        .voice_audio_source_options
                        .adjust_input(&mut self.options.voice_options.input_source, delta)
                } else {
                    self.options
                        .voice_audio_source_options
                        .adjust_output(&mut self.options.voice_options.output_source, delta)
                };
                if changed {
                    self.mark_options_changed();
                    self.queue_current_voice_audio_sources_update();
                }
            }
            VoiceOption::MicrophoneSensitivity => {
                let previous = self.options.voice_options.microphone_sensitivity;
                self.options.voice_options.microphone_sensitivity = previous.adjust(delta);
                if self.options.voice_options.microphone_sensitivity != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::MicrophoneVolume => {
                let previous = self.options.voice_options.microphone_volume;
                self.options.voice_options.microphone_volume = previous.adjust(delta);
                if self.options.voice_options.microphone_volume != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::VoiceVolume => {
                let previous = self.options.voice_options.voice_output_volume;
                self.options.voice_options.voice_output_volume = previous.adjust(delta);
                if self.options.voice_options.voice_output_volume != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::Muted
            | VoiceOption::Deafened
            | VoiceOption::AllowMicrophoneTransmit
            | VoiceOption::PushToTalk
            | VoiceOption::PushToTalkShortcut
            | VoiceOption::NoiseSuppression => {}
        }
    }

    pub fn open_options_category_from_shortcut(&mut self, shortcut: OptionsCategoryShortcut) {
        match shortcut {
            OptionsCategoryShortcut::Display => {
                self.open_options_category(OptionsCategory::Display)
            }
            OptionsCategoryShortcut::Composer => {
                self.open_options_category(OptionsCategory::Composer)
            }
            OptionsCategoryShortcut::Notifications => {
                self.open_options_category(OptionsCategory::Notifications)
            }
            OptionsCategoryShortcut::Voice => self.open_options_category(OptionsCategory::Voice),
            OptionsCategoryShortcut::Connections => {
                self.open_options_category(OptionsCategory::Connections)
            }
            OptionsCategoryShortcut::Privacy => {
                self.open_options_category(OptionsCategory::Privacy)
            }
            OptionsCategoryShortcut::Access => self.open_options_category(OptionsCategory::Access),
            OptionsCategoryShortcut::Account => {
                self.open_options_category(OptionsCategory::Account)
            }
        }
    }

    fn open_selected_options_category(&mut self) {
        match self.selected_option_index() {
            Some(0) => self.open_options_category(OptionsCategory::Display),
            Some(1) => self.open_options_category(OptionsCategory::Composer),
            Some(2) => self.open_options_category(OptionsCategory::Notifications),
            Some(3) => self.open_options_category(OptionsCategory::Voice),
            _ => {}
        }
    }

    pub(super) fn mark_options_changed(&mut self) {
        self.clear_message_row_content_metrics_cache();
        self.options.config_save_pending = true;
    }

    pub(in crate::tui) fn queue_current_voice_audio_sources_update(&mut self) {
        self.enqueue_pending_command(AppCommand::UpdateVoiceAudioSources {
            input_source: self.options.voice_options.input_source.clone(),
            output_source: self.options.voice_options.output_source.clone(),
        });
    }

    fn queue_current_voice_capture_permission_update(&mut self) {
        let Some(voice) = self.runtime.voice_connection else {
            return;
        };
        let Some(channel_id) = voice.channel_id else {
            return;
        };

        self.enqueue_pending_command(AppCommand::UpdateVoiceCapturePermission {
            scope: voice.scope,
            channel_id,
            allow_microphone_transmit: self.options.voice_options.allow_microphone_transmit,
            noise_suppression: self.options.voice_options.noise_suppression,
            microphone_sensitivity: self.options.voice_options.microphone_sensitivity,
            microphone_volume: self.options.voice_options.microphone_volume,
            voice_output_volume: self.options.voice_options.voice_output_volume,
        });
    }
}

fn microphone_sensitivity_percent(sensitivity: MicrophoneSensitivityDb) -> u16 {
    (i16::from(sensitivity.value()) + 100) as u16
}
