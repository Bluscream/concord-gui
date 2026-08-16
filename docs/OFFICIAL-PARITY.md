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

Soundboard: both sound lists, playing one into a voice channel, and hearing
what other people play. Uploading a new guild sound is not done; renaming and
deleting are wired but not yet reachable from either client.

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

Roles can be *assigned* and that is all. Creating, deleting, renaming,
recolouring, reordering, and editing a role's permissions are all absent, as is
the per-channel overwrite editor.

### Guild settings

Name, icon, banner, splash, verification level, default notification level,
explicit content filter, system channel, AFK channel and timeout, vanity URL,
discovery settings, and the community toggles.

### Account settings

Username, email and password changes; two-factor enrolment and removal;
backup codes; account deletion and disabling. Currently the client can read the
account and edit the profile, but nothing that changes credentials.

### Connections

Linked accounts - Spotify, GitHub, Steam, and the rest - are neither listed nor
manageable, and their visibility toggles are what make them appear on a
profile. Other people's connections are also not shown on their profiles.

### Privacy and safety

Who may send a DM, whether DMs are scanned, friend-request sources, and the
data controls.

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

The GUI has a message action row and the TUI has action popups, but neither has
the right-click menu the official client puts on every user, channel, guild and
message. This is less a feature than the way most people reach the features
that already exist.

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

Keyword filters, spam presets, allow lists, and the actions a rule takes. None
of it exists.

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
8. **Connections and privacy.**
9. **Members, onboarding, welcome screen, widget.**
10. **Video** - camera on the voice connection, receiving remote video,
    pop-out windows, per-user mute and hide. Needs a real account to verify.
11. **Events, stages, templates, discovery.**

Then, and only then, the extras from AGENTS.md: offline-first, merged
multi-account, and the rest.

## Built but unreachable

Core work whose interface has not been built yet. A legitimate intermediate
state, but only while it is written down - `docs/PARITY.md` has the sweep that
regenerates this list, and the two must be kept in step.

- `ReorderChannels`, `SetChannelOverwrite`, `DeleteChannelOverwrite` - all
  three want the permission editor, which is shared with role editing and is
  therefore built alongside it rather than twice.
- `SetVoiceChannelStatus` - small, and grouped with the other smaller gaps.
- `RenameSoundboardSound`, `DeleteSoundboardSound` - want the expression
  management panel that emoji, stickers and sounds will share.
