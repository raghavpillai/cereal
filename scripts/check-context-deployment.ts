import {randomBytes} from "node:crypto"
// Exercises only fresh synthetic notes and an unbound instance ICS occurrence.
// Real model calls require the same explicit opt-in as the other live probes.
const token=process.env.ADMIN_API_TOKEN,base=new URL(process.env.CEREAL_SERVER_URL??"http://127.0.0.1:3000")
if(!token)throw new Error("Configure the operator credential privately before running the probe")
if(base.protocol!=="https:"&&!(base.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(base.hostname)))throw new Error("Use HTTPS or loopback for the verification server")
const headers={Authorization:"Bearer "+token,"Content-Type":"application/json"},created:string[]=[]
async function request(path:string,method="GET",body?:unknown,expected=200){const r=await fetch(new URL(path,base),{method,headers,redirect:"error",signal:AbortSignal.timeout(120000),...(body===undefined?{}:{body:JSON.stringify(body)})});if(r.status!==expected)throw new Error("Context verification failed with HTTP "+r.status);return r.json()}
try{
 const calendars=await request("/api/v1/calendars"),ids=new Set(calendars.filter((c:any)=>c.enabled&&c.provider==="ics").map((c:any)=>c.id)),now=new Date(),end=new Date(now.getTime()+89*86400000)
 const events=await request("/api/v1/calendar/events?"+new URLSearchParams({from:now.toISOString(),to:end.toISOString()})),event=events.find((e:any)=>ids.has(e.calendarId)&&e.noteId===null)
 if(!event)throw new Error("Verification requires an unbound upcoming ICS event; no existing note will be modified")
 const note=await request("/api/v1/notes","POST",{title:"New note",body:"Synthetic context verification. No microphone or system recording was used."});created.push(note.id)
 await request("/api/v1/notes/"+note.id+"/context","PUT",{eventId:event.id})
 const loaded=await request("/api/v1/notes/"+note.id);if(loaded.title!==event.title||loaded.recordingState!=="idle")throw new Error("Default title or capture state was not preserved correctly")
 const givenName="Mira"+randomBytes(3).toString("hex"),displayName=givenName+" Vale",person={id:crypto.randomUUID(),displayName,givenName,familyName:"Vale"}
 await request("/api/v1/notes/"+note.id+"/context/participants","POST",person);await request("/api/v1/notes/"+note.id+"/context/participants","POST",person)
 const reopened=await request("/api/v1/calendar/events/"+event.id+"/note","POST");if(reopened.id!==note.id)throw new Error("Upcoming created a duplicate note")
 const context=await request("/api/v1/notes/"+note.id+"/context");if(context.meeting.participants.filter((p:any)=>p.id===person.id).length!==1)throw new Error("Manual attendees were lost or duplicated")
 const other=await request("/api/v1/notes","POST",{title:"Synthetic conflict check",body:"This content must not be merged."});created.push(other.id)
 await request("/api/v1/notes/"+other.id+"/context","PUT",{eventId:event.id},409)
 const live=process.env.RUN_LIVE_PROVIDER_TESTS==="1",question={message:"Return the explicitly supplied display names of scheduled invitees for this note, without claiming attendance. If none are supplied, answer UNKNOWN.",history:[]}
 if(live){const reply=await request("/api/v1/notes/"+note.id+"/chat","POST",question);if(!reply.answer.toLowerCase().includes(givenName.toLowerCase()))throw new Error("The real model did not receive the explicit attendee")}
 await request("/api/v1/notes/"+note.id+"/context","PUT",{eventId:null})
 if((await request("/api/v1/notes/"+note.id+"/context")).meeting!==null)throw new Error("Cleared context reappeared")
 if(live){const reply=await request("/api/v1/notes/"+note.id+"/chat","POST",question);if(reply.answer.toLowerCase().includes(givenName.toLowerCase()))throw new Error("Cleared attendee metadata still reached chat")}
 console.log(JSON.stringify({deployedContext:"passed",defaultTitle:"passed",upcomingReuse:"passed",manualRetry:"passed",conflict409:"passed",clear:"passed",realProvider:live?"passed":"not requested",recording:"not used"}))
}finally{
 for(const id of created)await request("/api/v1/notes/"+id,"DELETE")
 if(created.length)console.log("Synthetic context verification notes cleaned")
}
