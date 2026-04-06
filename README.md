<p align="center">
  <img src="assets/logo.svg" width="200" alt="ward">
</p>

<h3 align="center">A git workspace lifecycle manager with provable safety</h3>

<p align="center">
  Classify, consolidate, bundle-archive, and restore git repositories.<br>
  Dry-run by default. Every destructive action emits a safety proof and every archive is verified by clone.
</p>

---

<p align="center">
  <img src="assets/demo.svg" width="800" alt="ward in action">
</p>

---

## What it does

`ward` treats your development workspace as a fleet of git repositories with lifecycles. It classifies each repo into one of six verdicts, archives stale repos as verified `git bundle` files with a manifest, consolidates duplicate clones into worktrees, and sweeps out build artefacts. The differentiator is **safety by evidence**. Every archive is verified by cloning the bundle into a temporary directory and comparing refs before the original is removed.

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

Most developer cleanup tools answer *"which files can I delete"*. Ward answers a different question. *"Which git repositories can I safely remove, and can you prove it?"*

| Tool              | Bundle-based archive | Verify-by-clone | Worktree dedupe | Safety proofs | Rapid AI-prototype reclaim |
|-------------------|----------------------|-----------------|-----------------|---------------|----------------------------|
| `du` / `ncdu`     | No                   | No              | No              | No            | No                         |
| `npkill`          | No                   | No              | No              | No            | Partial (node_modules)     |
| `cargo-sweep`     | No                   | No              | No              | No            | Partial (Rust)             |
| `devclean`        | No                   | No              | No              | No            | Partial (known caches)     |
| `git bundle`      | Yes (primitive)      | No              | No              | No            | No                         |
| **`ward`**        | **Yes**              | **Yes**         | **Yes**         | **Yes**       | **Yes**                    |

**Strengths.** Bundle-based archival preserves full git history in a single verifiable file, roughly 30 to 70% smaller than tarring the working tree. Safety proofs show per-repo remote reachability, ref pushed status, stash count, uncommitted files, and local-only refs **before** any destructive action. Worktree planner converts duplicate clones of the same remote into git worktrees so you keep your branches without paying 4x disk cost. AI-prototype detection flags throwaway experiments (low commit count, short lifetime, single author) as a cohort you can bundle and remove in one command.

**Weaknesses.** No interactive TUI, keyboard-first stays in scope for a later release. No cloud backend for bundles, archives live at `~/.ward/archives/` only. Worktree conversion is experimental, it pushes local-only branches to the keeper's origin first, so offline-only branches require manual review.

## Install

Build from source.

```
git clone git@github.com:michaelmillar/ward.git
cd ward
cargo build --release
ln -s $(pwd)/target/release/ward ~/.local/bin/ward
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
5. Captures untracked-not-ignored files to a companion `.untracked.tar.gz` if any
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

Ward uses `git bundle` as the archival primitive instead of tarballs. A bundle is a single file containing delta-compressed objects, refs, and config. It can be cloned from directly (`git clone repo.bundle newdir`) and verified without restoring (`git bundle verify`). This is roughly 30 to 70% smaller than tarring a working tree and it preserves branches, tags, and history as first-class citizens.

The safety model stacks three checks.

1. **Pre-flight assessment.** Before any action, assess each repo for remote presence, branch pushed status, uncommitted changes, stashes, local-only refs, untracked files, and worktrees.
2. **Post-archive verification.** After writing a bundle, clone it into a temp directory and compare ref list and HEAD to the source. If anything mismatches, abort and keep the source.
3. **Post-restore verification.** Before restoring, re-hash the bundle and confirm against the manifest. Refuse if the hash has changed since archive time.

Every manifest records `verified_at` and `verifier_version` so you can audit what was checked and when.

## Status

Single binary, 2800 lines of Rust. Pure Rust, no runtime C dependencies (SQLite is bundled for optional pm integration).

Not yet implemented.

- Interactive TUI
- Cloud archive backends (S3, B2)
- Fleet mode for devboxes and CI runners

## Licence

Private. Not currently published.
