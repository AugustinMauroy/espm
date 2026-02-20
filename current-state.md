# ESPM Current State (Implementation vs Spec)

## Purpose

This file captures the **current implementation status** of `espm` so an LLM (or contributor) can quickly compare it against [`spec.md`](spec.md).

## Snapshot

- CLI structure split into modules: `cli`, `logger`, `models`, `specifier`, `jsr_npm` + command logic in `main`.
- Lockfile used by install: `espm-lock.json`.
- Install is lockfile-first, supports skip-if-installed and `--force` reinstall.
- `publish` now packs the current directory; `--dry-run` writes `espm-publish.tgz` locally and real publish requires `JSR_TOKEN`/`NPM_TOKEN` for network upload. `setup` is still not implemented.

## Command Status Matrix

| Command | Spec Expectation | Current Status | Notes |
|---|---|---|---|
| `init` | create `espm.json` | ✅ Implemented | Creates `espm.json` if missing |
| `add` | add dep to import map (+ dev option) and install | ✅ Implemented | Supports `jsr:` / `npm:` / `file:` / `http(s):`; for `file:` and `http(s):` the package name is inferred from package metadata; lockfile is refreshed after add |
| `install` | install deps (+ `--dev`) | ✅ Implemented (+ `--force`) | Lockfile-first (`espm-lock.json`), fallback resolve from `espm.json`, skip reinstall when version already installed |
| `update <pkg>` | update to latest compatible version and reinstall | ✅ Implemented | Works for JSR and NPM; `file:` and `http(s):` are intentionally treated as not updateable and are skipped with a warning |
| `remove <pkg>` | remove from config and installed tree | ✅ Implemented | Removes from both maps and from `node_modules` (including `file:` and `http(s):` dependencies by package key) |
| `publish` | publish package (JSR/NPM) | Partial | Packs current dir, supports `--dry-run` and requires `JSR_TOKEN`/`NPM_TOKEN` for network publish |
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

### Transactional installs

- `install` now performs a transactional update of `node_modules`:
  - If `./node_modules` exists it is moved to `./node_modules.backup` (or a numbered backup if that path exists) before installation.
  - Installation proceeds normally into a fresh `./node_modules`.
  - On successful completion the backup is removed.
  - If installation fails, the incomplete `./node_modules` is removed and the backup is restored, preserving the prior state.
  - Warnings are logged if backup/restore steps fail; effort is made to avoid data loss.

## Spec Drift / Known Gaps

1. `publish` still only supports minimal dry-run and token-based PUT; full npm/jsr protocol isn't implemented.
2. `setup` remains unimplemented.
3. `update` does not upgrade `file:` or `http(s):` dependencies (intentionally not updateable).
4. `add` currently refreshes via full dependency reinstall; optimization target is affected-graph-only refresh.
5. Spec mentions lockfile concept; implementation uses `espm-lock.json` naming consistently (not `espm.lock`).

## Future Goals

- Finish the full publish workflow, including metadata handling and proper npm/jsr API compatibility so `espm publish` can be relied on in production.
- Decouple from `package.json`; allow `espm.json` to serve as authoritative manifest to support projects without a package.json file.
- Support overriding the JSR registry base URL via an environment variable (JSR is open source and can be self-hosted).
- Provide a comprehensive end-to-end test script that clones the JSR repository, boots a local instance, and exercises publishing against that local registry.
- Provide a script that test that node.js can consume packages installed by `espm` in a clean environment, validating real-world compatibility. (why not try bun too  ? deno can't consume node_module out of the box so don't care)

## Test State

- Unit tests exist in all source files under `src/`.
- E2E tests for CLI commands exist under `tests/`.
- Last observed test run in this workspace: `cargo test -q` passing.
- Last observed QA `cargo clippy -q` with no warnings.

To do a manual test you can execute these commands:
```bash
# Initialize a new package
cargo run -- init
# Add a dependency (example with a JSR package)
cargo run -- add jsr:@augustinmauroy/matrix-n@0.1.0
# Add a dependency (example with an npm package)
cargo run -- add npm:chalk@1.0.0
# Install dependencies
cargo run -- install
# Update a dependency
cargo run -- update @augustinmauroy/matrix-n
```