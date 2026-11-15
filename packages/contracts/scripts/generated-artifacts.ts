import { readdir, unlink } from "node:fs/promises"
import { join } from "node:path"

/** The generator owns regular JSON files in this directory, and nothing else. */
export async function pruneObsoleteSchemas(directory: string, schemaNames: ReadonlyArray<string>) {
  if (schemaNames.some((name) => !/^[A-Za-z][A-Za-z0-9_]*$/.test(name))) {
    throw new Error("Invalid generated schema identifier")
  }
  const expected = new Set(schemaNames.map((name) => `${name}.json`))
  const entries = await readdir(directory, { withFileTypes: true })
  const obsolete = entries.filter((entry) => entry.isFile() && entry.name.endsWith(".json") && !expected.has(entry.name))
  await Promise.all(obsolete.map((entry) => unlink(join(directory, entry.name))))
}
