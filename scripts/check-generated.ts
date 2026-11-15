import { readdir, readFile } from "node:fs/promises"
import { resolve } from "node:path"

const root = resolve(import.meta.dir, "..")
async function files(directory: string): Promise<string[]> {
  const entries = await readdir(resolve(root, directory), { withFileTypes: true })
  return (await Promise.all(entries.map((entry) => entry.isDirectory() ? files(`${directory}/${entry.name}`) : [`${directory}/${entry.name}`]))).flat().sort()
}
async function snapshot() {
  const paths = [...await files("packages/contracts/generated"), "apps/ios/Packages/Contracts/Sources/Contracts/Generated.swift", "crates/api-types/src/generated.rs"]
  return new Map(await Promise.all(paths.map(async (path) => [path, await readFile(resolve(root, path), "utf8")] as const)))
}
const before = await snapshot()
const process = Bun.spawn(["bun", "run", "--cwd", "packages/contracts", "generate"], { cwd: root, stdout: "inherit", stderr: "inherit" })
if (await process.exited !== 0) throw new Error("Contract generation failed")
const after = await snapshot()
const changed = [...new Set([...before.keys(), ...after.keys()])].filter((path) => before.get(path) !== after.get(path))
if (changed.length) {
  console.error("Generated contracts were out of date. Review and commit the regenerated files:\n" + changed.join("\n"))
  globalThis.process.exit(1)
}
console.log("Generated contracts are current")
