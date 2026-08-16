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
}

impl ServerPanelTab {
    pub const ALL: [Self; 7] = [
        Self::Settings,
        Self::Invites,
        Self::Roles,
        Self::Emoji,
        Self::Sounds,
        Self::AutoMod,
        Self::AuditLog,
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
        }
    }

    /// What to fetch when this tab opens.
    ///
    /// `None` for roles: they arrive with the guild and live in the snapshot,
    /// so the tab reads them rather than asking for them.
    fn load(self, guild_id: Id<GuildMarker>) -> Option<AppCommand> {
        Some(match self {
            // Both read from the snapshot rather than fetching: the guild and
            // its roles arrive together.
            Self::Settings | Self::Roles => return None,
            Self::Invites => AppCommand::LoadGuildInvites { guild_id },
            Self::Emoji => AppCommand::LoadGuildEmojis { guild_id },
            Self::Sounds => AppCommand::LoadSoundboardSounds {
                guild_id: Some(guild_id),
            },
            Self::AutoMod => AppCommand::LoadAutoModRules { guild_id },
            Self::AuditLog => AppCommand::LoadGuildAuditLog { guild_id },
        })
    }
}

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
        tab.load(guild_id)
    }

    pub fn close_server_management(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ServerManagement) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn server_management_state(&self) -> Option<&ServerManagementState> {
        self.popups.server_management()
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
        };
        state.loading = !already_loaded;
        let guild_id = state.guild_id;
        match tab {
            ServerPanelTab::Roles => self.fill_server_roles(),
            ServerPanelTab::Settings => self.fill_guild_settings(),
            _ => {}
        }
        (!already_loaded).then(|| tab.load(guild_id)).flatten()
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
        tab.load(guild_id)
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
