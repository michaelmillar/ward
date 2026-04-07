<p align="center">
  <img src="assets/logo.svg" width="200" alt="ward">
</p>

<h3 align="center">Archive stale Git repos only after a verified restore rehearsal</h3>

<p align="center">
  Classify local repositories by lifecycle stage. Build an archive package
  around a Git bundle plus captured local state. Rehearse restore in a
  temporary directory. Verify refs and metadata. Only then permit deletion.
</p>

---

<p align="center">
  <img src="assets/demo.svg" width="800" alt="ward in action">
</p>

---

## What it does

`ward` treats your development workspace as a fleet of git repositories with lifecycles. It classifies each repo into one of six verdicts, archives stale repos as verified `git bundle` files with a manifest, consolidates duplicate clones into worktrees, and sweeps out build artefacts. The differentiator is **proof-before-delete**. Every archive is verified by cloning the bundle into a temporary directory and comparing refs before the original is removed. Stashes are preserved via temporary refs in the bundle. Hooks, per-repo config, and untracked files travel in a companion extras tar alongside the bundle.

```
$ ward status ~/projects
Disk Usage
  Total              42.1 GB
  Build artefacts    31.4 GB (74%)
  Source and other   10.7 GB (25%)
  Git repositories   47

Lifecycle
  archive        8   2.1 GB
  prototype     12   340.2 MB
  keep          15   4.8 GB
  local-work     9   3.2 GB
  no-remote      3    14.5 MB
```

```
$ ward archive ~/projects --execute
  [ARCHIVE] ~/projects/old-service (145.3 MB, last commit 2025-11-03)
    Safety proof
      remote           github.com/acme/old-service
      head             a1b2c3d4e5
      commits          234 across 4 author(s)
      branches         3 (0 local-only, 0 ahead)
      uncommitted      no
      stashes          0
      tags             12
      untracked        0 (ignored 2)
      worktrees        0
      size             145.3 MB
  Bundling ~/projects/old-service ...
    verifying by clone ...
    ok archived ~/.ward/archives/old-service-20260405.bundle (42.1 MB)
```

## How ward differs

Existing tools mostly clean build artefacts or caches. Ward answers a different question. *"Which local git repositories can I archive and remove with evidence, and can I verify the archive before deletion?"*

| Tool              | Archive repos | Verify-by-clone | Capture stashes/hooks/config | Safety proofs | Lifecycle triage |
|-------------------|---------------|-----------------|------------------------------|---------------|------------------|
| `du` / `ncdu`     | No            | No              | No                           | No            | No               |
| `npkill`          | No            | No              | No                           | No            | No               |
| `cargo-sweep`     | No            | No              | No                           | No            | No               |
| `devclean`        | No            | No              | No                           | No            | No               |
| `git-workspace`   | Moves to dir  | No              | No                           | No            | No               |
| `git bundle`      | Yes (manual)  | No              | Refs only                    | No            | No               |
| **`ward`**        | **Yes**       | **Yes**         | **Yes (bundle + extras tar)**| **Yes**       | **Yes**          |

`git-workspace` manages a local workspace as a collection of repositories and can move deleted repos into an archive directory, but does not verify archives before deletion or capture local state beyond the Git graph. Among the tools reviewed, ward is the only one that combines lifecycle classification, clone-verified archival, and lossless local-state capture in a single workflow.

**Strengths.** Archive package centred on a Git bundle for refs and reachable commits, with stashes promoted to temporary refs so they survive bundling. Companion extras tar captures hooks (including `core.hooksPath` externals), per-repo config (`.git/config` and `config.worktree`), and untracked files. Both artefacts are SHA256-hashed and the bundle is clone-verified before the source is removed. Safety proofs show remote reachability, per-branch pushed status, stash count, local-only refs, submodule status, and untracked file count before any destructive action. Worktree planner converts duplicate clones of the same remote into git worktrees. Prototype triage surfaces throwaway experiments as candidates for review, not automatic deletion.

**Weaknesses.** No interactive TUI, keyboard-first stays in scope for a later release. No cloud backend for bundles, archives live at `~/.ward/archives/` only. Worktree conversion is experimental, it pushes local-only branches to the keeper's origin first, so offline-only branches require manual review.

## Install

```
cargo install git-ward
```

Or build from source:

```
git clone https://github.com/michaelmillar/ward.git
cd ward
cargo install --path .
```

## Quick start

```
ward status                         # overview with lifecycle stats
ward scan                           # per-repo verdicts with rationale
ward scan --prototypes              # just the throwaway AI experiments
ward scan --json                    # machine-readable output
ward dedupe                         # cluster duplicate clones, propose worktree plan
ward dedupe --convert               # execute the worktree plan
ward archive                        # dry-run archive of stale repos
ward archive --prototypes --execute # archive the prototype cohort
ward restore                        # list archives with verify status
ward restore <name> --verify        # verify integrity without restoring
ward restore <name>                 # restore to original path
ward clean --older-than 30d         # dry-run artefact sweep
ward clean --execute                # actually remove artefacts
ward sweep --execute                # clean + archive in one pass
ward config init                    # write default config
ward cache-clear                    # reset assessment cache
```

## Commands

### status

Disk usage breakdown plus a lifecycle histogram for every git repo under the path. Shows which repos are ready for `ward archive`.

```
ward status [path]
```

### scan

Runs the decision engine. For each git repo under `path`, produces a verdict.

| Verdict        | Meaning                                                               |
|----------------|-----------------------------------------------------------------------|
| `archive`      | Remote, all refs pushed, no local work, last commit over 90 days ago  |
| `prototype`    | Short lifetime, few commits, one author, remote or not                |
| `worktree`     | Candidate for worktree conversion (used by dedupe)                    |
| `keep`         | Active repo with recent work                                          |
| `local-work`   | Has uncommitted changes, stashes, local-only branches, or unpushed    |
| `no-remote`    | Git repo with no configured remote                                    |

```
ward scan [path] [--prototypes] [--verdict archive|prototype|keep|local-work|no-remote]
```

### dedupe

Finds duplicate clones by canonical remote URL **combined with root commit SHA** (catches forks, mirrors, renamed remotes). Clusters clones, marks the one with the most recent commit as the keeper, and proposes an action for each duplicate.

- **worktree** if the clone has local-only branches, converts via `git worktree add` after pushing the local branches to origin
- **remove** if no local work, clone is safe to delete
- **skip** if dirty or stashed, refuses to touch

```
ward dedupe [path] [--convert]
```

### archive

Assesses each git repo and archives eligible ones as `git bundle` files.

For each archive:
1. Creates `<name>-<timestamp>.bundle` via `git bundle create --all`
2. Runs `git bundle verify` on the resulting file
3. Clones the bundle into a temp directory and compares refs to source
4. Writes `<name>-<timestamp>.json` manifest with SHA256, HEAD, all refs, remotes, commit count, and verification timestamp
5. Captures extras (untracked files, hooks, `.git/config`, `config.worktree`) to a companion `.extras.tar.gz`, verifies contents
6. Only then removes the original directory

```
ward archive [path] [--execute] [--prototypes] [--include-no-remote] [--no-cache] [--json]
```

### restore

Lists archives with verified status. With a name argument, verifies integrity and restores to the original path. Refuses if target exists or hashes mismatch.

```
ward restore [name] [--verify]
```

### clean

Removes regenerable build artefacts. Recognises Rust (`target`), Node (`node_modules`, `dist`, `.next`, `.turbo`, `.vite`, `.parcel-cache`, `.swc`, `.pnpm-store`), Python (`.venv`, `venv`, `__pycache__`, `.ruff_cache`, `.mypy_cache`, `.pytest_cache`, `.tox`, `.nox`), Gradle, CMake, Zig, Dart, Xcode DerivedData.

```
ward clean [path] [--execute] [--older-than <duration>]
```

### sweep

Clean + archive in one pass. Runs the artefact cleanup first, then the archive flow. Shared `--execute` flag.

```
ward sweep [path] [--execute] [--prototypes] [--older-than <duration>]
```

## Configuration

Ward reads `~/.ward/config.toml` if it exists, falling back to sensible defaults.

```
ward config init    # write default config
ward config show    # print effective config
```

### Thresholds

Control how ward classifies repositories.

```toml
[thresholds]
archive_stale_days = 90
prototype_max_commits = 10
prototype_max_authors = 1
prototype_max_lifetime_days = 30
```

### Custom artefact rules

Add your own artefact patterns alongside the 25+ built-in rules.

```toml
[[artefact_rules]]
name = "my-build"
ecosystem = "custom"
requires_sibling = ["Makefile"]
```

### Exclude paths

```toml
[exclude]
paths = ["~/projects/keep-forever"]
```

### Workspace root

Set a default path so you can run `ward status` without arguments.

```toml
[workspace]
root = "~/projects"
```

## Caching

Ward caches repo assessments at `~/.ward/cache.json`, keyed on `.git` directory mtime. Repeat scans skip re-assessment for repos whose git state has not changed.

```
ward cache-clear          # clear cache
ward scan --no-cache      # bypass cache for one run
```

Typical speedup on repeat runs is 3 to 8x depending on workspace size.

## JSON output

All assessment commands support `--json` for scripting, CI integration, and pipeline composition.

```
ward scan --json              # per-repo verdicts as JSON
ward status --json            # disk usage + assessments as JSON
ward archive --prototypes --json  # eligible repos as JSON
ward scan --json | jq '.[].verdict' | sort | uniq -c  # example pipeline
```

## Architecture

### What the archive contains

A ward archive is not just a Git bundle. It is three artefacts.

| Artefact | Contents | Verified by |
|----------|----------|-------------|
| `<name>.bundle` | Refs, commits, trees, blobs, tags, stashes (via temporary refs) | SHA256 + clone-and-compare |
| `<name>.extras.tar.gz` | Untracked files, `.git/config`, `config.worktree`, custom hooks | SHA256 + content verification |
| `<name>.json` | Manifest with all metadata, hashes, and verification timestamps | Human and machine readable |

A plain `git bundle --all` captures refs and reachable objects but **not** the working tree, stash reflog, hooks, or per-repo config. Ward fills those gaps explicitly.

### Safety model

The safety model stacks three checks.

1. **Pre-flight assessment.** Before any action, assess each repo for remote presence, branch pushed status, uncommitted changes, stashes, local-only refs, untracked files, submodules, and worktrees.
2. **Post-archive verification.** After writing a bundle, clone it into a temp directory and compare ref list and HEAD to the source. If anything mismatches, abort and keep the source. This is stronger than `git bundle verify`, which checks bundle validity and prerequisite history but does not simulate a full restore.
3. **Post-restore verification.** Before restoring, re-hash the bundle and extras tar against the manifest. Refuse if hashes have changed since archive time.

Every manifest records `verified_at` and `verifier_version` so you can audit what was checked and when.

### What is not captured

Modified tracked files in the working tree (uncommitted changes to tracked files) are not archived. Ward blocks archival when these are present rather than silently losing them. The index state is also not captured separately since it is derivable from HEAD for a clean working tree.

## Status

Single binary, ~3000 lines of Rust. Pure Rust, no runtime C dependencies (SQLite is bundled for optional pm integration).

Not yet implemented.

- Interactive TUI
- Cloud archive backends (S3, B2)
- Fleet mode for devboxes and CI runners

## Licence

MIT. See [LICENSE](LICENSE).
