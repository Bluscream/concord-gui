# Feature survey

What the 114 clients in `.references/` advertise, and where concord-gui
stands against them.

## Method, and its limits

Built by scanning each repository's README and other top-level markdown for
feature keywords, then cross-referencing against this fork. Two caveats worth
stating before anyone plans from it:

- **It measures what a project claims, not what it does.** A README bullet is
  not a working feature, and an undocumented feature is invisible here.
- **Keyword matching over-counts.** "Per-channel tabs" catches any mention of
  tabs; "Custom emoji" catches the word emoji anywhere. Treat the counts as a
  rough popularity signal, not a census.

Regenerate with:

```bash
python3 scripts/survey-references.py ../.references
```

## The matrix

Sorted by how many clients advertise the feature - a proxy for what users of
third-party Discord clients actually expect.

| Feature | Clients with it | concord-gui |
|---|--:|---|
| DMs / group DMs | 36 | done |
| File upload | 30 | done |
| Theming | 28 | done |
| Unread / mentions | 27 | done |
| Per-channel tabs | 26 | **missing** |
| Custom emoji | 25 | done |
| Threads | 22 | done |
| Search | 22 | done |
| Embeds | 21 | done |
| Notifications | 21 | done |
| Reply | 19 | done |
| Cross-platform | 17 | partial |
| Token login | 16 | done |
| Keybindings / vim | 16 | done |
| Rich presence / activity | 15 | partial |
| Reactions | 14 | done |
| Moderation (kick/ban) | 14 | done |
| Voice / calls | 14 | done |
| Friends list | 13 | partial |
| Markdown parsing | 12 | done |
| Pins | 11 | done |
| Forums | 9 | done |
| Guild folders | 9 | done |
| QR login | 9 | done |
| Animated emoji | 9 | partial |
| Sounds | 9 | yes |
| Slash commands | 8 | done |
| Typing indicators | 8 | done |
| Screenshare | 7 | partial |
| Stickers | 7 | done |
| Edit / delete | 6 | done |
| User profiles | 5 | done |
| Video / camera | 5 | **missing** |
| Noise suppression | 4 | done |
| Server management | 4 | partial |
| Image viewer | 4 | partial |
| Inline images | 4 | done |
| Multi-account | 3 | **missing** |
| Not Electron | 3 | done |
| E2EE voice (DAVE) | 3 | done |
| Browser impersonation | 3 | partial |
| Custom status | 3 | done |
| Read receipts / ack | 3 | done |
| Polls | 3 | done |
| Audit log | 2 | **missing** |
| Role management | 1 | done |
| Invite management | 1 | partial |
| Emoji management | 1 | **missing** |

## Missing, most-wanted first

### Per-channel tabs

*26 clients* — 6cord, Dorion, Echo-Client, Fast-Discord, SailDiscord, Turbulence

The single most common thing this fork lacks. Open several channels at once and switch without losing scroll position. Both clients would need a tab strip and per-tab view state.

### Multi-account

*3 clients* — TriCord, acheron, endcord

One session per process today. Needs the token store, gateway and state keyed
by account, which touches the core deeply.

The plan here goes further than any of those three: not an account switcher
but a **merge** - one DM list, one server list, one unread state, with the
source account shown as a corner badge on each avatar and a source selector in
the composer when more than one account could send. Design in AGENTS.md.

### Video / camera

*5 clients* — NativeCord, Voxa, cordless, popcord, reliable-discord-client-irc-daemon

Voice works; camera does not. The core has no capture path for it.

### Audit log

*2 clients* — OldCordV3, abaddon

GET /guilds/{id}/audit-logs, plus a viewer. Abaddon has it.

### Emoji management

*1 client (Abaddon)* — abaddon

Upload, rename and delete guild emoji.

## Partial

- **Rich presence / activity** — Custom status works; showing and setting full activities does not. Tracked as task 7.
- **Server management** — Kick, ban, timeout and roles are done. Invites, emoji and audit log are not.
- **Invite management** — Joining by invite works. Creating, listing and revoking invites does not.
- **Animated emoji and avatars** — These already worked; the survey entry was
  an assumption rather than a check. GPUI decodes animated WebP and GIF, the
  core builds animated URLs for both, and Discord's `a_` hash prefix selects
  the GIF. What was genuinely missing - and Abaddon offers - is a way to turn
  animation *off*, which now exists and matters on a Raspberry Pi.
- **Sounds** - Message and voice sounds now play in both clients. They used to
  live in the TUI's runtime, so the GUI was silent; the playback module moved
  to `src/sound` and both front ends call it. Sounds have their own switch
  rather than following the desktop-notification one, because wanting the
  popup without the noise is common.
- **Screenshare** — Compile-verified only - blocked by a zbus/tokio incompatibility reaching us through GPUI's own dependencies.
- **Image viewer** — Images render inline. There is no full-size viewer with zoom.
- **Friends list** — Relationships are parsed and cached; neither client offers management.
- **Browser impersonation** — The core sends web-client super-properties, but this has never been audited end to end. Abaddon documents the standard to match.
- **Cross-platform** — Linux is exercised. Windows and macOS are unverified - GPUI supports both, but neither has been built or run here.

## What this fork has that almost nothing else does

Worth knowing, because it is the argument for the project existing:

- **E2EE voice (DAVE)** — 3 of 114 clients
- **Noise suppression** — 4 of 114
- **Polls** — 3 of 114
- **Read state / acking** — 3 of 114
- **Two front ends on one core** — a TUI and a GPU-accelerated GUI sharing
  `src/discord/` entirely, including the keymap and theme files

Abaddon is the closest comparison overall, and remains ahead on invites,
emoji management and the audit log.

