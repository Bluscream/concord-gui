# Parity tracking

Two targets, in order: the concord TUI first, then the official client minus
the parts we do not want.

## Against the concord TUI

### How this is measured

Five checks, because each one missed gaps the next found:

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
   actually drawn. This found eight panels - the quick switcher, slash picker,
   emoji picker, screenshare picker, confirmation dialog, reaction users,
   mention inbox - with full state and key handling and no render path at all.
   `switcher_view` had zero callers in the crate.

   Its cheap form is a caller-count sweep, which also catches handlers with no
   control attached:

   ```sh
   for f in $(grep -rhoE "pub fn [a-z_]+" crates/gui/src/ui/*.rs | awk '{print $3}' | sort -u); do
     n=$(grep -rc "\b$f\b" crates/gui/src/ --include=*.rs | awk -F: '{s+=$2} END{print s}')
     d=$(grep -rc "pub fn $f\b" crates/gui/src/ --include=*.rs | awk -F: '{s+=$2} END{print s}')
     [ "$n" -le "$d" ] && echo "UNCALLED: $f"
   done
   ```

4. **Event handling.** `DiscordState::apply_event` has a no-op arm listing 41
   events it deliberately does not absorb. A snapshot-driven UI reprojects
   everything else for free, but these carry information stored nowhere, so
   ignoring one loses it outright - `MessageSendFailed` meant a rejected
   message vanished silently. 24 were unhandled.
5. **Compiler warnings.** `never used`, `never read` and `never constructed`
   are the same reachability question the compiler already answers. Reading
   them found staged attachments that rendered nothing, guild mention counts
   projected and never drawn, three settings with labels and no row, and
   pin confirmations nothing constructed. Build with `--tests` too: a helper
   used only by tests looks dead to `cargo check`, and removing it breaks the
   test build without breaking the normal one.

The lesson from all five: a proxy for functionality is not functionality.
Checks 2 and 4 are exact and should be re-run before any parity claim; 3 and
5 are cheap and should be swept. The pattern so far is that each new check
found what the earlier ones structurally could not, so a clean run means
"the checks I know how to write pass", not "nothing is missing".

### Status

Command coverage **69 of 69**, event coverage **46 of 46**, the uncalled
sweep empty, and the GUI crate compiles with zero warnings.

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

### Configuration

A sixth check - which config surfaces each client reads. The five earlier
checks cannot see this, because the GUI hardcodes equivalent behaviour:
commands, events and reachability all look clean while the user's own
configuration is ignored.

| Surface | State |
|---|---|
| `config.toml` | Read, with warnings reported |
| `ui_state.toml` | Read and written, shared with the TUI |
| `keymap.toml` | Read; resolved by the core so both clients agree |
| `theme.toml` | Read; foreground colours applied to the palette |

Keymap resolution lives in `tui::keybindings::external` and theme resolution
in `tui::theme::external`, both narrow facades over the TUI's own code. A
second implementation in the GUI would drift from the file it is meant to be
reading.

Two deliberate differences, both forced by the front ends not being alike:

- **Bare-letter bindings.** The TUI is modal, so its defaults bind plain
  letters - `q` is Quit. This client has no such mode; the composer is always
  live. An unmodified character while the message pane has focus is therefore
  always typed, never dispatched. Without this, the TUI's own default keymap
  would quit the application when someone began a message with "q".
- **Theme scope.** Only foregrounds are taken, and only for the groups with a
  real counterpart. A terminal highlight's background applies to one span; on
  a GUI surface it would flood the window. Groups about terminal mechanics -
  scrollbars, border shapes, dim - are left alone rather than mapped onto
  something the user did not ask for. ANSI colours resolve to nothing, since
  they mean whatever the user's terminal palette says.

### A reversed decision, worth recording

`LoadAttachmentPreview` was initially left unsent, on the grounds that GPUI
can load an image from a URL and sending it would fetch twice. That was half
right. It avoided the double fetch but routed image loading around the
session's headers and the core's cache, and left demo mode with no previews
at all. `gpui::Image::from_bytes` renders the core's bytes directly, which
gets one fetch on the path the TUI already uses.

The general shape of the mistake: "the GUI achieves this differently" is
worth stating out loud, because saying it exposes whether it is a better
route or a rationalisation.

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

### The rule for anything beyond TUI parity

New features go in the **core**, and land in **both** front ends. Otherwise
the core grows capabilities that half its clients cannot reach - which is
exactly what happened when invites and forwarding were built for the GUI
alone, and had to be retrofitted into the TUI afterwards.

### Done

| Feature | GUI | TUI |
|---|---|---|
| Invites (join a server) | `+` in the guild rail | Guild actions, `j` |
| Message forwarding | Message toolbar | Message actions, `f` |
| Sticker rendering | Inline image | Existing name display |
| Sticker sending | Composer control, image grid | Leader-s, name list |
| Moderation (kick/ban/timeout) | Profile panel | Member actions, `k`/`b`/`t` |

Joining mattered most: a client could leave a guild but never join one, so
the official client was still required for the most basic thing after reading
messages. Both clients preview the invite - server, channel, inviter, member
counts - before joining, because an invite code says nothing about where it
leads. Forwarding reuses each client's own channel switcher, so destinations
rank identically in both.

Sticker *sending* is not done: the core carries ids and formats now, which is
what rendering needed, but there is no send path or picker.

### Still to do

None are wiring jobs. The core has no commands or REST routes for these, so
each means building from the transport layer up, the way invites were:

| Feature | What it needs |
|---|---|
| Per-guild identities | Profile REST exists; needs per-guild plumbing |
| Role assignment UI | `SetMemberRoles` is wired; neither client offers a picker |
| Ban list / unban | `UnbanMember` is wired; nothing lists existing bans |

### On using the references

`.references/` holds 114 surveyed clients, and they are worth reading before
writing a request. Moderation endpoints came from Abaddon - the only one with
it working - including its note that the official client does not use the
single-role endpoints, which is why `set_member_roles` sends the whole set.
The sticker payload was confirmed twice over, against a native client's send
path and against the Spacebar server's own schema.

Guessing at a payload and discovering it from a rejection is slower than
reading an implementation that already works.

## Risk warnings

Both clients warn before the actions Discord's anti-spam checks watch, from the
same list and in the same words. The kinds live in `src/risk.rs`; anything that
sends one of the watched commands should go through them.

```bash
# Every command that should be behind a warning, and where it is sent from.
grep -rn "AcceptInvite\|LeaveGuild\|UpdateUserProfile\|AddFriend\|BlockUser\|RemoveRelationship\|SendFriendRequest" \
  src/tui crates/gui/src --include=*.rs | grep -v test
```

Each hit should be next to a `request_risky` in the TUI or a `friend_action` /
`confirm_risk` in the GUI. One that is not is a way round the warning.

## Uncalled functions

A feature can be fully built and completely unreachable. `create_invite_here`
was written, tested and wired to nothing; only a release-profile dead-code
warning caught it. Clippy in release is the check:

```bash
cargo clippy --release --features fixtures --all-targets
cargo clippy --release -p concord-gui --features fixtures --all-targets
```

A `never used` warning on a `pub fn` in a front end means a control was never
attached to it.

## Commands only one client sends

Clippy cannot catch a core `AppCommand` variant that no front end ever
constructs - it is public API, so it is never "dead". This finds them, and
finds the ones only one client can reach:

```bash
for v in $(grep -oE '^    [A-Z][A-Za-z]+ \{' src/discord/commands.rs | tr -d ' {'); do
  t=$(grep -rl "AppCommand::$v" src/tui --include=*.rs 2>/dev/null | grep -vc test)
  g=$(grep -rl "AppCommand::$v" crates/gui/src --include=*.rs 2>/dev/null | grep -v demo.rs | grep -vc "^$")
  [ "$t" -eq 0 ] && [ "$g" -gt 0 ] && echo "GUI only: $v"
  [ "$g" -eq 0 ] && [ "$t" -gt 0 ] && echo "TUI only: $v"
done
```

And the commands neither client reaches, which the asymmetry check above
cannot see because a hole in both is symmetric:

```bash
for v in $(grep -oE '^    [A-Z][A-Za-z]+ \{' src/discord/commands.rs | tr -d ' {'); do
  n=$(grep -rn "AppCommand::$v" src/tui crates/gui/src --include=*.rs 2>/dev/null \
    | grep -v "demo.rs" | grep -vc test)
  [ "$n" -eq 0 ] && echo "unreachable: $v"
done
```

Both lists should be empty. `RenameEmoji` was dispatched and reachable from
neither; `CreateChannelInvite` and `RenameEmoji` were later reachable only from
the GUI. All three were found this way rather than by noticing.

Ignore `Custom` - it is a `ReactionEmoji` variant the pattern also matches.

Anything else on the unreachable list is core work whose interface has not been
built yet. That is a legitimate intermediate state, but only while it is
written down: see the "Built but unreachable" section of
`docs/OFFICIAL-PARITY.md`, which must be kept in step with this list.

## Permission bits

The client keeps named constants for the permissions it checks, and
`src/discord/permissions_catalogue.rs` holds all 53 of Discord's, generated
from its own table in
`.references/official-internals/userdoccers__github/pages/topics/permissions.mdx`.

The two lists are maintained separately - hot paths want named constants - so a
test checks every constant against the generated table. That is how
`MODERATE_MEMBERS` was found to be `1 << 42`, which is `USE_SOUNDBOARD`: every
timeout permission check had been asking whether the member could use the
soundboard.

Regenerate the catalogue when Discord adds permissions. Deprecated ones are
struck through in the table and excluded, which is why the bits are not
contiguous - 47 was Clyde.
