#!/usr/bin/env bash
#
# Track chojs23/concord from this fork.
#
#   ./scripts/upstream.sh status      what upstream has that we do not
#   ./scripts/upstream.sh sync-main   fast-forward the main mirror and push it
#   ./scripts/upstream.sh preview [R] trial-merge in a throwaway worktree
#   ./scripts/upstream.sh merge [R]   start the real merge on its own branch
#   ./scripts/upstream.sh finish      after you resolve: fix imports, relock
#   ./scripts/upstream.sh gate        fmt, clippy and tests, both feature sets
#
# [R] is any upstream ref, and defaults to upstream/main. Pass a release tag
# to take one release at a time, which is the whole point: 87 commits at once
# conflicted in 111 files, and no amount of tooling makes that a good morning.
#
# Why this exists
# ---------------
# `gui` had not taken a single upstream commit between 14 August and the day
# this was written: 87 commits of drift, and a trial merge with 111 conflicted
# files. Almost none of that was interesting. It was the same three mechanical
# problems repeated, because this fork rearranged the tree upstream keeps
# editing:
#
#   src/tui/**              ->  crates/tui/src/tui/**
#   src/tui/theme.rs        ->  crates/ui/src/theme.rs
#   src/tui/keybindings/**  ->  crates/ui/src/keybindings/**
#   src/discord/<big>.rs    ->  src/discord/<big>/ (split into submodules)
#
# and because a moved file that is also edited stops looking like a rename,
# which turns an ordinary content conflict into a delete/modify one. Raising
# git's rename threshold recovers most of them: on that trial merge it took
# the delete/modify count from 16 down to 9 without losing anything.
#
# On top of the move, every relocated file needs the same edit: upstream
# writes `crate::discord`, and over here the core is a dependency, so it has
# to read `concord::discord`. That is 370 occurrences in src/tui alone, and it
# is the single most common thing a human would otherwise fix by hand.
#
# So: this script does the mechanical part and gets out of the way. It does
# not resolve a real conflict and does not pretend to. What it buys is that
# the conflicts left over are the ones worth a person's attention.
#
# The one thing that helps more than any of this is merging often. 87 commits
# gave 111 conflicted files; one upstream release at a time would give a
# handful, and `git rerere` (which `merge` turns on) replays the resolutions
# you have already made when the same conflict comes round again.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

UPSTREAM_REMOTE="upstream"
ORIGIN_REMOTE="origin"
WORK_BRANCH="gui"
MIRROR_BRANCH="main"
BOX="build-box"

# Measured on the 14 Aug -> 23 Sep drift: at git's default 50% a heavily
# rewritten relocation stops being a rename and becomes delete/modify, which
# is the conflict shape with the least information in it. 25% recovered seven
# of those files and misattributed none.
RENAME_THRESHOLD="25%"

# Upstream is a single crate, so it says `crate::` for what is, over here, a
# dependency.
#
# Two names are deliberately absent. `crate::tui` is correct on both sides:
# crates/tui still has its own `tui` module. `crate::app` is worse - it is
# correct on both sides but means different things, because crates/tui has its
# own `mod app` beside the core's `concord::app`. Rewriting that one silently
# repoints working code at a different module, so it stays a decision for
# whoever is doing the merge.
REWRITE_RE='crate::(discord|config|logging|support|risk|translation)\b'

# Subtrees this fork moved wholesale, as "upstream prefix -> our prefix". A
# file that merely moved is not lost: git's rename detection finds it and the
# merge conflicts normally. Only `orphans` needs this, to tell a move apart
# from a split.
RELOCATIONS=(
    "src/tui/=crates/tui/src/tui/"
)

info() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

require_remote() {
    git remote get-url "$1" >/dev/null 2>&1 ||
        die "no '$1' remote. Add it with: git remote add $1 https://github.com/chojs23/concord.git"
}

require_clean() {
    if ! git diff --quiet || ! git diff --cached --quiet; then
        die "working tree is dirty. Commit or stash first."
    fi
}

base_rev() { git merge-base "$WORK_BRANCH" "$UPSTREAM_REMOTE/$MIRROR_BRANCH"; }

# The thing being merged. Defaults to the tip; a release tag is better.
target_rev() {
    local want="${1:-$UPSTREAM_REMOTE/$MIRROR_BRANCH}"
    git rev-parse --verify --quiet "$want^{commit}" >/dev/null ||
        die "no such ref: $want"
    git merge-base --is-ancestor "$want" "$UPSTREAM_REMOTE/$MIRROR_BRANCH" ||
        die "$want is not on $UPSTREAM_REMOTE/$MIRROR_BRANCH - refusing to merge it"
    printf '%s' "$want"
}

# A short name for the ref, for branch names and messages.
target_slug() { printf '%s' "${1##*/}" | tr -c 'A-Za-z0-9._-' '-'; }

# ---------------------------------------------------------------------------
# status
# ---------------------------------------------------------------------------
cmd_status() {
    require_remote "$UPSTREAM_REMOTE"
    info "Fetching $UPSTREAM_REMOTE"
    git fetch --quiet "$UPSTREAM_REMOTE" --tags

    local base ahead behind
    base="$(base_rev)"
    behind="$(git rev-list --count "$base..$UPSTREAM_REMOTE/$MIRROR_BRANCH")"
    ahead="$(git rev-list --count "$UPSTREAM_REMOTE/$MIRROR_BRANCH..$WORK_BRANCH")"

    echo
    echo "Fork point   $(git show -s --format='%h %ad  %s' --date=short "$base")"
    echo "Upstream     $behind commits we do not have"
    echo "This fork    $ahead commits upstream does not have"

    if [[ "$behind" -eq 0 ]]; then
        echo
        info "Up to date."
        return 0
    fi

    local last_tag
    last_tag="$(git tag --list --sort=-v:refname --merged "$UPSTREAM_REMOTE/$MIRROR_BRANCH" 'v*' | head -n1)"
    [[ -n "$last_tag" ]] && echo "Latest release $last_tag"

    echo
    echo "Where those $behind commits land:"
    local area n
    for area in src/discord src/tui src/app src/config src/translation .github docs; do
        n="$(git log --oneline "$base..$UPSTREAM_REMOTE/$MIRROR_BRANCH" -- "$area" | wc -l)"
        [[ "$n" -gt 0 ]] && printf '  %-18s %4s commits%s\n' "$area" "$n" \
            "$(relocation_note "$area")"
    done

    echo
    echo "Files both sides have touched since the fork point:"
    comm -12 \
        <(git diff --name-only "$base..$UPSTREAM_REMOTE/$MIRROR_BRANCH" | sort) \
        <(git diff --name-only "$base..$WORK_BRANCH" | sort) |
        sed 's/^/  /' | head -40
    local overlap
    overlap="$(comm -12 \
        <(git diff --name-only "$base..$UPSTREAM_REMOTE/$MIRROR_BRANCH" | sort) \
        <(git diff --name-only "$base..$WORK_BRANCH" | sort) | wc -l)"
    [[ "$overlap" -gt 40 ]] && echo "  ... and $((overlap - 40)) more"

    echo
    echo "Unmerged upstream releases - one of these is a sitting, all of them is not:"
    local tag n
    while read -r tag; do
        n="$(git rev-list --count "$WORK_BRANCH..$tag")"
        printf '  %-10s %s  %3s commits\n' "$tag" \
            "$(git show -s --format=%ad --date=short "$tag")" "$n"
    done < <(unmerged_releases)

    echo
    echo "Run './scripts/upstream.sh preview <tag>' for what one of them would cost."
}

# Upstream's release tags that gui does not already contain, oldest first.
# `--no-merged` is the whole test: a tag gui has already taken is not a
# sitting's work, it is history.
unmerged_releases() {
    git tag --list 'v*' --sort=v:refname --merged "$UPSTREAM_REMOTE/$MIRROR_BRANCH" \
        --no-merged "$WORK_BRANCH"
}

# Upstream paths this fork has moved. Said out loud in the status report
# because "48 commits in src/tui" reads as harmless until you know src/tui is
# not where this fork keeps the TUI any more.
relocation_note() {
    case "$1" in
        src/tui) printf '  -> crates/tui, crates/ui here' ;;
        src/discord) printf '  (split into submodules here)' ;;
        src/translation) printf '  (absent from this fork)' ;;
        *) printf '' ;;
    esac
}

# ---------------------------------------------------------------------------
# sync-main
# ---------------------------------------------------------------------------
#
# `main` is a mirror and nothing else, so this is the one part of tracking
# upstream that is safe to do without reading anything. It refuses any push
# that is not a fast-forward, which is the whole guarantee the mirror rests on.
cmd_sync_main() {
    require_remote "$UPSTREAM_REMOTE"
    require_remote "$ORIGIN_REMOTE"
    require_clean

    info "Fetching both remotes"
    git fetch --quiet "$UPSTREAM_REMOTE" --tags
    git fetch --quiet "$ORIGIN_REMOTE" --no-tags

    if ! git merge-base --is-ancestor "$ORIGIN_REMOTE/$MIRROR_BRANCH" "$UPSTREAM_REMOTE/$MIRROR_BRANCH"; then
        die "$ORIGIN_REMOTE/$MIRROR_BRANCH is not an ancestor of $UPSTREAM_REMOTE/$MIRROR_BRANCH.
     Something has been committed to the mirror. Sort that out by hand:
     the mirror is worth nothing the moment our work is mixed into it."
    fi

    if [[ "$(git rev-parse "$ORIGIN_REMOTE/$MIRROR_BRANCH")" == "$(git rev-parse "$UPSTREAM_REMOTE/$MIRROR_BRANCH")" ]]; then
        info "Mirror is already current."
        return 0
    fi

    info "Fast-forwarding $ORIGIN_REMOTE/$MIRROR_BRANCH to $UPSTREAM_REMOTE/$MIRROR_BRANCH"
    # Pushed straight from the remote-tracking ref: nothing needs checking out,
    # so this works from any branch and cannot pick up local state by accident.
    git push "$ORIGIN_REMOTE" "$UPSTREAM_REMOTE/$MIRROR_BRANCH:refs/heads/$MIRROR_BRANCH"
    git push "$ORIGIN_REMOTE" --tags
    git branch --force "$MIRROR_BRANCH" "$UPSTREAM_REMOTE/$MIRROR_BRANCH"
    info "Mirror and tags are current. The gui branch is untouched."
}

# ---------------------------------------------------------------------------
# preview
# ---------------------------------------------------------------------------
#
# The point of doing this in a worktree is that it costs nothing to abandon.
# Knowing a merge is 111 files before starting it is the difference between
# scheduling the work and discovering it.
preview_cleanup() {
    [[ -n "${PREVIEW_WORKTREE:-}" ]] || return 0
    git worktree remove --force "$PREVIEW_WORKTREE" >/dev/null 2>&1 || true
    rm -rf "$PREVIEW_WORKTREE"
}

cmd_preview() {
    require_remote "$UPSTREAM_REMOTE"
    git fetch --quiet "$UPSTREAM_REMOTE" --tags
    local target
    target="$(target_rev "${1:-}")"

    # Deliberately not `local`: the EXIT trap runs after this function has
    # returned, and a local would be out of scope by then - which under `set
    # -u` kills the trap and leaves the worktree behind.
    PREVIEW_WORKTREE="$(mktemp -d)"
    trap preview_cleanup EXIT

    info "Trial-merging $target in a throwaway worktree"
    git worktree add --quiet --detach "$PREVIEW_WORKTREE" "$WORK_BRANCH"

    local conflicts=0
    if git -C "$PREVIEW_WORKTREE" merge --no-commit --no-ff \
        -X "find-renames=$RENAME_THRESHOLD" "$target" >/dev/null 2>&1; then
        info "Clean merge. Nothing to think about."
        return 0
    fi

    conflicts="$(git -C "$PREVIEW_WORKTREE" diff --name-only --diff-filter=U | wc -l)"
    echo
    echo "$conflicts conflicted files."
    echo
    echo "By kind:"
    git -C "$PREVIEW_WORKTREE" status --short | awk '$1 ~ /^(UU|AA|UA|AU|DU|UD|DD)$/ {print $1}' |
        sort | uniq -c | sort -rn |
        sed -e 's/UU/both modified/' -e 's/DU/we deleted, they changed/' \
            -e 's/UD/we changed, they deleted/' -e 's/UA/they added/' \
            -e 's/AU/we added/' -e 's/DD/both deleted/' -e 's/AA/both added/' |
        sed 's/^/  /'

    echo
    echo "By area:"
    git -C "$PREVIEW_WORKTREE" diff --name-only --diff-filter=U |
        sed -E 's#^(crates/[^/]+|src/[^/]+|[^/]+)(/.*)?$#\1#' |
        sort | uniq -c | sort -rn | sed 's/^/  /'

    echo
    echo "Mechanical, and 'merge' will do these for you:"
    # grep -c prints 0 *and* exits 1 when nothing matches, so `|| echo 0`
    # printed the count twice. Let the empty case be the default instead.
    local lock_hunks
    lock_hunks="$(grep -c '^<<<<<<<' "$PREVIEW_WORKTREE/Cargo.lock" 2>/dev/null || true)"
    printf '  %-28s %s\n' "Cargo.lock" \
        "${lock_hunks:-0} hunks - regenerated, not merged"
    printf '  %-28s %s\n' "crate:: -> concord::" \
        "$(git -C "$PREVIEW_WORKTREE" grep -lE "$REWRITE_RE" -- 'crates/*' 2>/dev/null | wc -l) relocated files carry it"

    echo
    cmd_orphans "$target" || true

    echo
    echo "Nothing has been changed. Run 'merge' when you want the real thing."
}

# ---------------------------------------------------------------------------
# merge
# ---------------------------------------------------------------------------
cmd_merge() {
    require_remote "$UPSTREAM_REMOTE"
    require_clean
    git fetch --quiet "$UPSTREAM_REMOTE" --tags

    # rerere remembers how a conflict was resolved and replays it the next
    # time the same one appears. Every recurring conflict here is structural -
    # the same relocated file, the same import line - so this pays from the
    # second merge onwards. It is set locally rather than asked for in a doc
    # nobody reads.
    git config rerere.enabled true
    git config rerere.autoupdate true

    local target branch
    target="$(target_rev "${1:-}")"
    branch="merge-upstream-$(target_slug "$target")"
    git rev-parse --verify --quiet "$branch" >/dev/null &&
        die "branch $branch already exists. Finish or delete it first."

    local n
    n="$(git rev-list --count "$WORK_BRANCH..$target")"
    info "Branching $branch off $WORK_BRANCH"
    git checkout --quiet -b "$branch" "$WORK_BRANCH"

    local base
    base="$(git merge-base "$WORK_BRANCH" "$target")"

    info "Merging $target - $n commits (rename threshold $RENAME_THRESHOLD)"
    git merge --no-commit --no-ff \
        -X "find-renames=$RENAME_THRESHOLD" "$target" || true

    remap_relocated "$base" "$target"
    cmd_finish
    divergence_report "$base"
    split_report "$base" "$target"

    local left
    left="$(git diff --name-only --diff-filter=U | wc -l)"
    echo
    if [[ "$left" -eq 0 ]]; then
        info "Nothing left conflicted. Run gate, then commit."
    else
        warn "$left files need reading. scripts/conflict-hunks.py takes them a"
        warn "hunk at a time, which beats an editor when a file wants ours in one"
        warn "place and theirs in the next. Then:"
        echo "     ./scripts/upstream.sh finish   (re-runs the mechanical passes)"
        echo "     ./scripts/upstream.sh gate"
        echo "     git commit"
    fi
    echo
    echo "Remember rule 1 in AGENTS.md: an upstream feature that landed in the TUI"
    echo "is not merged until it exists in crates/gui too. The merge cannot tell you"
    echo "that; read the upstream log for what arrived:"
    echo "     git log --oneline $(base_rev)..$UPSTREAM_REMOTE/$MIRROR_BRANCH"
}

# How far each conflicted file has actually drifted from upstream.
#
# "The fork barely touches crates/tui" is true in aggregate - four commits -
# and useless per file. On v2.5.10, most of that tree was upstream's own code
# on both sides and wanted theirs; state/tests/leader_actions.rs differed by
# 1,170 lines and taking theirs threw away a whole fork type. Seventeen
# compile errors, all from one heuristic applied without measuring.
#
# So measure. The number here is our file against upstream's at the merge
# base, with the import rewrite applied first so the move is not counted as
# drift. Small means upstream's copy is the file and theirs is usually right.
# Large means the fork has put work in and every hunk wants reading.
divergence_report() {
    local base="$1" f up tmp ours theirs n

    tmp="$(mktemp -d)"
    ours="$tmp/ours"; theirs="$tmp/theirs"

    echo
    echo "How far each conflicted file has drifted from upstream:"
    while IFS= read -r f; do
        up=""
        [[ "$f" == crates/tui/src/tui/* ]] && up="src/tui/${f#crates/tui/src/tui/}"
        [[ -n "$up" ]] || continue
        git cat-file -e "$base:$up" 2>/dev/null || continue

        git show ":2:$f" >| "$ours" 2>/dev/null || continue
        git show "$base:$up" | sed -E "s/\b$REWRITE_RE/concord::\1/g" >| "$theirs"
        n="$(diff "$ours" "$theirs" | grep -c '^[<>]' || true)"

        if [[ "${n:-0}" -le 20 ]]; then
            printf '  %-56s %5s lines - upstream\047s file, theirs is usually right\n' "$f" "${n:-0}"
        else
            printf '  %-56s %5s lines - WE HAVE WORK IN HERE, read every hunk\n' "$f" "${n:-0}"
        fi
    done < <(git diff --name-only --diff-filter=U | sort -u)

    rm -rf "$tmp"
    return 0
}

# Say which conflicts are file splits, because those are the expensive ones.
#
# The relocated TUI is the loud problem and the cheap one: it conflicts in a lot
# of files and every conflict is about imports. The quiet, expensive problem is
# the files this fork split out of an upstream monolith - voice.rs, events.rs,
# gateway.rs and the rest. Upstream still edits the single file, so its type and
# API changes arrive on the side of the conflict our split makes us discard,
# and nothing says so. They surface later as compile errors in files the merge
# never touched.
#
# On v2.5.10 that was the whole tail of the work: 63 errors across fixtures and
# tests, every one of them our own code following an upstream API change that
# the merge had silently dropped on the floor.
#
# So: name them, and print the command that shows what upstream actually did.
split_report() {
    local base="$1" target="$2" f dir up n found=0

    while IFS= read -r f; do
        up=""
        # a/b.rs conflicted and a/b/ exists here: we split b.rs
        dir="${f%.rs}"
        if [[ "$f" == *.rs && -d "$dir" ]]; then
            up="$f"
        else
            # a/b/c.rs conflicted and upstream keeps a/b.rs whole
            dir="$(dirname "$f")"
            if git cat-file -e "$target:$dir.rs" 2>/dev/null; then
                up="$dir.rs"
            fi
        fi
        [[ -n "$up" ]] || continue
        git cat-file -e "$base:$up" 2>/dev/null || continue

        n="$(git diff --numstat "$base:$up" "$target:$up" 2>/dev/null | awk '{print $1 + $2}')"
        [[ -n "$n" && "$n" -gt 0 ]] || continue

        if [[ "$found" -eq 0 ]]; then
            echo
            warn "These conflicts are files we split. Upstream changed the whole"
            warn "file; the part you did not take is not noise, it is the change:"
            found=1
        fi
        printf '  %-52s %s lines changed upstream\n' "$f" "$n"
        printf '      git diff %s:%s %s:%s\n' "${base:0:8}" "$up" "$target" "$up"
    done < <(git diff --name-only --diff-filter=U | sort -u)

    # The warning above only reaches files that conflicted. A split file
    # upstream changed and we no longer have does not conflict at all, so it
    # needs its own pass.
    echo
    cmd_orphans "$target" || true

    return 0
}

# ---------------------------------------------------------------------------
# finish
# ---------------------------------------------------------------------------
#
# The mechanical passes, run again once the conflicts are gone. Both of them
# need a tree that parses, so neither can do its job in the middle of a merge:
# cargo cannot read a Cargo.toml with conflict markers in it, and rewriting an
# import inside a conflict hunk produces a file that looks resolved and is
# not. `merge` calls this too, which catches the files that merged cleanly and
# still came in with upstream's prefixes - those are the dangerous ones,
# because they do not conflict, they just fail to compile later.
cmd_finish() {
    split_crate_imports
    rewrite_imports
    dedupe_imports
    resolve_lockfile
    widen_for_workspace
    # Picking hunks leaves import blocks in whatever order the two sides had.
    # rustfmt sorts them, and doing it here keeps the diff about the merge
    # rather than about whitespace.
    in_box "cargo fmt --all" >/dev/null 2>&1 || warn "cargo fmt did not run"

    # Say where the merge actually stands. Without this the pass was silent on
    # success and silent on failure, so the only way to find out was to run
    # cargo yourself - which is the step this is supposed to save.
    local errors
    errors="$(in_box "cargo check --workspace --all-targets --features fixtures -j 4 --message-format short" 2>&1 |
        grep -cE "^[^ ]+: error(\[|:)" || true)"
    if [[ "${errors:-0}" -eq 0 ]]; then
        info "Workspace compiles. Run 'gate' before committing."
    else
        warn "$errors compile error(s) left. These are yours:"
        in_box "cargo check --workspace --all-targets --features fixtures -j 4 --message-format short" 2>&1 |
            grep -E "^[^ ]+: error(\[|:)" | head -20 | sed 's/^/  /'
    fi
    return 0
}

# Drop `use` lines a resolution duplicated.
#
# Taking upstream's side of an import hunk re-adds names the fork imports a few
# lines down, because our copy was rewritten and no longer looks like the same
# line to git. E0252 across a dozen files, every release, entirely mechanical.
# `use crate::{discord::…, tui::…}` has to become two statements here. The
# flat form is a substitution rewrite_imports can do; this one is not, and it
# arrived as a compile error at every merge until it was automated.
split_crate_imports() {
    local files clean=() f
    files="$(git diff --name-only --diff-filter=ACMU HEAD -- 'crates/*.rs' | sort -u)"
    [[ -n "$files" ]] || return 0
    # Skip anything still conflicted, as rewrite_imports does. A conflict
    # marker sits inside the brace group this pass rewrites, so it was read as
    # one more import and emitted into the middle of a statement - turning a
    # conflict you could still resolve into a file neither side recognised.
    while IFS= read -r f; do
        [[ -f "$f" ]] || continue
        grep -q '^<<<<<<<' "$f" && continue
        clean+=("$f")
    done <<<"$files"
    [[ "${#clean[@]}" -gt 0 ]] || return 0
    python3 "$REPO/scripts/split-crate-imports.py" "${clean[@]}" >/dev/null 2>&1 || true
    return 0
}

dedupe_imports() {
    local files
    files="$(git diff --name-only --diff-filter=ACMU HEAD -- 'crates/*.rs' 'src/*.rs' | sort -u)"
    [[ -n "$files" ]] || return 0
    local dropped
    # shellcheck disable=SC2086  # deliberate word splitting: one path per arg
    dropped="$(python3 "$REPO/scripts/dedupe-imports.py" $files 2>&1 | tail -n1 || true)"
    [[ "$dropped" == "0 removed" ]] || info "Imports: ${dropped:-nothing removed}"
    return 0
}

# Upstream is one crate; we are a workspace.
#
# `pub(crate)` reaches upstream's own TUI and does not reach ours, so every
# item the front ends touch has to widen. The compiler names them, so ask it
# rather than guessing: eleven items and two structs' worth of fields needed
# this on v2.5.10, and each one is an error that only appears after everything
# else compiles.
widen_for_workspace() {
    local names name hit widened=0 bt
    # rustc quotes the item in backticks; kept in a variable so neither bash
    # nor shellcheck reads them as a command substitution.
    bt=$'\x60'
    # `|| true`, and it matters: cargo exits non-zero whenever the tree does
    # not compile, `set -o pipefail` promotes that to the pipeline, and the
    # failing assignment then killed `finish` under `set -e` - silently, and
    # only ever when there was work to do. It looked like a no-op pass.
    names="$(in_box "cargo check --workspace --all-targets --features fixtures -j 4 --message-format short" 2>&1 |
        grep -oE "(method|struct|function|enum|field|associated function) ${bt}[A-Za-z_][A-Za-z0-9_]*${bt}( of struct ${bt}[A-Za-z_][A-Za-z0-9_:]*${bt})? is private" |
        sed -E "s/ of struct ${bt}[A-Za-z_][A-Za-z0-9_:]*${bt}//" |
        grep -oE "${bt}[A-Za-z_][A-Za-z0-9_]*${bt}" | tr -d "${bt}" | sort -u || true)"
    [[ -n "$names" ]] || return 0

    # `pub(in crate::tui)` is upstream's idiom and is valid inside crates/tui,
    # which has that module - and nowhere else in this workspace. Anywhere
    # else the path does not resolve at all, so it is an error, not a warning.
    local stray
    stray="$(git grep -l 'pub(in crate::tui)' -- 'crates/*' ':!crates/tui/*' 2>/dev/null || true)"
    if [[ -n "$stray" ]]; then
        while IFS= read -r f; do
            [[ -f "$f" ]] || continue
            sed -i 's/pub(in crate::tui) /pub /g' "$f"
            info "Widened pub(in crate::tui) in $f - that module exists only in crates/tui"
        done <<<"$stray"
    fi

    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        # Items, then struct fields - `pub(crate) text: String` is as
        # unreachable from a front-end crate as a `pub(crate) fn`, and rustc
        # words it differently enough that it used to be missed.
        # Every one of these needs `|| true`: grep exits non-zero when it
        # matches nothing, `set -o pipefail` carries that to the pipeline, and
        # a failing assignment under `set -e` kills the pass outright - so the
        # first name that was not an item silently ended the widening.
        # `pub(crate)` and `pub(super)` both, and in the shared crates as well
        # as the core: a front-end crate cannot reach either, and upstream
        # writes whichever its single-crate layout happened to allow.
        hit="$(grep -rln "pub(\(crate\|super\)) \(fn\|struct\|enum\) $name\b" src/ crates/ui/src/ crates/cache/src/ 2>/dev/null | head -n1 || true)"
        if [[ -z "$hit" ]]; then
            hit="$(grep -rln "pub(\(crate\|super\)) $name *:" src/ crates/ui/src/ crates/cache/src/ 2>/dev/null | head -n1 || true)"
            [[ -n "$hit" ]] || continue
            sed -i "s/pub(crate) $name *:/pub $name:/; s/pub(super) $name *:/pub $name:/" "$hit"
            widened=$((widened + 1))
            continue
        fi
        sed -i -E "s/pub\\((crate|super)\\) (fn|struct|enum) $name\\b/pub \\2 $name/" "$hit"
        widened=$((widened + 1))
    done <<<"$names"

    [[ "$widened" -gt 0 ]] && info "Widened $widened item(s) the front-end crates could not reach"
    return 0
}

# A lockfile is generated, so merging it line by line is meaningless - that
# trial merge had 58 conflict hunks in Cargo.lock alone. Take our side and let
# cargo reconcile it against whatever Cargo.toml the merge produced: it adds
# and drops entries as needed and leaves every other pin alone.
resolve_lockfile() {
    if git ls-files -u --error-unmatch Cargo.lock >/dev/null 2>&1; then
        info "Taking our Cargo.lock; it is generated, not written"
        git checkout --ours -- Cargo.lock
        git add Cargo.lock
    fi

    # Cargo can only reconcile the lockfile once every Cargo.toml in the
    # workspace parses, so this is a no-op until the conflicts are gone.
    #
    # Tested by looking in the files, not by asking the index: a manifest you
    # resolved but have not staged yet is resolved, and reporting it as
    # conflicted sent you away to relock something that was already fine.
    local manifest
    while IFS= read -r manifest; do
        [[ -f "$manifest" ]] || continue
        if grep -q '^<<<<<<<' "$manifest"; then
            warn "$manifest is still conflicted - relock after you resolve it"
            return 0
        fi
    done < <(git ls-files -- 'Cargo.toml' '*/Cargo.toml')

    info "Reconciling Cargo.lock against the merged manifests"
    if in_box "cargo metadata --format-version 1 --quiet >/dev/null"; then
        git add Cargo.lock
    else
        warn "cargo could not read the workspace; Cargo.lock left as ours"
    fi
}

# Re-merge the relocated TUI against a base git cannot see.
#
# crates/tui/src/tui/X is upstream's src/tui/X, moved, with `crate::` rewritten
# to `concord::`. Git's merge base for it is the pre-move file, which still
# says `crate::` - so our prefix rewrite looks like OUR edit on every import
# line, and collides with any import line upstream touches. That is not a
# disagreement about anything; it is an artifact of the move.
#
# Feeding a 3-way merge the same base with the prefixes already rewritten
# removes the artifact. Measured on v2.5.10: 19 conflicted files and 31 hunks
# became 15 and 19.
#
# Only a clean result is taken. When this still conflicts, the two sides
# genuinely disagree and git's own conflict - which carries its rename and
# context handling - is the better thing to hand a person.
remap_relocated() {
    local base="$1" target="$2"
    local f up tmp ours theirs newbase fixed=0

    tmp="$(mktemp -d)"
    ours="$tmp/ours"; theirs="$tmp/theirs"; newbase="$tmp/base"

    while IFS= read -r f; do
        [[ "$f" == crates/tui/src/tui/* ]] || continue
        up="src/tui/${f#crates/tui/src/tui/}"
        git cat-file -e "$base:$up" 2>/dev/null || continue
        git cat-file -e "$target:$up" 2>/dev/null || continue

        # `--ours` is the fork's file as it stood before this merge.
        git show ":2:$f" >| "$ours" 2>/dev/null || continue
        git show "$base:$up"   | sed -E "s/\b$REWRITE_RE/concord::\1/g" >| "$newbase"
        git show "$target:$up" | sed -E "s/\b$REWRITE_RE/concord::\1/g" >| "$theirs"

        if git merge-file -q -L ours -L base -L theirs "$ours" "$newbase" "$theirs" 2>/dev/null; then
            cat "$ours" >| "$f"
            git add -- "$f"
            fixed=$((fixed + 1))
        fi
    done < <(git diff --name-only --diff-filter=U)

    rm -rf "$tmp"
    [[ "$fixed" -gt 0 ]] && info "Re-merged $fixed relocated file(s) against a prefix-corrected base"
    return 0
}

# Upstream's own code says `crate::discord`. Here the core is a dependency, so
# every file that moved out of the root crate says `concord::discord`. An
# incoming hunk therefore arrives with the wrong prefix even when it merges
# cleanly, and it fails to compile rather than conflicting - which is the
# worse failure, because nothing points at it.
#
# Only files under crates/ are touched: in the root crate `crate::discord` is
# still correct and rewriting it would break the build.
rewrite_imports() {
    local files
    # Against HEAD, not the index. A file that merged cleanly is already
    # staged and identical to the working tree, so a bare `git diff` does not
    # list it - which silently skipped exactly the files this pass is for.
    files="$(git diff --name-only --diff-filter=ACMU HEAD -- 'crates/*.rs' | sort -u)"
    [[ -n "$files" ]] || return 0

    local touched=0 f
    while IFS= read -r f; do
        [[ -f "$f" ]] || continue
        grep -qE "$REWRITE_RE" "$f" || continue
        # Conflict markers are left alone: rewriting inside one produces a
        # file that looks resolved and is not.
        if grep -q '^<<<<<<<' "$f"; then
            warn "  $f still has conflict markers - imports left for you"
            continue
        fi
        sed -i -E "s/\b$REWRITE_RE/concord::\1/g" "$f"
        # Staged, not just written. A merge refuses to `--abort` while a file
        # it needs to restore differs from the index, so leaving these
        # unstaged took away the one escape hatch from a merge gone wrong.
        git add -- "$f"
        touched=$((touched + 1))
    done <<<"$files"

    [[ "$touched" -gt 0 ]] && info "Rewrote crate:: -> concord:: in $touched file(s)"
    return 0
}

# ---------------------------------------------------------------------------
# gate
# ---------------------------------------------------------------------------
#
# Both feature sets, because they are different compilations: the core failed
# to build without `fixtures` for a month while every documented command
# passed --features fixtures and never noticed.
cmd_gate() {
    # cargo serialises on the build directory, and two gates started by
    # accident do not queue politely - they sit on each other's locks until
    # one is killed. One at a time.
    # No `2>/dev/null` here. On an `exec` with no command, a redirection
    # applies to the shell itself and stays applied - so that one silenced
    # every error this script wrote for the rest of the run, including the
    # gate's own "Gate failed." verdict.
    mkdir -p "$REPO/target"
    exec 9>"$REPO/target/.upstream-gate.lock" || true
    if ! flock -n 9 2>/dev/null; then
        die "another gate is already running in this checkout"
    fi

    local jobs fail=0
    jobs=$(( $(nproc) / 5 ))
    [[ "$jobs" -lt 1 ]] && jobs=1

    # `-D warnings` because CI does, and because a merge produces exactly the
    # warnings that matter: imports left behind by a resolution, variables
    # orphaned by a field upstream deleted. A lenient gate passed twelve of
    # those on v2.5.10 that CI would have rejected.
    #
    # `--workspace` on the test step, and `-p concord-tui` named explicitly.
    # A bare `cargo test` at the root resolves to the `concord` package alone,
    # so `crates/tui` - which holds the relocated half of upstream's test
    # suite, over a thousand tests - was never run. Four of them were failing
    # across v2.5.12 to v2.5.15 and this gate reported clean every time.
    local step failed=()
    for step in \
        "cargo fmt --all -- --check" \
        "cargo clippy --workspace --all-targets --features fixtures -j $jobs -- -D warnings" \
        "cargo clippy -p concord --all-targets -j $jobs -- -D warnings" \
        "cargo test --workspace --features fixtures -j $jobs" \
        "cargo test -p concord-gui --features fixtures -j $jobs" \
        "cargo test -p concord-tui --features fixtures -j $jobs" \
        "cargo test -p concord -j $jobs"
    do
        info "$step"
        # Captured per step, and the tail printed on failure. The gate used to
        # report only which step failed: a log with six lines in it and no way
        # to tell whether clippy found two unused imports or the build died.
        local log="$REPO/target/.upstream-gate-step.log"
        if ! in_box "nice -n 19 $step" >"$log" 2>&1; then
            warn "FAILED: $step"
            tail -40 "$log" | sed 's/^/  /'
            failed+=("$step")
            fail=1
        else
            grep -E "^test result:" "$log" | sed 's/^/  /' || true
        fi
    done

    # The verdict goes to stdout, last, and names every step that failed. It
    # used to be a `die` to stderr, which a reader tailing or grepping the log
    # could miss entirely - and did: a failing gate was read as a passing one
    # because the tail of the log was full of passing test counts.
    echo
    if [[ "$fail" -ne 0 ]]; then
        warn "Gate failed. ${#failed[@]} step(s):"
        printf '  %s\n' "${failed[@]}"
        exit 1
    fi
    info "Gate clean."
}

# The host is immutable and has no cmake, which opusic-sys needs, so every
# cargo invocation goes through the container. See scripts/appimage.sh.
in_box() {
    command -v distrobox >/dev/null 2>&1 || die "distrobox not found"
    local list
    list="$(distrobox list 2>/dev/null || true)"
    grep -q "| *$BOX *|" <<<"$list" ||
        die "distrobox '$BOX' not found. Create it with: setup-build-box.sh --create"
    distrobox enter "$BOX" -- bash -lc "cd $(printf '%q' "$REPO") && $1"
}

# ---------------------------------------------------------------------------
# orphans
#
# The expensive failure mode of this fork is not a conflict - it is the
# absence of one. When upstream changes a file the fork has split, renamed
# beyond the rename threshold, or deleted, git has nothing on our side to
# conflict with, so the change merges "cleanly" by disappearing. Nothing in
# the merge output says so, and the loss surfaces later as a failing test, or
# not at all.
#
# This lists exactly those files: changed upstream in the range, absent from
# our tree. Every one needs a human or an agent to decide where the change
# belongs, or to record that it does not apply.
# ---------------------------------------------------------------------------
# True when $1 lives here under one of the relocated prefixes.
relocated_here() {
    local entry from to
    for entry in "${RELOCATIONS[@]}"; do
        from="${entry%%=*}"
        to="${entry#*=}"
        [[ "$1" == "$from"* ]] || continue
        git cat-file -e "$WORK_BRANCH:${to}${1#"$from"}" 2>/dev/null && return 0
    done
    return 1
}

cmd_orphans() {
    require_remote "$UPSTREAM_REMOTE"
    git fetch --quiet "$UPSTREAM_REMOTE" --tags
    local target base
    target="$(target_rev "${1:-}")"
    base="$(git merge-base "$WORK_BRANCH" "$target")"

    local found=0 path stat candidates
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        git cat-file -e "$WORK_BRANCH:$path" 2>/dev/null && continue
        # Moved, not lost: the merge will find it by rename detection.
        relocated_here "$path" && continue
        # Added upstream in this range. It has no home here because it has no
        # history here; the merge adds it as a new file, visibly.
        git cat-file -e "$base:$path" 2>/dev/null || continue
        if (( found == 0 )); then
            echo "Upstream changed these files; our tree has no such path."
            echo "Each one merged without a conflict and without an effect."
            echo
            printf '  %-46s %7s  %s\n' "UPSTREAM PATH" "CHANGE" "LIKELY HOME HERE"
            found=1
        fi
        stat="$(git diff --numstat "$base" "$target" -- "$path" |
            awk '{printf "+%s-%s", $1, $2}')"
        # A split file usually became a directory of the same stem, and a
        # relocated one keeps its basename. Check the first, fall back to the
        # second; both are hints, not answers.
        local stem="${path%.rs}"
        candidates="$(git ls-tree -r --name-only "$WORK_BRANCH" |
            grep -E "^$stem/" | head -4 | tr '\n' ' ')"
        [[ -n "$candidates" ]] || candidates="$(git ls-tree -r --name-only "$WORK_BRANCH" |
            grep -E "/$(basename "$path")$" | head -3 | tr '\n' ' ')"
        printf '  %-46s %7s  %s\n' "$path" "$stat" "${candidates:-<gone - confirm it should be>}"
    done < <(git diff --name-only "$base" "$target")

    if (( found == 0 )); then
        info "No orphaned upstream changes in this range."
        return 0
    fi
    echo
    echo "Read each upstream diff before you finish the merge:"
    echo "  git show $target -- <upstream path>"
    echo
    echo "Exit 1 is deliberate: an unreviewed orphan is a failed check."
    return 1
}

# ---------------------------------------------------------------------------
case "${1:-}" in
    status)    cmd_status ;;
    sync-main) cmd_sync_main ;;
    preview)   cmd_preview "${2:-}" ;;
    merge)     cmd_merge "${2:-}" ;;
    orphans)   cmd_orphans "${2:-}" ;;
    finish)    cmd_finish ;;
    gate)      cmd_gate ;;
    -h|--help|"") sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \?//' ;;
    *)         die "unknown command: $1 (try --help)" ;;
esac
