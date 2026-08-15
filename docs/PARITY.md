# Parity tracking

Two targets, in order: the concord TUI first, then the official client minus
the parts we do not want.

## Against the concord TUI

Derived from the TUI's 152 keybinding actions (`src/tui/keybindings/actions.rs`)
and its 12 builtin slash commands · reply-ping toggle · external editor · sign out · own profile · polls · attachment download ·
external media playback · remove embeds · thread archive/follow ·
application commands · pane show/hide · vim-style scrolling ·
debug log viewer · open links · collapse categories · quit ·
pane focus cycling · pane filtering (`src/discord/builtin_commands.rs`). Pure
input primitives (`Char`, `Ctrl`, `Key`, `ModifiedKey`) are excluded - they are
not features.

### Done

Login (token, QR, password, MFA) · guild/channel navigation · threads ·
forums · message list with markdown, mentions, avatars, inline custom emoji ·
send · reply · edit · delete · react · emoji picker · spoilers · attachments
(stage, send) · member list · profiles · unread and mention badges · typing
indicators · guild voice · DM calls · mute/deafen · screenshare controls ·
search with jump · desktop notifications · settings · history paging ·
composer with selection, undo, clipboard, multi-line · mark read / mark all
read · quick switcher · jump to replied message · copy message text and link ·
reaction users · pins · mute channel · notification inbox · builtin slash
commands

### Missing

Audited by diffing which `AppCommand` variants each client sends, which is a
stronger check than the action enum: a command the TUI can issue and the GUI
cannot is a real gap regardless of what the keybinding is called. The TUI
sends all 69; the GUI sent 43.

| Command | Feature |
|---|---|
| `TriggerTyping` | Others never see this client typing |
| `SignOut` | Sign-out is local only; Discord is never told |
| `UpdateCurrentUserStatus` | Set online/idle/dnd/invisible |
| `UpdateCurrentUserActivity` | Set a custom status |
| `LeaveGuild` | Leave a server |
| `CreateForumPost` | Forum posts *can* be created - only threads cannot |
| `DeleteThread`, `EditThread` | Rename or delete a thread |
| `SetThreadLocked`, `SetThreadMuted`, `SetThreadPinned` | Remaining thread management |
| `SearchGuildMembers`, `LoadGuildMembersByIds` | Member search and hydration |
| `LoadVoiceAudioSources`, `UpdateVoiceAudioSources` | Input/output device selection |
| `UpdateVoiceCapturePermission` | Microphone permission |
| `WatchVoiceStream` | Watch someone else's stream |
| `RequestApplicationCommandAutocomplete` | Argument autocomplete for bot commands |
| `LoadMessageHistoryAfter`, `RefreshMessageHistory` | Page forward, refresh a channel |
| `ScheduleAckChannel` | Deferred read marking |
| `LoadAttachmentPreview`, `LoadProfileAvatarPreview` | Attachment and avatar previews |
| `LoadThreadPreview` | Thread preview without opening |
| `LoadInboxChannelHistory` | Context around an inbox mention |
| `UpdateGuildFolderSettings` | Guild folders |

### Not possible against this core

Guild and channel notification *levels* have no command either: the core
exposes mute/unmute for both, and `SetThreadNotificationLevel` only for
threads, which is also all the TUI uses. The level control is therefore
offered on threads alone.

`OpenThread` opens an existing thread; the core exposes no thread *creation*
command at all. Threads arrive from Discord or from a forum post, so
"create thread" is not a parity gap - it is outside what this core does.
Listing it as missing was a misreading of the action's name.

## Against the official client

Deliberately **not** planned, as the "bloat" this project exists to avoid:

- Nitro upsells, boosts, shop, collectibles
- Activities, embedded games, watch-together
- Discovery, quests, sponsored surfaces
- Analytics and telemetry beyond what the API requires

Worth having once TUI parity lands: stickers, message forwarding, folders in
the guild rail, per-guild identities, invites, and full moderation
(kick/ban/roles - Abaddon has these and they are the strongest argument for
that client over this one).
