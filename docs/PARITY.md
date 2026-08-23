# Parity tracking

Two targets, in order: the concord TUI first, then the official client minus
the parts we do not want.

## Against the concord TUI

### How this is measured

Five checks, because each one missed gaps the next found:

1. **Keybinding actions.** The TUI's 152 actions in
   `crates/ui/src/keybindings/actions.rs`. Too weak: it counts names, not behaviour.
2. **Command coverage.** Which `AppCommand` variants each client sends,
   diffed:

   ```sh
   comm -23 \
     <(grep -rhoE "AppCommand::[A-Z][A-Za-z]+" crates/tui/src/tui/ | sort -u) \
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
  crates/tui/src crates/gui/src --include=*.rs | grep -v test
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
constructs - it is public API, so it is never "dead". This finds those, and the
ones only one client can reach:

```bash
python3 scripts/unreachable-commands.py
```

It should report nothing. `RenameEmoji` was dispatched and reachable from
neither; `CreateChannelInvite` and `RenameEmoji` were later reachable only from
the GUI; `DeleteAutoModRule` was GUI-only while the TUI's AutoMod tab could
only toggle. All were found this way rather than by noticing.

It also finds a second thing the variant check cannot: a function that builds a
command but that nothing calls. The variant is still mentioned, so the sweep
would report the client as reaching it, and clippy stays quiet because these
functions are `pub` and so never dead code. Both `mark_event_interest` and
`move_role` sat orphaned this way after a button was rewired.

Verify the check still works by orphaning a call on purpose and running it -
a check nobody has seen fail is a check nobody should trust.

This replaced a pair of shell one-liners whose regex was `^    [A-Z][A-Za-z]+ \{`
- which matches only struct variants. Three unit variants (`LoadConnections`,
`ModifyConnection`, `DeleteConnection`) sat unreachable without the check
seeing them, because a check that silently skips part of its input reports
success either way. The script matches `{`, `,` and `(` forms.

Anything else on the unreachable list is core work whose interface has not been
built yet. That is a legitimate intermediate state, but only while it is
written down: see the "Built but unreachable" section of
`docs/OFFICIAL-PARITY.md`, which must be kept in step with this list.

## Running the GUI tests

The GUI's model tests are behind `#[cfg(all(test, feature = "fixtures"))]`, so
`cargo test -p concord-gui` runs 76 of them and silently skips 66. The feature
is not optional for verification:

```bash
cargo test -p concord-gui --features fixtures --all-targets
```

Without it, `crates/gui/src/demo.rs` is not compiled either - and its match over
`AppCommand` is deliberately exhaustive, so it is the check that catches a
command added to the core with no offline answer. Thirty-three had accumulated
behind the missing feature flag before this was noticed.

## Checking an endpoint against the references

`../.references/` holds open-source Discord clients and
`official-internals/userdoccers` holds the community API documentation. Checking
a guessed endpoint against them has caught real errors that no test here could:

- `/discoverable-guilds` takes no `query`. Searching is a separate route,
  `/discoverable-guilds/search`, so a query sent to the listing route was
  silently ignored and returned the default list - a search that appeared to
  match everything.
- `onboarding_prompts_seen` and `onboarding_responses_seen` map ids to Unix
  millisecond timestamps, not booleans. `true` is the wrong type for the field.

Both were shipped and both looked right. Check the shape before writing the
struct, not after.

## Values Discord serves

Some of what this client needs is Discord's to decide rather than ours. Those
live in `src/discord/remote_config.rs` behind three layers, in this order: what
Discord last told us and we wrote to disk, then a fresh fetch, then a
compiled-in default.

The defaults are what Discord serves today, not placeholders - a first run with
no network behaves exactly as it would with one. That is the point: the
fallback is a working client, not a degraded one.

The gateway URL is why this exists. Discord's own guidance is that clients
cache it and refetch only when the cached one fails, so the reader never
touches the network and the fetcher runs once at gateway start. A fetch that
fails leaves the cache stale on purpose, so the next start retries rather than
waiting a day.

A URL Discord returns that is not a websocket URL is refused rather than
cached: writing one would break every start until the cache expired, which is
worse than never updating.

Caps that Discord documents but does not serve - name lengths, sticker size,
keyword counts - stay compiled in. Those are documentation, not configuration,
and pretending to fetch them would suggest they can change under us.

## Two clients on one store

`crates/cache/src/concurrent.rs` states the argument in full; the short version is
that this is not the general distributed-write problem and should not be solved
like one.

Discord is the single writer. Neither client invents data - both cache what one
server told them, and that server stamps what it sends. So:

- **Duplicates** are answered by the primary key, because every entity carries
  an id Discord assigned. Checking whether a row exists before inserting would
  be both unnecessary and a race, since another client can write between the
  read and the insert.
- **Stale writes** are answered by a revision guard inside the upsert. Guilds
  and channels carry Discord's monotonic `version`; the guard is in the SQL
  rather than in a read-then-write for the same reason as above.
- **Deletes** need a tombstone, because the two rules above do not stop a
  client with stale state re-inserting a row another client has just learned is
  gone. A tombstone is lifted only by a higher revision, which is what makes a
  genuine re-creation still work.

The clock never comes from a client. Two clients disagree about the time by
however far their clocks differ, and a cache resolving conflicts by local
timestamp would let the more wrong one win.

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

## Where the code lives

Six crates. The core knows about one account, keeps its state in memory, and
has no idea whether anything is drawing it or writing it down:

| Crate | What it is |
| --- | --- |
| `concord` | The core. Gateway, REST, state. No renderer, no database. |
| `concord-ui` | What both front ends share: key bindings, theme resolution, fuzzy matching, the vocabulary of panes and action menus. |
| `concord-cache` | The offline store, attached to the core as a `ClientExtension`. |
| `concord-merge` | One view across several signed-in accounts. Early. |
| `concord-fixtures` | A fake Discord - a populated state, event builders, and a backend that answers commands - for running a front end with no account. |
| `concord-tui` | The terminal client. |
| `concord-gui` | The GPUI client. |

The rule for `concord-ui` is a renderer, not the core: depending on `concord`
is fine, since it sits below both front ends. Needing ratatui or GPUI is what
makes a thing one front end's. One type failed that test and stayed behind -
`LocalUploadPreviewView`, which borrows a ratatui image protocol.

`concord-ui` owns its own vocabulary for the things a front end would
otherwise have to borrow from a console library: `key::{KeyCode, KeyModifiers,
KeyEvent}` and `style::{Color, Modifier, Style, BorderType}`. Crossterm and
ratatui are optional, behind the `terminal` feature, and exist only to convert
between those and their own. The terminal front end turns the feature on; the
GPUI one does not, and neither library appears anywhere in its dependency
tree - which is the point, and worth re-checking with:

```
cargo tree -p concord-gui -e normal -i crossterm
```

It should report no match. The accessors on `Theme` hand back whichever
vocabulary the build is for, so the terminal front end is not converting at
several hundred call sites and the GPUI one never sees a ratatui type.

Extensions attach through `concord::discord::ClientExtension`: three methods,
and an `EventInjector` for putting events back. That is the whole surface. The
cache uses it to replay what it has; the merge crate will use it the same way.

## Running against the fake

`concord-fixtures` is a normal dependency of both front ends, gated by their
own `fixtures` feature, not a dev-dependency. That is deliberate: it means the
real binary can be run against a fake world, not only the test harness.

Two pieces are re-exported from the core rather than living in that crate, and
for the same reason both times - the core's own tests use them, and a crate
cannot depend on something that depends on it:

- `world`, the populated `DiscordState`. Building one from nothing means
  reaching inside caches that are private on purpose, since state is otherwise
  meant to change only by applying events.
- `events`, the gateway event builders.

A front end imports both from `concord_fixtures` and does not need to know
which side of that line anything falls on.

### Driving it

`FakeBackend` takes an `AppCommand` and returns what happened. It holds no
channels and knows about no front end, because the two wire it up differently:

- The GPUI client forwards each emission into its `Update` stream.
- The terminal client publishes the events through a real `DiscordClient` that
  never opens a gateway, so the state store, snapshot revisions and effect
  ordering all behave exactly as they do against Discord.

Both are reached the same way:

```
CONCORD_TOKEN=demo cargo run
```

One wrinkle worth knowing if you write a test against it: `Ready` mutates state
without being delivered as an effect, so a test that waits on the effect stream
for it will hang rather than fail. Watch the snapshot channel instead.
