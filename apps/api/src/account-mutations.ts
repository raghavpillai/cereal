import type {Database} from "@meeting-notes/database"

// Better Auth's adapter uses pooled connections independently of this advisory
// lock transaction. Keep at most one management lock waiter per process so a
// burst of admin requests cannot occupy every pool slot and starve the holder.
// This queue affects account/workspace administration only, never note/audio I/O.
let managementTail:Promise<void>=Promise.resolve()
export async function withAccountMutation<A>(db:Database,key:string,operation:()=>Promise<A>):Promise<A> {
  const previous=managementTail
  let release!:()=>void
  managementTail=new Promise<void>(resolve=>{release=resolve})
  await previous
  try {
    return await db.$transaction(async tx=>{
      await tx.$queryRawUnsafe("SELECT 1 AS locked FROM pg_advisory_xact_lock(hashtext($1))",key)
      return operation()
    },{timeout:15_000})
  } finally { release() }
}
