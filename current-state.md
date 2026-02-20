# ESPM Current State (Implementation vs Spec)

## Purpose

This file captures the **current implementation status** of `espm` so an LLM (or contributor) can quickly compare it against [`spec.md`](spec.md).

## Snapshot

- CLI structure split into modules: `cli`, `logger`, `models`, `specifier`, `jsr_npm` + command logic in `main`.
- Lockfile used by install: `espm-lock.json`.
- Install is lockfile-first, supports skip-if-installed and `--force` reinstall.
- `publish` and `setup` commands are declared but still not implemented.

## Command Status Matrix

| Command | Spec Expectation | Current Status | Notes |
|---|---|---|---|
| `init` | create `espm.json` | ✅ Implemented | Creates `espm.json` if missing |
| `add` | add dep to import map (+ dev option) and install | ✅ Implemented | Supports `jsr:` / `npm:` / `file:` / `http(s):`; for `file:` and `http(s):` the package name is inferred from package metadata; lockfile is refreshed after add |
| `install` | install deps (+ `--dev`) | ✅ Implemented (+ `--force`) | Lockfile-first (`espm-lock.json`), fallback resolve from `espm.json`, skip reinstall when version already installed |
| `update <pkg>` | update to latest compatible version and reinstall | ✅ Implemented | Works for JSR and NPM; `file:` and `http(s):` are intentionally treated as not updateable and are skipped with a warning |
| `remove <pkg>` | remove from config and installed tree | ✅ Implemented | Removes from both maps and from `node_modules` (including `file:` and `http(s):` dependencies by package key) |
| `publish` | publish package (JSR/NPM) | ❌ Not implemented | Emits warning at runtime |
| `setup` | pin/setup CLI version | ❌ Not implemented | Emits warning at runtime |

## Dependency Source Support

| Source Type | Spec | Current Status | Notes |
|---|---|---|---|
| `jsr:@scope/name@version` | required | ✅ Implemented | Uses npm.jsr metadata and tarball |
| `npm:name@version` / `npm:@scope/name@version` | required | ✅ Implemented | Scoped + unscoped supported |
| `file:...` | listed in spec | ✅ Implemented | Supports local directory and local `.tgz` package installs |
| `http(s)://...` | listed in spec | ✅ Implemented | Supports remote `.tgz` package installs |

## Lockfile Behavior

- Preferred lockfile path in implementation: `espm-lock.json`.
- `install` behavior:
  - if lockfile exists: install from lockfile tarball URLs;
  - otherwise: resolve from `espm.json`, install, then write lockfile.
- `--force` bypasses skip checks and reinstalls packages.
- for `file:` dependencies lockfile stores resolved local source path used for deterministic reinstall.
- for `http(s):` dependencies lockfile stores exact tarball URL used for deterministic reinstall.

## Install Behavior Details

- `install` checks `node_modules/<pkg>/package.json` version and skips package when expected version matches.
- Works for scoped and unscoped package layout.
- Transitive dependencies from package metadata are resolved and installed.
- Resolver applies semver compatibility for selected versions.

## Spec Drift / Known Gaps

1. `publish` and `setup` remain unimplemented.
2. `update` does not upgrade `file:` or `http(s):` dependencies (intentionally not updateable).
3. `add` currently refreshes via full dependency reinstall; optimization target is affected-graph-only refresh.
4. Spec mentions lockfile concept; implementation uses `espm-lock.json` naming consistently (not `espm.lock`).

## Code Organization (Current)

- `src/cli.rs`: CLI commands/options.
- `src/logger.rs`: console logger helpers.
- `src/models.rs`: core config/install data structures.
- `src/specifier.rs`: specifier parsing and helper builders.
- `src/jsr_npm.rs`: registry response DTOs.
- `src/main.rs`: orchestration and command handlers.

## Test State

- Unit tests exist in all source files under `src/`.
- Last observed test run in this workspace: `cargo test -q` passing.
