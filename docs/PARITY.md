# Parity tracking

Two targets, in order: the concord TUI first, then the official client minus
the parts we do not want.

## Against the concord TUI

Derived from the TUI's 152 keybinding actions (`src/tui/keybindings/actions.rs`)
and its 12 builtin slash commands · reply-ping toggle (`src/discord/builtin_commands.rs`). Pure
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

Ordered roughly by how much a daily driver misses them.

| Feature | TUI action | Notes |
|---|---|---|
| Polls | `OpenPollVotePicker` | `PollInfo` is already projected but not rendered. |
| Attachment download / view | `DownloadSelected`, `ViewAttachment` | Chips render; nothing opens or saves. |
| Media playback | `PlayMedia` | Upstream shells out to mpv. |
| Own profile | `OpenCurrentUserProfile` | Other users' profiles work. |
| Sign out | `SignOut` | No way to switch accounts without editing config. |
| Create thread | `OpenThread` | Threads are readable, not creatable. |
| Remove embeds | `RemoveEmbeds`, `RemoveMessageEmbeds` | |
| Application commands | `RunApplicationCommand` | Server-side slash commands. |
| Compose in external editor | `OpenInEditor` | |
| Pane show/hide, resize, zoom | `Toggle*Pane`, `Resize*`, `ToggleZoom` | Layout is fixed. |
| Vim-style navigation | `HalfPageUp/Down`, `JumpTop/Bottom` | |
| Debug log viewer | `OpenDebugLog` | |

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
