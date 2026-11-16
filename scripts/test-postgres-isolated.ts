import {randomBytes} from "node:crypto"
import {readdir,readFile,cp,mkdir,mkdtemp,rm} from "node:fs/promises"
import {tmpdir} from "node:os"
import {resolve,dirname} from "node:path"
// A disposable database/role in the existing Compose PostgreSQL; never drops or
// modifies the application's own database. Every Docker call names its context.
const context=process.env.TEST_DOCKER_CONTEXT??"colima-meeting-notes",container=process.env.TEST_POSTGRES_CONTAINER??"meeting-notes-db-1"
const suffix=randomBytes(6).toString("hex"),name="mn_test_"+suffix,password=randomBytes(24).toString("hex")
const root=resolve(import.meta.dir,"..")
async function run(args:string[],input?:string){const p=Bun.spawn(args,{cwd:root,stdin:input===undefined?"ignore":new Blob([input]),stdout:"pipe",stderr:"pipe"});const[out,err,code]=await Promise.all([new Response(p.stdout).text(),new Response(p.stderr).text(),p.exited]);if(code!==0){console.error((out+err).replaceAll(password,"[test secret]"));throw new Error("Isolated PostgreSQL command failed")}if(args.includes("test"))console.log((out+err).replaceAll(password,"[test secret]"))}
const psql=(database:string,sql:string)=>run(["docker","--context",context,"exec","-i",container,"psql","-U","meeting_notes","-d",database,"-v","ON_ERROR_STOP=1"],sql)
let created=false
try{
 await psql("postgres",'CREATE ROLE "'+name+'" LOGIN PASSWORD \''+password+'\';\nCREATE DATABASE "'+name+'" OWNER "'+name+'";');created=true
 for(const dir of(await readdir(resolve(root,"packages/database/prisma/migrations"))).sort())if(/^\d/.test(dir))await psql(name,await readFile(resolve(root,"packages/database/prisma/migrations",dir,"migration.sql"),"utf8"))
 // Applied by a superuser above; grant only this throwaway role its schema data.
 await psql(name,'GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO "'+name+'"; GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO "'+name+'";')
 const testContainer="mn-tests-"+suffix
 try {
  await run(["docker","--context",context,"run","-d","--name",testContainer,"--network","meeting-notes_default","--entrypoint","sleep","-e","DATABASE_URL=postgresql://"+name+":"+password+"@db:5432/"+name,"-e","RUN_PG_TESTS=1","-e","RUN_AUTH_PG_TESTS=1","meeting-notes:local","600"])
  const staging=await mkdtemp(resolve(tmpdir(),"mn-pg-tests-"))
  try{for(const path of["package.json","bun.lock","apps/api","apps/web/package.json","packages/contracts","packages/database"]){await mkdir(dirname(resolve(staging,path)),{recursive:true});await cp(resolve(root,path),resolve(staging,path),{recursive:true,filter:source=>!source.split("/").includes("node_modules")})};await run(["docker","--context",context,"cp",staging+"/.",testContainer+":/app"])}finally{await rm(staging,{recursive:true,force:true})}
  await run(["docker","--context",context,"exec","-u","0",testContainer,"bun","--no-env-file","install","--frozen-lockfile"])
  // Better Auth's in-memory IP/path rate limiter is shared across engines in a
  // process. Keep real-login suites isolated without relaxing production limits.
  // The notes list benchmark builds a 500-note account and reads an 11 MB response. In the same
  // process as every other file it is killed for memory (exit 137) in a small Docker VM, so it runs alone.
  const separate=["auth-postgres.test.ts","sign-in-postgres.test.ts","mcp-auth-postgres.test.ts","organizations-postgres.test.ts","note-summaries-postgres.test.ts","onboarding-postgres.test.ts","plain-http-postgres.test.ts","google-claim-postgres.test.ts","import-postgres.test.ts"]
  const tests=(await readdir(resolve(root,"apps/api/test"))).filter(name=>name.endsWith(".test.ts")&&!separate.includes(name)).map(name=>"apps/api/test/"+name)
  for(const files of [["apps/api/test/auth-postgres.test.ts"],["apps/api/test/sign-in-postgres.test.ts"],tests,["apps/api/test/note-summaries-postgres.test.ts"],
   // The note import, on its own for the same reason as the benchmark: it sends a 24 MB request of 50,000 segments.
   ["apps/api/test/import-postgres.test.ts"],["apps/api/test/mcp-auth-postgres.test.ts"],["apps/api/test/organizations-postgres.test.ts"],
   // On its own and last: it returns the bootstrap owner to "never claimed" and claims it again.
   ["apps/api/test/onboarding-postgres.test.ts"],
   // Real sign-ins over plain HTTP addresses, on their own for the same reason as the other sign-in files.
   ["apps/api/test/plain-http-postgres.test.ts"],
   // Claims the bootstrap owner with Google and with a password, and returns it to "never claimed" afterwards.
   ["apps/api/test/google-claim-postgres.test.ts"]])
   await run(["docker","--context",context,"exec",testContainer,"bun","--no-env-file","test","--timeout","30000",...files])
 }finally{await run(["docker","--context",context,"rm","-f",testContainer])}
 console.log("Isolated PostgreSQL database tests passed")
}finally{if(created){await psql("postgres",'DROP DATABASE IF EXISTS "'+name+'" WITH (FORCE);\nDROP ROLE IF EXISTS "'+name+'";')}}
