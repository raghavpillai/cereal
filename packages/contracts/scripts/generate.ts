import { mkdir, writeFile } from "node:fs/promises"
import { resolve } from "node:path"
import { Schema } from "effect"
import { OpenApi } from "effect/http-api"
import { AcronymStyleOptions } from "quicktype-core/dist/esm/support/Acronyms.js"
import { InputData, JSONSchemaInput, JSONSchemaStore, quicktype } from "quicktype-core"
import { MeetingsApi } from "../src/api"
import { WireSchemas } from "../src/index"
import { LiveWireSchemas } from "../src/live"
import { ChatWireSchemas } from "../src/chats"
import { TeamSpaceWireSchemas } from "../src/team-space"
import { NoteAccessWireSchemas } from "../src/note-access"
import { pruneObsoleteSchemas } from "./generated-artifacts"

const root = resolve(import.meta.dir, "../../..")
const generated = resolve(root, "packages/contracts/generated")
await mkdir(resolve(generated, "json-schema"), { recursive: true })
// The live transcription socket's messages (src/live.ts) are generated with the HTTP DTOs, one root per message.
const documents = Object.entries({ ...WireSchemas, ...LiveWireSchemas, ...ChatWireSchemas, ...TeamSpaceWireSchemas, ...NoteAccessWireSchemas }).map(([name, schema]) => ({ name, document: Schema.toJsonSchemaDocument(schema) }))
for (const { name, document } of documents) {
  await writeFile(resolve(generated, "json-schema", `${name}.json`), JSON.stringify({ $schema: "https://json-schema.org/draft/2020-12/schema", ...document.schema, $defs: document.definitions }, null, 2) + "\n")
}
await writeFile(resolve(generated, "openapi.json"), JSON.stringify(OpenApi.fromApi(MeetingsApi), null, 2) + "\n")

// Publish the original draft-2020-12 schema. Normalize only the codegen input
// to draft-07 and plain strings so every client uses UUID/ISO wire strings.
function forCodegen(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(forCodegen)
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).filter(([key]) => key !== "format").map(([key, entry]) => [key, key === "$ref" && typeof entry === "string" ? entry.replace("#/$defs/", "#/definitions/") : forCodegen(entry)]))
  return value
}
const address = "https://meeting-notes.invalid/wire.json"
const combined = forCodegen({
  $schema: "http://json-schema.org/draft-07/schema#",
  definitions: Object.assign({}, ...documents.map(({ document }) => document.definitions)),
}) as Record<string, unknown>
class MemorySchemaStore extends JSONSchemaStore {
  async fetch(uri: string) { return uri === address ? combined : undefined }
}
const schemaInput = new JSONSchemaInput(new MemorySchemaStore())
for (const { name } of documents) {
  await schemaInput.addSource({ name, uris: [`${address}#/definitions/${name}`] })
}
const inputData = new InputData()
inputData.addInput(schemaInput)
const swift = await quicktype({ inputData, lang: "swift", rendererOptions: { "access-level": "public", "sendable": "true", "struct-or-class": "struct", "initializers": "false", "acronym-style": AcronymStyleOptions.Original, "just-types": "false" } })
const rust = await quicktype({ inputData, lang: "rust", rendererOptions: { "visibility": "public", "derive-debug": "true", "derive-clone": "true", "derive-partial-eq": "true", "skip-serializing-none": "false" } })
const banner = "// Generated from packages/contracts/src Effect Schema definitions. Do not edit by hand.\n"
const swiftPath = resolve(root, "apps/ios/Packages/Contracts/Sources/Contracts/Generated.swift")
const rustPath = resolve(root, "crates/api-types/src/generated.rs")
await mkdir(resolve(swiftPath, ".."), { recursive: true })
await mkdir(resolve(rustPath, ".."), { recursive: true })
// Preserve the wire distinction between required nullable fields and omitted
// optional keys. Codable's synthesized encodeIfPresent and a global Serde skip
// option otherwise erase that distinction. This derives from schema metadata.
type ObjectSchema = { properties?: Record<string, unknown>; required?: Array<string> }
const objects = new Map(documents.map(({ name, document }) => [name, document.definitions[name] as ObjectSchema]))
let swiftSource = swift.lines.join("\n")
// Quicktype recognizes acronyms such as MCP and may capitalize a schema's
// `Mcp` prefix even with original acronym style. Target its emitted symbol,
// rather than assuming the schema identifier is also the Swift spelling.
const swiftSymbols = new Map([...swiftSource.matchAll(/^public struct (\w+):/gm)].map(match => [match[1]!.toLowerCase(), match[1]!]))
for (const [name, schema] of objects) {
  if (!schema?.properties) continue
  const properties = Object.entries(schema.properties)
  const required = new Set(schema.required ?? [])
  const needsExplicitNull = properties.some(([key, value]) => required.has(key) && JSON.stringify(value).includes('"type":"null"'))
  if (!needsExplicitNull) continue
  const swiftName = swiftSymbols.get(name.toLowerCase())
  if (!swiftName) throw new Error(`Missing generated Swift object for ${name}`)
  const keys = properties.map(([key]) => `        case ${key}`).join("\n")
  // Quicktype renames a property that would collide with a Swift name (`prefix` becomes
  // `storageSettingsDTOPrefix`), so read the property each wire key maps to from the struct itself.
  const start = swiftSource.search(new RegExp(`^public struct ${swiftName}:`, "m"))
  const body = swiftSource.slice(start, swiftSource.indexOf("\n}\n", start))
  const property = (key: string) => new RegExp(`case (\\w+) = "${key}"`).exec(body)?.[1] ?? key
  const encode = properties.map(([key]) => `        try container.${required.has(key) ? "encode" : "encodeIfPresent"}(${property(key)}, forKey: .${key})`).join("\n")
  swiftSource += `\n\nextension ${swiftName} {\n    private enum ContractWireKeys: String, CodingKey {\n${keys}\n    }\n    public func encode(to encoder: Encoder) throws {\n        var container = encoder.container(keyedBy: ContractWireKeys.self)\n${encode}\n    }\n}\n`
}
// A key that may be left out of the JSON may be left out of the memberwise initializer too (`= nil`), so
// adding an optional field to a contract never breaks Swift code that builds the type by hand.
for (const [name, schema] of objects) {
  if (!schema?.properties) continue
  const required = new Set(schema.required ?? [])
  const optional = Object.keys(schema.properties).filter(key => !required.has(key))
  const swiftName = swiftSymbols.get(name.toLowerCase())
  if (!optional.length || !swiftName) continue
  const start = swiftSource.search(new RegExp(`^public struct ${swiftName}:`, "m")), end = swiftSource.indexOf("\n}\n", start)
  let body = swiftSource.slice(start, end)
  const signature = /^    public init\(.*\) \{$/m.exec(body)?.[0]
  if (!signature) continue
  let defaulted = signature
  for (const key of optional) {
    const property = new RegExp(`case (\\w+) = "${key}"`).exec(body)?.[1] ?? key
    defaulted = defaulted.replace(new RegExp(`([(, ]${property}: [^,)]+\\?)(?=, |\\) \\{$)`), "$1 = nil")
  }
  body = body.replace(signature, defaulted)
  swiftSource = swiftSource.slice(0, start) + body + swiftSource.slice(end)
}
// A generated type with the name of a SwiftUI type makes every `@State` or `View` in a file that imports
// Contracts ambiguous, and only an iPhone build shows it. Name the schema instead (see OnboardingStepState).
const swiftUINames = new Set(["State", "Binding", "View", "Text", "Color", "Image", "Button", "Environment", "Label", "Link", "List", "Section", "Group", "Form", "Menu", "Picker", "Toggle", "Font", "Animation", "Shape", "Path", "Alignment", "Scene", "App", "Observable", "Transaction", "Namespace"])
const colliding = [...swiftSource.matchAll(/^public (?:enum|struct|class) (\w+)\b/gm)].map(match => match[1]!).filter(name => swiftUINames.has(name))
if (colliding.length) throw new Error(`Generated Swift types collide with SwiftUI: ${colliding.join(", ")}. Give the schema an identifier.`)
let structName = ""
const snake = (name: string) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase()
const rustSource = rust.lines.join("\n").replace(/\b([A-Z]\w*)Dto\b/g, "$1DTO").split("\n").flatMap((line) => {
  const declaration = line.match(/^pub struct (\w+) \{/)
  if (declaration) structName = declaration[1]!
  const field = line.match(/^    pub (\w+): Option</)
  const schema = objects.get(structName)
  const optional = schema && Object.keys(schema.properties ?? {}).some((key) => snake(key) === field?.[1] && !(schema.required ?? []).includes(key))
  return field && optional ? ['    #[serde(skip_serializing_if = "Option::is_none")]', line] : [line]
}).join("\n")
await writeFile(swiftPath, banner + swiftSource.trimEnd() + "\n")
await writeFile(rustPath, banner + rustSource.trimEnd() + "\n")
await pruneObsoleteSchemas(resolve(generated, "json-schema"), documents.map(({ name }) => name))
console.log(`Generated OpenAPI, ${documents.length} JSON Schemas, Swift Codable/Sendable DTOs, and Rust Serde DTOs`)
