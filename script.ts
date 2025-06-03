import fs from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { env } from 'node:process';
import { styleText } from 'node:util';
import { setGlobalDispatcher, ProxyAgent } from 'undici';
import { extract } from 'tar';

const logger = {
    info: (message) => console.log(`${styleText(["cyan", 'bold'], "[INFO]")} ${message}`),
    warn: (message) => console.warn(`${styleText(["yellow", 'bold'], "[WARN]")} ${message}`),
    error: (message) => console.error(`${styleText(["red", 'bold'], "[ERROR]")} ${message}`),
    debug: (message) => console.debug(`${styleText(["blue", 'bold'], "[DEBUG]")} ${message}`),
    success: (message) => console.log(`${styleText(["green", 'bold'], "[SUCCESS]")} ${message}`),
}

/**
 * @typedef {Object} ImportMap
 * @property {Object.<string, string>} imports - A mapping of module specifiers to their corresponding URLs.
 * @property {Object.<string, Object.<string, string>>} [scopes] - An optional mapping of scopes to module specifiers and their URLs.
 * @description
 * An import map is a JSON object that defines how module specifiers are resolved to URLs.
 * It allows developers to control the resolution of module imports in JavaScript applications.
 * Import maps are particularly useful for managing dependencies in a modular way, enabling the use of custom module paths or CDN URLs.
 * This import map structure is used to define the mapping of module specifiers to their corresponding URLs,
 * allowing for flexible and controlled module resolution in JavaScript applications.
 * @example
 * ```js
 * const importMap = {
 *    imports: {
 *      "example-module": "https://cdn.example.com/example-module@1.0.0/index.js",
 *      "another-module": "https://cdn.example.com/another-module@2.0.0/index.js"
 *   },
 *  scopes: {
 *     "@my-scope/": {
 *      "my-module": "https://cdn.example.com/@my-scope/my-module@1.0.0/index.js",
 *      "my-other-module": "https://cdn.example.com/@my-scope/my-other-module@2.0.0/index.js"
 *   }
 * }
 * 
 * const pkg = await import("example-module");
 * const pkg2 = await import("@my-scope/my-module");
 * ```
 */
type ImportMap = {
    imports: {
        [key: string]: string;
    };
    scopes?: {
        [scope: string]: {
            [key: string]: string;
        };
    };
};

const PACKAGE_SCOPE = "@am";
const PACKAGE_NAME = "neuralnetwork";
const PACKAGE_VERSION = "1.0.0";

/**
 * This hacky way to set the global dispatcher for undici
 * so if the app is running behind a proxy, it will use the proxy
 */
if (env.HTTP_PROXY) {
	const dispatcher = new ProxyAgent({
		uri: new URL(env.HTTP_PROXY).toString(),
	});
	setGlobalDispatcher(dispatcher);
}

/**
 * 
 * @returns {Promise<ImportMap>} A promise that resolves to the import map object.
 * @throws {Error} If there is an error reading or parsing the import map file.
 */
async function readImportMap(): Promise<ImportMap> {
    const raw = await fs.readFile('./import_map.json', 'utf-8');
    return JSON.parse(raw);
}

/**
 * Converts a JSR package name to an npm package name.
 *  For example, converts `@jsr/luca-flag` to `@jsr/luca__flag`.
 * @param {string} scope - The scope of the JSR package.
 * @param {string} name - The name of the JSR package.
 * @returns {string} The corresponding npm package name.
 */
function jsrPackage2npmPackage(scope, name) {
    return "@jsr/" + scope.replace(/-/g, "__").replace(/@/g, "") + "__" + name.replace(/-/g, "__");
}

/**
 * Downloads a tarball from the given URL and saves it to the current directory.
 * @param {string} tarballUrl - The URL of the tarball to download.
 * @returns {Promise<void>} A promise that resolves when the download is complete.
 */
async function downloadTarball(tarballUrl, scope, name) {
    const response = await fetch(tarballUrl);
    if (!response.ok) {
        throw new Error(`Failed to download tarball: ${response.status} ${response.statusText}`);
    }

    // Ensure the directory exists
    const packageDir = `./node_modules/${scope}/${name}`;
    await createDirectoryIfNotExists(packageDir);
    // Extract directly from the response stream, do not keep the .tgz file
    if (!response.body) {
        throw new Error("Response body is null");
    }

    await pipeline(
        response.body,
        extract({ cwd: packageDir, strip: 1 })
    );
}

/**
 * Creates a directory if it does not exist.
 * @param {string} dir - The directory path to create.
 * @returns {Promise<void>} A promise that resolves when the directory is created.
 * If the directory already exists, it resolves immediately.
 * @throws {Error} If there is an error creating the directory.
 */
async function createDirectoryIfNotExists(dir) {
    try {
        await fs.mkdir(dir, { recursive: true });
    } catch (error) {
        if (error.code !== 'EEXIST') {
            throw error;
        }
    }
}

const API_URL = `https://npm.jsr.io/${jsrPackage2npmPackage(PACKAGE_SCOPE, PACKAGE_NAME)}`;

let isFetched = false;
for (let i = 0; i < 10 && !isFetched; i++) {
    try {
        const response = await fetch(API_URL);

        const data = await response.json();
        const versionData = data.versions[PACKAGE_VERSION];
        if (!versionData) {
            throw new Error('Version not found');
        }
        const tarballUrl = versionData.dist.tarball;
        await downloadTarball(tarballUrl, PACKAGE_SCOPE, PACKAGE_NAME);
        logger.success(`Downloaded tarball for version ${styleText(["magenta",  "bold"],PACKAGE_VERSION)}`);
        isFetched = true;
    } catch (error) {
        if (error.message === 'Version not found') {
            logger.error(`Version ${styleText(["magenta",  "bold"],PACKAGE_VERSION)} not found for package ${styleText(["magenta",  "bold"],PACKAGE_NAME)}`);
            break;
        }
        logger.error(error);
    }
}
