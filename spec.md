# `espm` - ES Module Package Manager

## Concept

- Only for ES Modules
- Avoid `package.json` such as possible
- Reduce config files

## Questions ???

- How to handle workspaces?
- How to download the CLI tool
- How to execute a package like `npx`?

## CLI

`espm init` + (optional) `-y` || `--yes` => init a new package/project

`espm add` + `jsr:@<scope>/<package>@<version>` || `npm:@<scope>/<package>@<version>` || `file://` || `http(s)://` + (optional) `-d` || `--dev` => add a package from the [JavaScript Registry](https://jsr.io) or the [NPM Registry](https://www.npmjs.com) or a local file or a remote URL. If `-d` or `--dev` is specified, the package will be added as dev dependency.

`espm install` + (optional) `-d` || `--dev` => install all dependencies

`espm update` + `specifier` => update a package if possible (NPM or JSR)

`espm remove` + `specifier` => remove a package from the project

`espm publish` + (optional) `--npm` => publish the project to the JSR or if specify NPM registry

`espm setup` + `version` => setup the version of CLI needed for the project

## Download dependencies

- use `espm add` to add a package
- use `espm install` to download all dependencies
- use `espm update` to update a package
- use `espm remove` to remove a package
- by default, the tool will store the downloaded packages in a local cache directory (e.g. `~/.espm/cache`) and create a symlink in the `node_modules` directory of the project

## Files

- `espm.json(c)` - ???

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

## Related links

- [JavaScript Registry](https://jsr.io)
- [NPM Registry](https://www.npmjs.com)
- [Import Maps - MDN](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/script/type/importmap)
- [Import Maps - W3C](https://html.spec.whatwg.org/multipage/webappapis.html#import-maps)
- [`package.json` - Node.js](https://nodejs.org/docs/latest-v22.x/api/packages.html#nodejs-packagejson-field-definitions)
