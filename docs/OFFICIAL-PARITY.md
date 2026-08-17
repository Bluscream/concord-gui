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

Bulk-ban is on the bans panel in both clients - `B` in the TUI, a button in the
GUI - as a typed list of ids rather than a member picker. That is what raid
cleanup looks like in practice: the ids come pasted from somewhere else. The
parser takes any separator for that reason, and the panel shows how many it
read, so a mistyped delimiter shows as a number that does not match the paste.

It goes through the risk prompt, and no message deletion is applied by default:
what to delete is a separate decision from who to ban, and the destructive
default would be the wrong one.

Discord bans who it can and reports the rest, so a partial success is normal
rather than an error - and an absent failure list means everyone succeeded, not
nobody, which is the most alarming way to be wrong about a mass ban.

Members are a tab on the server panel in both clients, read from the snapshot
rather than fetched - the gateway keeps the list changing, and a cached copy
would show members who have since left. Searching matches display name,
username and nickname, because people search for whichever of the three they
know.

A member whose roles Discord never sent says so rather than "no roles": an
omitted list is not an empty one, and the row would otherwise be describing the
fetch rather than the member.

The search query goes in the panel title rather than as a row in the list. A
row for it would shift every index below, and the selection is what decides
which member enter acts on.

Onboarding is a tab on the server panel in both clients, and it answers the
questions rather than only listing them. That is the point: until now the
client detected onboarding as a participation restriction and told people to go
and finish it in the official Discord app, which is the one thing this client
exists not to say. That message now points at the tab.

Each answer says what it grants - roles, channels, or neither. Onboarding is
the one place a client hands out roles on your behalf, and doing that silently
would be the wrong shape.

The picking rules are in the core because Discord's are not obvious: a
single-select question replaces its answer rather than adding to it, and only
its own answers are cleared - a filter that was not scoped to the question
would silently blank the rest of the form. A question row is not selectable,
since an index that did not distinguish the two would let enter "pick" a
question title and look like it worked.

An incomplete form names the questions still missing. Discord rejects the
submission with a message that does not.

Still missing, new-member actions, and membership screening - which the
client detects as a participation restriction but can neither configure nor
complete.

The welcome description and the widget's invite channel are edited by text in
both clients - `e` in the TUI, the row's action in the GUI. The channel is
given by name, and an ambiguous one is refused rather than resolved to whichever
came first: Discord allows two channels with the same name, and guessing would
aim the invite somewhere nobody chose while looking exactly like success. An
empty description clears it; an empty channel means the widget issues no invite,
which is a real state rather than a missing value.

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

Creating an event is one typed line - `name | start | end | where` - in both
clients, parsed by one function in the core so the accepted format cannot drift
between them. One line rather than a five-step wizard: neither client has a
date picker, and five prompts in sequence would be worse than one line with a
stated format.

The validation is in the core too, because Discord's rules here are not
guessable from its error messages: an event somewhere else needs an end time,
while one in a channel must not have one - Discord ends that when the channel
empties. The two also send different entity types, and transposing them fails
with a message about a field the form never showed.

Stage channels are joinable in both clients, which they were not: `is_voice`
excluded them, so every permission check and every join path answered no and a
stage was a channel you could see and not enter. The GUI has its own
`ChannelKind`, where comparing against `Voice` alone would have reintroduced
exactly that - so the check is `joins_voice`, with a test.

Starting, retitling and ending a stage is one field in both clients. Which of
Discord's three endpoints it uses is decided in the core: start rejects a
running stage, patch rejects one that is not, and an emptied topic means end -
and the three are indistinguishable from the form alone.

Asking to speak and inviting someone up are in both clients too, on the channel
and member menus.

Editing an event is the same typed line, seeded with the event as it stands -
so what is shown is exactly what will be sent back, and a change is a
correction rather than a retype. A round-trip test keeps those two in step: the
line an event produces must parse into the event it came from, or editing one
field would silently rewrite another.

A channel event seeds no place and no end time, because Discord ignores both
there - showing them would invite typing something that goes nowhere.

Discovery is in the join-server prompt in both clients: type a link and it
resolves, type anything else and it searches Discord's public list. People have
one or the other, and asking which before they type is a question with no good
answer. It is the only way into a server that does not need someone to hand you
a link first.

Joining goes through the ordinary invite path using the server's vanity code,
rather than a discovery endpoint of its own - that path is already written and
tested, and a second one guessed at would ship something that silently fails. A
server with no vanity code cannot be joined from here, and its row says so
rather than showing a dead button.

Member counts are described as "about", because Discord's own field says
approximate and a bare number reads as exact.

Still missing: the owner-facing discovery metadata - which category a server
lists itself under, and its keywords.

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
Every numbered item on this list is now done, in both clients, except the one
below. A wider comparison against every documented API route - not just this
plan - is in `docs/FEATURE-BACKLOG.md`, which found 391 unused routes and about
seventy features worth building. The command sweep in `docs/PARITY.md` reports nothing unreachable.

1. **Receiving video** - decoding someone else's camera or screen and drawing
   it, plus pop-out windows for a feed. Sending is done: cameras are
   enumerated over v4l2, share the encode and RTP path with screen capture,
   and are now announced as `type: "video"` rather than `"screen"` - Discord's
   clients decide layout and quality from that field. The receive side reads
   the stream and its counters but decodes no frames.

   None of the sending path has run against a real account.
Discovery metadata is a tab on the server panel in both clients: primary
category, search keywords, emoji discoverability and the long description.
Every cap is checked before the request, because Discord enforces all four and
its rejection does not name the field that was wrong.

The endpoint replaces rather than merges, so every field is sent - an omitted
one is reset to its default rather than left alone, which Discord's own note
says outright. Without a primary category the server is not listed at all, and
the row says so rather than showing a blank.

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

Nothing. Every command the core defines is reachable from both clients.

Roles and channels move by nudging rather than dragging: `K`/`J` on the roles
tab, `<leader> m k` / `<leader> m j` on the channel list, and buttons in the
GUI. Only the two rows that changed are sent, since sending the whole list
writes an audit entry for every row that did not move.
