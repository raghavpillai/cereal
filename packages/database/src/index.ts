import { PrismaPg } from "@prisma/adapter-pg"
import { PrismaClient } from "../generated/client"

/** Internal persistence client. Wire DTOs are defined in @meeting-notes/contracts. */
export function createDatabase(connectionString: string) {
  if (!connectionString) throw new Error("DATABASE_URL is required")
  return new PrismaClient({ adapter: new PrismaPg({ connectionString, connectionTimeoutMillis: 3_000, query_timeout: 5_000 }) })
}

export type Database = ReturnType<typeof createDatabase>
