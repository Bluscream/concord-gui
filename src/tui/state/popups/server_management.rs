//! Administering a server: its invites, its emoji, and what has been done.
//!
//! One popup with three tabs rather than three popups, for the same reason the
//! GUI has one panel: they are all "administering this server", and three
//! entry points to find would be worse than one with tabs in it.

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{AppCommand, AuditLogEntryInfo, GuildEmojiInfo, GuildInviteInfo};
use crate::tui::text_input::{TextEditAction, TextInputState};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

/// Which list the popup is showing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerPanelTab {
    Settings,
    Invites,
    Roles,
    Emoji,
    Sounds,
    AutoMod,
    AuditLog,
    /// The welcome screen, the widget, and pruning - how members arrive and
    /// leave. One tab because each is two or three rows, and three tabs of
    /// three rows is more hunting than reading.
    Membership,
    Events,
    Templates,
}

impl ServerPanelTab {
    pub const ALL: [Self; 10] = [
        Self::Settings,
        Self::Invites,
        Self::Roles,
        Self::Emoji,
        Self::Sounds,
        Self::AutoMod,
        Self::AuditLog,
        Self::Membership,
        Self::Events,
        Self::Templates,
    ];

    pub(in crate::tui) fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Invites => "Invites",
            Self::Roles => "Roles",
            Self::Emoji => "Emoji",
            Self::Sounds => "Sounds",
            Self::AutoMod => "AutoMod",
            Self::AuditLog => "Audit log",
            Self::Membership => "Membership",
            Self::Events => "Events",
            Self::Templates => "Templates",
        }
    }

    /// What to fetch when this tab opens.
    ///
    /// A list rather than one command: membership needs three, and an earlier
    /// version that returned one and queued the rest at the call site meant
    /// the tab-switch path fetched nothing at all. Empty for settings and
    /// roles, which arrive with the guild and are read from the snapshot.
    fn load(self, guild_id: Id<GuildMarker>) -> Vec<AppCommand> {
        match self {
            Self::Settings | Self::Roles => Vec::new(),
            Self::Invites => vec![AppCommand::LoadGuildInvites { guild_id }],
            Self::Emoji => vec![AppCommand::LoadGuildEmojis { guild_id }],
            Self::Sounds => vec![AppCommand::LoadSoundboardSounds {
                guild_id: Some(guild_id),
            }],
            Self::AutoMod => vec![AppCommand::LoadAutoModRules { guild_id }],
            Self::AuditLog => vec![AppCommand::LoadGuildAuditLog { guild_id }],
            Self::Events => vec![AppCommand::LoadScheduledEvents { guild_id }],
            Self::Templates => vec![AppCommand::LoadGuildTemplates { guild_id }],
            Self::Membership => vec![
                AppCommand::LoadWelcomeScreen { guild_id },
                AppCommand::LoadGuildWidget { guild_id },
                AppCommand::LoadPruneCount {
                    guild_id,
                    days: DEFAULT_PRUNE_DAYS,
                    include_roles: Vec::new(),
                },
            ],
        }
    }
}

/// The membership tab's rows, in order.
///
/// A list rather than match arms on an index: rows were renumbered by hand
/// three times elsewhere in this client and it broke the tests every time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MembershipRow {
    WelcomeEnabled,
    WelcomeDescription,
    WidgetEnabled,
    WidgetChannel,
    PruneDays,
    Prune,
}

/// Discord's own default prune window.
const DEFAULT_PRUNE_DAYS: u16 = 30;

pub(in crate::tui) const fn membership_label(row: MembershipRow) -> &'static str {
    match row {
        MembershipRow::WelcomeEnabled => "Welcome screen",
        MembershipRow::WelcomeDescription => "Welcome description",
        MembershipRow::WidgetEnabled => "Widget",
        MembershipRow::WidgetChannel => "Widget invite channel",
        MembershipRow::PruneDays => "Prune inactive after",
        MembershipRow::Prune => "Prune",
    }
}

pub(in crate::tui) const MEMBERSHIP_ROWS: [MembershipRow; 6] = [
    MembershipRow::WelcomeEnabled,
    MembershipRow::WelcomeDescription,
    MembershipRow::WidgetEnabled,
    MembershipRow::WidgetChannel,
    MembershipRow::PruneDays,
    MembershipRow::Prune,
];

/// What the emoji text field is being used for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmojiEdit {
    /// A new name for the guild.
    GuildName,
    /// A path to an image to use as the guild icon.
    GuildIcon,
    /// A new name for the emoji at this index.
    Rename(usize),
    /// A path to an image to add.
    AddImage,
    /// A name for a new role.
    NewRole,
    /// A name for a new server template.
    NewTemplate,
    /// The line shown to people arriving at the server.
    WelcomeDescription,
    /// The channel the widget's invite points at, by name.
    WidgetChannel,
    /// A new scheduled event, typed as one line of fields.
    ///
    /// One field rather than five: the panel has one text input, and five
    /// stacked prompts to fill in order would be worse than one line with a
    /// stated format.
    NewEvent,
    /// An existing event, by its id. Same line format as creating one.
    EditEvent(u64),
}

// Not Eq: a sound carries a float volume, so the panel can only be compared
// partially.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui) struct ServerManagementState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) tab: ServerPanelTab,
    pub(super) selection: SelectablePopupState,
    pub(super) invites: Vec<GuildInviteInfo>,
    /// Read from the snapshot when the tab opens, highest first - the order
    /// that decides which role wins a conflict.
    pub(super) roles: Vec<crate::discord::RoleState>,
    /// The guild's settings as label and value, read from the snapshot.
    pub(super) settings: Vec<(String, String)>,
    pub(super) emojis: Vec<GuildEmojiInfo>,
    /// The guild's own sounds. The default sounds belong in the picker, not
    /// here: they cannot be renamed or deleted by anyone.
    pub(super) sounds: Vec<crate::discord::SoundboardSound>,
    pub(super) automod: Vec<crate::discord::AutoModRule>,
    pub(super) audit_log: Vec<AuditLogEntryInfo>,
    pub(super) welcome: Option<crate::discord::WelcomeScreen>,
    pub(super) widget: Option<crate::discord::GuildWidget>,
    /// How far back a prune would reach, and how many it would remove.
    pub(super) prune_days: u16,
    /// `None` until the count has been asked for - which is not the same as
    /// zero, and a panel that showed them alike would offer to prune nobody.
    pub(super) prune_count: Option<u64>,
    pub(super) events: Vec<crate::discord::ScheduledEvent>,
    pub(super) templates: Vec<crate::discord::GuildTemplate>,
    /// Set while the open tab's fetch is outstanding, so the popup can say so
    /// rather than looking like an empty list.
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    /// The emoji field being filled in, while one is open.
    pub(super) renaming: Option<(EmojiEdit, TextInputState)>,
}

impl ServerManagementState {
    pub(in crate::tui) fn tab(&self) -> ServerPanelTab {
        self.tab
    }

    pub(in crate::tui) fn invites(&self) -> &[GuildInviteInfo] {
        &self.invites
    }

    pub(in crate::tui) fn sounds(&self) -> &[crate::discord::SoundboardSound] {
        &self.sounds
    }

    pub(in crate::tui) fn automod(&self) -> &[crate::discord::AutoModRule] {
        &self.automod
    }

    pub(in crate::tui) fn emojis(&self) -> &[GuildEmojiInfo] {
        &self.emojis
    }

    pub(in crate::tui) fn audit_log(&self) -> &[AuditLogEntryInfo] {
        &self.audit_log
    }

    pub(in crate::tui) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(in crate::tui) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// How many rows the open tab has, which is what the selection moves over.
    /// The text being typed, while an emoji field is open.
    pub(in crate::tui) fn renaming(&self) -> Option<(EmojiEdit, &TextInputState)> {
        self.renaming.as_ref().map(|(edit, input)| (*edit, input))
    }

    pub(in crate::tui) fn row_count(&self) -> usize {
        match self.tab {
            ServerPanelTab::Invites => self.invites.len(),
            ServerPanelTab::Settings => self.settings.len(),
            ServerPanelTab::Roles => self.roles.len(),
            ServerPanelTab::Emoji => self.emojis.len(),
            ServerPanelTab::Sounds => self.sounds.len(),
            ServerPanelTab::AutoMod => self.automod.len(),
            ServerPanelTab::AuditLog => self.audit_log.len(),
            // Welcome screen on/off, its description, the widget, the widget's
            // channel, the prune window, and the prune itself.
            ServerPanelTab::Membership => MEMBERSHIP_ROWS.len(),
            ServerPanelTab::Events => self.events.len(),
            ServerPanelTab::Templates => self.templates.len(),
        }
    }

    pub(in crate::tui) fn roles(&self) -> &[crate::discord::RoleState] {
        &self.roles
    }

    pub(in crate::tui) fn settings(&self) -> &[(String, String)] {
        &self.settings
    }
}

impl DashboardState {
    pub fn open_server_management(
        &mut self,
        guild_id: Id<GuildMarker>,
        tab: ServerPanelTab,
    ) -> Option<AppCommand> {
        self.popups
            .set_modal(ModalPopup::ServerManagement(ServerManagementState {
                guild_id,
                tab,
                selection: SelectablePopupState::default(),
                invites: Vec::new(),
                roles: Vec::new(),
                settings: Vec::new(),
                emojis: Vec::new(),
                sounds: Vec::new(),
                automod: Vec::new(),
                audit_log: Vec::new(),
                welcome: None,
                widget: None,
                prune_days: DEFAULT_PRUNE_DAYS,
                prune_count: None,
                events: Vec::new(),
                templates: Vec::new(),
                loading: true,
                error: None,
                renaming: None,
            }));
        // Roles need no fetch, so opening on that tab fills from the snapshot
        // and asks for nothing.
        match tab {
            ServerPanelTab::Roles => self.fill_server_roles(),
            ServerPanelTab::Settings => self.fill_guild_settings(),
            _ => {}
        }
        self.queue_tab_fetches(tab, guild_id)
    }

    pub fn close_server_management(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ServerManagement) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn server_management_state(&self) -> Option<&ServerManagementState> {
        self.popups.server_management()
    }

    /// Queue a tab's fetches, returning the first for the caller to send.
    ///
    /// One place rather than three: every caller of `load` had the same
    /// "return one, queue the rest" shape, and the tab-switch copy of it was
    /// missing entirely, so switching to a multi-fetch tab fetched nothing.
    fn queue_tab_fetches(
        &mut self,
        tab: ServerPanelTab,
        guild_id: Id<GuildMarker>,
    ) -> Option<AppCommand> {
        let mut fetches = tab.load(guild_id).into_iter();
        let first = fetches.next();
        for command in fetches {
            self.enqueue_pending_command(command);
        }
        first
    }

    /// Move to the next tab, fetching its list if it has not arrived.
    pub fn next_server_tab(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        let index = ServerPanelTab::ALL
            .iter()
            .position(|tab| *tab == state.tab)
            .unwrap_or(0);
        let tab = ServerPanelTab::ALL[(index + 1) % ServerPanelTab::ALL.len()];

        state.tab = tab;
        state.error = None;
        state.selection = SelectablePopupState::default();

        // Only fetch what has not arrived. Refetching on every tab switch
        // would spend requests for no new information; that is what reload is
        // for.
        let already_loaded = match tab {
            ServerPanelTab::Invites => !state.invites.is_empty(),
            ServerPanelTab::Settings | ServerPanelTab::Roles => true,
            ServerPanelTab::Emoji => !state.emojis.is_empty(),
            ServerPanelTab::Sounds => !state.sounds.is_empty(),
            ServerPanelTab::AutoMod => !state.automod.is_empty(),
            ServerPanelTab::AuditLog => !state.audit_log.is_empty(),
            ServerPanelTab::Membership => state.welcome.is_some() && state.widget.is_some(),
            ServerPanelTab::Events => !state.events.is_empty(),
            ServerPanelTab::Templates => !state.templates.is_empty(),
        };
        state.loading = !already_loaded;
        let guild_id = state.guild_id;
        match tab {
            ServerPanelTab::Roles => self.fill_server_roles(),
            ServerPanelTab::Settings => self.fill_guild_settings(),
            _ => {}
        }
        if already_loaded {
            return None;
        }
        self.queue_tab_fetches(tab, guild_id)
    }

    /// Delete the highlighted AutoMod rule.
    ///
    /// Separate from enter, which toggles: deleting throws away the keyword
    /// list, and the destructive path should not be the one under the key
    /// people press without looking.
    pub fn delete_selected_automod_rule(&mut self) -> Option<AppCommand> {
        let index = self.selected_server_row()?;
        let state = self.popups.server_management_mut()?;
        if state.tab != ServerPanelTab::AutoMod || index >= state.automod.len() {
            return None;
        }
        let guild_id = state.guild_id;
        let rule = state.automod.remove(index);

        Some(AppCommand::DeleteAutoModRule {
            guild_id,
            rule_id: rule.id,
            label: rule.name,
        })
    }

    /// Start renaming the highlighted emoji.
    ///
    /// Seeded with the current name: a rename is usually a correction, and
    /// retyping the whole thing to fix one letter is busywork.
    pub fn start_emoji_rename(&mut self) {
        let Some(index) = self.selected_server_row() else {
            return;
        };
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        // Emoji and sounds both rename; the other tabs have nothing to.
        if !matches!(state.tab, ServerPanelTab::Emoji | ServerPanelTab::Sounds) {
            return;
        }
        let current = match state.tab {
            ServerPanelTab::Emoji => state.emojis.get(index).map(|emoji| emoji.name.clone()),
            ServerPanelTab::Sounds => state.sounds.get(index).map(|sound| sound.name.clone()),
            _ => None,
        };
        let Some(current) = current else {
            return;
        };

        let mut input = TextInputState::default();
        input.set_value(current);
        state.renaming = Some((EmojiEdit::Rename(index), input));
    }

    /// Start setting the guild's icon from an image on disk.
    pub fn start_guild_icon(&mut self) {
        let Some(guild_id) = self.popups.server_management().map(|state| state.guild_id) else {
            return;
        };
        if !self.discord.cache.can_manage_guild(guild_id) {
            self.show_error_toast(
                "you do not have permission".to_owned(),
                std::time::Instant::now(),
            );
            return;
        }
        if let Some(state) = self.popups.server_management_mut()
            && state.tab == ServerPanelTab::Settings
        {
            state.renaming = Some((EmojiEdit::GuildIcon, TextInputState::default()));
        }
    }

    /// Start adding an emoji from an image on disk.
    pub fn start_emoji_upload(&mut self) {
        if let Some(state) = self.popups.server_management_mut()
            && state.tab == ServerPanelTab::Emoji
        {
            state.renaming = Some((EmojiEdit::AddImage, TextInputState::default()));
        }
    }

    pub fn cancel_emoji_rename(&mut self) {
        if let Some(state) = self.popups.server_management_mut() {
            state.renaming = None;
        }
    }

    pub fn insert_emoji_rename_char(&mut self, value: char) {
        if let Some(state) = self.popups.server_management_mut()
            && let Some((_, input)) = &mut state.renaming
        {
            input.insert_char(value);
        }
    }

    pub fn edit_emoji_rename(&mut self, action: TextEditAction) {
        if let Some(state) = self.popups.server_management_mut()
            && let Some((_, input)) = &mut state.renaming
        {
            input.apply_edit_action(action);
        }
    }

    /// Send whatever the emoji field was for.
    pub fn submit_emoji_rename(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        let (edit, input) = state.renaming.take()?;
        let text = input.value().trim().to_owned();
        let guild_id = state.guild_id;

        let index = match edit {
            EmojiEdit::Rename(index) => index,
            EmojiEdit::GuildIcon => {
                if text.is_empty() {
                    return None;
                }
                return Some(AppCommand::SetGuildIcon {
                    guild_id,
                    image: Box::new(crate::discord::ProfileAvatarUpload::from_path(text.into())),
                    label: self
                        .discord
                        .guild(guild_id)
                        .map(|guild| guild.name.clone())
                        .unwrap_or_default(),
                });
            }
            EmojiEdit::GuildName => {
                if !crate::discord::is_valid_guild_name(&text) {
                    return None;
                }
                self.fill_guild_settings();
                return Some(AppCommand::ModifyGuild {
                    guild_id,
                    edit: Box::new(crate::discord::GuildEdit {
                        name: Some(text.clone()),
                        ..crate::discord::GuildEdit::default()
                    }),
                    label: text,
                });
            }
            EmojiEdit::NewRole => {
                if text.is_empty() {
                    return None;
                }
                return Some(AppCommand::CreateRole {
                    guild_id,
                    name: text,
                });
            }
            EmojiEdit::EditEvent(event_id) => {
                let event = crate::discord::parse_new_event(&text)?;
                if event.problem().is_some() {
                    return None;
                }
                return Some(AppCommand::ModifyScheduledEvent {
                    guild_id,
                    event_id,
                    event: Box::new(event),
                });
            }
            EmojiEdit::NewEvent => {
                let event = crate::discord::parse_new_event(&text)?;
                // Refused here rather than by Discord, whose message does not
                // say which of five fields is the problem.
                if event.problem().is_some() {
                    return None;
                }
                return Some(AppCommand::CreateScheduledEvent {
                    guild_id,
                    event: Box::new(event),
                });
            }
            EmojiEdit::WelcomeDescription => {
                return Some(AppCommand::ModifyWelcomeScreen {
                    guild_id,
                    // Empty clears it, which is a real thing to want and is
                    // distinct from leaving the description alone.
                    edit: crate::discord::WelcomeScreenEdit {
                        description: Some((!text.is_empty()).then_some(text)),
                        ..Default::default()
                    },
                });
            }
            EmojiEdit::WidgetChannel => {
                let mut widget = self
                    .popups
                    .server_management()
                    .and_then(|state| state.widget.clone())
                    .unwrap_or_default();
                // Empty means "issue no invite", which the endpoint expresses
                // as a null channel rather than an omitted one.
                widget.channel_id = if text.is_empty() {
                    None
                } else {
                    self.channel_id_by_name(guild_id, &text)
                };
                if !text.is_empty() && widget.channel_id.is_none() {
                    // A name nobody recognises would otherwise silently clear
                    // the invite, which is not what was asked for.
                    return None;
                }
                return Some(AppCommand::ModifyGuildWidget { guild_id, widget });
            }
            EmojiEdit::NewTemplate => {
                if text.is_empty() {
                    return None;
                }
                return Some(AppCommand::CreateGuildTemplate {
                    guild_id,
                    name: text,
                });
            }
            EmojiEdit::AddImage => {
                if text.is_empty() {
                    return None;
                }
                // The name comes from the filename, which is what people mean
                // nine times out of ten and can be corrected with a rename.
                let Some(name) = crate::discord::emoji_name_from_filename(&text) else {
                    self.show_error_toast(
                        format!("{text} does not make a usable emoji name"),
                        std::time::Instant::now(),
                    );
                    return None;
                };
                return Some(AppCommand::CreateEmoji {
                    guild_id,
                    name,
                    image: Box::new(crate::discord::ProfileAvatarUpload::from_path(text.into())),
                });
            }
        };

        let name = text;
        if state.tab == ServerPanelTab::Sounds {
            let sound = state.sounds.get_mut(index)?;
            if !crate::discord::is_valid_sound_name(&name) || sound.name == name {
                return None;
            }
            // Applied locally too: the list is a snapshot, and leaving the old
            // name showing makes a successful rename look like it failed.
            sound.name = name.clone();
            let sound_id = sound.sound_id;
            return Some(AppCommand::RenameSoundboardSound {
                guild_id,
                sound_id,
                name,
            });
        }
        let emoji = state.emojis.get_mut(index)?;

        // An empty name is a cancel rather than a rename to nothing, which
        // Discord would reject anyway.
        if name.is_empty() || emoji.name == name {
            return None;
        }

        // Applied locally too: the list is a snapshot, and leaving the old
        // name showing makes a successful rename look like it failed.
        emoji.name = name.clone();
        Some(AppCommand::RenameEmoji {
            guild_id,
            emoji_id: emoji.id,
            name,
        })
    }

    pub fn reload_server_management(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        let (tab, guild_id) = (state.tab, state.guild_id);
        // Roles come from the snapshot, so a refresh re-reads rather than
        // spending a request that would fetch nothing.
        match tab {
            ServerPanelTab::Roles => {
                self.fill_server_roles();
                return None;
            }
            ServerPanelTab::Settings => {
                self.fill_guild_settings();
                return None;
            }
            _ => {}
        }
        state.loading = true;
        state.error = None;
        self.queue_tab_fetches(tab, guild_id)
    }

    pub fn move_server_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::ServerManagement,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_server_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::ServerManagement,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    /// Cancel or delete the highlighted event.
    ///
    /// Cancelling first: Discord keeps a cancelled event visible so people who
    /// said they were coming can see it is off, which deleting does not.
    pub fn remove_selected_event(&mut self) -> Option<AppCommand> {
        let index = self.selected_server_row()?;
        let state = self.popups.server_management_mut()?;
        if state.tab != ServerPanelTab::Events {
            return None;
        }
        let guild_id = state.guild_id;
        let event = state.events.get(index)?;
        if event.status.is_cancellable() {
            return Some(AppCommand::CancelScheduledEvent {
                guild_id,
                event_id: event.id,
                label: event.name.clone(),
            });
        }
        // Already finished, so there is nothing to cancel and the row can only
        // be removed outright.
        let event = state.events.remove(index);
        Some(AppCommand::DeleteScheduledEvent {
            guild_id,
            event_id: event.id,
            label: event.name,
        })
    }

    pub fn delete_selected_template(&mut self) -> Option<AppCommand> {
        let index = self.selected_server_row()?;
        let state = self.popups.server_management_mut()?;
        if state.tab != ServerPanelTab::Templates || index >= state.templates.len() {
            return None;
        }
        let guild_id = state.guild_id;
        let template = state.templates.remove(index);
        Some(AppCommand::DeleteGuildTemplate {
            guild_id,
            code: template.code,
            label: template.name,
        })
    }

    pub(in crate::tui) fn set_scheduled_events(
        &mut self,
        events: Vec<crate::discord::ScheduledEvent>,
    ) {
        if let Some(state) = self.popups.server_management_mut() {
            state.events = events;
            state.loading = false;
        }
    }

    pub(in crate::tui) fn set_guild_templates(
        &mut self,
        templates: Vec<crate::discord::GuildTemplate>,
    ) {
        if let Some(state) = self.popups.server_management_mut() {
            state.templates = templates;
            state.loading = false;
        }
    }

    pub(in crate::tui) fn scheduled_events(&self) -> &[crate::discord::ScheduledEvent] {
        self.popups
            .server_management()
            .map_or(&[], |state| state.events.as_slice())
    }

    pub(in crate::tui) fn guild_templates(&self) -> &[crate::discord::GuildTemplate] {
        self.popups
            .server_management()
            .map_or(&[], |state| state.templates.as_slice())
    }

    /// The membership tab's rows as label and value.
    pub(in crate::tui) fn membership_rows(&self) -> Vec<(String, String)> {
        let Some(state) = self.popups.server_management() else {
            return Vec::new();
        };
        MEMBERSHIP_ROWS
            .into_iter()
            .map(|row| {
                let value = match row {
                    MembershipRow::WelcomeEnabled => state
                        .welcome
                        .as_ref()
                        // Says "unknown" rather than "off": a screen that has
                        // not arrived is not one Discord confirmed is off.
                        .map_or("unknown".to_owned(), |screen| {
                            if screen.enabled { "on" } else { "off" }.to_owned()
                        }),
                    MembershipRow::WelcomeDescription => state
                        .welcome
                        .as_ref()
                        .and_then(|screen| screen.description.clone())
                        .unwrap_or_else(|| "not set".to_owned()),
                    MembershipRow::WidgetEnabled => state
                        .widget
                        .as_ref()
                        .map_or("unknown".to_owned(), |widget| {
                            if widget.enabled { "on" } else { "off" }.to_owned()
                        }),
                    MembershipRow::WidgetChannel => state
                        .widget
                        .as_ref()
                        .and_then(|widget| widget.channel_id)
                        .map_or_else(
                            || "no invite".to_owned(),
                            |channel_id| self.channel_label(channel_id),
                        ),
                    MembershipRow::PruneDays => format!("{} days", state.prune_days),
                    MembershipRow::Prune => match state.prune_count {
                        // Zero is a real answer and the commonest one: Discord
                        // exempts every member who has any role at all.
                        Some(count) => format!("{count} members would be removed"),
                        None => "counting".to_owned(),
                    },
                };
                (membership_label(row).to_owned(), value)
            })
            .collect()
    }

    /// The prune the membership tab is offering, for the risk prompt.
    pub(in crate::tui) fn pending_prune(&self) -> Option<AppCommand> {
        let state = self.popups.server_management()?;
        if state.tab != ServerPanelTab::Membership {
            return None;
        }
        if MEMBERSHIP_ROWS.get(self.selected_server_row()?) != Some(&MembershipRow::Prune) {
            return None;
        }
        // Nothing to confirm when the count is zero or has not arrived: a
        // warning about removing nobody teaches the wrong lesson about the
        // warning.
        if state.prune_count.unwrap_or(0) == 0 {
            return None;
        }
        Some(AppCommand::PruneGuild {
            guild_id: state.guild_id,
            days: state.prune_days,
            include_roles: Vec::new(),
            label: self
                .guild_name(state.guild_id)
                .unwrap_or("this server")
                .to_owned(),
        })
    }

    pub(in crate::tui) fn set_welcome_screen(&mut self, screen: crate::discord::WelcomeScreen) {
        if let Some(state) = self.popups.server_management_mut() {
            state.welcome = Some(screen);
            state.loading = false;
        }
    }

    pub(in crate::tui) fn set_guild_widget(&mut self, widget: crate::discord::GuildWidget) {
        if let Some(state) = self.popups.server_management_mut() {
            state.widget = Some(widget);
            state.loading = false;
        }
    }

    pub(in crate::tui) fn set_prune_count(&mut self, count: u64) {
        if let Some(state) = self.popups.server_management_mut() {
            state.prune_count = Some(count);
        }
    }

    pub(in crate::tui) fn selected_server_row(&self) -> Option<usize> {
        self.popups
            .server_management()
            .map(|state| state.selection.selected_for_len(state.row_count()))
    }

    /// Act on the highlighted row: revoke an invite, or delete an emoji.
    ///
    /// The row goes straight away rather than waiting for a refetch: the list
    /// is a snapshot, and leaving a revoked invite on screen invites a second
    /// revoke for a code that no longer exists. The audit log has no action -
    /// history is a record, not something to be edited from here.
    pub fn activate_selected_server_row(&mut self) -> Option<AppCommand> {
        let index = self.selected_server_row()?;
        let state = self.popups.server_management_mut()?;
        let guild_id = state.guild_id;

        match state.tab {
            ServerPanelTab::Events => {
                let event = state.events.get(index)?;
                // Enter says you are coming rather than cancelling: interest
                // is what most people open this list to change, and cancelling
                // is destructive.
                Some(AppCommand::SetEventInterest {
                    guild_id,
                    event_id: event.id,
                    interested: true,
                })
            }
            ServerPanelTab::Templates => {
                let template = state.templates.get(index)?;
                // Syncing rather than deleting, for the same reason.
                Some(AppCommand::SyncGuildTemplate {
                    guild_id,
                    code: template.code.clone(),
                    label: template.name.clone(),
                })
            }
            ServerPanelTab::Membership => {
                let row = *MEMBERSHIP_ROWS.get(index)?;
                match row {
                    MembershipRow::WelcomeEnabled => {
                        let screen = state.welcome.as_mut()?;
                        screen.enabled = !screen.enabled;
                        Some(AppCommand::ModifyWelcomeScreen {
                            guild_id,
                            edit: crate::discord::WelcomeScreenEdit {
                                enabled: Some(screen.enabled),
                                ..Default::default()
                            },
                        })
                    }
                    MembershipRow::WidgetEnabled => {
                        let widget = state.widget.as_mut()?;
                        widget.enabled = !widget.enabled;
                        Some(AppCommand::ModifyGuildWidget {
                            guild_id,
                            widget: widget.clone(),
                        })
                    }
                    MembershipRow::PruneDays => {
                        // Cycles through what Discord accepts, wrapping, so
                        // every window is reachable from every other.
                        let current = crate::discord::PRUNE_DAYS
                            .iter()
                            .position(|days| *days == state.prune_days)
                            .unwrap_or(0);
                        let next = (current + 1) % crate::discord::PRUNE_DAYS.len();
                        state.prune_days = crate::discord::PRUNE_DAYS[next];
                        // The old count describes the old window, so it is
                        // cleared rather than left to describe the wrong one.
                        state.prune_count = None;
                        Some(AppCommand::LoadPruneCount {
                            guild_id,
                            days: state.prune_days,
                            include_roles: Vec::new(),
                        })
                    }
                    // The description, the widget channel and the prune itself
                    // are not plain toggles: the first two need text, and the
                    // last is irreversible and goes through the risk prompt.
                    MembershipRow::WelcomeDescription
                    | MembershipRow::WidgetChannel
                    | MembershipRow::Prune => None,
                }
            }
            ServerPanelTab::Invites => {
                if index >= state.invites.len() {
                    return None;
                }
                let invite = state.invites.remove(index);
                Some(AppCommand::RevokeInvite { code: invite.code })
            }
            ServerPanelTab::Emoji => {
                if index >= state.emojis.len() {
                    return None;
                }
                let emoji = state.emojis.remove(index);
                Some(AppCommand::DeleteEmoji {
                    guild_id,
                    emoji_id: emoji.id,
                    label: emoji.name,
                })
            }
            ServerPanelTab::Settings => {
                if !self.discord.cache.can_manage_guild(guild_id) {
                    self.show_error_toast(
                        "you do not have permission".to_owned(),
                        std::time::Instant::now(),
                    );
                    return None;
                }
                match index {
                    // Name: a field. Verification: a cycle, since it is a
                    // fixed set and typing a number would mean nothing.
                    0 => {
                        let current = self.discord.guild(guild_id)?.name.clone();
                        let mut input = TextInputState::default();
                        input.set_value(current);
                        if let Some(state) = self.popups.server_management_mut() {
                            state.renaming = Some((EmojiEdit::GuildName, input));
                        }
                        None
                    }
                    1 => self.cycle_guild_verification(guild_id),
                    // Owner and boosts are facts about the guild rather than
                    // settings, so there is nothing to change.
                    _ => None,
                }
            }
            ServerPanelTab::Roles => {
                if index >= state.roles.len() {
                    return None;
                }
                let role = state.roles[index].clone();
                // @everyone is the guild id and cannot be deleted; Discord
                // refuses, so the row says why rather than failing.
                if role.id.get() == guild_id.get() {
                    self.show_error_toast(
                        "@everyone cannot be deleted".to_owned(),
                        std::time::Instant::now(),
                    );
                    return None;
                }
                // Only a role below your own: Discord refuses otherwise, and
                // the refusal is worth explaining rather than round-tripping.
                if !self.can_edit_role(guild_id, &role) {
                    self.show_error_toast(
                        format!("{} is at or above your highest role", role.name),
                        std::time::Instant::now(),
                    );
                    return None;
                }

                if let Some(state) = self.popups.server_management_mut() {
                    state.roles.remove(index);
                }
                Some(AppCommand::DeleteRole {
                    guild_id,
                    role_id: role.id,
                    label: role.name,
                })
            }
            ServerPanelTab::Sounds => {
                if index >= state.sounds.len() {
                    return None;
                }
                let sound = state.sounds.remove(index);
                Some(AppCommand::DeleteSoundboardSound {
                    guild_id,
                    sound_id: sound.sound_id,
                    label: sound.name,
                })
            }
            ServerPanelTab::AutoMod => {
                let rule = state.automod.get(index)?.clone();
                // Toggling rather than deleting: a rule switched off can be
                // switched back on, and its keyword list survives. Deleting is
                // the destructive path and is not on enter.
                let enabled = !rule.enabled;
                state.automod[index].enabled = enabled;
                Some(AppCommand::SetAutoModRuleEnabled {
                    guild_id,
                    rule_id: rule.id,
                    enabled,
                    label: rule.name,
                })
            }
            ServerPanelTab::AuditLog => None,
        }
    }

    /// Whether this account may change a role.
    ///
    /// Discord refuses any change to a role at or above your own highest,
    /// which is what stops someone granting themselves more than they have.
    fn can_edit_role(&self, guild_id: Id<GuildMarker>, role: &crate::discord::RoleState) -> bool {
        if !self.discord.cache.can_manage_roles(guild_id) {
            return false;
        }
        self.discord.cache.can_assign_role(guild_id, role.id)
    }

    /// Start creating a role, reusing the emoji name field.
    pub fn start_role_create(&mut self) {
        if let Some(state) = self.popups.server_management_mut()
            && state.tab == ServerPanelTab::Roles
        {
            state.renaming = Some((EmojiEdit::NewRole, TextInputState::default()));
        }
    }

    /// The channel with this name in the guild, if there is exactly one.
    ///
    /// Names are not unique in Discord, so an ambiguous one resolves to
    /// nothing rather than to whichever happened to be first - picking one
    /// would point the widget's invite at a channel nobody chose.
    fn channel_id_by_name(
        &self,
        guild_id: Id<GuildMarker>,
        name: &str,
    ) -> Option<Id<crate::discord::ids::marker::ChannelMarker>> {
        let wanted = name.trim().trim_start_matches('#');
        let mut matches = self
            .discord
            .cache
            .channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(|channel| channel.name == wanted);
        let first = matches.next()?;
        matches.next().is_none().then_some(first.id)
    }

    /// Start editing the highlighted event.
    ///
    /// Seeded with the event as one line, so a change is a correction rather
    /// than a retype - and so what is shown is what will be sent back.
    pub fn start_event_edit(&mut self) {
        let Some(index) = self.selected_server_row() else {
            return;
        };
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.tab != ServerPanelTab::Events {
            return;
        }
        let Some(event) = state.events.get(index) else {
            return;
        };
        let (id, line) = (event.id, event.to_line());
        let mut input = TextInputState::default();
        input.set_value(line);
        state.renaming = Some((EmojiEdit::EditEvent(id), input));
    }

    /// Start creating a scheduled event.
    pub fn start_event_create(&mut self) {
        if let Some(state) = self.popups.server_management_mut()
            && state.tab == ServerPanelTab::Events
        {
            state.renaming = Some((EmojiEdit::NewEvent, TextInputState::default()));
        }
    }

    /// Start editing whichever membership row needs text.
    ///
    /// The two that are not toggles: the welcome description and the widget's
    /// invite channel. Seeded with what is there, since both are usually being
    /// corrected rather than written from nothing.
    pub fn start_membership_edit(&mut self) {
        let Some(index) = self.selected_server_row() else {
            return;
        };
        let Some(row) = MEMBERSHIP_ROWS.get(index).copied() else {
            return;
        };
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.tab != ServerPanelTab::Membership {
            return;
        }
        let (edit, current) = match row {
            MembershipRow::WelcomeDescription => (
                EmojiEdit::WelcomeDescription,
                state
                    .welcome
                    .as_ref()
                    .and_then(|screen| screen.description.clone())
                    .unwrap_or_default(),
            ),
            MembershipRow::WidgetChannel => (EmojiEdit::WidgetChannel, String::new()),
            // The rest are toggles, which enter already handles.
            _ => return,
        };
        let mut input = TextInputState::default();
        input.set_value(current);
        state.renaming = Some((edit, input));
    }

    /// Test helpers for the widget-channel field, which is otherwise only
    /// reachable by selecting the right row first.
    #[cfg(test)]
    pub(in crate::tui) fn start_membership_edit_for_widget_channel(&mut self) {
        if let Some(state) = self.popups.server_management_mut() {
            state.renaming = Some((EmojiEdit::WidgetChannel, TextInputState::default()));
        }
    }

    #[cfg(test)]
    pub(in crate::tui) fn set_membership_edit_text(&mut self, text: &str) {
        if let Some(state) = self.popups.server_management_mut()
            && let Some((_, input)) = state.renaming.as_mut()
        {
            input.set_value(text.to_owned());
        }
    }

    #[cfg(test)]
    pub(in crate::tui) fn discord_cache_channel_names(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Vec<String> {
        let channels = self.discord.cache.channels_for_guild(Some(guild_id));
        channels
            .iter()
            .map(|channel| channel.name.clone())
            .filter(|name| {
                // Only names exactly one channel has; an ambiguous one is what
                // the other half of the test is about.
                channels.iter().filter(|other| &other.name == name).count() == 1
            })
            .collect()
    }

    /// Start creating a server template, reusing the same field.
    pub fn start_template_create(&mut self) {
        if let Some(state) = self.popups.server_management_mut()
            && state.tab == ServerPanelTab::Templates
        {
            state.renaming = Some((EmojiEdit::NewTemplate, TextInputState::default()));
        }
    }

    /// Open the permission grid on the highlighted role.
    pub fn open_selected_role_permissions(&mut self) {
        let Some(index) = self.selected_server_row() else {
            return;
        };
        let Some(state) = self.popups.server_management() else {
            return;
        };
        if state.tab != ServerPanelTab::Roles {
            return;
        }
        let Some((guild_id, role_id)) =
            state.roles.get(index).map(|role| (state.guild_id, role.id))
        else {
            return;
        };

        // Replaces the panel rather than stacking: the grid is 53 rows and
        // wants the whole popup area.
        self.open_role_permissions(guild_id, role_id);
    }

    /// Step the guild's verification level.
    ///
    /// A cycle rather than a field: it is a fixed set of five, and typing a
    /// number would mean nothing to anyone.
    fn cycle_guild_verification(&mut self, guild_id: Id<GuildMarker>) -> Option<AppCommand> {
        use crate::discord::GuildVerificationLevel as Level;
        let current = self.discord.guild(guild_id)?.verification_level?;
        let order = [
            Level::None,
            Level::Low,
            Level::Medium,
            Level::High,
            Level::VeryHigh,
        ];
        // An unrecognised level from a newer Discord starts the cycle rather
        // than being treated as None, which would silently weaken it.
        let index = order
            .iter()
            .position(|level| *level == current)
            .unwrap_or(0);
        let next = order[(index + 1) % order.len()];

        Some(AppCommand::ModifyGuild {
            guild_id,
            edit: Box::new(crate::discord::GuildEdit {
                verification_level: Some(next),
                ..crate::discord::GuildEdit::default()
            }),
            label: self.discord.guild(guild_id)?.name.clone(),
        })
    }

    /// Read the guild's settings out of the snapshot.
    fn fill_guild_settings(&mut self) {
        let Some(guild_id) = self.popups.server_management().map(|state| state.guild_id) else {
            return;
        };
        let Some(guild) = self.discord.guild(guild_id) else {
            return;
        };

        // Only what the snapshot actually carries. Default notifications and
        // the explicit-content filter are not parsed off the wire yet, so
        // showing them would mean showing a guess.
        let settings = vec![
            ("Name".to_owned(), guild.name.clone()),
            (
                "Verification".to_owned(),
                crate::discord::verification_label(guild.verification_level.unwrap_or_default()),
            ),
            (
                "Owner".to_owned(),
                guild
                    .owner_id
                    .map(|id| id.get().to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            ),
            (
                "Boosts".to_owned(),
                format!("{} ({:?})", guild.boost_count, guild.boost_tier),
            ),
        ];

        if let Some(state) = self.popups.server_management_mut() {
            state.settings = settings;
        }
    }

    /// Copy the guild's roles into the panel, highest first.
    fn fill_server_roles(&mut self) {
        let Some(guild_id) = self.popups.server_management().map(|state| state.guild_id) else {
            return;
        };
        let mut roles: Vec<_> = self
            .discord
            .cache
            .roles_for_guild(guild_id)
            .into_iter()
            .cloned()
            .collect();
        // Highest first, which is the order that decides which role wins a
        // permission conflict and so the order people reason about them in.
        roles.sort_by_key(|role| std::cmp::Reverse(role.position));

        if let Some(state) = self.popups.server_management_mut() {
            state.roles = roles;
        }
    }

    pub(in crate::tui) fn apply_guild_invites(
        &mut self,
        guild_id: Id<GuildMarker>,
        invites: Vec<GuildInviteInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        // A reply for a different guild belongs to a popup that has since been
        // closed and reopened elsewhere.
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.invites = invites;
    }

    /// Take the guild's sounds when the panel asked for them.
    pub(in crate::tui) fn apply_panel_sounds(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
        sounds: Vec<crate::discord::SoundboardSound>,
    ) {
        // Only the guild's own list. The defaults arrive on the same event and
        // belong to the picker, where they can be played but not managed.
        let Some(guild_id) = guild_id else {
            return;
        };
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.sounds = sounds;
    }

    pub(in crate::tui) fn apply_automod_rules(
        &mut self,
        guild_id: Id<GuildMarker>,
        rules: Vec<crate::discord::AutoModRule>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.automod = rules;
    }

    pub(in crate::tui) fn apply_guild_emojis(
        &mut self,
        guild_id: Id<GuildMarker>,
        emojis: Vec<GuildEmojiInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.emojis = emojis;
    }

    pub(in crate::tui) fn apply_guild_audit_log(
        &mut self,
        guild_id: Id<GuildMarker>,
        entries: Vec<AuditLogEntryInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.audit_log = entries;
    }

    pub(in crate::tui) fn apply_server_management_failure(
        &mut self,
        guild_id: Id<GuildMarker>,
        message: String,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = Some(message);
    }
}
