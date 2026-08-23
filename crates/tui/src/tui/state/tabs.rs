//! Several channels open at once.
//!
//! The most common thing the surveyed clients have that this one did not: 26
//! of them offer tabs. The GUI grew them first; this is the same model, so a
//! tab opened in one client is a tab in the other - `UiStateOptions.open_tabs`
//! is written by both.
//!
//! A tab is not just a channel id. What makes tabs worth having is that
//! returning to one does not lose what was typed there or where the log was
//! scrolled to, so both travel with it.

use concord::discord::ids::{Id, marker::ChannelMarker};

use super::DashboardState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelTab {
    pub(in crate::tui) channel_id: Id<ChannelMarker>,
    pub(in crate::tui) name: String,
    /// What was typed and not sent. Restored on return.
    pub(in crate::tui) draft: String,
}

#[derive(Clone, Debug, Default)]
pub(in crate::tui) struct TabState {
    pub(in crate::tui) tabs: Vec<ChannelTab>,
    pub(in crate::tui) active: usize,
}

impl DashboardState {
    /// The channel the channel pane's cursor is on, if it is on one.
    ///
    /// A category header is a row but not a channel, so it opens no tab.
    fn channel_under_cursor(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel())? {
            super::model::ChannelPaneEntry::Channel { state, .. } => Some(state.id),
            _ => None,
        }
    }

    pub(in crate::tui) fn channel_tabs(&self) -> &[ChannelTab] {
        &self.tabs.tabs
    }

    pub(in crate::tui) fn active_channel_tab(&self) -> usize {
        self.tabs.active
    }

    /// Open the highlighted channel in a tab of its own.
    ///
    /// The cursor, not `selected_channel_id` - that is the channel already
    /// open, so using it would only ever tab the channel you are reading.
    pub fn open_selected_channel_in_new_tab(&mut self) {
        let Some(channel_id) = self.channel_under_cursor() else {
            return;
        };

        // Already open: switch to it rather than opening a second tab onto
        // the same channel, which would give it two drafts.
        if let Some(index) = self
            .tabs
            .tabs
            .iter()
            .position(|tab| tab.channel_id == channel_id)
        {
            self.activate_channel_tab(index);
            return;
        }

        self.stash_active_channel_tab();
        let name = self
            .discord
            .channel(channel_id)
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| format!("channel-{}", channel_id.get()));

        self.tabs.tabs.push(ChannelTab {
            channel_id,
            name,
            draft: String::new(),
        });
        self.tabs.active = self.tabs.tabs.len() - 1;
        self.activate_channel(channel_id);
        self.clear_composer_for_tab();
        self.persist_channel_tabs();
    }

    /// Switch to a tab, restoring what was typed there.
    pub fn activate_channel_tab(&mut self, index: usize) {
        let Some(tab) = self.tabs.tabs.get(index).cloned() else {
            return;
        };

        self.stash_active_channel_tab();
        self.tabs.active = index;
        self.activate_channel(tab.channel_id);
        self.set_composer_for_tab(&tab.draft);
        self.persist_channel_tabs();
    }

    /// Move to the next or previous tab, wrapping at each end.
    pub fn cycle_channel_tab(&mut self, forward: bool) {
        let count = self.tabs.tabs.len();
        if count < 2 {
            return;
        }
        let next = if forward {
            (self.tabs.active + 1) % count
        } else {
            (self.tabs.active + count - 1) % count
        };
        self.activate_channel_tab(next);
    }

    /// Close a tab, moving to the one on its left.
    pub fn close_channel_tab(&mut self, index: usize) {
        if index >= self.tabs.tabs.len() {
            return;
        }
        self.tabs.tabs.remove(index);

        if self.tabs.tabs.is_empty() {
            self.tabs.active = 0;
            self.persist_channel_tabs();
            return;
        }

        // Prefer the tab to the left, which is where attention was before the
        // closed one existed.
        self.tabs.active = self.tabs.active.min(self.tabs.tabs.len() - 1);
        if index <= self.tabs.active && self.tabs.active > 0 {
            self.tabs.active -= 1;
        }
        let active = self.tabs.active;
        self.activate_channel_tab(active);
    }

    pub fn close_active_channel_tab(&mut self) {
        let active = self.tabs.active;
        self.close_channel_tab(active);
    }

    /// Keep the active tab's draft before leaving it.
    fn stash_active_channel_tab(&mut self) {
        let draft = self.composer_input().to_owned();
        if let Some(tab) = self.tabs.tabs.get_mut(self.tabs.active) {
            tab.draft = draft;
        }
    }

    fn set_composer_for_tab(&mut self, draft: &str) {
        self.composer.composer_input.set_value(draft.to_owned());
        // A draft belongs to the channel it was typed in, so an edit or reply
        // aimed at the tab being left must not follow the composer across.
        self.composer.edit_target_message = None;
        self.composer.reply_target_message_id = None;
    }

    fn clear_composer_for_tab(&mut self) {
        self.set_composer_for_tab("");
    }

    /// Reopen the tabs from last time.
    ///
    /// A channel that has since been deleted, or that this account can no
    /// longer see, is dropped rather than kept as a tab that opens nothing.
    pub(in crate::tui) fn restore_channel_tabs(&mut self) {
        if !self.tabs.tabs.is_empty() || self.options.ui_state_open_tabs.is_empty() {
            return;
        }

        let saved = self.options.ui_state_open_tabs.clone();
        let active = self.options.ui_state_active_tab;

        self.tabs.tabs = saved
            .into_iter()
            .filter_map(|channel_id| {
                let channel = self.discord.channel(channel_id)?;
                Some(ChannelTab {
                    channel_id,
                    name: channel.name.clone(),
                    draft: String::new(),
                })
            })
            .collect();

        self.tabs.active = active.min(self.tabs.tabs.len().saturating_sub(1));
    }

    fn persist_channel_tabs(&mut self) {
        self.options.ui_state_open_tabs = self.tabs.tabs.iter().map(|tab| tab.channel_id).collect();
        self.options.ui_state_active_tab = self.tabs.active;
        // The UI-state file, not config.toml: which tabs are open is a
        // position, not a preference, and rewriting the config for it would
        // churn a file the user edits by hand.
        self.options.ui_state_save_pending = true;
    }
}
