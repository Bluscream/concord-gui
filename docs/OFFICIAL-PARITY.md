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

Username, email and password changes, two-factor enrolment and removal, and
backup codes - all in both clients (TUI `<leader> u`, GUI the same action).

The form is `AccountForm` in the core, not written once per client: the parts
that drift when duplicated are which fields are credentials, that a new password
must be typed twice, and that the current password is required for any change at
all. Submitting consumes the form, so no copy of three passwords is left in
panel state.

The confirmation field is this client's, not Discord's. Discord accepts whatever
new password it is given, so a typo would become the real password with no way
to notice until the next sign-in.

This client never computes a one-time code - the authenticator app does, which
is the point of the arrangement. So there is no HMAC and no clock arithmetic,
only a 20-byte secret from `OsRng`, base32-encoded and checked against RFC 4648's
own test vectors. The account name in the `otpauth://` URI is percent-encoded: a
username containing `?` or `#` would otherwise end the path and produce an
enrolment that silently does not match.

No password is ever drawn or printed. Fields render as bullets counted in
characters rather than bytes, and every type that can hold one has a
hand-written `Debug` - `Secret`, `TextInputState`, `AccountForm` - because a
derived one prints in full and `{:?}` on a whole panel is what a debug log does.

Still missing: account deletion and disabling.

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

Per-guild direct-message restrictions are on the server itself rather than in
this panel, which is where Discord's own client puts them: the TUI's guild
action menu (`D`) and the GUI's guild right-click. The endpoint replaces the
whole restricted list rather than merging, so the toggle carries every other
guild's restriction - both clients have a test for that.

The privacy group is complete.

### Sessions and authorised apps

Both in one panel in both clients - TUI `<leader> k`, GUI the same action -
because they answer the same question, and after a scare hunting through two
panels is the last thing wanted.

Sessions are selected and logged out in a batch; applications are revoked one
at a time. The difference is Discord's: a session logout needs the account
password and an app revocation does not, so batching only earns its keep on the
side that has a prompt to amortise.

The password is typed, used for that one request and dropped. It is never
stored, never written to config, and cannot reach a log: it travels as
`Secret`, whose `Debug` and `Display` both print `[redacted]`, and it is typed
into a masked `TextInputState` whose hand-written `Debug` does the same. Both
have tests, because `AppCommand` derives `Debug` and a plain `String` there
would be printed in full by any `{:?}` written later by someone with no reason
to suspect that variant carries a credential.



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

Pruning, the welcome screen and the guild widget are a Membership tab on the
server panel in both clients - one tab rather than three, because each is two
or three rows.

Pruning goes through the risk prompt, unlike every other row there: it is
irreversible and it acts on people who are not present to object. It is not
offered at all when the count is zero or has not arrived, since a warning about
removing nobody teaches the wrong lesson about the warning. Zero is the
commonest answer, because Discord exempts every member who has any role at all
unless that role is named in the request.

A value that has not arrived reads as unknown rather than off in both clients:
a welcome screen that has not loaded is not one Discord confirmed is disabled.

Still missing: browsing and searching members from settings, bulk-ban,
onboarding questions, new-member actions, and membership screening - which the
client detects as a participation restriction but can neither configure nor
complete. Editing the welcome description and the widget's channel needs a text
prompt each; only the toggles and the prune window are wired.

### Scheduled events, stages, templates, discovery

Events and Templates are tabs on the server panel in both clients.

Events list what is on, where, and how many said they are coming. Enter marks
you interested rather than cancelling: interest is what most people open the
list to change, and cancelling is destructive. An event still to come is
cancelled rather than deleted, which is Discord's own distinction - a cancelled
event stays visible so people who said they were coming can see it is off. One
already finished cannot be cancelled at all, so that row offers delete instead
rather than a button that always fails.

Templates list the shareable link, how many servers came from it, and whether
the template has fallen behind the server it was taken from - which the
template alone does not reveal. Enter syncs, `N` takes a new one, `d` deletes.

Still missing: creating and editing a scheduled event, which is a form of its
own; stage channels; and discovery.

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
8. **Members, onboarding, welcome screen, widget.**
9. **Video** - camera on the voice connection, receiving remote video,
   pop-out windows, per-user mute and hide. Needs a real account to verify.
10. **Creating and editing events, stage channels, discovery.**

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
