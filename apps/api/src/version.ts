import { readFileSync } from "node:fs"
import { resolve } from "node:path"

/** A product version as it appears in file names and in the downloads answer: three numbers, then an
 * optional pre-release or build suffix. Nothing in it can be a path separator. */
export const APP_VERSION_PATTERN = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/
const valid = (value: unknown): value is string => typeof value === "string" && value.length <= 40 && APP_VERSION_PATTERN.test(value)

/** The repository's root package.json: the one place the product version is written for the server.
 * The desktop and iPhone apps carry the same number (`scripts/check-version.ts` fails when they differ),
 * and the Docker image copies this file to /app/package.json. */
export const ROOT_PACKAGE_PATH = resolve(import.meta.dir, "../../../package.json")

/** The version of Cereal this server is: `version` in the root package.json, read once at start-up.
 * This is the product version ("0.1.0"), not the API version in the health answer ("v1").
 * `APP_VERSION` replaces it for tests only; a deployment never sets it. */
export function readAppVersion(options: { env?: Record<string, string | undefined>; packagePath?: string } = {}): string {
  const override = (options.env ?? process.env).APP_VERSION
  if (override !== undefined && override !== "") {
    if (!valid(override)) throw new Error("APP_VERSION must look like 1.2.3")
    return override
  }
  const path = options.packagePath ?? ROOT_PACKAGE_PATH
  let version: unknown
  try { version = (JSON.parse(readFileSync(path, "utf8")) as { version?: unknown }).version }
  catch { throw new Error(`Cannot read the product version from ${path}`) }
  if (!valid(version)) throw new Error(`${path} must carry the product version, such as "version": "1.2.3"`)
  return version
}
