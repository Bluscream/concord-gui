//! The user's keymap, applied to the GUI.
//!
//! `keymap.toml` is shared with the TUI: the same file, the same 55 actions,
//! the same leader sequences and conflict rules. Resolution happens in the
//! core (`tui::keybindings::external`) rather than here, so a custom binding
//! cannot mean one thing in the TUI and another in this client.
//!
//! What belongs here is only the two ends the core cannot know about:
//! translating a GPUI key event into a `KeyPress`, and deciding what each
//! action does to a `Workspace`.

use concord::config;
use concord::tui::keybindings::KeyBindings;
use concord::tui::keybindings::external::UiAction;
use concord::tui::keybindings::external::{Key, KeyPress, PendingSequence, Resolution};
use gpui::{Context, KeyDownEvent};

use crate::ui::messages::MessageAction;
use crate::ui::workspace::{Pane, Workspace};

/// The loaded keymap, plus whatever chord sequence is part-way entered.
pub struct Keymap {
    bindings: KeyBindings,
    pending: PendingSequence,
    /// Warnings from parsing `keymap.toml`, surfaced with the other config
    /// complaints rather than dropped.
    pub warnings: Vec<String>,
}

impl Keymap {
    pub fn load() -> Self {
        let (options, warnings) = config::load_keymap_options_with_warnings().unwrap_or_default();

        Self {
            bindings: KeyBindings::from_options(&options),
            pending: PendingSequence::default(),
            warnings,
        }
    }

    /// Whether a chord sequence is part-way entered.
    ///
    /// While one is, the composer must not receive the keys: a leader sequence
    /// would otherwise type its own letters into the message being written.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn cancel(&mut self) {
        self.pending.clear();
    }

    /// Resolve one key press against the keymap.
    ///
    /// `composer_live` reflects the fundamental difference between the two
    /// front ends. The TUI is modal: plain letters are bindings until you
    /// enter the composer, so `q` quits. This client has no such mode - the
    /// composer is always ready - so an unmodified character must always be
    /// typeable, and only chords and named keys may act as bindings.
    ///
    /// Without this, the TUI's own default keymap would quit the application
    /// when someone began a message with "q".
    pub fn resolve(&mut self, event: &KeyDownEvent, composer_live: bool) -> Resolution {
        let Some(key) = Key::parse(event.keystroke.key.as_str()) else {
            // A key the core does not model cannot match a binding, and must
            // not disturb a sequence in progress.
            return Resolution::Unbound;
        };

        let modifiers = event.keystroke.modifiers;
        let chorded = modifiers.control || modifiers.platform || modifiers.alt;

        // A sequence already in progress is exempt: its later chords are
        // frequently plain letters, and the leader press established intent.
        if composer_live && !chorded && !self.is_pending() && matches!(key, Key::Char(_)) {
            return Resolution::Unbound;
        }

        self.bindings.resolve(
            &mut self.pending,
            KeyPress {
                key,
                // The platform key (cmd) is folded into ctrl, which is how the
                // rest of this client already treats it.
                ctrl: modifiers.control || modifiers.platform,
                alt: modifiers.alt,
                shift: modifiers.shift,
            },
        )
    }
}

/// Carry out an action from the keymap.
///
/// The match is exhaustive on purpose. Adding an action to the core makes this
/// fail to compile, which is the only reliable way to notice that a new TUI
/// binding does nothing here - the class of gap that repeatedly went unseen.
///
/// Returns whether the action was handled. An action with no GUI meaning
/// returns `false` and says why, rather than silently swallowing the key.
pub fn apply(workspace: &mut Workspace, action: UiAction, cx: &mut Context<Workspace>) -> bool {
    match action {
        // ---- navigation ---------------------------------------------------
        UiAction::ChannelSwitcher => workspace.open_switcher(),
        UiAction::SelectNext => workspace.move_message_selection(1),
        UiAction::SelectPrevious => workspace.move_message_selection(-1),
        UiAction::JumpTop => workspace.scroll_to_top(),
        UiAction::JumpBottom => workspace.scroll_to_bottom(),
        UiAction::HalfPageDown => workspace.scroll_by_pages(0.5),
        UiAction::HalfPageUp => workspace.scroll_by_pages(-0.5),
        UiAction::ScrollViewportDown => workspace.scroll_by_pages(0.15),
        UiAction::ScrollViewportUp => workspace.scroll_by_pages(-0.15),

        // ---- panes --------------------------------------------------------
        UiAction::CycleFocusNext => workspace.cycle_focus(true),
        UiAction::CycleFocusPrevious => workspace.cycle_focus(false),
        UiAction::FocusGuildPane => workspace.focus_pane(Pane::Guilds),
        UiAction::FocusChannelPane => workspace.focus_pane(Pane::Channels),
        UiAction::FocusMessagePane => workspace.focus_pane(Pane::Messages),
        UiAction::FocusMemberPane => workspace.focus_pane(Pane::Members),
        UiAction::ToggleGuildPane => workspace.toggle_pane(Pane::Guilds),
        UiAction::ToggleChannelPane => workspace.toggle_pane(Pane::Channels),
        UiAction::ToggleMemberPane => workspace.toggle_pane(Pane::Members),
        UiAction::ResizePaneLeft => workspace.resize_pane(-1),
        UiAction::ResizePaneRight => workspace.resize_pane(1),
        UiAction::OpenPaneFilter => workspace.toggle_pane_filter(),
        UiAction::OpenFocusedPaneAction => workspace.open_focused_pane_action(),

        // ---- messages -----------------------------------------------------
        UiAction::ReplyMessage => workspace.act_on_selection(MessageAction::Reply),
        UiAction::EditMessage => workspace.act_on_selection(MessageAction::Edit),
        UiAction::DeleteMessage => workspace.act_on_selection(MessageAction::Delete),
        UiAction::ReactMessage => workspace.act_on_selection(MessageAction::React),
        UiAction::PinMessage => workspace.act_on_selection(MessageAction::TogglePin),
        UiAction::CopyMessage => workspace.act_on_selection(MessageAction::CopyText),
        UiAction::OpenMessageUrl => workspace.act_on_selection(MessageAction::OpenLink(0)),
        UiAction::OpenThread => workspace.act_on_selection(MessageAction::OpenThread),
        UiAction::ShowMessageProfile => workspace.act_on_selection(MessageAction::OpenProfile),
        UiAction::ShowReactionUsers => workspace.show_first_reaction_users(),
        UiAction::GoToReferencedMessage => workspace.act_on_selection(MessageAction::JumpToReplied),
        UiAction::RemoveMessageEmbeds => workspace.act_on_selection(MessageAction::RemoveEmbeds),
        UiAction::ViewMessageAttachment => {
            workspace.act_on_selection(MessageAction::DownloadAttachment(0))
        }
        UiAction::PlayMedia => workspace.act_on_selection(MessageAction::PlayAttachment(0)),
        UiAction::OpenPollVotePicker => workspace.act_on_selection(MessageAction::VotePoll(0)),
        UiAction::StartComposer => workspace.focus_composer(),

        // ---- voice --------------------------------------------------------
        UiAction::VoiceMute => workspace.toggle_voice_flag(false),
        UiAction::VoiceDeafen => workspace.toggle_voice_flag(true),
        UiAction::VoiceLeave => workspace.leave_voice(),
        UiAction::ToggleStream => workspace.toggle_stream(),

        // ---- panels -------------------------------------------------------
        UiAction::OpenNotificationInbox => workspace.open_inbox(),
        UiAction::OpenCurrentUserProfile => workspace.open_own_profile(),
        UiAction::OpenDebugLog => workspace.toggle_debug_log(),
        UiAction::ClosePopup => workspace.close_popup(),
        // The GUI has one settings window rather than the TUI's four separate
        // options popups. Every one of these opens it, which is closer to the
        // intent than leaving three of the four bindings dead.
        UiAction::OpenOptions
        | UiAction::OpenDisplayOptions
        | UiAction::OpenNotificationOptions
        | UiAction::OpenVoiceOptions
        | UiAction::OpenComposerOptions => workspace.open_settings_window(cx),

        // ---- application --------------------------------------------------
        UiAction::Quit => workspace.quit(cx),
        // The TUI redraws a terminal that another program may have corrupted.
        // GPUI owns its surface, so the useful equivalent is re-fetching the
        // channel, which is what a user pressing this actually wants.
        UiAction::RefreshScreen => workspace.refresh_history(),

        // ---- no GUI equivalent --------------------------------------------
        //
        // The message log wraps rather than scrolling sideways, so there is no
        // horizontal viewport to move. Reported as unhandled so the key falls
        // through instead of appearing to work.
        UiAction::ScrollHorizontalLeft | UiAction::ScrollHorizontalRight => return false,
    }

    true
}
