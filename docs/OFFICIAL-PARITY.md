# Parity with the official client

`docs/FEATURES.md` measures this fork against 114 third-party clients. This
measures it against Discord's own client, which is a higher bar and a different
list: the third-party clients skip most of what a server admin or an account
holder needs.

**Parity, not bloat.** The test for whether something belongs here is whether
its absence makes the client unable to do a thing people actually do with
Discord - not whether Discord ships it. The Nitro shop, quests, the activity
launcher, message-effect purchases and Clyde are deliberately out of scope.

## Done

Messaging, reactions, replies, forwarding, pins, threads, forums, polls,
search, stickers, custom emoji rendering, markdown, embeds, attachments,
typing indicators, unread and mention state, guild folders, notification
levels and mutes.

Voice: join and leave, mute and deafen, push-to-talk, noise suppression, DAVE
end-to-end encryption, per-participant volume, audio device selection, Go Live
screenshare, watching someone else's stream.

Moderation: kick, ban, unban, timeout, role assignment, the ban list, the
audit log.

Server administration: invites (list, create, revoke), custom emoji (list,
upload, rename, delete).

Soundboard: both sound lists, playing one into a voice channel, hearing what
other people play, and renaming or deleting a guild's own sounds from the
server panel. Uploading a new sound is not done.

Account: profile editing including per-guild identity, custom status, rich
activity, friends and blocking, QR and token login.

## Missing

Ordered by how often it stops someone doing something.

### Channel management

No way to create, rename, delete or reorder a channel or category, set a
topic, set slowmode, mark a channel NSFW, or edit permission overwrites. This
is the largest single hole in server administration: everything else in a
server's settings assumes the channels already exist.

### Role management

Creating, deleting and editing a role's permissions are done, as is the
per-channel overwrite editor for @everyone. Still missing: renaming and
recolouring a role, reordering, and choosing a role other than @everyone for a
channel overwrite - the last wants a role picker in front of the grid.

### Guild settings

The REST side takes name, verification level, default notifications, the
explicit-content filter, AFK channel and timeout, and the system channel; the
icon has its own call because encoding an image should not abandon a name
change made in the same form. A settings tab shows what the snapshot holds.

The settings tab edits the name, cycles the verification level, and sets the
icon from a file. Still missing: AFK channel and timeout, the system channel,
banner and splash, vanity URL, discovery, and the community toggles. Two fields
cannot even be displayed yet - default notifications and the explicit-content
filter are not parsed off the wire, so showing them would mean showing a
guess.

### Account settings

Username, email and password changes; two-factor enrolment and removal;
backup codes; account deletion and disabling. Currently the client can read the
account and edit the profile, but nothing that changes credentials.

### Connections

Both clients list your linked accounts - Spotify, GitHub, Steam, and the rest -
show or hide each on your profile, turn activity sharing on or off, and unlink
one. In the TUI it is an options category (`<leader> l`, or `l` from the options
picker); in the GUI it is a panel on the same action.

Linking a new account is deliberately absent: it is an OAuth flow through a
browser, which would mean this client handling someone else's credentials.
The panel says so rather than leaving an empty list to look broken.

An unverified connection is labelled as such. It looks linked but Discord will
not show it on a profile, which otherwise reads as a bug in this client.

Still missing: other people's connections on their profiles.

### Privacy and safety

Both clients show and change direct-message scanning, whether servers you join
from now on may send you direct messages, and the three friend-request sources.
TUI: an options category (`<leader> y`, or `y` from the picker). GUI: a panel on
the same action.

The rows live in the core (`PrivacySetting`) rather than in each front end,
because the part that drifts when written twice is that the three friend-request
flags share one field - toggling one has to carry the other two, or it clears
them. Same reason the "new servers" row is inverted there: Discord stores the
restriction and the row is phrased as the permission.

A setting that never arrived reads as unknown rather than off. Showing a default
would describe the account as more exposed than it may be.

Written through the legacy `/users/@me/settings` endpoint, which is where these
same fields are read from in READY - editing one representation while displaying
another would leave a rejected write showing as applied.

Also the data controls: being found by email or phone, local account detection,
contact sync, and screen-reader detection. Both of the first two live in one
bitfield, so they carry each other the same way the friend sources do.

Both clients render `PrivacySetting::ALL`, so a row added to the core appears in
both without either being edited - which is the point of it living there.

Still missing: per-guild direct-message restrictions (`restricted_guilds`) for
servers already joined. That needs a guild-scoped entry point rather than a row
in this panel.

### Sessions and authorised apps

No list of active sessions and no way to log another one out, which is the
first thing anyone wants after a scare. No list of authorised OAuth
applications or bots, and no way to revoke one.

### Blocked and ignored users

Blocking works; there is no list, so unblocking someone requires finding them.

### Sticker management

Stickers render. Uploading, renaming and deleting a guild's stickers does not
exist, unlike emoji which now does.

### Webhooks and integrations

Neither listed nor manageable.

### Context menus

Done in the TUI: right-clicking a row focuses its pane, moves the selection to
the row under the pointer, and opens that pane's action menu. The menus already
existed and were reachable by keyboard, so this was routing rather than new
menus.

Done in the GUI too: right-clicking a message, channel or member opens a menu
at the pointer, built from the same permission checks the panels use, so a menu
never offers what a panel refuses. Entries that cannot be used are greyed with
the reason rather than hidden.

Still missing: right-click on a guild in the sidebar, and the menu does not yet
cover everything the panels do - pin, forward and thread actions are on the
hover row only.

### Video

Camera in a call, and receiving anyone else's camera. Camera capture exists and
goes out over Go Live; the voice connection's own video path does not. Streams
and camera feeds should also be poppable into their own windows.

Media is no longer a build flag. A client that cannot hear a call or see a
stream is not a client, and making it optional meant the paths nobody built
were the paths nobody tested - which is how the claim that it did not build
survived long after it had stopped being true.

### Per-user controls

Locally muting someone, hiding their camera, and ignoring them - Discord's
lighter-touch alternative to blocking. Per-participant volume already exists.

### Voice channel status

Setting the short status line on a voice channel.

### AutoMod

Both clients list a guild's rules, turn one on or off, and delete it. A rule
says what fires it and what it then does, and an unrecognised trigger or action
is shown with its number rather than hidden - Discord adds them, and a rule
nobody can name is still filtering the server.

Still missing: creating a rule, and editing its keyword list, regex patterns,
mention cap or exempt roles and channels. That is a form of its own rather than
a row in a list.

### Members, onboarding and welcome

Browsing and searching members from settings, pruning, bulk-ban; the welcome
screen, onboarding questions, new-member actions, and membership screening -
which the client detects as a participation restriction but can neither
configure nor complete.

### Scheduled events, stages, templates, discovery

All absent. Stage channels were previously listed as a maybe and are now in
scope.

## Deliberately out of scope

Nitro and its shop, boosts as a purchase flow, quests, the activity launcher
and Watch Together, message effects, avatar decorations, profile themes, Clyde,
and the gift inventory - Discord selling something rather than Discord working.

Also out: creator monetisation, role subscriptions, entitlements, SKUs,
premium referrals, billing, payments, the store, promotions, guild analytics
and top-games. Selling and measuring, neither of which this client does.

## Order of work

Roughly by how often the absence stops someone, with shared machinery pulled
forward so it is not built twice.

1. **Channel settings form** - topic, slowmode, NSFW. Create, rename and delete
   are done; these three want a form rather than a one-field prompt.
2. **Roles and permissions.** The permission editor is shared by role editing
   and per-channel overwrites, so both land together, and channel reordering
   comes with them.
3. **Guild settings** - name, icon, verification, system and AFK channels.
4. **Context menus.** Not a feature so much as how people reach the features
   that already exist; the TUI's work is mouse routing, the GUI's is the menus.
5. **Expression management** - one panel for emoji, stickers and sounds, which
   finishes soundboard rename/delete and sticker management at the same time.
6. **AutoMod.**
7. **Account settings** - credentials, 2FA, sessions, authorised apps.
   Password and 2FA fields are the user's to drive, never the agent's.
8. **Per-guild DM restrictions** - the last of the privacy group.
9. **Members, onboarding, welcome screen, widget.**
10. **Video** - camera on the voice connection, receiving remote video,
    pop-out windows, per-user mute and hide. Needs a real account to verify.
11. **Events, stages, templates, discovery.**

Then, and only then, the extras from AGENTS.md: offline-first, merged
multi-account, and the rest.

## Where the two clients differ

Everything is meant to work in both. Where one is ahead, it goes here rather
than being described as done.

- **Guild settings**: both edit the name and set the icon. The TUI also cycles
  the verification level; the GUI shows it without changing it.
- **Channel settings**: both do create, rename, delete, topic and slowmode.
  Age-restriction and the voice user limit are TUI only.
- **Adding a soundboard sound**: neither client can upload one.

## Built but unreachable

Core work whose interface has not been built yet. A legitimate intermediate
state, but only while it is written down - `docs/PARITY.md` has the sweep that
regenerates this list, and the two must be kept in step.

- `ReorderChannels`, `ReorderRoles` - both want drag-and-drop or a move
  control, which neither client has a pattern for yet. The REST side sends all
  moved rows in one request, since position decides who wins a permission
  conflict and a sequence of single moves would briefly hand out the wrong
  permissions.
- `SetVoiceChannelStatus` - small, and grouped with the other smaller gaps.
