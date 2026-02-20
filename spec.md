# `espm` - ECMAScript Package Manager

## Concept

- Only for ES Modules (warn when downloading packages that don't have ESM entry points)
- Avoid `package.json` such as possible
- Reduce config files

## Non-Goals

- bundling, transpilation, or other build steps (focus on package management and registry interactions) "package manager" not "build tool"

## Future goals

- npm cli compatibility (e.g., `package.json`, `package-lock.json`) so if project use npm you can just use espm

## Questions ???

- How to download the CLI tool
- How to execute a package like `npx`?

## Commands

`espm init` [`-y`|`--yes`]
  - Action: Initializes a new project, creating an `espm.json` file with default values.
  - Options:
    - `-y`, `--yes`: Skip prompts and use default values for initialization.

`espm add <package_source>` [`-d`|`--dev`]
  - `<package_source>`: The package to add. Must be one of:
    - JSR package: `jsr:@<scope>/<package>[@<version>]` (e.g., `jsr:@std/fs@^0.220.0`)
    - NPM package: `npm:[@<scope>/]<package>[@<version>]` (e.g., `npm:lodash@^4.17.21`, `npm:@types/lodash@^4.17.0`)
    - Local file path: `file:<path_to_package_directory_or_tarball>` (e.g., `file:../my-local-lib`, `file:./pkg.tgz`)
    - Remote URL: `http(s)://<url_to_package_tarball>` (e.g., `https://example.com/my-package.tgz`)
  - Options:
    - `-d`, `--dev`: Add the package as a development dependency. It will be added to `import_map_dev` in `espm.json`.
  - Action: Adds the specified package to the project's `espm.json` file (either to `imports` or `import_map_dev.imports`) and installs it into `node_modules`.
    - For `file:` and `http(s):` dependencies, `espm` infers the dependency key from the package metadata (`package.json` name).
    - `add` refreshes `espm-lock.json` immediately after config changes to keep deterministic reinstall behavior aligned.

`espm install` [`--dev`] [`--force`]
  - Action:
    - Installs dependencies from `espm-lock.json` when the lockfile is present (deterministic install).
    - Falls back to resolving dependencies from `espm.json` when no lockfile is available.
    - Skips reinstalling packages already present in `node_modules` with the expected version.
  - Options:
    - `--dev`: Also includes dependencies from `import_map_dev.imports`.
    - `--force`: Reinstalls packages even when the same version is already installed.
  - Output: Updates/creates `espm-lock.json` after a resolver-based install.

`espm update <package_name>`
  - `<package_name>`: The name of the package to update as it appears as a key in `espm.json`'s import maps (e.g., `lodash`, `@foo/bar`).
  - Action: For JSR or NPM packages, this command checks for the latest compatible version according to the version constraint in `espm.json`. If a newer version is found, it updates the version in `espm.json` and reinstalls the package.
  - `file:` and `http(s):` dependencies are intentionally treated as **not updateable** and are skipped with a warning.

`espm remove <package_name>`
  - `<package_name>`: The name of the package to remove as it appears as a key in `espm.json`'s import maps.
  - Action: Removes the package from `espm.json` (from both `import_map.imports` and `import_map_dev.imports` if present) and removes its symlink from the `node_modules` directory.

`espm publish` [`--npm`]
  - Action: Publishes the project based on the metadata in `espm.json` (like `name`, `version`, `exports`, and `publish` fields).
  - By default, it publishes to the JSR (JavaScript Registry).
  - Options:
    - `--npm`: Publish the project to the NPM registry instead. The `espm.json` should contain NPM-specific metadata if necessary, or `espm` will adapt JSR metadata where possible.

`espm setup <cli_version>`
  - `<cli_version>`: The desired version of the `espm` CLI (e.g., `0.1.0`, `latest`).
  - Action: Configures the project to use a specific version of the `espm` CLI. This might involve updating an `espm_version` field in `espm.json` or setting up a local shim/wrapper for the `espm` command.
  - Blocked: We need to define the publish pipeline of the CLI tool.

## Managing Dependencies

- **Adding Packages**: Use `espm add <package_source>` to add a new dependency. This updates `espm.json` and installs the package. Use the `-d` flag for development-only dependencies.
- **Installing All Dependencies**: Use `espm install` to perform a lockfile-first deterministic install. If no `espm-lock.json` exists, `espm` resolves from `espm.json`, installs packages, and writes `espm-lock.json`.
- **Skipping Existing Packages**: During install, `espm` checks installed package versions in `node_modules` and skips packages already at the expected version.
- **Forcing Reinstall**: Use `espm install --force` to reinstall everything (or everything selected by `--dev`) even if versions already match.
- **Updating Packages**: Use `espm update <package_name>` to upgrade a specific JSR or NPM package to its latest allowed version. This updates `espm.json` and the installed package.
- **Removing Packages**: Use `espm remove <package_name>` to delete a dependency from `espm.json` and remove it from `node_modules`.
- **`node_modules` behavior**: `espm` downloads package tarballs and extracts them into the project's `node_modules` directory.

### Install examples

- `espm install` → lockfile-first install for production dependencies.
- `espm install --dev` → lockfile-first install including development dependencies.
- `espm install --force` → force reinstall of production dependencies.
- `espm install --dev --force` → force reinstall of production + development dependencies.


## Files

- `espm.json(c)` - ???
- `espm-lock.json` - Lockfile containing resolved package versions and tarball URLs used for deterministic installs.
  - For `file:` dependencies, the lockfile stores the resolved local source path used for deterministic reinstall.
  - For `http(s):` dependencies, the lockfile stores the exact tarball URL used for deterministic reinstall.

```jsonc
{
    "name": "@scope/my-project",
    "version": "1.0.0",
    "description": "My project description",
    "license": "MIT",
    // JSR exports
    "exports" : {
        ".": "./index.js",
        "./submodule": "./submodule.js"
    },
    // JSR publish
    "publish": {
        "include": [
            "LICENSE",
            "README.md",
            "src/**/*.ts",
            "jsr.json"
        ],
        "exclude": ["src/**/*.test.ts"]
    },
    // NPM style workspaces if this setup considered a monorepo 
    // So this espm.json is the root of the monorepo and can't be published
    // if this is include cli will thow an error if it's found `publish`, "exports" keys
    "workspaces": [
        "packages/*"
    ],
    "import_map": {
        "imports": {
            "my-lib": "https://cdn.example.com/my-lib.js",
            "@foo/bar": "jsr:@foo/bar@1.0.0",
            "lodash": "npm:lodash@4.17.21"
        }
    },
    "import_map_dev": {
        "imports": {
            "my-dev-lib": "jsr:@foo/my-dev-lib@1.0.0"
        }
    },
    "espm_version": "0.0.0",
}
```

## Missing parts in spec

- How to support other registries like GitHub Packages, private registries, etc.?
- How to handle workspaces and monorepos? Should `espm` support a workspace feature similar to npm/yarn?

## Related links

- [JavaScript Registry](https://jsr.io)
- [NPM Registry](https://www.npmjs.com)
- [Import Maps - MDN](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/script/type/importmap)
- [Import Maps - W3C](https://html.spec.whatwg.org/multipage/webappapis.html#import-maps)
- [`package.json` - Node.js](https://nodejs.org/docs/latest-v22.x/api/packages.html#nodejs-packagejson-field-definitions)
