# Alternatives — Third-Party Discord Client Survey

Survey conducted 2026-08-14 for the RostFaden project, to justify selecting
[chojs23/concord](https://github.com/chojs23/concord) as the base for this GUI rewrite.

**114 repositories** were located and fully cloned (default branch, complete history) to
`../.references/`. Sources: the [Discord Client Encyclopedia](https://github.com/Discord-Client-Encyclopedia-Management/Discord3rdparties)
(including its full git history, to recover entries deleted from the README over time),
~50 GitHub repo-search queries, 5 GitHub topic sweeps, GitLab, Codeberg, SourceHut,
and general web search.

## Contents

- [Why concord](#why-concord)
- [Capability matrix — the serious contenders](#capability-matrix--the-serious-contenders)
- [Full inventory](#full-inventory)
- [Corrections to commonly-cited descriptions](#corrections-to-commonly-cited-descriptions)
- [Methodology](#methodology)

---

## Why concord

Selection criteria were: performant, actively maintained, voice calls (DM + guild),
screenshare, camera, and viable on aarch64 Linux (Raspberry Pi 4B).

**No client met all criteria.** Camera is the wall: a sweep of all 114 repos for
`v4l2`, `/dev/video`, `libcamera`, `getUserMedia`, `AVCaptureDevice` and `MediaCapture`
found that *every* camera hit belongs to a WebView-based client delegating to
Chromium/WebKit. **Zero native clients implement camera capture.**

concord was selected because it is the only native client with a real video pipeline
at all, and because its architecture is unusually amenable to a UI swap:

| Property | Finding |
|---|---|
| Core/UI coupling | **Zero** — `src/discord/` (85k lines) has no `ratatui`/`crossterm` references |
| Existing seam | `src/lib.rs` already exposes a library target; 131 `AppEvent` + 70 `AppCommand` variants |
| State portability | Only **8 of 71** files in `tui/state/` touch terminal crates, shallowly |
| Voice | DAVE (Discord's E2EE voice protocol), Opus, full RTP stack |
| Screenshare send | PipeWire + xdg-desktop-portal + dmabuf/EGL zero-copy, OpenH264 |
| Licence | GPL-3.0-only — forking permitted |
| Activity | 894 commits; active as of 2026-08-14 |
| aarch64 | Ships `aarch64-unknown-linux-gnu` release binaries |

**Verified limitations** (these are inherited by this rewrite):

- **No camera.** No capture path exists in the core. Net-new work, not UI work.
- **Screenshare *viewing* shells out to `mpv`** — it is not decoded in-process.
- **Hardware encode is Windows/macOS only** (Media Foundation / VideoToolbox).
  Linux falls back to software OpenH264 — the binding constraint on a Pi 4B.

---

## Capability matrix — the serious contenders

Capabilities verified by reading source, not by trusting README claims.
`send` = can broadcast; `view` = can display incoming.

| Client | Lang / UI | Voice | Screenshare | Camera | Active | ARM64 | Notes |
|---|---|---|---|---|---|---|---|
| **[concord](https://github.com/chojs23/concord)** | Rust / ratatui TUI | ✅ DM+guild | ✅ send · view via `mpv` | ❌ | ✅ 2026-08 | ✅ binaries | **Selected.** DAVE E2EE; dmabuf zero-copy |
| [Abaddon](https://github.com/uowuo/abaddon) | C++ / GTK3 | ✅ | ❌ | ❌ | ⚠️ 0 commits/90d | build | Most mature GUI; only client with moderation tooling (kick/ban, roles, audit log) |
| [Acheron](https://github.com/ouwou/acheron) | C++ / Qt6 | ✅ E2EE | ❌ | ❌ | ✅ 70 commits/90d | build | Abaddon's successor; multi-account, tabs, forums |
| [endcord](https://github.com/sparklost/endcord) | Python / ncurses | ✅ DM+guild | ❌ | ❌ | ✅ 2026-08 | source | Broadest *text* feature set of any client. **Source-available, not OSS — modified public forks prohibited** |
| [Dissent](https://github.com/diamondburned/dissent) | Go / GTK4 | ❌ | ❌ | ❌ | ✅ | build | Polished; explicitly rejects voice. Omits `X-Super-Properties` → ban-flag risk |
| [discordo](https://github.com/ayn2op/discordo) | Go / TUI | ❌ | ❌ | ❌ | ✅ 2026-08 | build | Most-starred TUI (5.7k); text only |
| [Discord Messenger](https://github.com/DiscordMessenger/dm) | C++ / Win32 | ❌ | ❌ | ❌ | ✅ | ❌ | Targets Windows NT 3.1+ |
| [Unicord](https://github.com/UnicordDev/Unicord) | C# / UWP | ✅ | ❌ | ❌ | ⚠️ 2026-01 | ❌ | Windows 10/Mobile |
| [Quarrel](https://github.com/WindowsAppCommunity/Quarrel) | C# / UWP | ✅ | ❌ | ❌ | ❌ 2022 | ❌ | Xbox support; 5090 commits |
| [Swiftcord](https://github.com/SwiftcordApp/Swiftcord) | Swift / SwiftUI | ✅ +video | — | — | ⚠️ | ❌ | macOS only; encyclopedia marks Broken |
| [Paicord](https://github.com/llsc12/Paicord) | Swift / SwiftUI | ✅ | ❌ | ❌ | ✅ 2026-08 | ❌ | macOS + iOS |
| [oxidecord](https://github.com/s1099/oxidecord) | Rust / **GPUI** | ❌ | ❌ | ❌ | ✅ 2026-08 | build | **Read before choosing a toolkit** — GPUI Discord client, 39 commits |
| [scope](https://github.com/scopeclient/scope) | Rust / GPUI | ❌ | ❌ | ❌ | ⚠️ 2025-06 | build | Also GPUI; "for power users" |
| [kind](https://github.com/txtsd/kind) | C++ / Qt | ❌ | ❌ | ❌ | ⚠️ 2026-04 | build | From-scratch Qt client |
| [Turbulence](https://github.com/TurbulenceDev/Turbulence) | C# | ❌ | ❌ | ❌ | ⚠️ 2024 | build | Largest of the newly-discovered clients |
| [LibreDiscord](https://gitlab.com/zipdox/librediscord) | C / GTK3 | ✅ | ❌ | ❌ | ⚠️ 2025-10 | build | **Audio only — no text chat by design.** README: video not implemented |

### If camera is required

No native client can do it. The only working options are Electron wrappers,
which ship aarch64 Linux builds and get camera + screenshare via Chromium:

| Client | Version | ARM64 | Notes |
|---|---|---|---|
| [Equibop](https://github.com/Equicord/equibop) | 3.2.2 (2026-07) | ✅ deb/rpm/AppImage | Best-maintained; bundles venmic for screenshare **with audio** |
| [Vesktop](https://github.com/Vencord/Vesktop) | 1.6.5 (2026-02) | ✅ | Same lineage; known aarch64 *source-build* issues — use prebuilt |
| [Legcord](https://github.com/legcord/legcord) | 1.3.0 (2026-07) | ✅ | Formerly ArmCord (DMCA'd by ARM) |
| [Dorion](https://github.com/SpikeHD/Dorion) | 6.13.0 (2026-07) | ✅ | Tauri/WebKitGTK, ~10 MB; WebKitGTK screenshare is unreliable |

> On a Pi 4B, video encode is CPU-bound regardless of client — the Pi's hardware
> H.264 encoder is not reachable from any of these. Expect low framerates.

---

## Full inventory

All 114 cloned repositories, by category. Commit counts at time of survey.

### Native desktop (34)

| Repo | Lang | Commits |
|---|---|---|
| [uowuo/abaddon](https://github.com/uowuo/abaddon) | C++/GTK3 | 1373 |
| [diamondburned/dissent](https://github.com/diamondburned/dissent) | Go/GTK4 | 638 |
| [DiscordMessenger/dm](https://github.com/DiscordMessenger/dm) | C++/Win32 | 528 |
| [gitlab: zipdox/librediscord](https://gitlab.com/zipdox/librediscord) | C/GTK3 | 233 |
| [gnome gitlab: ranchester/mirdorph](https://gitlab.gnome.org/ranchester/mirdorph) | Python/libadwaita | 493 |
| [not-nullptr/Aerochat](https://github.com/not-nullptr/Aerochat) | C# | 3758 |
| [UnicordDev/Unicord](https://github.com/UnicordDev/Unicord) | C#/UWP | 387 |
| [txtsd/kind](https://github.com/txtsd/kind) | C++/Qt | 381 |
| [mak448a/QTCord](https://github.com/mak448a/QTCord) | Python/Qt | 369 |
| [Naticord/naticord](https://github.com/Naticord/naticord) | C#/DirectUI | 342 |
| [ruslang02/discord-qt](https://github.com/ruslang02/discord-qt) | TS/NodeGui | 282 |
| [diamondburned/gtkcord3](https://github.com/diamondburned/gtkcord3) | Go/GTK3 | 252 |
| [Coding-Bunker/unofficial-discord-client](https://github.com/Coding-Bunker/unofficial-discord-client) | C++/Qt | 247 |
| [EnyoYoen/Fast-Discord](https://github.com/EnyoYoen/Fast-Discord) | C++/Qt | 233 |
| [TurbulenceDev/Turbulence](https://github.com/TurbulenceDev/Turbulence) | C# | 210 |
| [ouwou/acheron](https://github.com/ouwou/acheron) | C++/Qt6 | 208 |
| [maltejur/discord-screenaudio](https://github.com/maltejur/discord-screenaudio) | C++/QtWebEngine | 188 |
| [Gaztin/QDiscord](https://github.com/Gaztin/QDiscord) | C++/Qt | 153 |
| [scopeclient/scope](https://github.com/scopeclient/scope) | Rust/GPUI | 119 |
| [SrWither/DiscordBSD](https://github.com/SrWither/DiscordBSD) | JS | 105 |
| [michioxd/bbcord](https://github.com/michioxd/bbcord) | C++/QNX | 104 |
| [liz3/Raven](https://github.com/liz3/Raven) | Kotlin | 70 |
| [Tech-Tac/nebulon](https://github.com/Tech-Tac/nebulon) | Dart/Flutter | 68 |
| [gaouservbf/EveryDiscord](https://github.com/gaouservbf/EveryDiscord) | VB6/Win32 | 40 |
| [s1099/oxidecord](https://github.com/s1099/oxidecord) | Rust/GPUI | 39 |
| [hlafaille/Harmony](https://github.com/hlafaille/Harmony) | Java | 34 |
| [dosdude1/discord-lite](https://github.com/dosdude1/discord-lite) | Obj-C | 27 |
| [drtheodor/lothocord](https://github.com/drtheodor/lothocord) | GDScript/Godot | 27 |
| [ZenithRogue/DiscordFlex](https://github.com/ZenithRogue/DiscordFlex) | JS/Vue | 23 |
| [perrot1/cppcord](https://github.com/perrot1/cppcord) | C++ | 12 |
| [OJddJO/MiniCord](https://github.com/OJddJO/MiniCord) | C++ | 10 |
| [thehatkid/Minicord](https://github.com/thehatkid/Minicord) | Python | 9 |
| [SeungheonOh/DisCpp](https://github.com/SeungheonOh/DisCpp) | C++ | 4 |
| [volatide/concord](https://github.com/volatide/concord) | Python/Qt5 | 80 |

### TUI (21)

| Repo | Lang | Commits |
|---|---|---|
| [ayn2op/discordo](https://github.com/ayn2op/discordo) | Go | 1545 |
| [linuxmobile/oxicord](https://github.com/linuxmobile/oxicord) | Rust | 1492 |
| [Bios-Marcel/cordless](https://github.com/Bios-Marcel/cordless) | Go | 1128 |
| **[chojs23/concord](https://github.com/chojs23/concord)** | **Rust** | **894** |
| [sparklost/endcord](https://github.com/sparklost/endcord) | Python | 689 |
| [YetAnotherMechanicusEnjoyer/vimcord](https://github.com/YetAnotherMechanicusEnjoyer/vimcord) | Rust | 488 |
| [gitlab: diamondburned/6cord](https://gitlab.com/diamondburned/6cord) | Go | 398 |
| [mitchweaver/Discline](https://github.com/mitchweaver/Discline) | Python | 329 |
| [lexffe/discorder](https://github.com/lexffe/discorder) | Go | 288 |
| [freyamade/crcophony](https://github.com/freyamade/crcophony) | Crystal | 208 |
| [xynxynxyn/terminal-discord](https://github.com/xynxynxyn/terminal-discord) | JS | 170 |
| [ploglabs/molly-terminal](https://github.com/ploglabs/molly-terminal) | Go | 106 |
| [Traumatism/ToastCord](https://github.com/Traumatism/ToastCord) | Python | 99 |
| [rustbaeceans/discordterm](https://github.com/rustbaeceans/discordterm) | Rust | 94 |
| [mcxiv/10cord](https://github.com/mcxiv/10cord) | Python | 37 |
| [RX14/discord-curses](https://github.com/RX14/discord-curses) | TS | 31 |
| [brianwalczak/Echo-Client](https://github.com/brianwalczak/Echo-Client) | JS | 27 |
| [DvorakDwarf/disrust](https://github.com/DvorakDwarf/disrust) | Rust | 15 |
| [junhg0211/discore](https://github.com/junhg0211/discore) | JS | 15 |
| [Tamino1230/Distord](https://github.com/Tamino1230/Distord) | Python | 11 |
| [diamondburned/protocord](https://github.com/diamondburned/protocord) | Go | 5 |

### macOS / iOS (13)

| Repo | Lang | Commits |
|---|---|---|
| [llsc12/Paicord](https://github.com/llsc12/Paicord) | Swift | 499 |
| [evelyneee/accord](https://github.com/evelyneee/accord) | Swift | 426 |
| [cellomonster/iOS-Discord-Classic](https://github.com/cellomonster/iOS-Discord-Classic) | Obj-C | 113 |
| [ToruTheRedFox/iOS-Discord-Classic](https://github.com/ToruTheRedFox/iOS-Discord-Classic) | Obj-C | 202 |
| [slice/cyclone](https://github.com/slice/cyclone) | Swift | 202 |
| [voxa-org/Voxa](https://github.com/voxa-org/Voxa) | Swift/WebKit | 171 |
| [kotleni/DiscordApp-iOS](https://github.com/kotleni/DiscordApp-iOS) | Swift | 104 |
| [Stossycord/Stossycord](https://github.com/Stossycord/Stossycord) | Swift | 60 |
| [ChloeVPin/popcord](https://github.com/ChloeVPin/popcord) | Swift/WebKit | 25 |
| [SwiftcordApp/Swiftcord](https://github.com/SwiftcordApp/Swiftcord) | Swift | 9 |
| [pranavkarthik10/DiscordSwiftUI](https://github.com/pranavkarthik10/DiscordSwiftUI) | Swift | 7 |
| [andre4ik3/NativeCord](https://github.com/andre4ik3/NativeCord) | Swift | 2 |
| [neemaxon/SwiftyCord](https://github.com/neemaxon/SwiftyCord) | Swift | 1 |

### Other platforms (17)

| Repo | Platform | Commits |
|---|---|---|
| [roundedrectangle/SailDiscord](https://github.com/roundedrectangle/SailDiscord) | SailfishOS | 974 |
| [EionRobb/purple-discord](https://github.com/EionRobb/purple-discord) | libpurple/Pidgin | 920 |
| [mk-fg/reliable-discord-client-irc-daemon](https://github.com/mk-fg/reliable-discord-client-irc-daemon) | IRC bridge | 883 |
| [gtrxAC/discord-j2me](https://github.com/gtrxAC/discord-j2me) | Java ME | 1453 |
| [terminal-discord/weechat-discord](https://github.com/terminal-discord/weechat-discord) | WeeChat | 413 |
| [sm00th/bitlbee-discord](https://github.com/sm00th/bitlbee-discord) | BitlBee | 275 |
| [henry232323/Pesterchum-Discord](https://github.com/henry232323/Pesterchum-Discord) | Pesterchum | 162 |
| [gtrxAC/discord-wap](https://github.com/gtrxAC/discord-wap) | WAP 1.x | 57 |
| [soukouki/micro-discord](https://github.com/soukouki/micro-discord) | Ruby | 42 |
| [maem0/diskcord](https://github.com/maem0/diskcord) | BlackBerry 10 | 35 |
| [ruslang02/discord-symbian](https://github.com/ruslang02/discord-symbian) | Symbian^3 | 18 |
| [cyan-2048/Discord4KaiOS](https://github.com/cyan-2048/Discord4KaiOS) | KaiOS | 15 |
| [tbodt/crocodile](https://github.com/tbodt/crocodile) | TempleOS | 12 |
| [nakidai/fbdc](https://github.com/nakidai/fbdc) | Filesystem-based | 12 |
| [MCJack123/discordcc](https://github.com/MCJack123/discordcc) | ComputerCraft/Lua | 8 |
| [ruslang02/discord-aos](https://github.com/ruslang02/discord-aos) | SailfishOS/Qt | 3 |
| [vistafan12/discord-ppc](https://github.com/vistafan12/discord-ppc) | PowerPC | 3 |

### Consoles / embedded (8)

| Repo | Platform | Commits |
|---|---|---|
| [WindowsAppCommunity/Quarrel](https://github.com/WindowsAppCommunity/Quarrel) | Windows/Xbox | 5090 |
| [2b-zipper/TriCord](https://github.com/2b-zipper/TriCord) | Nintendo 3DS | 234 |
| [Grarak/NXCord](https://github.com/Grarak/NXCord) | Nintendo Switch | 92 |
| [vbe0201/switchcord](https://github.com/vbe0201/switchcord) | Nintendo Switch | 59 |
| [devingDev/VitaCord](https://github.com/devingDev/VitaCord) | PS Vita | 30 |
| [yourWaifu/Unofficial-Discord-3DS-Client](https://github.com/yourWaifu/Unofficial-Discord-3DS-Client) | 3DS | 20 |
| [XeathJP/A-client-for-Discord-for-3DS](https://github.com/XeathJP/A-client-for-Discord-for-3DS) | 3DS | 15 |
| [cheuble/3DiScord](https://github.com/cheuble/3DiScord) | 3DS | 11 |

### WebView / hybrid (7)

| Repo | Engine | Commits |
|---|---|---|
| [SpikeHD/Dorion](https://github.com/SpikeHD/Dorion) | Tauri/WebKitGTK | 1551 |
| [gamingdoom/datcord](https://github.com/gamingdoom/datcord) | Firefox | 562 |
| [japandotorg/LemonCord](https://github.com/japandotorg/LemonCord) | Rust/WebView | 114 |
| [RoboChimera/ChimeraCord](https://github.com/RoboChimera/ChimeraCord) | FreeBSD | 101 |
| [DiscordTauri/discord-tauri](https://github.com/DiscordTauri/discord-tauri) | Tauri | 76 |
| [iamtraction/WebCord](https://github.com/iamtraction/WebCord) | Go/WebView | 8 |
| [ulughann/rustCord](https://github.com/ulughann/rustCord) | Pake/Tauri | 3 |

### Protocol / server reimplementations (12)

Relevant as API references — several document undocumented Discord endpoints.

| Repo | Lang | Commits |
|---|---|---|
| [spacebarchat/server](https://github.com/spacebarchat/server) | TS | 5815 |
| [MathMan05/Fermi](https://github.com/MathMan05/Fermi) · [upstream](https://codeberg.org/MelodyChat/Fermi) | TS | 2566 |
| [codeberg: polyphony/chorus](https://codeberg.org/polyphony/chorus) | Rust | 1648 |
| [gitlab: litecord/litecord](https://gitlab.com/litecord/litecord) | Python | 1416 |
| [oldcordapp/OldCordV3](https://github.com/oldcordapp/OldCordV3) | JS | 1298 |
| [spacebarchat/client](https://github.com/spacebarchat/client) | TS | 883 |
| [V3L0C1T13S/reflectcord](https://github.com/V3L0C1T13S/reflectcord) | TS | 806 |
| [codeberg: polyphony/symfonia](https://codeberg.org/polyphony/symfonia) | Rust | 666 |
| [yepcord/server](https://github.com/yepcord/server) | Python | 507 |
| [FlickerTeam/Client](https://github.com/FlickerTeam/Client) | TS | 279 |
| [yepcord/client](https://github.com/yepcord/client) | TS | 58 |

### Android (2)

| Repo | Lang | Commits |
|---|---|---|
| [Aliucord/Aliucord](https://github.com/Aliucord/Aliucord) | Kotlin/Java | 662 |
| [MateriiApps/OpenCord](https://github.com/MateriiApps/OpenCord) | Kotlin | 404 |

---

## Corrections to commonly-cited descriptions

Verified against source; these differ from the encyclopedia and/or project marketing:

1. **LibreDiscord is not a "voice and video client."** Its README states video is not
   implemented, and it deliberately has *no text chat*. The `self_video` symbols parse
   other users' state and drive an unwired UI button.
2. **Abaddon and Acheron have no video path.** Their `self_video` references are gateway
   serialisation plus a channel-list camera *icon*. Voice only.
3. **concord does not decode video in-process.** Screenshare viewing spawns `mpv`.
   `ratatui-image` handles only static images, emoji and upload previews.
4. **Swiftcord's "full voice and video"** applies to macOS only; the encyclopedia marks
   the project Broken.
5. **Dissent intentionally omits `X-Super-Properties`** and does not spoof a real client
   — documented ban/spam-flag risk.
6. **endcord is not open source.** Source-available; modified public forks are prohibited
   by its licence. Readable as reference, **not** reusable as code.

## Methodology

Search surfaces covered: GitHub repo search (~50 queries), GitHub topic search
(`discord-client`, `discord-clients`, `discord-tui`, `third-party-discord-client`,
`discord-desktop`), GitLab.com API, Codeberg API (name + topic), SourceHut, general
web search, and the Discord Client Encyclopedia **including all 377 commits of its
history** — which recovered entries deleted from the current README.

Dead upstreams found in that history and *not* recoverable as source (404, archived
README links only): `topminipie/opencord`, `mariomario720/ios-discord-raspberry`,
`kunamech/voidcord`, `logoskosmos/voidcord`, `atlx/discord-term`,
`therealcyber71/discord-lite`.

Capability claims in the matrix were verified by grepping implementation symbols
(`v4l2`, `getDisplayMedia`, `libopus`, `openh264`, `pipewire`, `self_video`, …)
rather than by reading project descriptions — see the corrections above for why.
