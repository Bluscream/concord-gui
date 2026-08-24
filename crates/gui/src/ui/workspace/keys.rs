//! The keyboard.
//!
//! Split out because this is one `else if` chain several hundred lines
//! long, and its failure mode is silence: an arm naming only the key
//! claims it outright, so every later arm holding that key behind a state
//! guard becomes unreachable without anything failing to compile. Two of
//! those shipped. `no_key_binding_is_shadowed_by_an_unguarded_one_above_it`
//! guards the shape now, and having the chain in a file of its own is what
//! lets that test read a few hundred lines instead of ten thousand.

use concord::discord::password_auth::MfaMethod;
use gpui::{ClipboardItem, Context, KeyDownEvent, Window};

use super::{ActivityDraft, LoginAction, Pane, Screen, Workspace};
use concord_ui::keybindings::external::Resolution;

use crate::keymap;
use crate::ui::composer::ClipboardIntent;
use crate::ui::emoji;
use crate::ui::login::{LoginScreen, PasswordField};
use crate::ui::messages::MessageAction;

impl Workspace {
    /// Handle a key press.
    ///
    /// Ordered as one chain rather than a match, because most arms test the
    /// key together with a modifier or a piece of state - and the order they
    /// sit in is what decides which surface owns a key when several want it.
    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &mut self.screen {
            Screen::Login(login) => {
                let key = event.keystroke.key.as_str();
                let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;

                // ctrl-r toggles credential persistence on any sub-screen.
                if key == "r" && ctrl {
                    let action = LoginAction::ToggleRemember;
                    self.handle_login_action(action, window, cx);
                    return;
                }

                match login.screen {
                    // ---- Picker: number-key or letter shortcuts ----
                    LoginScreen::Picker => {
                        let action = match key {
                            "1" => Some(LoginAction::PickPassword),
                            "2" => Some(LoginAction::PickToken),
                            "3" => Some(LoginAction::PickQr),
                            // Gated with the button: without fixtures
                            // there is no demo to enter, and the key
                            // would start a real login with the
                            // literal token "test", which Discord
                            // rejects with a bare 4004.
                            "4" | "d" if cfg!(feature = "fixtures") => Some(LoginAction::PickDemo),
                            _ => None,
                        };
                        if let Some(a) = action {
                            self.handle_login_action(a, window, cx);
                        }
                    }

                    // ---- Password: two-field entry ----------------
                    LoginScreen::Password => {
                        match key {
                            "escape" => {
                                self.handle_login_action(LoginAction::Back, window, cx);
                            }
                            "tab" => {
                                // Cycle focus between login and password fields.
                                if let Screen::Login(l) = &mut self.screen {
                                    l.password.focused_field = l.password.focused_field.next();
                                }
                            }
                            "enter" => {
                                self.handle_login_action(LoginAction::SubmitPassword, window, cx);
                            }
                            _ => {
                                let pasted = (key == "v" && ctrl)
                                    .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                    .flatten();
                                if let Screen::Login(l) = &mut self.screen {
                                    let field = l.password.focused_field;
                                    match field {
                                        PasswordField::Login => l
                                            .password
                                            .login
                                            .handle_key_with_clipboard(event, pasted),
                                        PasswordField::Password => l
                                            .password
                                            .password
                                            .handle_key_with_clipboard(event, pasted),
                                    };
                                }
                            }
                        }
                    }

                    // ---- MFA method select: number keys -----------
                    LoginScreen::MfaSelect => {
                        // Pick a method by number key or Escape to go back.
                        let methods: Vec<MfaMethod> = login
                            .password
                            .mfa
                            .as_ref()
                            .map(|c| c.methods.clone())
                            .unwrap_or_default();
                        match key {
                            "escape" => {
                                self.handle_login_action(LoginAction::Back, window, cx);
                            }
                            "1" if !methods.is_empty() => {
                                self.handle_login_action(
                                    LoginAction::PickMfaMethod(methods[0]),
                                    window,
                                    cx,
                                );
                            }
                            "2" if methods.len() >= 2 => {
                                self.handle_login_action(
                                    LoginAction::PickMfaMethod(methods[1]),
                                    window,
                                    cx,
                                );
                            }
                            _ => {}
                        }
                    }

                    // ---- MFA code entry ---------------------------
                    LoginScreen::MfaCode => match key {
                        "escape" => {
                            if let Screen::Login(l) = &mut self.screen {
                                l.screen = LoginScreen::MfaSelect;
                            }
                        }
                        "enter" => {
                            self.handle_login_action(LoginAction::SubmitMfaCode, window, cx);
                        }
                        _ => {
                            let pasted = (key == "v" && ctrl)
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            if let Screen::Login(l) = &mut self.screen {
                                l.password.mfa_code.handle_key_with_clipboard(event, pasted);
                            }
                        }
                    },

                    // ---- Token entry ------------------------------
                    LoginScreen::Token => match key {
                        "escape" => {
                            self.handle_login_action(LoginAction::Back, window, cx);
                        }
                        _ => {
                            let pasted = (key == "v" && ctrl)
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            let submit = if let Screen::Login(l) = &mut self.screen {
                                l.token.handle_key_with_clipboard(event, pasted)
                            } else {
                                false
                            };
                            if submit {
                                self.handle_login_action(LoginAction::SubmitToken, window, cx);
                            }
                        }
                    },

                    // ---- QR scan: only Escape to cancel ----------
                    LoginScreen::QrScan => {
                        if key == "escape" {
                            self.handle_login_action(LoginAction::Back, window, cx);
                        }
                    }
                }
                return; // consumed
            }
            Screen::Ready => {
                let key = event.keystroke.key.as_str();

                // Modal text entry takes the keyboard outright; the
                // composer only takes unmodified characters, which
                // `resolve` handles via `composer_live`.
                let modal_text = self.pane_filter.is_some()
                    || self.prompt.is_some()
                    || self.editing_status.is_some()
                    || self.editing_activity.is_some()
                    || self.viewing_image.is_some()
                    || self.renaming_folder.is_some()
                    // The search panel owns typing whenever it is open;
                    // it has no separate focus flag.
                    || self.search.is_some();

                if !modal_text || self.keymap.is_pending() {
                    let composer_live = self.focus_pane == Pane::Messages && !modal_text;
                    match self.keymap.resolve(event, composer_live) {
                        Resolution::Action(action) => {
                            if keymap::apply(self, action, cx) {
                                cx.notify();
                                return;
                            }
                        }
                        // Mid-sequence: swallow the key so a leader
                        // chord does not also type its own letters.
                        Resolution::Pending => {
                            // Escape abandons the sequence even when it
                            // is itself a valid next chord; otherwise a
                            // half-typed leader has no way out.
                            if key == "escape" {
                                self.keymap.cancel();
                            }
                            cx.notify();
                            return;
                        }
                        Resolution::Unbound => {}
                    }
                }

                if self.confirming.is_some() {
                    match key {
                        "enter" => self.confirm(),
                        "escape" => self.confirming = None,
                        _ => {}
                    }
                } else if let Some(switcher) = &mut self.switcher {
                    match key {
                        "escape" => self.switcher = None,
                        "up" => switcher.move_selection(-1),
                        "down" => switcher.move_selection(1),
                        "enter" => self.activate_switcher(),
                        _ => {
                            let pasted = (key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            switcher.query.handle_key_with_clipboard(event, pasted);
                            self.rerank_switcher();
                        }
                    }
                } else if key == "k"
                    && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                {
                    self.open_switcher();
                } else if self.prompt.is_some() {
                    match key {
                        "escape" => self.prompt = None,
                        "enter" => self.submit_prompt(),
                        _ => {
                            let pasted = (key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            if let Some((_, text)) = &mut self.prompt {
                                text.handle_key_with_clipboard(event, pasted);
                            }
                        }
                    }
                } else if self.viewing_image.is_some() {
                    match key {
                        "escape" => self.viewing_image = None,
                        "left" => self.step_viewed_image(false),
                        "right" => self.step_viewed_image(true),
                        // Both spellings of each: the key is "+" on
                        // some layouts and "=" on others, and nobody
                        // should have to find out which.
                        "+" | "=" => self.zoom_viewed_image(true),
                        "-" | "_" => self.zoom_viewed_image(false),
                        _ => {}
                    }
                } else if self.editing_activity.is_some() {
                    match key {
                        "escape" => self.editing_activity = None,
                        "enter" => self.submit_activity(),
                        // Tab walks the form, which is what a form is
                        // expected to do and cheaper than reaching for
                        // the mouse between three short fields.
                        "tab" => {
                            if let Some(draft) = &mut self.editing_activity {
                                let back = event.keystroke.modifiers.shift;
                                draft.focused = if back {
                                    (draft.focused + ActivityDraft::FIELDS - 1)
                                        % ActivityDraft::FIELDS
                                } else {
                                    (draft.focused + 1) % ActivityDraft::FIELDS
                                };
                            }
                        }
                        _ => {
                            let pasted = (key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            if let Some(draft) = &mut self.editing_activity {
                                let focused = draft.focused;
                                draft.fields[focused].handle_key_with_clipboard(event, pasted);
                            }
                        }
                    }
                } else if self.editing_status.is_some() {
                    match key {
                        "escape" => self.editing_status = None,
                        "enter" => self.submit_custom_status(),
                        _ => {
                            let pasted = (key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            if let Some(text) = &mut self.editing_status {
                                text.handle_key_with_clipboard(event, pasted);
                            }
                        }
                    }
                } else if self.renaming_folder.is_some() {
                    match key {
                        "escape" => self.renaming_folder = None,
                        "enter" => self.submit_folder_rename(),
                        _ => {
                            let pasted = (key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();
                            if let Some((_, name)) = &mut self.renaming_folder {
                                name.handle_key_with_clipboard(event, pasted);
                            }
                        }
                    }
                } else if self.stream_picker.is_some() && key == "escape" {
                    self.stream_picker = None;
                } else if self.picker.is_some() {
                    // The picker owns the keyboard while open.
                    match key {
                        "escape" => self.picker = None,
                        "left" => self.move_picker(-1),
                        "right" => self.move_picker(1),
                        // The grid reflows with width, so up/down move
                        // by a nominal row rather than a measured one.
                        "up" => self.move_picker(-8),
                        "down" => self.move_picker(8),
                        "enter" => {
                            let glyph = emoji::flat()
                                .get(self.picker.as_ref().map_or(0, |p| p.cursor))
                                .copied();
                            if let Some(glyph) = glyph {
                                self.pick_emoji(glyph);
                            }
                        }
                        _ => {}
                    }
                } else if key == "comma"
                    && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                {
                    self.open_settings_window(cx);
                } else if key == "a"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.mark_all_read();
                } else if key == "o" && event.keystroke.modifiers.control {
                    self.attach_files(cx);
                } else if key == "q"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.sign_out(cx);
                } else if key == "p"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.open_own_profile();
                } else if key == "e"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.compose_externally(cx);
                } else if event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                    && matches!(key, "-" | "=" | "+")
                {
                    // ctrl-shift +/-: output volume. Shifted so it does
                    // not collide with the zoom bindings on the same
                    // keys, which are far more frequently used.
                    self.adjust_output_volume(if key == "-" { -5 } else { 5 });
                } else if event.keystroke.modifiers.control && matches!(key, "d" | "u") {
                    // ctrl-d / ctrl-u: half page, as in vim and less.
                    self.scroll_by_pages(if key == "d" { 0.5 } else { -0.5 });
                } else if event.keystroke.modifiers.control && matches!(key, "home" | "end") {
                    if key == "home" {
                        self.message_scroll
                            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
                    } else {
                        self.message_scroll.scroll_to_bottom();
                    }
                } else if event.keystroke.modifiers.control && matches!(key, "1" | "2" | "3") {
                    let pane = match key {
                        "1" => Pane::Guilds,
                        "2" => Pane::Channels,
                        _ => Pane::Members,
                    };
                    self.toggle_pane(pane);
                } else if self.pane_filter.is_some() && !event.keystroke.modifiers.control {
                    // While filtering, the pane owns typing so the
                    // query does not leak into the composer.
                    if key == "escape" {
                        self.pane_filter = None;
                    } else if let Some(filter) = &mut self.pane_filter
                        && filter.handle_key(event)
                    {
                        self.pane_filter = None;
                    } else if self.focus_pane == Pane::Members {
                        // Filtering members searches the server too:
                        // the member list holds only the ranges self
                        // client subscribed to, so filtering alone
                        // cannot find someone further down it.
                        let query = self
                            .pane_filter
                            .as_ref()
                            .map(|filter| filter.text().to_string())
                            .unwrap_or_default();
                        self.search_members(query);
                    }
                } else if self.focus_pane == Pane::Messages
                    && matches!(
                        key,
                        "up" | "down" | "escape" | "r" | "e" | "y" | "p" | "delete"
                    )
                    && self.composer.is_empty()
                {
                    // Only when the composer is empty: otherwise these
                    // are ordinary characters being typed.
                    match key {
                        "up" => self.move_message_selection(-1),
                        "down" => self.move_message_selection(1),
                        "escape" => self.clear_message_selection(),
                        "r" => self.act_on_selection(MessageAction::Reply),
                        "e" => self.act_on_selection(MessageAction::Edit),
                        "y" => self.act_on_selection(MessageAction::CopyText),
                        "p" => self.act_on_selection(MessageAction::TogglePin),
                        _ => self.act_on_selection(MessageAction::Delete),
                    }
                } else if event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                    && matches!(key, "left" | "right")
                {
                    self.resize_pane(if key == "right" { 20 } else { -20 });
                } else if event.keystroke.modifiers.control && matches!(key, "=" | "+" | "-" | "0")
                {
                    match key {
                        "-" => self.adjust_zoom(-0.1),
                        "0" => crate::theme::set_zoom(1.0),
                        _ => self.adjust_zoom(0.1),
                    }
                } else if key == "t"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.toggle_tts();
                } else if key == "slash" && event.keystroke.modifiers.control {
                    self.toggle_pane_filter();
                } else if key == "tab" && self.slash.is_none() {
                    // Guarded, because the slash picker below claims
                    // tab to complete a command: unguarded self arm
                    // reaches it first and cycles panes instead.
                    self.cycle_focus(!event.keystroke.modifiers.shift);
                } else if key == "escape" {
                    // One branch for the whole key: escape belongs to
                    // the topmost thing on screen, so it has to be
                    // decided in overlay order rather than split into
                    // arms that a bare `key == "escape"` above them
                    // would shadow.
                    if !self.close_popup() {
                        if self.slash.is_some() {
                            self.slash = None;
                        } else if self.profile.is_some() {
                            self.profile = None;
                        } else if self.search.is_some() {
                            self.search = None;
                        } else if self.replying_to.is_some() || self.editing.is_some() {
                            self.cancel_compose_context();
                        } else {
                            self.mark_read();
                        }
                    }
                } else if key == "q"
                    && event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.shift
                {
                    self.quit(cx);
                } else if key == "l"
                    && event.keystroke.modifiers.control
                    && event.keystroke.modifiers.shift
                {
                    self.toggle_debug_log();
                } else if key == "i" && event.keystroke.modifiers.control {
                    self.open_inbox();
                } else if key == "f" && event.keystroke.modifiers.control {
                    self.toggle_search();
                } else if let Some(search) = &mut self.search {
                    // While the search panel is open it owns the
                    // keyboard, so typing does not leak into the
                    // composer behind it.
                    if search.input.handle_key(event) {
                        self.run_search();
                    }
                } else if self.slash.is_some() && matches!(key, "up" | "down" | "tab") {
                    match key {
                        "up" => {
                            if let Some(picker) = &mut self.slash {
                                picker.move_selection(-1);
                            }
                        }
                        "down" => {
                            if let Some(picker) = &mut self.slash {
                                picker.move_selection(1);
                            }
                        }
                        // Tab completes; Enter still sends, so a
                        // fully-typed command is not intercepted.
                        _ => self.accept_slash(),
                    }
                } else {
                    // Read the clipboard only for the paste chord, so
                    // ordinary typing does not hit the platform on
                    // every keystroke.
                    let pasted = (event.keystroke.key == "v"
                        && (event.keystroke.modifiers.control
                            || event.keystroke.modifiers.platform))
                        .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                        .flatten();

                    let send = self.composer.handle_key_with_clipboard(event, pasted);

                    // The composer reports copy/cut rather than
                    // reaching the clipboard itself, so perform it here.
                    // Taken once: a second take would clear the intent
                    // and silently turn every cut into a copy.
                    let intent = self.composer.take_clipboard_intent();
                    if intent != ClipboardIntent::None
                        && let Some(selected) = self.composer.selected_text()
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
                        if intent == ClipboardIntent::Cut {
                            self.composer.cut_selection();
                        }
                    }

                    if send {
                        self.send_message();
                    } else {
                        self.refresh_slash();
                        if !self.composer.is_empty() {
                            self.notify_typing();
                        }
                    }
                }
            }
        }
        cx.notify();
    }
}
