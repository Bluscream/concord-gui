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
| Per-channel tabs | 26 | done |
| Custom emoji | 25 | done |
| Threads | 22 | done |
| Search | 22 | done |
| Embeds | 21 | done |
| Notifications | 21 | done |
| Reply | 19 | done |
| Cross-platform | 17 | partial |
| Token login | 16 | done |
| Keybindings / vim | 16 | done |
| Rich presence / activity | 15 | done |
| Reactions | 14 | done |
| Moderation (kick/ban) | 14 | done |
| Voice / calls | 14 | done |
| Friends list | 13 | done |
| Markdown parsing | 12 | done |
| Pins | 11 | done |
| Forums | 9 | done |
| Guild folders | 9 | done |
| QR login | 9 | done |
| Animated emoji | 9 | done |
| Sounds | 9 | done |
| Slash commands | 8 | done |
| Typing indicators | 8 | done |
| Screenshare | 7 | partial |
| Stickers | 7 | done |
| Edit / delete | 6 | done |
| User profiles | 5 | done |
| Video / camera | 5 | partial |
| Noise suppression | 4 | done |
| Server management | 4 | done |
| Image viewer | 4 | done |
| Inline images | 4 | done |
| Multi-account | 3 | **missing** |
| Not Electron | 3 | done |
| E2EE voice (DAVE) | 3 | done |
| Browser impersonation | 3 | partial |
| Custom status | 3 | done |
| Read receipts / ack | 3 | done |
| Polls | 3 | done |
| Audit log | 2 | done |
| Role management | 1 | done |
| Invite management | 1 | done |
| Emoji management | 1 | done |

## Missing, most-wanted first

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

**Half done, and worth being precise about which half.**

A camera is now a capture target alongside screens and windows, so its frames
go through the same scaling, encoding and RTP path the screenshare already
uses. On Linux that is V4L2, converting YUYV or MJPEG to RGBA in-process
rather than pulling in libv4l. Cameras appear in the existing capture picker.

What that gives you is a camera over **Go Live**, which is the stream path.
What it does not give you is Discord's "turn on camera" in a call, which puts
video on the *voice* connection under a different SSRC and negotiation. That
is the remaining half.

macOS and Windows enumerate no cameras: the capture backends for those
platforms are screen-only, and the honest failure is an empty list rather than
a target that cannot start.

Untested against real hardware, like everything else here.

## Partial

- **Rich presence / activity** - Both clients show activities and can set one.
  The GUI's editor offers Playing, Listening, Watching and Competing plus the
  three lines Discord renders; Streaming is left out because it shows nothing
  without a verified stream URL, and a custom status keeps its own prompt.
  An activity set elsewhere - another client, or a local app over the RPC
  socket - is absorbed from our own presence update, so the button shows what
  is really being broadcast rather than only what this client last sent.
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
- **Image viewer** - Clicking an inline image opens it full size, with three
  zoom steps and arrows through the other images in the same message. The zoom
  steps are the TUI's own `AttachmentViewerZoom`, re-exported rather than
  reinvented, so "large" means the same thing in both clients.

  Paging carries every image in the message rather than looking them up again
  from the row, which may have scrolled away. It skips non-images: a message
  mixing a screenshot with a zip should not page onto the zip.
- **Friends list** - Both clients can now send, accept, cancel and decline
  requests, unfriend, block and unblock. The four outcomes of "remove" are one
  endpoint at Discord, so each is named for what it does rather than sharing a
  generic label. Requests by name go through `/friend`; the name is parsed in
  the core, and a name that cannot be parsed is refused before a request is
  spent on it. Every friend action is behind the rule 6 warning, since
  managing the friends list is one of the things Discord's anti-spam checks
  watch. One deliberate deviation from every surveyed client: they all send a
  discriminator unconditionally because they predate pomelo, and this sends
  one only when the typed name has a `#`.
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


- **Server management** - Both clients can now list and revoke invites, make an
  invite to the open channel, list and delete custom emoji, and read the audit
  log. One panel with three tabs rather than three entry points, since they are
  all "administering this server".

  Two deliberate choices. The audit log has no row action: history is a record,
  not something to edit from the client that reads it. And an unrecognised
  audit action keeps its number rather than being dropped - Discord adds action
  types regularly, and silently hiding them would hide moderation from the log
  people read specifically to find out what was done.

  Discord writes "no limit" as 0 for both an invite's uses and its lifetime.
  Passing that through would render as "3/0", which reads as already spent, so
  both are carried as `Option` and shown in words.

- **Per-channel tabs** - Both clients now have them. The same model in each,
  writing the same `open_tabs` key, so a tab opened in one is a tab in the
  other. A tab carries its draft, because a tab that forgets what you typed is
  only a bookmark.

  Opening a channel that is already tabbed switches to it rather than opening a
  second tab: two tabs onto one channel would give it two drafts and no way to
  tell which a message would be sent from. Tabs are restored on the first event
  that brings channels rather than at Ready, where every one would be dropped
  as unknown.

- **Emoji upload** - Adding an emoji from an image on disk works in both
  clients, reusing the avatar encoder rather than a second one. The name comes
  from the filename with anything Discord rejects replaced by an underscore,
  since "party parrot.png" is an ordinary filename and an illegal emoji name;
  a filename that leaves nothing usable is refused before the upload rather
  than after it. The 256 KB limit is checked locally for the same reason.
