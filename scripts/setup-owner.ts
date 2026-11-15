import {Schema} from "effect"
import {CreateUserInput,UserDTO} from "@meeting-notes/contracts"
// Supply OWNER_PASSWORD via a password manager's environment injection, such as
// op run. Never accept passwords in command arguments or print request bodies.
const token=process.env.ADMIN_API_TOKEN,base=new URL(process.env.PUBLIC_BASE_URL??"http://localhost:3000")
if(!token||token.length<32)throw new Error("Configure the operator ADMIN_API_TOKEN first")
if(base.protocol!=="https:"&&!(base.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(base.hostname)))throw new Error("Owner initialization requires HTTPS or loopback")
const decoded=Schema.decodeUnknownExit(CreateUserInput)({name:process.env.OWNER_NAME,email:process.env.OWNER_EMAIL,password:process.env.OWNER_PASSWORD})
if(decoded._tag==="Failure")throw new Error("Supply valid OWNER_NAME, OWNER_EMAIL and OWNER_PASSWORD through protected environment injection; password must be 12–128 characters")
const response=await fetch(new URL("/api/v1/admin/owner/setup",base),{method:"POST",redirect:"error",headers:{Authorization:"Bearer "+token,"Content-Type":"application/json"},body:JSON.stringify(decoded.value),signal:AbortSignal.timeout(30000)})
if(response.status===409)throw new Error("The owner account has already been initialized; sign in normally")
if(!response.ok)throw new Error("Owner initialization failed with HTTP "+response.status)
Schema.decodeUnknownSync(UserDTO)(await response.json())
console.log("Owner initialized. Sign in at "+new URL("/login",base).href)
