import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto"
import { constants } from "node:fs"
import { mkdir, open, lstat } from "node:fs/promises"
import { dirname } from "node:path"

export interface SecretBox { seal(value: string): string; open(value: string): string }
export function secretBoxFromKey(key: Uint8Array): SecretBox {
  if (key.length !== 32) throw new Error("Settings encryption key must contain 32 bytes")
  return {
    seal(value) {
      const iv = randomBytes(12), cipher = createCipheriv("aes-256-gcm", key, iv)
      const ciphertext = Buffer.concat([cipher.update(value, "utf8"), cipher.final()])
      return ["v1", iv.toString("base64url"), ciphertext.toString("base64url"), cipher.getAuthTag().toString("base64url")].join(".")
    },
    open(value) {
      const [version, iv, ciphertext, tag, extra] = value.split(".")
      if (version !== "v1" || !iv || !ciphertext || !tag || extra) throw new Error("Stored secret could not be read")
      const decipher = createDecipheriv("aes-256-gcm", key, Buffer.from(iv, "base64url"))
      decipher.setAuthTag(Buffer.from(tag, "base64url"))
      return Buffer.concat([decipher.update(Buffer.from(ciphertext, "base64url")), decipher.final()]).toString("utf8")
    },
  }
}
/** A stable private file key survives owner-token rotation. Back it up with the
 * database; the key itself must not be stored in the database. */
export async function loadSecretBox(path: string): Promise<SecretBox> {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 })
  try {
    const file = await open(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    try { await file.writeFile(randomBytes(32)); await file.sync() } finally { await file.close() }
  } catch (error) { if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error }
  const stat = await lstat(path)
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== 32) throw new Error("Invalid settings encryption key file")
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try { await file.chmod(0o600); return secretBoxFromKey(await file.readFile()) } finally { await file.close() }
}
