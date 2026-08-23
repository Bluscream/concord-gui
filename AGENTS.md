# Working on this repository

## What this is

**concord** is a third-party Discord client written in Rust, with a terminal
UI. Its `src/discord/` is a complete, front-end-agnostic Discord
implementation — gateway, REST, state, permissions, voice — with no terminal
coupling. `src/tui/` is one front end on top of it.

**concord-gui** is this fork. It adds `crates/gui/`, a second front end built
on GPUI (Zed's GPU-accelerated toolkit), reusing that same core unchanged.

The goal is a Discord client that is fast, native, and free of the official
client's bloat — no Nitro upsells, no activities, no discovery, no telemetry
beyond what the API requires. It should be good enough to daily-drive, on
hardware as modest as a Raspberry Pi.

## Rules

### 1. Put it in the core, and give it to both front ends

Anything beyond parity with the TUI goes in `src/discord/` first, then lands
in **both** `src/tui/` and `crates/gui/`. A feature that exists in one client
only means the core grows capabilities half its front ends cannot reach.

This has been got wrong before: invites and forwarding were built for the GUI
and had to be retrofitted into the TUI afterwards. Do it in the right order.

Fixes follow the same rule. A bug fixed in a front end that was really a core
bug leaves the other client broken.

### 2. Check every change against both clients

`cargo check` is not enough, twice over:

- It does not build tests. Test-only breakage has slipped through **four
  times** in this repository. Run `cargo test`, not `cargo check`.
- The two front ends compile separately. Run both:

```bash
cargo test --features fixtures && cargo test -p concord-gui --features fixtures
```

Before claiming anything is done, also run clippy over everything:

```bash
cargo clippy --features fixtures --all-targets
```

Both crates are expected to be warning-free. Warnings are not noise here —
`never used`, `never read` and `never constructed` have repeatedly been the
only evidence that a feature was unreachable from the UI.

### 3. Follow the existing style

New code should look like the code already around it. The original concord
has consistent conventions — module layout, visibility (`pub(in crate::tui)`
and friends), naming, how popups and state are structured, how commands and
events flow. Match them. Depart only when there is a real reason, and say what
it is.

This applies to comments as much as to code. The existing comments explain
*why*, not *what*. Keep doing that.

### 4. Write down quirks when you find them

When something turns out to behave unexpectedly — an API that rejects a value
rather than clamping it, a field that means something other than its name, a
protocol rule that is not obvious — fix it **and leave a short comment saying
what the quirk was**. Otherwise the next person removes the workaround because
it looks unnecessary.

Real examples already in the tree:

```rust
// The kind is the core's own string name, not a numeric wire value.
// Discord carries a custom status in `state`, not `details`; putting it in
// the wrong field shows nothing to anyone.
// Over the maximum, Discord rejects the whole request rather than trimming it.
// 1 is FORWARD; 0 is the ordinary reply this shares a field with.
```

One or two lines. Enough that the constraint survives the next refactor.

### 5. Protect the user, do not restrict them

Something risky, unusual, or likely to go wrong should be **explained and
confirmed**, never made impossible. The user owns the account and the
decision; our job is to make sure it is an informed one.

- Say what the risk actually is, in plain words. "This may get your account
  flagged" is useful; a greyed-out button is not.
- Require an explicit confirmation for the dangerous path.
- Offer "don't ask again" where the warning would otherwise become nagging.
  Someone who has read it once and meant it should not be asked forever.
- Never silently refuse, and never quietly do something different from what
  was asked.

The same applies to permissions: where Discord will refuse an action, show it
with the reason rather than hiding it. A control that explains itself teaches;
a control that vanishes confuses.

The list of risky actions, their wording and their opt-outs live in
`src/risk.rs`, shared by both clients, so a warning added for one is worded the
same in the other. Each risk has its own switch: a shared one would silence
warnings the user never read, which is the one way "don't ask again" can do
harm. A warning stacks over whatever is open rather than replacing it - a form
that vanished behind a warning would come back empty.

### 6. Third-party clients and the spam filter

Discord's anti-spam heuristics treat third-party clients more harshly than the
official one, and a false positive means a disabled account or a forced
password reset. Abaddon's README documents which actions most often trigger
it:

- **Joining or leaving servers** — the usual cause
- Frequently disconnecting and reconnecting
- Starting new DMs
- Managing the friends list
- Editing your user profile while connected via a third-party client

Features touching these should warn before acting, per rule 5 — explain the
risk, confirm, allow "don't ask again". Do not disable them.

Abaddon's other lesson is worth copying: it identifies as the web client,
uses API v9 throughout, and deliberately avoids endpoints the web client does
not itself use. New requests should follow that principle — if the official
web client would not send it, think hard before we do.

### 7. Losing access is not the same as removing data

The cache must distinguish **losing access** to a guild or group - leaving,
being kicked, being banned, a group DM you were removed from - from the user
**removing it** from the client.

While any data for it remains cached - messages, channel names, member
metadata - the guild or group stays in the list and stays browsable. What
changes is that it becomes inert:

- The message log, channel list and member list read normally from cache.
- Everything that writes is off: the composer is read-only, no editing
  channels or guild settings, no reactions, no invites, no moderation.
- The entry says plainly why it is inert - left, kicked, banned - rather than
  looking like a broken channel.
- A **Remove** item in its context menu discards the cached data and takes it
  out of the list. That is the only thing that removes it.
- Rejoining, by invite or otherwise, restores it to normal without the user
  losing the history they still had.

The reasoning is that a chat log is the user's, not the server's. Being
kicked from somewhere should not silently delete a conversation you were part
of, and the client should never destroy data as a side effect of someone
else's action. Deleting is a decision, so it needs a deliberate act.

This has consequences worth planning for rather than discovering: cached
guilds need a flag for why they are inert, the gateway must not treat a
GUILD_DELETE as "drop everything", and read-only needs to be a first-class
state in both front ends rather than a permission check that happens to fail.

### 8. Offline first

Rule 7 keeps data; this is what to do with it. The client should be useful
with no connection at all, not merely survive losing one.

**Everything known stays browsable.** Guild and channel lists, past messages,
member lists, profiles, pins, attachments already fetched. Nothing that is
already on disk should require the network to look at again.

**Search works locally.** Over cached messages, without the server. A remote
search adds results it could not have known about; it should not be the only
way to find something the client already has.

**Actions queue instead of failing.** Sending a message, joining a voice
channel or call, reacting, editing - these are accepted while offline and
carried out when the connection returns. A queued action is visible as queued,
and cancellable, rather than looking sent.

#### Draining the queue

This is the part to get right, because rule 6 applies directly: a burst of
traffic immediately after reconnecting is one of the patterns that gets
third-party clients flagged.

- **Pace the drain.** Space queued sends out rather than flushing them at
  once. Reconnecting and then emitting ten messages in a second is precisely
  the shape of a bot.
- **Keep order.** Messages queued for one channel send in the order they were
  written, or a conversation arrives scrambled.
- **Survive a restart.** The queue is on disk. Closing the client with unsent
  messages must not discard them silently.
- **Report failures.** A queued send can fail for reasons that did not exist
  when it was queued - the channel is gone, permissions changed, the guild was
  left. Say so and keep the text, per rule 5. Never drop it quietly.
- **Voice is a request, not a resumption.** A queued call join should ask
  again on reconnect rather than silently connecting audio to a channel the
  user queued an hour ago. Confirm if it is stale.

### 9. Lurking - confirmed, and it is read-only

Lurking is a real Discord state, it is entered explicitly, and **you cannot
send messages while in it**. That last point is the one that matters for
anything built on this, and it is the one the reference clients do not say.

What the sources establish:

- **It is read-only.** Discord's `PREVIEW_ENABLED` guild feature is documented
  as "Guild is accessable (read-only) without passing member verification",
  and preview is described as letting members who have not passed the
  verification gate view the guild *without interacting with it*. A pending
  member has to finish membership screening before becoming a full member who
  can talk. So a lurked guild is browsable and nothing more.
- **`LURKABLE` itself is deprecated.** The userdoccers guild-features list
  shows it struck through; the live mechanism is `PREVIEW_ENABLED`. discordgo's
  "lurkable guild" wording predates that rename, so treat it as historical.
- **It is entered by joining.** A Vencord patch in `discord-screenaudio`'s
  bundle matches `guildId:(\i),lurker:(\i)` in the official client's own
  `joinGuild` call. Lurking is a join with `lurker: true`, not a side effect of
  reading a guild you have no relationship with.
- **Leaving distinguishes the two.** `DELETE /users/@me/guilds/{id}` carries
  `{"lurking": bool}` - the `dm` client sends `false`, chorus models it as
  `GuildLeaveSchema { lurking }`.
- **Widget endpoints are unauthenticated** (`reflectcord` exempts
  `/guilds/{id}/widget.(json|png)`), but widget.json carries channel names and
  an online count - not messages.

**Two consequences for this client.** A lurk still goes through the join
endpoint, so it carries the same anti-spam risk a normal join does and belongs
behind the rule 6 warning. And a lurked guild must present as read-only: the
composer has to be disabled with a reason, the same way rule 5 says a refused
action is shown with its reason rather than hidden. Offering a composer that
silently fails would be the worst of both.

### How a source is shown

Avatars carry two pieces of information at once:

- **A coloured ring** around the avatar is the person's online status - the
  ring is presence, and nothing else.
- **A small badge in the lower-right corner** is the *source*: the profile
  picture of whichever of your accounts this DM or guild came through.

So at a glance: the ring says how they are, the corner says who you are to
them.

### When several accounts share a conversation

If two or more of your accounts have a DM with the same person, it is **one
chat**, not two. Messages you sent appear under whichever account sent them,
so the thread reads as a single conversation with your side attributed. The
corner badge changes from one account's picture to a *multiple people* icon
(the [flaticon 33308](https://cdn-icons-png.flaticon.com/512/33/33308.png)
shape) to say "more than one of you is here".

Guilds work the same way: a guild several of your accounts are in appears
once, badged as multi-source.

### Sending, when there is a choice

Where only one account can send, sending just works. Where more than one
could - a shared DM, a shared guild - the composer grows a **source selector
on its left**, so the account being spoken as is visible before the message
goes and changeable without leaving the input.

### What this implies

Worth knowing before starting:

- **Where the merge lives.** Not in the GUI, and not tangled into the core
  either: a layer of its own, above `src/discord/` and below both front ends.
  The core stays single-session and unaware - one `DiscordState` per account,
  exactly as today - and the merge tier owns fanning commands out to the right
  session and folding several sessions' state into one view.

  That keeps three things true: the core does not grow multi-account
  complexity it does not need, the TUI gets merged accounts for free rather
  than needing its own implementation, and a single-account session is just
  the merge tier with one account in it.
- The merge keys are the obvious ones: recipient user id for DMs, guild id for
  guilds. Everything else - unread counts, read state, typing, notifications -
  has to merge or dedupe on top of those.
- Notifications need deduping, or a shared guild notifies once per account.
- Several simultaneous sessions is exactly the pattern rule 6 warns about.
  Reconnect behaviour should be gentle, and connecting many accounts at once
  is worth a warning.

### 10. Every user-facing string goes through the catalogue

No literal text in the interface. Strings live in `i18n/*.ftl` and are read
with `t!("some-key")`; see `docs/TRANSLATING.md`.

The catalogue is in the core, so a string added for one front end is available
to the other. Fluent rather than gettext because it handles plurals and
grammatical agreement per language, and because Weblate hosts it natively -
community translation with suggestions and voting is the point, not an
afterthought.

Counts and names are **arguments**, not formatting:

```rust
t!("unread-count", "count" => unread as i64)   // yes
format!("{} unread", t!("unread"))             // no - unformattable in most languages
```

A missing translation falls back to English, so a partial language is usable.
A key with no entry at all returns itself, which looks like a bug in the
interface rather than an empty control.

### 11. Icons must render

Glyphs come from the Basic Multilingual Plane. Astral-plane emoji - U+1F3A4
and friends - have no coverage in the shipped fonts and draw as empty boxes,
which is what the first version of the voice controls did across four
buttons. Geometric and technical symbols (U+25xx, U+26xx, U+27xx) are safe.

Where the meaning is a colour, **draw it** rather than reaching for an emoji:
a filled circle is a div, and always renders.

Every icon needs a tooltip, and every tooltip goes through the catalogue. Use
`chrome::icon_button`, which does both.

## Practical notes

- **Building**: the host is immutable, so builds run in the `arch` distrobox
  (`opusic-sys` needs cmake). `scripts/deploy.sh` builds, installs and runs.
  It includes demo data **by default** while the project is pre-release -
  `--no-test` builds without. That default must flip before the first real
  release; a shipped build should not carry synthetic state or accept the
  literal `test` token.
- **Graphical Testing via Linux GUI MCP**:
  - **MANDATORY**: ALWAYS use the registered `linux-gui` MCP tools (`list_windows`, `find_window`, `run_app`, `click`, `type_text`, `press_keys`, etc.) directly for all GUI interaction and testing. NEVER call raw python scripts or sub-process imports to emulate or bypass MCP tool calls.
  - Always check if an existing `concord-gui` window/process exists (`find_window` or `list_windows`) before launching a new instance, and kill lingering processes to prevent duplicate windows.
  - Launch using `run_app` with `executable: "distrobox"`, `args: ["enter", "arch", "--", "/path/to/concord-gui"]`, and explicit Wayland/KDE environment variables (`DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`, `XDG_RUNTIME_DIR=/run/user/1000`, `RUST_LOG=debug`). Do not pass an auto-connect token argument if testing the login picker screen.
- **Verify in release too.** A dead-code warning that only the release profile
  reports has already caught a feature whose button was never actually added.
  `cargo build --release` before claiming a UI change landed.
- **Demo mode**: `--token test` or `--token demo` (or selecting Demo Mode on the login screen) loads offline fixture data — guilds, channels, DMs, messages, voice — so the UI can be exercised without an account. New commands should be answered in `crates/gui/src/demo.rs`, or the feature silently does nothing there.
- **References**: `.references/` holds 114 surveyed third-party clients. Read
  them before guessing at a request shape. Moderation endpoints came from
  Abaddon; the sticker payload was confirmed against two independent
  implementations. Reading working code beats discovering a payload from a
  rejection.
- **Parity tracking**: `docs/PARITY.md` records what is done, what is missing,
  and the checks used to tell the difference. Keep it honest — it exists
  because "done" was claimed several times when it was not.

## A caution

Nothing in this fork has been verified against a real Discord account. Every
check so far is structural: tests, the offline fixture, the type checker. Six
different audit methods have each found gaps the previous ones could not see,
so treat a clean run as "the checks I know how to write pass", not "nothing is
missing".
