import { expect, test } from "bun:test"
import { mkdtemp, mkdir, readFile, rm, stat, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pruneObsoleteSchemas } from "../scripts/generated-artifacts"

test("removed contracts lose stale schema artifacts without deleting unrelated files or following links", async () => {
  const directory = await mkdtemp(join(tmpdir(), "meeting-contract-artifacts-"))
  try {
    await writeFile(join(directory, "CurrentDTO.json"), "current")
    await writeFile(join(directory, "RemovedDTO.json"), "obsolete")
    await writeFile(join(directory, "notes.txt"), "unrelated")
    await mkdir(join(directory, "directory.json"))
    await symlink(join(directory, "notes.txt"), join(directory, "linked.json"))
    await pruneObsoleteSchemas(directory, ["CurrentDTO"])
    expect(await readFile(join(directory, "CurrentDTO.json"), "utf8")).toBe("current")
    expect(await Bun.file(join(directory, "RemovedDTO.json")).exists()).toBe(false)
    expect(await readFile(join(directory, "notes.txt"), "utf8")).toBe("unrelated")
    expect(await readFile(join(directory, "linked.json"), "utf8")).toBe("unrelated")
    expect((await stat(join(directory, "directory.json"))).isDirectory()).toBe(true)
    await pruneObsoleteSchemas(directory, ["CurrentDTO"])
    expect(await readFile(join(directory, "CurrentDTO.json"), "utf8")).toBe("current")
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test("schema identifiers cannot escape their generated directory", async () => {
  await expect(pruneObsoleteSchemas("unused", ["../Outside"])).rejects.toThrow("Invalid generated schema identifier")
})
