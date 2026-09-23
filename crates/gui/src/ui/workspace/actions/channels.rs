use super::super::*;

use concord::config::CredentialStoreMode;
use concord::discord::{AppCommand, Id, MessageSearchQuery, VoiceScope, marker};
use concord::token_store;
use gpui::{Context, Window, WindowHandle};
use tokio::sync::mpsc;

use crate::model::projection::Selection;

use crate::ui::login::{LoginEvent, LoginHandle, LoginScreen};
use crate::ui::workspace::{MfaMethod, PasswordAuthEvent, QrEvent};

impl Workspace {
    pub fn open_channel(&mut self, channel_id: Id<marker::ChannelMarker>) {
        // The active tab follows the channel, so an ordinary click reuses the
        // tab rather than growing the strip without being asked.
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.channel_id != channel_id {
                tab.channel_id = channel_id;
                tab.selection = self.nav.selection;
                tab.draft = String::new();
                tab.scroll = gpui::point(gpui::px(0.), gpui::px(0.));
            }
        } else if self.tabs.is_empty() {
            self.tabs.push(ChannelTab {
                channel_id,
                selection: self.nav.selection,
                name: String::new(),
                draft: String::new(),
                scroll: gpui::point(gpui::px(0.), gpui::px(0.)),
            });
            self.active_tab = 0;
        }

        self.nav.channel = Some(channel_id);
        if let Some(pos) = self
            .model
            .channels
            .iter()
            .position(|c| c.id == Some(channel_id))
        {
            self.model.selected_channel = pos;
        }
        self.messages.clear();

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::SetSelectedMessageChannel {
                channel_id: Some(channel_id),
            });
            handle.send(AppCommand::LoadMessageHistory {
                channel_id,
                before: None,
            });

            match self.nav.selection {
                Selection::Guild(guild_id) => {
                    handle.send(AppCommand::SubscribeGuildChannel {
                        guild_id,
                        channel_id,
                    });
                    // Discord streams the member list in windowed ranges; the
                    // first two cover what fits on screen without over-fetching.
                    handle.send(AppCommand::UpdateMemberListSubscription {
                        guild_id,
                        channel_id,
                        // The channel's own member list, not a thread's.
                        thread_id: None,
                        ranges: vec![(0, 99), (100, 199)],
                    });
                }
                Selection::DirectMessages => {
                    handle.send(AppCommand::SubscribeDirectMessage { channel_id });
                }
            }
        }

        self.reproject();
    }

    /// Advance the login state machine based on a user action in the current sub-screen.
    ///
    /// Called from the key handler with a `LoginAction` that describes what
    /// the user just did (submit, back, pick a method, etc.).
    pub fn handle_login_action(
        &mut self,
        action: LoginAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            // -- Picker: user chose a method --------------------------------
            LoginAction::PickPassword => {
                if let Screen::Login(l) = &mut self.screen {
                    l.screen = LoginScreen::Password;
                    l.error = None;
                }
            }
            LoginAction::PickToken => {
                if let Screen::Login(l) = &mut self.screen {
                    l.screen = LoginScreen::Token;
                    l.error = None;
                }
            }
            LoginAction::PickQr => {
                if let Screen::Login(l) = &mut self.screen {
                    // Abort any stale handle.
                    l.handle = None;
                    l.qr.reset();
                    l.screen = LoginScreen::QrScan;
                    l.error = None;

                    let rx = crate::session::spawn_qr_login();
                    l.handle = Some(LoginHandle {
                        rx: Self::wrap_qr(rx),
                    });

                    if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                        Workspace::pump_login(wh, cx);
                    }
                }
            }
            LoginAction::PickDemo => {
                if cfg!(feature = "fixtures") {
                    self.start_token_session("test".to_string(), false, window, cx);
                } else if let Screen::Login(login) = &mut self.screen {
                    // Reachable through the keyboard shortcut even when the
                    // button is hidden, so it explains itself rather than
                    // failing as a bad credential.
                    login.error = Some(
                        "This build has no demo data. Rebuild with --features fixtures."
                            .to_string(),
                    );
                }
            }

            // -- Back: return to picker -------------------------------------
            LoginAction::Back => {
                if let Screen::Login(l) = &mut self.screen {
                    // Abort any running auth task.
                    l.handle = None;
                    l.screen = LoginScreen::Picker;
                    l.error = None;
                }
            }

            // -- Token screen: submit ---------------------------------------
            LoginAction::SubmitToken => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.token_submittable() {
                    return;
                }
                let token = login.token.take();
                let remember = login.remember;
                self.start_token_session(token, remember, window, cx);
            }

            // -- Password screen: submit ------------------------------------
            LoginAction::SubmitPassword => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.password.is_submittable() {
                    return;
                }
                let login_id = login.password.login.text().to_string();
                let pw = login.password.password.text().to_string();
                login.password.in_progress = true;
                login.password.status = "Authenticating with Discord…".to_string();
                login.error = None;

                let rx = crate::session::spawn_password_login(login_id, pw);
                login.handle = Some(LoginHandle {
                    rx: Self::wrap_password(rx),
                });

                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump_login(wh, cx);
                }
            }

            // -- MFA select: user picked a method ---------------------------
            LoginAction::PickMfaMethod(method) => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                let Some(challenge) = login.password.mfa.clone() else {
                    return;
                };
                match method {
                    MfaMethod::Totp => {
                        login.password.mfa_method = Some(MfaMethod::Totp);
                        login.password.status =
                            "Enter the 6-digit code from your authenticator app.".to_string();
                        login.screen = LoginScreen::MfaCode;
                    }
                    MfaMethod::Sms => {
                        // Ask Discord to send the SMS first.
                        login.password.in_progress = true;
                        login.password.status = "Requesting SMS code…".to_string();
                        login.error = None;

                        let rx = crate::session::spawn_sms_send(challenge.ticket.clone());
                        login.handle = Some(LoginHandle {
                            rx: Self::wrap_password(rx),
                        });

                        if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                            Workspace::pump_login(wh, cx);
                        }
                    }
                }
            }

            // -- MFA code: user submitted the code --------------------------
            LoginAction::SubmitMfaCode => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.password.is_mfa_submittable() {
                    return;
                }
                let Some(challenge) = login.password.mfa.clone() else {
                    return;
                };
                let Some(method) = login.password.mfa_method else {
                    return;
                };
                let code = login.password.mfa_code.text().to_string();
                login.password.in_progress = true;
                login.password.status = "Verifying…".to_string();
                login.error = None;

                let rx = crate::session::spawn_mfa_verify(
                    method,
                    code,
                    challenge.ticket.clone(),
                    challenge.login_instance_id.clone(),
                );
                login.handle = Some(LoginHandle {
                    rx: Self::wrap_password(rx),
                });

                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump_login(wh, cx);
                }
            }

            // -- Toggle remember --------------------------------------------
            LoginAction::ToggleRemember => {
                if let Screen::Login(l) = &mut self.screen {
                    l.remember = !l.remember;
                }
            }
        }
    }

    /// Convert a `PasswordAuthEvent` receiver into the unified `LoginEvent` channel.
    pub fn wrap_password(rx: mpsc::Receiver<PasswordAuthEvent>) -> mpsc::Receiver<LoginEvent> {
        let (tx, out) = mpsc::channel(8);
        // Shared runtime: this runs on GPUI's thread, which has no reactor.
        let _ = crate::runtime::spawn(async move {
            let mut rx = rx;
            while let Some(ev) = rx.recv().await {
                if tx.send(LoginEvent::Password(ev)).await.is_err() {
                    break;
                }
            }
        });
        out
    }

    /// Convert a `QrEvent` receiver into the unified `LoginEvent` channel.
    pub fn wrap_qr(rx: mpsc::Receiver<QrEvent>) -> mpsc::Receiver<LoginEvent> {
        let (tx, out) = mpsc::channel(8);
        // Shared runtime: this runs on GPUI's thread, which has no reactor.
        let _ = crate::runtime::spawn(async move {
            let mut rx = rx;
            while let Some(ev) = rx.recv().await {
                if tx.send(LoginEvent::Qr(ev)).await.is_err() {
                    break;
                }
            }
        });
        out
    }

    /// Drain the active login auth handle's event stream on GPUI's executor.
    ///
    /// Starts the token session as soon as a `Token` event arrives, or
    /// advances the MFA / QR state machine for intermediate events.
    pub fn pump_login(window: WindowHandle<Workspace>, cx: &mut gpui::App) {
        cx.spawn(async move |cx| {
            loop {
                // Peek: is there still a handle and does it have an event?
                let event = window.update(cx, |workspace, _window, _cx| {
                    let Screen::Login(login) = &mut workspace.screen else {
                        return None;
                    };
                    // Try to receive without blocking. We'll re-schedule if empty.
                    login.handle.as_mut().and_then(|h| h.rx.try_recv().ok())
                });

                match event {
                    Err(_) => break, // window gone
                    Ok(None) => {
                        // Nothing yet – yield to other GPUI work and try again
                        // via a small async sleep so we don't busy-spin.
                        // Use recv() properly by driving from a spawn.
                        // We reschedule ourselves in 16ms.
                        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                        continue;
                    }
                    Ok(Some(event)) => {
                        let done = window.update(cx, |workspace, win, cx| {
                            workspace.apply_login_event(event, win, cx)
                        });
                        match done {
                            Err(_) => break,
                            Ok(true) => break, // session started or fatal error
                            Ok(false) => {}    // keep pumping
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Apply a single `LoginEvent` to the login state.
    ///
    /// Returns `true` when pumping should stop (session started or unrecoverable).
    pub fn apply_login_event(
        &mut self,
        event: LoginEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Screen::Login(login) = &mut self.screen else {
            return true;
        };

        match event {
            // ---- Password / MFA events ------------------------------------
            LoginEvent::Password(pev) => match pev {
                PasswordAuthEvent::Status(s) => {
                    login.password.status = s;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::Token(token) => {
                    login.password.reset_sensitive();
                    login.handle = None;
                    let remember = login.remember;
                    cx.notify();
                    self.start_token_session(token, remember, window, cx);
                    true
                }
                PasswordAuthEvent::Failed(reason) => {
                    login.password.in_progress = false;
                    login.password.status.clear();
                    login.error = Some(format!("Login failed: {reason}"));
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::MfaRequired(challenge) => {
                    login.password.in_progress = false;
                    login.password.password.clear();
                    login.password.mfa = Some(challenge);
                    login.password.mfa_method = None;
                    login.password.mfa_code.clear();
                    login.password.status =
                        "Choose a two-factor authentication method.".to_string();
                    login.screen = LoginScreen::MfaSelect;
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::SmsSent { phone } => {
                    login.password.in_progress = false;
                    login.password.mfa_method = Some(MfaMethod::Sms);
                    login.password.mfa_code.clear();
                    login.password.status = match phone {
                        Some(p) => format!("SMS sent to {p}. Enter the code below."),
                        None => "SMS sent. Enter the code below.".to_string(),
                    };
                    login.screen = LoginScreen::MfaCode;
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::RequiredActions(actions) => {
                    login.password.reset_sensitive();
                    login.handle = None;
                    let list = actions
                        .into_iter()
                        .map(|a| match a.as_str() {
                            "update_password" => "update your account password".to_owned(),
                            other => other.to_owned(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    login.error = Some(format!(
                        "Discord requires you to {list} in the official client before Concord can log in."
                    ));
                    cx.notify();
                    false
                }
            },

            // ---- QR events -----------------------------------------------
            LoginEvent::Qr(qev) => match qev {
                QrEvent::Status(s) => {
                    login.qr.status = s;
                    cx.notify();
                    false
                }
                QrEvent::QrBitmap(bm) => {
                    login.qr.bitmap = Some(bm);
                    cx.notify();
                    false
                }
                QrEvent::UserPending {
                    username,
                    discriminator,
                } => {
                    let display = if discriminator == "0" {
                        username
                    } else {
                        format!("{username}#{discriminator}")
                    };
                    login.qr.pending_user = Some(display);
                    cx.notify();
                    false
                }
                QrEvent::Token(token) => {
                    login.handle = None;
                    let remember = login.remember;
                    cx.notify();
                    self.start_token_session(token, remember, window, cx);
                    true
                }
                QrEvent::Cancelled => {
                    login.handle = None;
                    login.qr.reset();
                    login.screen = LoginScreen::Picker;
                    login.error =
                        Some("QR login was cancelled in the Discord mobile app.".to_string());
                    cx.notify();
                    false
                }
                QrEvent::Failed(reason) => {
                    login.handle = None;
                    login.qr.reset();
                    login.screen = LoginScreen::Picker;
                    login.error = Some(format!("QR login failed: {reason}"));
                    cx.notify();
                    false
                }
            },
        }
    }

    /// Spawn the core session from a resolved token and transition to `Screen::Ready`.
    pub fn start_token_session(
        &mut self,
        token: String,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if remember {
            let _ = token_store::save_token(&token, CredentialStoreMode::default());
        }
        match crate::session::spawn(token) {
            Ok((updates, handle)) => {
                self.attach(handle);
                self.screen = Screen::Ready;
                self.model.status_line = "connecting…".to_string();
                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump(wh, updates, cx);
                }
            }
            Err(error) => {
                if let Screen::Login(login) = &mut self.screen {
                    login.error = Some(format!("could not start session: {error}"));
                }
            }
        }
    }

    /// Open the channel containing a search hit and load history around it.
    ///
    /// The surrounding history matters: jumping to a message with nothing
    /// above or below it gives no context for why it matched.
    pub fn jump_to(
        &mut self,
        channel_id: Id<marker::ChannelMarker>,
        message_id: Id<marker::MessageMarker>,
    ) {
        if self.nav.channel != Some(channel_id) {
            self.open_channel(channel_id);
        }

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::LoadMessageHistoryAround {
                channel_id,
                message_id,
            });
        }

        self.search = None;
    }

    /// Open or close the search panel.
    pub fn toggle_search(&mut self) {
        self.search = match self.search {
            Some(_) => None,
            None => Some(Search::default()),
        };
    }

    /// Run the current search query, scoped to the open guild.
    pub fn run_search(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        let content = search.input.text().trim().to_string();
        if content.is_empty() {
            return;
        }

        search.running = true;
        search.error = None;
        search.results.clear();
        search.request_id = search.request_id.wrapping_add(1);
        let request_id = search.request_id;

        let Some(handle) = &self.handle else {
            return;
        };

        handle.send(AppCommand::SearchMessages {
            request_id,
            query: MessageSearchQuery {
                guild_id: match self.nav.selection {
                    Selection::Guild(id) => Some(id),
                    Selection::DirectMessages => None,
                },
                // A DM search has no guild, so it is scoped to the open
                // channel instead; otherwise Discord rejects the query.
                channel_id: match self.nav.selection {
                    Selection::DirectMessages => self.nav.channel,
                    Selection::Guild(_) => None,
                },
                content: Some(content),
                ..Default::default()
            },
        });
    }

    /// The voice scope for a channel: guild channels are guild-scoped, DM and
    /// group-DM calls are private-scoped to the channel itself.
    pub fn voice_scope(&self, channel_id: Id<marker::ChannelMarker>) -> VoiceScope {
        match self.nav.selection {
            Selection::Guild(guild_id) => VoiceScope::Guild(guild_id),
            Selection::DirectMessages => VoiceScope::Private(channel_id),
        }
    }

    /// Open the audio device picker, asking the core for the device list.
    ///
    /// The list is fetched rather than cached: devices appear and disappear
    /// while the app runs, and a stale list offers a device that is gone.
    pub fn open_audio_devices(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };

        self.audio_sources_request = self.audio_sources_request.wrapping_add(1);
        self.audio_devices = Some(AudioDevices::default());
        handle.send(AppCommand::LoadVoiceAudioSources {
            request_id: self.audio_sources_request,
        });
    }

    /// Select an input or output device.
    pub fn set_audio_device(&mut self, input: Option<String>, output: Option<String>) {
        let Some(handle) = &self.handle else {
            return;
        };

        if let Some(devices) = &mut self.audio_devices {
            if input.is_some() {
                devices.selected_input = input.clone();
            }
            if output.is_some() {
                devices.selected_output = output.clone();
            }
        }

        handle.send(AppCommand::UpdateVoiceAudioSources {
            input_source: input,
            output_source: output,
        });
    }
}
