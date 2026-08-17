# Values Discord serves

Things the official clients fetch rather than compile in, and where each comes
from. Anything on this list that we hardcode is a value that can change under us
without a release.

The mechanism for holding these is `src/discord/remote_config.rs`: cached to
disk, refetched when stale, with a compiled-in default that is the current value
rather than a placeholder. `docs/PARITY.md` describes the layering.

Sourced from the community API documentation in `../.references/`.

## Status

| Value | Where from | Us |
| --- | --- | --- |
| Gateway URL | `GET /gateway` | fetched and cached |
| Discovery categories | `GET /discovery/categories` | fetched per open |
| Experiment rollouts | `GET /apex/experiments` | fetched, for the fingerprint only |
| Voice regions | `GET /voice/regions` | **hardcoded away** - we never choose one |
| Geo-ordered RTC regions | READY, `geo_ordered_rtc_regions` | **ignored** |
| Detected country | READY, `country_code` | **ignored** |
| API code version | READY, `api_code_version` | **ignored** |
| Preloaded user settings | READY, `user_settings_proto` | partly read |
| Session start limits | `GET /gateway/bot` | not applicable - user accounts have none |
| Report reason tree | `GET /reporting/menu/{type}` | **not implemented** |
| Report options | `GET /report/options` | **not implemented** |
| Discovery search term validity | `GET /discovery/valid-term` | **not implemented** |
| Guild discovery eligibility | `GET /guilds/{id}/discovery-requirements` | **not implemented** |
| Default sticker packs | `GET /sticker-packs` | **not implemented** |
| Detectable games | `GET /applications/detectable` | **not implemented** - needed for rich presence |
| Tutorial indicators | `GET /tutorial` | out of scope |
| OAuth application allowlist | `GET /oauth2/applications/{id}/allowlist` | developer portal, out of scope |

## The ones that matter

### Voice regions

`GET /voice/regions` lists them; READY carries `geo_ordered_rtc_regions`,
already sorted by distance from the user. We set no region at all, so Discord
picks. That is usually right and occasionally badly wrong - a call routed across
an ocean because the first person to join was there.

Worth having because the ordered list is free: it arrives in READY whether we
read it or not.

### The report reason tree

`GET /reporting/menu/{type}` returns the questions and reasons for reporting a
message, user or server. It is a tree Discord changes, which is exactly why it
is served rather than shipped.

This one is load-bearing for the reporting feature in `FEATURE-BACKLOG.md`: a
client that hardcoded the reasons would offer categories Discord has retired and
miss ones it has added, and a report filed under a stale reason is a report that
goes nowhere.

### Detectable games

`GET /applications/detectable` is the list of executables Discord recognises for
"playing X" presence. Thousands of entries, changes weekly, and there is no way
to derive it. Any rich-presence detection needs it fetched and cached - which is
the case this whole caching layer was built for.

### Upload limits

Not served as config. Discord documents them as a function of premium type and
guild boost tier, and `capabilities.rs` already derives them that way, which is
more accurate than a single served number. `remote_config` has a field for it
that nothing reads; either something should serve it or the field should go.

## What is documentation, not configuration

These are caps Discord publishes and does not serve. They stay compiled in, and
pretending to fetch them would suggest they can change under us:

- Name lengths: guild 2-100, role 100, channel 100, sticker 2-30, username 2-32
- Sticker file size, 500 KiB
- Discovery keywords, 10 of 30 characters; description 2400; subcategories 5
- Welcome screen: 140-character description, 5 channels
- Bulk ban, 200 users per request
- Prune windows: 1, 7, 14, 30, 90 days
- Stage topic, 1-120 characters
- Scheduled event name 100, description 1000

If one of these turns out to be served somewhere, it moves to the table above.
