# Taking changes from upstream

`concord-gui` is a fork of [chojs23/concord](https://github.com/chojs23/concord),
which is still actively developed. This is how its changes get here.

Everything below is driven by `scripts/upstream.sh`. Run it with no arguments
for the command list.

## The shape of the problem

Measured on 23 September 2026, against a fork point of 14 August:

| | |
|---|---|
| Upstream commits we did not have | 87 |
| Our commits upstream does not have | 266 |
| Files both sides had touched | 62 |
| **Conflicted files in a trial merge** | **111** |

None of that is upstream being difficult. It is four structural facts about
this fork, and they will produce the same conflicts every time:

1. **We moved the tree upstream keeps editing.** 48 of those 87 commits land
   in `src/tui/`, which does not exist here any more:

   | Upstream | Here |
   |---|---|
   | `src/tui/**` | `crates/tui/src/tui/**` |
   | `src/tui/theme.rs` | `crates/ui/src/theme.rs` |
   | `src/tui/keybindings/**` | `crates/ui/src/keybindings/**` |
   | `src/translation/**` | *(absent - never taken)* |

2. **We split files upstream still edits as one.** `src/discord/events.rs`,
   `gateway.rs`, `voice/broadcast.rs`, `voice/microphone.rs`, `voice/stream.rs`
   and others are directories here. Upstream edits the file; git sees a
   delete/modify conflict, which is the conflict shape with the least
   information in it.

3. **Every relocated file needs the same edit.** Upstream is one crate, so it
   says `crate::discord`. Here the core is a dependency, so it must read
   `concord::discord`. That is 370 occurrences in `src/tui` alone. An incoming
   hunk arrives with the wrong prefix *even when it merges cleanly*, so it
   does not conflict - it just fails to compile, with nothing pointing at why.

4. **`Cargo.lock` is generated and merges like text.** 58 conflict hunks in
   one file, none of them meaningful.

## The routine

**Merge one upstream release at a time, and do it when it ships.** This is
worth more than every piece of tooling below. 87 commits gave 111 conflicted
files; the conflicts do not add up linearly, they multiply, because each side
keeps editing around the other's changes. A single release is a morning.

```bash
./scripts/upstream.sh status      # what has upstream done, and where does it land
./scripts/upstream.sh sync-main   # fast-forward the mirror (safe, no judgement)
./scripts/upstream.sh preview     # trial-merge in a throwaway worktree
./scripts/upstream.sh merge       # the real thing, on merge-upstream-<date>
#   ... resolve what is left ...
./scripts/upstream.sh finish      # re-run the mechanical passes
./scripts/upstream.sh gate        # fmt, clippy, tests, both feature sets
git commit
```

`preview` changes nothing and can be run at any time, on a dirty tree. Use it
before promising anyone a timeline.

`merge` turns on `git rerere`, which records how you resolved a conflict and
replays it when the same one comes round again. Since every recurring conflict
here is structural - the same relocated file, the same import line - this pays
from the second merge onwards. Do not turn it off.

## What is automated, and what is not

**Done for you.** The mirror fast-forward. The `Cargo.lock` regeneration.
The `crate:: -> concord::` rewrite on everything under `crates/`. Rename
detection tuned to 25%, which on the trial merge turned seven delete/modify
conflicts back into ordinary content conflicts without misattributing any.

**Not done for you, and deliberately.** `crate::app` is left alone: it is
valid on both sides and means different things, because `crates/tui` has its
own `mod app` beside the core's `concord::app`. Rewriting it would silently
repoint working code.

**Not automatable at all.** Rule 1 in `AGENTS.md`: an upstream feature that
landed in the TUI is not merged until it exists in `crates/gui` too. The merge
cannot tell you that. Read the upstream log for what arrived:

```bash
git log --oneline $(git merge-base gui upstream/main)..upstream/main
```

## Noticing in the first place

`.github/workflows/upstream-drift.yml` checks every Monday and keeps a single
issue up to date with how far behind `gui` is and where the commits land. It
is the one step here nobody should have to remember, and it is why this fork
sat 40 days behind: checking was a thing you had to think of.

**It is inert today.** GitHub disables workflows on a fork until a maintainer
turns them on, and this repository has run zero workflows - no CI, no release
check, nothing. Until that switch is flipped in *Settings -> Actions*, the only
verification this fork has is `./scripts/upstream.sh gate` on someone's
machine, which makes that command load-bearing rather than a convenience.

## Resolving what is left: a playbook

`git status --short` during a merge tells you which kind of problem you have.
There are only five.

### `UU` - both modified

The ordinary case, and the majority. Take both sides. Upstream's change is
usually a bug fix in code we also restructured; our change is usually the
restructuring. Keep our structure, apply their fix inside it.

### `UA` / `AU` - one side added

Usually a file upstream added under `src/tui/` that git matched to our
relocated copy. Take theirs, then let `finish` fix the imports. If it is a new
file with no counterpart here, decide whether the TUI needs it - and if it
does, whether `crates/gui` needs the same feature.

### `DU` - we deleted, they changed

Almost always one of our module splits. Upstream edited `src/discord/voice.rs`;
we turned it into `src/discord/voice/`. Find which submodule now owns the code
they touched and apply the change there:

```bash
git show upstream/main:src/discord/voice.rs > /tmp/theirs.rs
git diff $(git merge-base gui upstream/main):src/discord/voice.rs /tmp/theirs.rs
```

Then `git rm` the path they modified, because it is not coming back.

### `UD` - we changed, they deleted

Upstream removed something we still use. Do not just keep ours: find out why
it went. Usually it was replaced, and the replacement is what we want.

### `DD` - both deleted

Take the deletion. `git rm` the path.

## For agents

Read this whole file before starting, then:

- **Run `preview` first and report the number.** A merge that will conflict in
  111 files is a different piece of work from one that conflicts in four, and
  the person asking deserves to know which they are getting before you begin.
- **Do not resolve a conflict you cannot explain.** "Took theirs" is not a
  resolution unless you can say what their change does. The `DU` cases in
  particular look like "we deleted it, so drop it", and that silently throws
  away an upstream bug fix.
- **`finish` is not optional and is not idempotent-by-accident.** Run it after
  resolving, before the gate. The import rewrite skips any file that still has
  conflict markers, by design - so anything you resolve by hand has not been
  rewritten yet.
- **The gate runs both feature configurations.** `--features fixtures` and
  plain. The core failed to build without `fixtures` for a month because every
  documented command passed that flag. Do not drop one to save time.
- **Report what you did not merge.** If upstream added a TUI feature and you
  did not port it to `crates/gui`, say so explicitly and add it to
  `docs/PARITY.md`. A silent gap is how invites and forwarding ended up being
  retrofitted.

## Branch model

| Branch | What it is |
|---|---|
| `gui` | This fork's work, and the default branch. Merge upstream *into* here. |
| `main` | A mirror of `upstream/main`, fast-forwarded only. Never commit to it. |

`sync-main` pushes straight from the remote-tracking ref and refuses anything
that is not a fast-forward, so the mirror cannot drift. If it ever does refuse,
someone has committed to `main` and that needs sorting out by hand - the mirror
is worth nothing the moment our work is mixed into it.
