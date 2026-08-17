# Feature backlog

Everything the official clients do that this one does not yet, found by
comparing every documented API route against every route this client calls.

Regenerate the comparison with:

```bash
python3 scripts/route-gaps.py
```

Sources are `../.references/`: the community API documentation in
`official-internals/userdoccers__github`, and the open-source clients beside it.
Not the decompiled official apps - a decompiled binary is a derivative work, and
everything below was derivable without it.

The route sweep finds what the API offers. It cannot find a feature that is
purely client-side (pop-out windows, keyboard shortcuts, themes), so those are
listed separately from what the sweep produced.

## Out of scope

Unchanged from `OFFICIAL-PARITY.md`, and filtered out of the sweep: Nitro and
its shop, boosts, quests, the activity launcher, collectibles, virtual currency,
gifting, billing, payments, subscriptions, entitlements, promotions, referrals,
the store and storefront, application-directory listings, guild analytics,
top-games, creator monetisation, role subscriptions, and the partner/social SDKs.

Also out: the developer-portal surface (creating and managing applications,
bots, assets, widget-configs, game claims). This is a client, not a dev portal.

## Desktop only

Things the desktop client has that mobile does not.

- **Show a login QR code.** Done - `src/discord/qr_auth.rs`.
- **Rich presence over RPC.** Partly done; the RPC server exists.
- **Screen and window capture, and going live.** Done.
- **Global push-to-talk.** Done, X11 and Wayland.
- **Keybinds and the quick switcher.** Done.

## Mobile only

Things the mobile client has that desktop does not. All are missing here.

1. **Scan a login QR code** - `POST /users/@me/remote-auth`, `/finish`,
   `/cancel`. The mirror of what this client already does: it *shows* a code for
   another device to scan. Scanning one means becoming the authorising device,
   which needs a camera and a confirmation step naming the device being let in.
2. ~~**Phone number as a credential**~~ - in the core, no front end. Three
   steps, not one: Discord sends a code, the code is exchanged for a token, and
   only then does the number attach, with the account password as well. A
   client modelling it as a single call would appear to work and leave the
   number unattached.
3. ~~**SMS two-factor**~~ - in the core, no front end. Needs an attached phone,
   which is why it lives beside it.
4. **Push notification registration** - `POST /users/@me/devices`,
   `PUT /users/@me/devices/sync`. Only meaningful with a push service, so this
   is the one item here that may never apply to a desktop build.
5. **Contact sync** - `PUT /users/@me/connections/contacts/{}`. The setting to
   turn it off is done; the sync itself is mobile's.

## Both clients, missing here

Ordered by how often a daily user would meet it.

### Messages and channels

1. **Pinned messages as a list** - `GET /channels/{}/pins`, `POST /pins/ack`.
   The client can pin, but not browse pins as their own view.
2. **Bulk delete** - `POST /channels/{}/messages/bulk-delete`. Moderation's
   most-used tool after ban.
3. **Clear all reactions on a message** - `DELETE /messages/{}/reactions`.
4. **Create a thread from a message** - `POST /messages/{}/threads`. Threads are
   read and joined; starting one from a message is not wired.
5. **Thread membership** - `GET /channels/{}/thread-members`, `PUT`/`DELETE` one.
   Who is in a thread, and adding someone.
6. **Archived thread browsing** - `GET /channels/{}/threads/archived/public`,
   `/private`, and the current user's own archived private threads.
7. **Forum tags** - `POST /channels/{}/tags`, `PUT`/`DELETE /tags/{}`. Forums are
   read and posted to; the tags that organise them cannot be managed.
8. **Follow an announcement channel** - `POST /channels/{}/followers`, and
   `POST /messages/{}/crosspost` to publish.
9. **Group DM membership** - `PUT`/`DELETE /channels/{}/recipients/{}`, and
   `POST /users/@me/channels/bulk-leave`.
10. **Ring a DM call** - `POST /channels/{}/call/ring`, `/stop-ringing`. Calls
    connect, but nobody's phone rings.
11. **Voice channel effects** - `POST /channels/{}/voice-channel-effects`. The
    reaction animations in a voice call.
12. **Expire a poll early** - `POST /channels/{}/polls/{}/expire`, and
    `GET /polls/{}/answers/{}` for who voted what.
13. **Message summaries** - `GET /channels/{}/summaries`.
14. **Saved messages** - `GET /users/@me/saved-messages`, `PUT`/`DELETE` one.
15. **Search tabs** - `POST /channels|guilds|users/.../messages/search/tabs`.
    Search exists; the saved-search tabs beside it do not.

### Server administration

16. ~~**Stickers**~~ - done, in both clients. Upload is multipart rather than a
    data URI, unlike emoji: Discord accepts Lottie JSON here, which is not an
    image and has no sensible data-URI content type.
17. **Webhooks** - `GET`/`POST /channels/{}/webhooks`, `GET /guilds/{}/webhooks`,
    and edit and delete. Entirely absent.
18. **Integrations** - `GET /guilds/{}/integrations`, delete and sync. Which bots
    and apps a server has, and removing one.
19. **Vanity URL** - `GET`/`PATCH /guilds/{}/vanity-url`. Also what discovery
    joining depends on; see `OFFICIAL-PARITY.md`.
20. **Role assignment by member** - `PUT`/`DELETE /guilds/{}/members/{}/roles/{}`.
    Roles are listed and edited; giving one to somebody is not wired.
21. **Role member counts and lists** - `GET /guilds/{}/roles/member-counts`,
    `/roles/{}/member-ids`.
22. **Ban search** - `GET /guilds/{}/bans/search`. The ban list is unpaged and
    unsearchable, which is unusable past a few hundred.
23. **Member search server-side** - `GET /guilds/{}/members/search`,
    `POST /members-search`. The Members tab filters what the gateway happened to
    send; this asks Discord.
24. **Membership screening** - `GET`/`PATCH /guilds/{}/member-verification`. The
    rules a member agrees to before participating. Detected as a restriction
    today, exactly as onboarding was before it was built.
25. **Join requests** - `GET`/`PATCH /guilds/{}/requests`, and the applicant's
    own `/requests/@me`. Both sides of asking to join a gated server.
26. **New member actions and welcome** - `GET /guilds/{}/new-member-actions`,
    `GET`/`PUT /guilds/{}/new-member-welcome`. The server guide.
27. **Guild profile** - `GET`/`PATCH /guilds/{}/profile`. The public-facing
    description, badge and traits.
28. **Incident actions** - `PUT /guilds/{}/incident-actions`. Pausing invites and
    DMs during a raid.
29. **AutoMod raid tools** - `POST /auto-moderation/report-raid`,
    `/clear-mention-raid`, `/false-alarm`, `/alert-action`.
30. **Unusual DM activity** - `GET /guilds/{}/members/unusual-dm-activity`.
31. **Guild MFA requirement** - `POST /guilds/{}/mfa`.
32. **Discovery requirements** - `GET /guilds/{}/discovery-requirements`. Why a
    server is or is not eligible; the metadata editor is done without it.
33. **Guild preview** - `GET /guilds/{}/preview`. What a server looks like before
    joining.
34. **Active threads server-wide** - `GET /guilds/{}/threads/active`.
35. **Event attendees** - `GET /guilds/{}/scheduled-events/{}/users`.
36. **Template preview by code** - `GET /guilds/templates/{}`, and creating a
    server from one - `POST /guilds`. Creating a server at all is missing.

### Account and social

37. **Create a server** - `POST /guilds`. Notable by its absence: this client can
    join, leave and administer a server but not make one.
38. **Account deletion and disabling** - `POST /users/@me/delete`, `/disable`.
    The last of the account settings.
39. **Email change and verification** - `PUT /users/@me/email`,
    `POST /users/@me/email/verify-code`, `GET`/`PATCH /email-settings`.
40. **WebAuthn as a second factor** - `GET`/`POST /users/@me/mfa/webauthn/credentials`,
    and edit and delete. Hardware keys, beside TOTP.
41. **Backup code challenge** - `POST /auth/verify/view-backup-codes-challenge`.
42. **Data harvest** - `GET`/`POST /users/@me/harvest`. Requesting your data.
43. **Notification centre** - `GET /users/@me/notification-center/items`, ack and
    delete. Distinct from the inbox this client has.
44. **Notification setting snapshots** - `GET`/`POST /notification-settings/snapshots`.
45. **Ignore someone** - `PUT`/`DELETE /users/@me/relationships/{}/ignore`. Block
    is done; ignore is the softer one and is missing.
46. **Bulk relationship changes** - `POST /users/@me/relationships/bulk`.
47. **Friend suggestions** - `GET /friend-suggestions`, and dismissing one.
48. **Personal invites** - `GET`/`POST`/`DELETE /users/@me/invites`.
49. **User notes in bulk** - `GET /users/@me/notes`. Notes are read one at a
    time today.
50. **Per-user audio settings, server-side** - `PATCH /users/@me/audio-settings/{}/{}`.
    Volume and mute are local here; Discord syncs them across devices.
51. **Avatar decorations and profile avatars** - `GET /users/@me/avatars`,
    `DELETE` one. The non-Nitro part: which stored avatar is in use.
52. **Family centre** - `GET /family-center/@me`, `/link-code`. Parental
    supervision.
53. **Safety hub** - `GET /safety-hub/@me`, and the suspension appeal routes.
    What Discord has actioned on your account, and disputing it.
54. **Reporting** - `POST /reports/channels/{}/messages/{}`, `/reports`,
    `GET /reporting/menu/{}`. Reporting a message, user or server. Entirely
    absent, and the one gap that is a safety feature rather than a convenience.

### Media and presence

55. **GIF picker** - `GET /gifs/trending`, `/search`, `/suggest`, `POST /select`.
    Discord's own, distinct from the emoji picker that is done.
56. **Sticker packs** - `GET /sticker-packs`, `/sticker-packs/{}`. The default
    packs; sending a sticker works, browsing the packs does not.
57. **Link unfurling** - `POST /unfurler/unfurl`, `/embed-urls`. Embed previews
    for links the gateway does not pre-embed.
58. **Stream preview images** - `POST /streams/{}/preview/video`,
    `PATCH /streams/{}/stream`, `POST /notify`.
59. **Video background filters** - `GET`/`POST /users/@me/video-filters/assets`.
60. **Voice regions** - `GET /voice/regions`, `GET /guilds/{}/regions`. Choosing
    where a call is hosted.
61. **Refresh expiring attachment URLs** - `POST /attachments/refresh-urls`.
    Discord's CDN links expire; without this, old images stop loading.
62. **Explicit media controls** - `PATCH /channels/{}/explicit-media`,
    `PATCH /messages/explicit-media`.
63. **Presence of arbitrary users** - `GET`/`POST /presences`.

### Client-side, not visible to the route sweep

64. **Pop-out windows for a camera or stream.** Named in `OFFICIAL-PARITY.md`
    and still open; needs the receive side first.
65. **Receiving video at all.** The remaining half of the video work.
66. **Per-server profiles** - a different nickname, avatar and banner per server.
    Partly reachable through the member-profile routes already used.
67. **Message forwarding.** Reply and quote exist; forwarding does not.
68. **Drafts that survive a restart.**
69. **Folder and guild reordering by drag.** Roles and channels now nudge with
    keys; guilds and folders do not move at all.
70. **Accessibility: reduced motion, larger text, screen-reader labels.** The
    setting for screen-reader *detection* is done; the accommodations are not.

## Offline-first storage

A cache that survives a restart, so the client draws something immediately
rather than waiting out a READY payload - and nothing at all with no network.
Two backends: a file beside the client's other state, or a MariaDB or MySQL
server several clients on a network can share.

Landed:

- `storage::dsn` - the connection string. `mariadb://192.168.2.10:3333/discord`,
  `sqlite:///path`, or a bare path. A password may contain an `@`.
- `storage::schema` - flat tables for users, guilds, channels and messages, in
  both dialects. Flat rather than blobs: a shared store is one another client
  reads, and that only works if the columns mean something without this
  codebase.
- `storage::concurrent` - what happens when two clients write. See below.
- `[storage]` in the config, on by default with an empty DSN meaning the local
  file.

Still to do, in order:

1. ~~**A driver.**~~ Done. `sqlx` with its `any` driver, so the backend stays a
   runtime choice: a shared store is something somebody points an already-built
   client at. Behind a `storage` feature, on by default - a build that never
   caches should not pay for a database driver.
2. **Write on gateway events.** The store reads and writes; what is missing is
   calling it as `GUILD_CREATE`, `MESSAGE_CREATE` and the rest arrive. Needs a
   revision for each: Discord's `version` where there is one, and for messages
   the id, which is monotonic and never changes.
3. **Read at startup, before the gateway.** The point of the whole thing.
   `DiscordState` fills from the store, then the gateway corrects it.
4. **Deletion.** Tombstones are in the schema and nothing writes them.
5. **Eviction.** The in-memory caches evict by LRU; a disk store needs a size
   or age bound or it grows forever.
6. **Migrations.** `SCHEMA_VERSION` exists and nothing reads it. A shared store
   may be in use by a client that knows a different version, so a newer schema
   must be left alone rather than migrated backwards.

## What to do first

Stickers (16) are done. Next by the same measure: webhooks (17) and
pinned-message browsing (1). Reporting (54) is the one to do on principle
rather than frequency - it is a safety feature, and its absence is the kind a
client should not ship with.

The mobile-only group (1-5) belongs in the core even though neither front end
here will surface all of it: a future mobile GUI would need it, and a core that
only serves the clients in this repository is a core that has to be reopened
later. `scripts/unreachable-commands.py` reports them under `core only` with the front
end that would use each, so an entry that stops being true is a wrong sentence
somebody can read rather than a silent exemption.
