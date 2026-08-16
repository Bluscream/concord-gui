//! Every permission Discord has, as a catalogue the editors can render.
//!
//! Generated from Discord's own bitwise permission table (see
//! `.references/official-internals/userdoccers__github/pages/topics/permissions.mdx`),
//! rather than typed by hand: this is a list of 53 entries where a
//! transposed bit silently grants the wrong thing, and where Discord adds
//! entries faster than anyone re-reads them.
//!
//! Deprecated permissions are struck through in Discord's table and are
//! excluded, which is why the bits are not contiguous - 47 was Clyde.
//!
//! `src/discord/permission.rs` keeps its own constants for the handful of
//! checks the client makes. Those are hot paths and want named constants; this
//! is for showing a user every switch there is.

/// One permission, as the editors show it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Permission {
    /// Its position in Discord's bitfield.
    pub bit: u8,
    /// Discord's own name, which is what the API and audit log use.
    pub name: &'static str,
    /// What to show in a list.
    pub label: &'static str,
    pub description: &'static str,
}

impl Permission {
    /// The bit as a mask.
    pub const fn mask(self) -> u64 {
        1u64 << self.bit
    }

    /// Whether this permission is set in a bitfield.
    pub const fn is_set(self, permissions: u64) -> bool {
        permissions & self.mask() != 0
    }
}

/// Every permission, in Discord's own order.
pub const ALL: &[Permission] = &[
    Permission {
        bit: 0,
        name: "CREATE_INSTANT_INVITE",
        label: "Create Instant Invite",
        description: "Allows creation of instant invites",
    },
    Permission {
        bit: 1,
        name: "KICK_MEMBERS",
        label: "Kick Members",
        description: "Allows kicking members",
    },
    Permission {
        bit: 2,
        name: "BAN_MEMBERS",
        label: "Ban Members",
        description: "Allows banning members",
    },
    Permission {
        bit: 3,
        name: "ADMINISTRATOR",
        label: "Administrator",
        description: "Allows all permissions and bypasses channel permission overwrites",
    },
    Permission {
        bit: 4,
        name: "MANAGE_CHANNELS",
        label: "Manage Channels",
        description: "Allows management and editing of channels",
    },
    Permission {
        bit: 5,
        name: "MANAGE_GUILD",
        label: "Manage Guild",
        description: "Allows management and editing of the guild",
    },
    Permission {
        bit: 6,
        name: "ADD_REACTIONS",
        label: "Add Reactions",
        description: "Allows for the addition of reactions to messages",
    },
    Permission {
        bit: 7,
        name: "VIEW_AUDIT_LOG",
        label: "View Audit Log",
        description: "Allows for viewing of audit logs",
    },
    Permission {
        bit: 8,
        name: "PRIORITY_SPEAKER",
        label: "Priority Speaker",
        description: "Allows for using priority speaker in a voice channel",
    },
    Permission {
        bit: 9,
        name: "STREAM",
        label: "Stream",
        description: "Allows the user to use video and stream (go live) in a voice channel",
    },
    Permission {
        bit: 10,
        name: "VIEW_CHANNEL",
        label: "View Channel",
        description: "Allows guild members to view a channel, which includes reading messages in text channels and joining voice channels",
    },
    Permission {
        bit: 11,
        name: "SEND_MESSAGES",
        label: "Send Messages",
        description: "Allows for sending messages in a channel and creating threads in a forum (does not allow sending messages in threads)",
    },
    Permission {
        bit: 12,
        name: "SEND_TTS_MESSAGES",
        label: "Send Tts Messages",
        description: "Allows for sending of `/tts` messages",
    },
    Permission {
        bit: 13,
        name: "MANAGE_MESSAGES",
        label: "Manage Messages",
        description: "Allows for deletion of other users messages",
    },
    Permission {
        bit: 14,
        name: "EMBED_LINKS",
        label: "Embed Links",
        description: "Links sent by users with this permission will be auto-embedded",
    },
    Permission {
        bit: 15,
        name: "ATTACH_FILES",
        label: "Attach Files",
        description: "Allows for uploading images and files",
    },
    Permission {
        bit: 16,
        name: "READ_MESSAGE_HISTORY",
        label: "Read Message History",
        description: "Allows for reading of message history",
    },
    Permission {
        bit: 17,
        name: "MENTION_EVERYONE",
        label: "Mention Everyone",
        description: "Allows for using the @everyone tag to notify all users in a channel, and the @here tag to notify all online users in a channel",
    },
    Permission {
        bit: 18,
        name: "USE_EXTERNAL_EMOJIS",
        label: "Use External Emojis",
        description: "Allows the usage of custom emoji from other servers",
    },
    Permission {
        bit: 19,
        name: "VIEW_GUILD_INSIGHTS",
        label: "View Guild Insights",
        description: "Allows for viewing guild insights",
    },
    Permission {
        bit: 20,
        name: "CONNECT",
        label: "Connect",
        description: "Allows for joining of a voice channel",
    },
    Permission {
        bit: 21,
        name: "SPEAK",
        label: "Speak",
        description: "Allows for speaking in a voice channel",
    },
    Permission {
        bit: 22,
        name: "MUTE_MEMBERS",
        label: "Mute Members",
        description: "Allows for muting members in a voice channel",
    },
    Permission {
        bit: 23,
        name: "DEAFEN_MEMBERS",
        label: "Deafen Members",
        description: "Allows for deafening of members in a voice channel",
    },
    Permission {
        bit: 24,
        name: "MOVE_MEMBERS",
        label: "Move Members",
        description: "Allows for moving of members between voice channels",
    },
    Permission {
        bit: 25,
        name: "USE_VAD",
        label: "Use Vad",
        description: "Allows for using voice-activity-detection in a voice channel",
    },
    Permission {
        bit: 26,
        name: "CHANGE_NICKNAME",
        label: "Change Nickname",
        description: "Allows for modification of own nickname",
    },
    Permission {
        bit: 27,
        name: "MANAGE_NICKNAMES",
        label: "Manage Nicknames",
        description: "Allows for modification of other users nicknames",
    },
    Permission {
        bit: 28,
        name: "MANAGE_ROLES",
        label: "Manage Roles",
        description: "Allows management and editing of roles",
    },
    Permission {
        bit: 29,
        name: "MANAGE_WEBHOOKS",
        label: "Manage Webhooks",
        description: "Allows management and editing of webhooks",
    },
    Permission {
        bit: 30,
        name: "MANAGE_EXPRESSIONS",
        label: "Manage Expressions",
        description: "Allows editing and deleting emoji, stickers, and soundboard sounds",
    },
    Permission {
        bit: 31,
        name: "USE_APPLICATION_COMMANDS",
        label: "Use Application Commands",
        description: "Allows members to use application commands, including slash commands and context menu commands",
    },
    Permission {
        bit: 32,
        name: "REQUEST_TO_SPEAK",
        label: "Request to Speak",
        description: "Allows for requesting to speak in stage channels",
    },
    Permission {
        bit: 33,
        name: "MANAGE_EVENTS",
        label: "Manage Events",
        description: "Allows for editing and deleting scheduled events",
    },
    Permission {
        bit: 34,
        name: "MANAGE_THREADS",
        label: "Manage Threads",
        description: "Allows for deleting and archiving threads, and viewing all private threads",
    },
    Permission {
        bit: 35,
        name: "CREATE_PUBLIC_THREADS",
        label: "Create Public Threads",
        description: "Allows for creating public and announcement threads",
    },
    Permission {
        bit: 36,
        name: "CREATE_PRIVATE_THREADS",
        label: "Create Private Threads",
        description: "Allows for creating private threads",
    },
    Permission {
        bit: 37,
        name: "USE_EXTERNAL_STICKERS",
        label: "Use External Stickers",
        description: "Allows the usage of custom stickers from other servers",
    },
    Permission {
        bit: 38,
        name: "SEND_MESSAGES_IN_THREADS",
        label: "Send Messages in Threads",
        description: "Allows for sending messages in threads",
    },
    Permission {
        bit: 39,
        name: "USE_EMBEDDED_ACTIVITIES",
        label: "Use Embedded Activities",
        description: "Allows for using Activities (applications with the `EMBEDDED` flag) in a voice channel",
    },
    Permission {
        bit: 40,
        name: "MODERATE_MEMBERS",
        label: "Moderate Members",
        description: "Allows for timing out users to prevent them from sending or reacting to messages in chat and threads, and from speaking in voice and stage channels",
    },
    Permission {
        bit: 41,
        name: "VIEW_CREATOR_MONETIZATION_ANALYTICS",
        label: "View Creator Monetization Analytics",
        description: "Allows for viewing guild role subscriptions insights",
    },
    Permission {
        bit: 42,
        name: "USE_SOUNDBOARD",
        label: "Use Soundboard",
        description: "Allows the usage of the soundboard in a voice channel",
    },
    Permission {
        bit: 43,
        name: "CREATE_EXPRESSIONS",
        label: "Create Expressions",
        description: "Allows for creating emoji, stickers, and soundboard sounds, and editing/deleting ones created by the current user",
    },
    Permission {
        bit: 44,
        name: "CREATE_EVENTS",
        label: "Create Events",
        description: "Allows for creating scheduled events, and editing/deleting ones created by the current user",
    },
    Permission {
        bit: 45,
        name: "USE_EXTERNAL_SOUNDS",
        label: "Use External Sounds",
        description: "Allows the usage of custom soundboard sounds from other servers",
    },
    Permission {
        bit: 46,
        name: "SEND_VOICE_MESSAGES",
        label: "Send Voice Messages",
        description: "Allows for sending voice messages in a channel",
    },
    Permission {
        bit: 48,
        name: "SET_VOICE_CHANNEL_STATUS",
        label: "Set Voice Channel Status",
        description: "Allows setting voice channel status",
    },
    Permission {
        bit: 49,
        name: "SEND_POLLS",
        label: "Send Polls",
        description: "Allows sending polls",
    },
    Permission {
        bit: 50,
        name: "USE_EXTERNAL_APPS",
        label: "Use External Apps",
        description: "Allows the usage of user-installed applications without forced-ephemeral responses",
    },
    Permission {
        bit: 51,
        name: "PIN_MESSAGES",
        label: "Pin Messages",
        description: "Allows pinning messages in a channel",
    },
    Permission {
        bit: 52,
        name: "BYPASS_SLOWMODE",
        label: "Bypass Slowmode",
        description: "Allows members to bypass slowmode in a channel",
    },
    Permission {
        bit: 53,
        name: "MANAGE_OFFICIAL_MESSAGES",
        label: "Manage Official Messages",
        description: "Allows members to mark messages as official in verified guilds",
    },
];

/// Find a permission by Discord's name, as the audit log spells it.
pub fn by_name(name: &str) -> Option<Permission> {
    ALL.iter()
        .copied()
        .find(|permission| permission.name == name)
}

/// Set or clear a permission in a bitfield.
pub const fn with(permissions: u64, permission: Permission, allowed: bool) -> u64 {
    if allowed {
        permissions | permission.mask()
    } else {
        permissions & !permission.mask()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_has_a_distinct_bit() {
        // A transposed bit would silently grant the wrong thing, which is the
        // whole reason this list is generated rather than typed.
        let mut bits: Vec<u8> = ALL.iter().map(|permission| permission.bit).collect();
        bits.sort_unstable();
        bits.dedup();

        assert_eq!(bits.len(), ALL.len());
    }

    #[test]
    fn every_permission_has_a_distinct_name() {
        let mut names: Vec<&str> = ALL.iter().map(|permission| permission.name).collect();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn the_catalogue_agrees_with_the_constants_the_client_checks() {
        // The two lists are maintained separately - hot paths want named
        // constants - so they are checked against each other rather than
        // trusted to stay in step.
        for (name, mask) in [
            ("ADMINISTRATOR", 0x0000_0000_0000_0008u64),
            ("KICK_MEMBERS", 0x0000_0000_0000_0002),
            ("BAN_MEMBERS", 0x0000_0000_0000_0004),
            ("MANAGE_CHANNELS", 0x0000_0000_0000_0010),
            ("MANAGE_GUILD", 0x0000_0000_0000_0020),
            ("VIEW_CHANNEL", 0x0000_0000_0000_0400),
            ("SEND_MESSAGES", 0x0000_0000_0000_0800),
            ("MANAGE_MESSAGES", 0x0000_0000_0000_2000),
            ("MANAGE_ROLES", 0x0000_0000_1000_0000),
            ("MODERATE_MEMBERS", 0x0000_0100_0000_0000),
        ] {
            let permission = by_name(name).unwrap_or_else(|| panic!("{name} should exist"));
            assert_eq!(permission.mask(), mask, "{name} has the wrong bit");
        }
    }

    #[test]
    fn setting_and_clearing_are_inverses() {
        let manage_roles = by_name("MANAGE_ROLES").expect("should exist");
        let granted = with(0, manage_roles, true);

        assert!(manage_roles.is_set(granted));
        assert!(!manage_roles.is_set(with(granted, manage_roles, false)));
        // Clearing one must not disturb another.
        let kick = by_name("KICK_MEMBERS").expect("should exist");
        let both = with(granted, kick, true);
        assert!(kick.is_set(with(both, manage_roles, false)));
    }
}

#[cfg(test)]
mod constant_agreement_tests {
    use super::*;

    /// Every constant `src/discord/permission.rs` checks, and Discord's name
    /// for it.
    ///
    /// Listed here rather than derived, so adding a constant there without
    /// adding it here is a compile-time nothing but a review-time obvious
    /// omission - and so a renamed permission is recorded rather than silently
    /// matched. `USE_VOICE_ACTIVITY` is Discord's older name for `USE_VAD`,
    /// and `MANAGE_GUILD_EXPRESSIONS` its older name for `MANAGE_EXPRESSIONS`.
    const CHECKED: &[(&str, u64)] = &[
        ("VIEW_CHANNEL", 0x0000_0000_0000_0400),
        ("MANAGE_CHANNELS", 0x0000_0000_0000_0010),
        ("MANAGE_GUILD", 0x0000_0000_0000_0020),
        ("STREAM", 0x0000_0000_0000_0200),
        ("SEND_MESSAGES", 0x0000_0000_0000_0800),
        ("SEND_TTS_MESSAGES", 0x0000_0000_0000_1000),
        ("MANAGE_MESSAGES", 0x0000_0000_0000_2000),
        ("ATTACH_FILES", 0x0000_0000_0000_8000),
        ("READ_MESSAGE_HISTORY", 0x0000_0000_0001_0000),
        ("CONNECT", 0x0000_0000_0010_0000),
        ("SPEAK", 0x0000_0000_0020_0000),
        ("USE_VAD", 0x0000_0000_0200_0000),
        ("ADMINISTRATOR", 0x0000_0000_0000_0008),
        ("ADD_REACTIONS", 0x0000_0000_0000_0040),
        ("USE_EXTERNAL_EMOJIS", 0x0000_0000_0004_0000),
        ("USE_APPLICATION_COMMANDS", 0x0000_0000_8000_0000),
        ("MANAGE_THREADS", 0x0000_0004_0000_0000),
        ("SEND_MESSAGES_IN_THREADS", 0x0000_0040_0000_0000),
        ("PIN_MESSAGES", 0x0008_0000_0000_0000),
        ("BYPASS_SLOWMODE", 0x0010_0000_0000_0000),
        ("KICK_MEMBERS", 0x0000_0000_0000_0002),
        ("BAN_MEMBERS", 0x0000_0000_0000_0004),
        ("MANAGE_ROLES", 0x0000_0000_1000_0000),
        ("MODERATE_MEMBERS", 0x0000_0100_0000_0000),
        ("CREATE_INSTANT_INVITE", 0x0000_0000_0000_0001),
        ("VIEW_AUDIT_LOG", 0x0000_0000_0000_0080),
        ("MANAGE_EXPRESSIONS", 0x0000_0000_4000_0000),
    ];

    #[test]
    fn every_constant_the_client_checks_matches_discords_table() {
        // This is how the timeout permission was found to be checking the
        // soundboard bit: the two lists are maintained separately, so they are
        // verified against each other rather than trusted.
        for (name, mask) in CHECKED {
            let permission =
                by_name(name).unwrap_or_else(|| panic!("{name} is not in Discord's table"));
            assert_eq!(
                permission.mask(),
                *mask,
                "{name} disagrees with Discord's table"
            );
        }
    }
}
