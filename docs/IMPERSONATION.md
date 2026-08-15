# Looking like the web client

Discord's anti-spam heuristics treat third-party clients more harshly than the
official one, and a false positive costs the user their account. Abaddon's
README sets the standard worth matching: identify as the web client, use API
v9 throughout, and avoid endpoints the web client would not itself send.

This records what has been checked, and - more importantly - what has not.

## Checked, and correct

**API version.** Every REST call is `api/v9`. One exception existed:
`remote-auth/login` was on v10. Abaddon, acheron and endcord all use v9 there;
only TriCord uses v10. A lone version bump is exactly the sort of thing that
separates a real web client from an imitation, so it now matches.

**Identity headers.** `X-Super-Properties` and `X-Discord-Locale` are sent,
which is what Abaddon does. The super-properties payload carries the full set
the web client sends - os, browser, versions, locale, release channel, launch
id, heartbeat session, client app state.

**Build number is real.** `client_build_number` is fetched from Discord's own
`/app` bundle rather than hardcoded. Abaddon hardcodes it, which goes stale;
this does not.

**Browser headers.** `Sec-Fetch-Dest`, `Sec-Fetch-Mode`, `Sec-Fetch-Site`,
`Sec-CH-UA-Mobile`, `Origin`, `Referer`, `Pragma`, `Cache-Control`, `Priority`
are all present, matching what a browser sends.

**Endpoints.** Every route this client calls is one the web client uses.
Notably `set_member_roles` sends the whole role set through
`PATCH /guilds/{id}/members/{id}` rather than the per-role endpoints, because
the web client does not use those - a point Abaddon's source makes explicitly.

**No telemetry.** `/science` and `/track` are never called. The web client
sends both; not sending them is a deviation, but in the direction of not
reporting the user's behaviour to Discord, which is the trade this project
exists to make.

## Not checked, and not checkable from here

Everything above is **static inspection of the source**. None of it has been
observed on the wire, and several things that matter cannot be:

- **Header order and casing.** Browsers send headers in a characteristic
  order. reqwest may not reproduce it, and no amount of reading the source
  will say.
- **TLS fingerprint (JA3/JA4).** rustls negotiates differently from Chrome.
  This is probably the single largest tell, and it is not fixable by setting
  headers.
- **HTTP/2 settings and frame ordering**, which fingerprinting also uses.
- **Request timing and ordering at startup.** The web client fetches things in
  a particular sequence; this client does not necessarily match it.

So: the parts a client can control are right, and the parts below the HTTP
layer are unverified and likely distinguishable. Nobody should read this
document as "we are indistinguishable from a browser".

## What to do with this

Rule 6 in AGENTS.md still applies, and is the practical protection: warn
before the actions that most often trigger the filter - joining and leaving
servers above all - rather than relying on impersonation being perfect.

Re-run the route list when adding REST calls:

```bash
grep -rhoE '"https://discord\.com/api/v9/[^"]*"' src/discord/rest*.rs src/discord/rest/*.rs \
  | sed 's|.*api/v9/||;s/"//' | sort -u
```

Anything on that list the official web client would not send is a problem.
