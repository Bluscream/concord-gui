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

## Practical notes

- **Building**: the host is immutable, so builds run in the `arch` distrobox
  (`opusic-sys` needs cmake). `scripts/deploy.sh` builds, installs and runs.
- **Demo mode**: `--token test` loads an offline fixture — guilds, channels,
  DMs, messages, voice — so the UI can be exercised without an account. New
  commands should be answered in `crates/gui/src/demo.rs`, or the feature
  silently does nothing there.
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
