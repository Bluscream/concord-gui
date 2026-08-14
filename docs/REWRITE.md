# concord-gui — rewrite plan

Replacing concord's terminal front-end with a native GPUI interface, while
consuming the existing core unmodified.

## Premise

The upstream crate is already layered for this. Measured, not assumed:

| Layer | Lines | Terminal coupling |
|---|---|---|
| `src/discord/` | 85,277 | **0** references to `ratatui`/`crossterm` |
| `src/app/` | 3,983 | **0** |
| `src/config/`, `src/support/` | 3,040 | 0 |
| `src/tui/` | 98,055 | the front-end being replaced |

`src/lib.rs` already exposes a library target (`App`, `DiscordClient`,
`AppEvent`, `paths`, `token_store`, `config`). The interface between core and
front-end is a command/event split:

- **131 `AppEvent` variants** flow up from the gateway (`src/discord/events.rs`)
- **70 `AppCommand` variants** flow down from the UI (`src/app/command_dispatch.rs`)

Nobody has to invent the abstraction boundary — it exists.

## Strategy

Consume `concord` as a path dependency rather than forking `src/` in place.
This keeps `upstream` merges tractable: the core continues to track
`chojs23/concord`, and this crate only ever *adds* `crates/gui/`.

```
concord-gui/
├── Cargo.toml          [workspace] + unmodified upstream package
├── src/                upstream core — do not edit without cause
│   ├── discord/        reused verbatim
│   ├── app/            reused verbatim
│   └── tui/            reference implementation; replaced, not deleted (yet)
└── crates/gui/         this rewrite
```

Keeping `src/tui/` in-tree during the rewrite is deliberate: it is the
executable specification for what each of the 70 commands is supposed to do.

## Phases

### Phase 0 — bootstrap ✅

- Fork, workspace, GUI crate skeleton.
- Prove the core links and compiles independently of the terminal front-end
  (`cargo check --no-default-features` — passes in 20.9s).
- Shell window rendering core status probed through the public API.
- Runtime verified on Wayland from the Arch distrobox: surface acquired
  (1 wayland fd), Vulkan device acquired via radv (2 DRI fds), 27 threads,
  80.9 MB RSS debug build, clean log.

### Phase 1 — decouple the portable state

`tui/state/` is 41,203 lines across 71 files, of which **only 8 touch terminal
crates**, shallowly. Genericise those and the state layer becomes reusable:

| File | Lines | Coupling | Substitution |
|---|---|---|---|
| `state.rs` | 2460 | `ratatui_image::protocol` | generic texture handle |
| `forum_post.rs` | 993 | `ratatui_image::protocol` | ″ |
| `options_voice.rs` | 948 | `crossterm::event` | abstract key type |
| `model.rs` | 803 | `ratatui_image::protocol` | ″ |
| `mod.rs` | 433 | `ratatui::text` | plain string/span type |
| `user.rs` | 330 | `ratatui` | ″ |
| `presentation.rs` | 183 | `ratatui::style` | own style enum |
| `local_upload_preview.rs` | 63 | `ratatui_image::protocol` | generic texture handle |

Five of the eight are a single concern: a cached image-protocol handle that
becomes a backend-agnostic texture handle. This is type substitution, not
restructuring.

### Phase 2 — event loop ✅

Bridged in `crates/gui/src/session.rs`: a dedicated tokio thread forwards
snapshot revisions and events down one channel; the workspace drains it on
GPUI's foreground executor. Reprojection is driven by the snapshot watch
rather than a parallel reducer, so GUI state cannot drift from the core's.

### Phase 2 (original note) — event loop

Bridge `AppEvent` → GPUI. The core is async (tokio); GPUI has its own executor.
Bridge over a channel, dispatching to entity updates on the foreground executor.

### Phase 3 — views (in progress)

| Surface | Status |
|---|---|
| Login (token) | ✅ done |
| Login (QR, password) | ✗ `qr_auth` exists upstream; password needs captcha/MFA UI |
| Guild rail + channel sidebar | ✅ done, click-to-navigate |
| Message list | ✅ grouped blocks, replies, attachments, reactions |
| Composer + send | ✅ minimal editor; no selection/clipboard/multi-line |
| Member list | ✅ groups, presence, role colours |
| Unread / mentions | ✅ from core notification state |
| Typing indicators | ✅ |
| Markdown / mentions rendering | ✗ content shown verbatim |
| Custom emoji + avatar images | ✗ no image loading yet |
| Message actions (edit, delete, react, reply) | ✗ commands exist, no UI |
| Search | ✗ `SearchMessages` exists |
| Threads / forums | ✗ filtered out of the sidebar |
| User profiles | ✗ |
| Voice controls | ✗ |
| Settings / keybindings | ✗ |
| Notifications (desktop) | ✗ |

### Phase 3 (original note) — views

`src/tui/ui/` (21,212 lines) and `src/tui/input/` (8,817) are the surfaces being
replaced; `media/`, `keybindings/`, `message/` (~15k) are ported selectively.

### Phase 4 — media

Voice inherits for free via `AppCommand::JoinVoiceChannel` etc. Two known gaps:

- **Screenshare viewing** shells out to `mpv` upstream. A GUI should decode
  in-process and render to a texture.
- **Camera does not exist** anywhere in the core. Net-new work in
  `src/discord/voice/capture/`, not UI work.

## Constraints

- **Licence: GPL-3.0-only.** Inherited from upstream and non-negotiable while
  this links against the core. Any distributed build must ship under GPL-3.0.
- **Hardware encode is Windows/macOS only** upstream (Media Foundation /
  VideoToolbox). Linux falls back to software OpenH264 — the binding constraint
  on low-power ARM targets such as a Raspberry Pi 4B.
- **No camera path** in the core, as above.

## Prior art

Two GPUI Discord clients exist and are worth reading before hardening the
toolkit choice — both are cloned under `../.references/native-desktop/`:

- [`oxidecord`](https://github.com/s1099/oxidecord) — `gpui` 0.2.2 +
  `gpui-component` 0.5.1 from crates.io. Same approach taken here.
- [`scope`](https://github.com/scopeclient/scope) — pins a Zed git branch
  (`feature/export-platform-window`). More capable, considerably more fragile.

Neither implements voice, screenshare or camera; concord's core is the reason
to start here rather than from either of them.

## Build

Bazzite (immutable) lacks `cmake`, which `opusic-sys` requires. Build in the
Arch distrobox:

```bash
distrobox enter arch -- cargo check -p concord-gui
```
