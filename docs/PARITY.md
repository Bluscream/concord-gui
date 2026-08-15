# Parity tracking

Two targets, in order: the concord TUI first, then the official client minus
the parts we do not want.

## Against the concord TUI

### How this is measured

Three checks, because the first two each missed real gaps:

1. **Keybinding actions.** The TUI's 152 actions in
   `src/tui/keybindings/actions.rs`. Too weak: it counts names, not behaviour.
2. **Command coverage.** Which `AppCommand` variants each client sends,
   diffed:

   ```sh
   comm -23 \
     <(grep -rhoE "AppCommand::[A-Z][A-Za-z]+" src/tui/ | sort -u) \
     <(grep -rhoE "AppCommand::[A-Z][A-Za-z]+" crates/gui/src/ | sort -u)
   ```

   Stronger, and it found 26 gaps that check 1 had reported as complete. But
   it asks only whether a command *can* be sent.
3. **Render reachability.** Whether the control that sends a command is
   actually drawn. This found seven panels - the quick switcher, emoji picker,
   screenshare picker, confirmation dialog, reaction users, mention inbox -
   that had full state and key handling and were never rendered at all.
   `switcher_view` had zero callers in the crate.

The lesson from all three: a proxy for functionality is not functionality.
Any future parity claim should re-run check 2 and spot-check check 3.

### Status

Command coverage is **68 of 69**. The one command the GUI does not send is
`LoadAttachmentPreview`, and this is deliberate - see below.

Working: login (token, QR, password, MFA) · guild/channel navigation ·
threads, including rename, delete, lock, mute, pin and previews · forums,
including post creation · message list with markdown, mentions, avatars,
inline custom emoji and inline image previews · send · reply · edit · delete ·
react · emoji picker · spoilers · attachments · member list, with server-side
member search and hydration · profiles · avatar change · unread and mention
badges · typing indicators, sent and received · guild voice · DM calls ·
mute/deafen · microphone permission · audio device selection · screenshare ·
watching someone else's stream · search with jump · desktop notifications ·
settings · history paging in both directions · channel refresh · composer with
selection, undo, clipboard, multi-line · mark read, deferred mark read, mark
all read · quick switcher · jump to replied message · copy text and link ·
reaction users · pins · mute channel · notification inbox with context ·
builtin slash commands · application commands with autocomplete · status and
custom status · leave guild · sign out · guild folders with rename

### Deliberate difference

`LoadAttachmentPreview` fetches image bytes into the process. The TUI needs
this because a terminal cannot fetch an image itself - the bytes have to be
re-encoded into a terminal graphics protocol. GPUI loads images from a URL
through the application's HTTP client, so the GUI renders the same previews
without the round trip through core state.

The feature is present; the command is not the way it is delivered. Sending it
anyway would fetch every image twice.

### Not possible against this core

Guild and channel notification *levels* have no command: the core exposes
mute/unmute for both, and `SetThreadNotificationLevel` only for threads, which
is also all the TUI uses. The level control is therefore offered on threads
alone.

`OpenThread` opens an existing thread; the core exposes no thread *creation*
command. Threads arrive from Discord or from a forum post, so "create thread"
is not a parity gap - it is outside what this core does. Listing it as missing
was a misreading of the action's name.

### Not verified against Discord

Nothing here has been run against a real account. Verification rests on the
test suite, the offline fixture, and the type checker. `--media` additionally
hits a zbus/tokio incompatibility that reaches us through GPUI's own
dependencies, so screenshare is compile-verified only.

## Against the official client

Deliberately **not** planned, as the "bloat" this project exists to avoid:

- Nitro upsells, boosts, shop, collectibles
- Activities, embedded games, watch-together
- Discovery, quests, sponsored surfaces
- Analytics and telemetry beyond what the API requires

Worth having next: stickers, message forwarding, per-guild identities,
invites, and full moderation (kick/ban/roles - Abaddon has these and they are
the strongest argument for that client over this one).
