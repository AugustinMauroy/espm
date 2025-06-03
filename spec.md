# `espm` - ES Module Package Manager

## Concept

- Only for ES Modules
- Avoid `package.json` such as possible
- Reduce config files

## Questions ???

- How to store the import map, knowing that you need dependence and dev dependence?
- How to store the package information, knowing that you need to store the version and the scope?
- How to download the CLI tool
- How to handle workspaces?
- How to execute a package like `npx`?

## CLI

`espm init` + (optional) `-y` || `--yes` => init a new package/project

`espm add` + `jsr:@<scope>/<package>@<version>` || `npm:@<scope>/<package>@<version>` || `file://` || `http(s)://` + (optional) `-d` || `--dev` => add a package from the [JavaScript Registry](https://jsr.io) or the [NPM Registry](https://www.npmjs.com) or a local file or a remote URL. If `-d` or `--dev` is specified, the package will be added as dev dependency.

`espm install` + (optional) `-d` || `--dev` => install all dependencies

`espm update` + `specifier` => update a package if possible (NPM or JSR)

`espm remove` + `specifier` => remove a package from the project

`espm publish` + (optional) `--npm` => publish the project to the JSR or if specify NPM registry

`espm setup` + `version` => setup the version of CLI needed for the project

## Files

- `espm.json` - ???
- `jsr.json(c)` - JavaScript Registry configuration file (https://jsr.io)
- `package.json` - Node.js related files E.G. `scripts`, `type`
- `import_map.json` - Import map for the project

## Related links

- [JavaScript Registry](https://jsr.io)
- [NPM Registry](https://www.npmjs.com)
- [Import Maps - MDN](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/script/type/importmap)
- [Import Maps - W3C](https://html.spec.whatwg.org/multipage/webappapis.html#import-maps)
- [`package.json` - Node.js](https://nodejs.org/docs/latest-v22.x/api/packages.html#nodejs-packagejson-field-definitions)