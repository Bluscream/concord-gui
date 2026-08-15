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

Account: profile editing including per-guild identity, custom status, rich
activity, friends and blocking, QR and token login.

## Missing

Ordered by how often it stops someone doing something.

### Soundboard

Nothing at all. Needs the guild sound list, the default sounds, playing one
into a voice channel, and hearing others'. Endpoints are
`/guilds/{id}/soundboard-sounds` and `/soundboard-default-sounds`, plus a voice
gateway opcode to play.

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
goes out over Go Live; the voice connection's own video path does not. See task
23 for why that is scoped separately.

### Voice channel status

Setting the short status line on a voice channel.

## Deliberately out of scope

Nitro and its shop, boosts as a purchase flow, quests, the activity launcher
and Watch Together, message effects, avatar decorations, profile themes, Clyde,
and the gift inventory. All of these are Discord selling something rather than
Discord working.

Stage channels are a maybe: they are real functionality, but they matter to a
vanishingly small number of servers and they carry a large protocol surface.
Revisit if anyone asks.
