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

## What one release actually costs

Measured by merging v2.5.10 - eleven upstream commits, the smallest unit
available:

| | |
|---|---|
| Conflicted files | 38 |
| Conflict hunks | 54 |
| Resolved mechanically by this script | 1 file, plus imports in 12 |
| **Compile errors after every conflict was resolved** | **63** |

That last row is the whole lesson. The conflicts are the easy part. The
expensive part arrives afterwards, in files the merge never touched, because
of the files this fork split out of an upstream monolith.

Upstream keeps `voice.rs`, `events.rs` and `gateway.rs` as single files. We
split each into a directory. When upstream adds a field to a struct in
`voice.rs`, git offers us the whole monolith as "theirs" against our
three-line module root as "ours" - and "ours" is obviously right, so the field
addition goes in the bin with it. Nothing reports this. It surfaces later as
`no field 'audio_codec'` somewhere else entirely.

On v2.5.10 that accounted for every one of the 63 errors: `VoiceSessionDescription`
grew three fields, `ChannelInfo` lost four, `GatewayCommand` gained one,
`ActivityKind::Unknown` became a tuple variant, `RelationshipInfo` gained
`ignored`, and six new `AppEventKind` variants needed classifying. None of it
is a disagreement with upstream. All of it is our own code following an API
change that the merge silently dropped.

`merge` now names those files and prints the diff that shows what upstream did
to them. Port that diff by hand; do not expect the merge to carry it.

### The bigger lever

The cheapest merge is the one against a tree shaped like upstream's. Two
choices this fork has already made are what the work above costs:

- **Moving `src/tui` into `crates/tui`** is unavoidable - it is the whole
  point of the workspace split - and it is cheap, because the conflicts it
  causes are import lines and a script can rewrite those.
- **Splitting upstream's files into directories** is not unavoidable, and it
  is expensive, because it hides upstream's changes rather than conflicting
  with them. `AGENTS.md` asks for modules under 1,000 lines; upstream does not
  agree. Every file split in `src/` is a bill paid at every future merge.

Splitting `crates/gui` costs nothing - upstream has no opinion about it.
Splitting `src/discord` costs on every release, forever.

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
from the second merge onwards. Do not turn it off. The v2.5.10 merge recorded
31 resolutions; those replay for free next time.

`scripts/conflict-hunks.py` takes a file a hunk at a time - `show` to read both
sides, `take ours theirs both` to pick. Most files want a different side in
different places, which is why `git checkout --ours` is usually the wrong tool.

## What is automated, and what is not

**Done for you.** The mirror fast-forward. The `Cargo.lock` regeneration.
The `crate:: -> concord::` rewrite on everything under `crates/`. Rename
detection tuned to 25%, which on the trial merge turned seven delete/modify
conflicts back into ordinary content conflicts without misattributing any.

**Measured and rejected.** `-X diff-algorithm`. On the same merge, myers,
minimal, patience and histogram gave 38 files and 52-54 hunks - a difference of
two hunks across all four. The conflicts look like reordering but are not, so
the knob is not worth its explanation.

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

The ordinary case, and the majority. Which side to prefer depends on how far
that *file* has drifted, not on how far the tree has - `merge` prints the
number for every conflicted file under `crates/tui`.

Under about twenty lines, our copy is upstream's file plus the import rewrite,
and theirs is usually right. Above it the fork has put work in, and every hunk
wants reading: `state/tests/leader_actions.rs` differs by 1,170 lines, and
taking theirs there deleted a fork type and cost seventeen compile errors.

Where both sides have really changed, keep our structure and apply their fix
inside it.

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

## Where the backlog stands

At the time of writing `gui` is 87 commits behind across 14 upstream releases,
and none of them have landed. `merge-upstream-v2.5.10` holds the first one:
every conflict resolved, the core library compiling, the test and fixture
targets not. It is committed unfinished on purpose, so the resolution work and
the rerere cache survive. `scripts/upstream.sh status` prints the rest.

What is left on that branch is all ours rather than the merge's:

- `crates/fixtures/src/backend.rs` still answers `LoadForumPosts` with
  `ForumPostsLoaded`. Upstream replaced both with `LoadForumPostData` /
  `ForumPostDataLoaded` and `LoadArchivedThreads` / `ArchivedThreadsLoaded`, so
  demo mode has to synthesise `ForumPostDataInfo` and `ArchivedThreadsPage`.
- `src/discord/fixtures.rs::forum_posts` takes a bool where it took the deleted
  `ForumPostArchiveState`; its callers have not followed.
- A handful of initialisers have not caught up with the struct changes above.

That shape will repeat: the merge itself is a morning, and then `crates/fixtures`
has to be taught whatever upstream changed. Demo mode is the fork's best
feature and its largest standing maintenance cost, because it mirrors an API
that is not ours.

## Branch model

| Branch | What it is |
|---|---|
| `gui` | This fork's work, and the default branch. Merge upstream *into* here. |
| `main` | A mirror of `upstream/main`, fast-forwarded only. Never commit to it. |

`sync-main` pushes straight from the remote-tracking ref and refuses anything
that is not a fast-forward, so the mirror cannot drift. If it ever does refuse,
someone has committed to `main` and that needs sorting out by hand - the mirror
is worth nothing the moment our work is mixed into it.
