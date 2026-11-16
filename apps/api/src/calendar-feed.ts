import { createHash } from "node:crypto"
import { lookup } from "node:dns/promises"
import { BlockList, isIP } from "node:net"
import { get, Agent } from "node:https"
import { connect } from "node:tls"
import ical from "node-ical"
import type { StoredCalendarEvent } from "./calendar-store"

export const MAX_FEED_BYTES = 2 * 1024 * 1024
const blocked = new BlockList()
for (const [address, prefix] of [["0.0.0.0",8],["10.0.0.0",8],["100.64.0.0",10],["127.0.0.0",8],["169.254.0.0",16],["172.16.0.0",12],["192.0.0.0",24],["192.0.2.0",24],["192.88.99.0",24],["168.63.129.16",32],["192.168.0.0",16],["198.18.0.0",15],["198.51.100.0",24],["203.0.113.0",24],["224.0.0.0",4],["240.0.0.0",4]] as const) blocked.addSubnet(address,prefix,"ipv4")
const global6 = new BlockList(); global6.addSubnet("2000::",3,"ipv6")
blocked.addSubnet("2001:db8::",32,"ipv6"); blocked.addSubnet("2001::",23,"ipv6"); blocked.addSubnet("2002::",16,"ipv6")
export function publicAddress(address: string): boolean {
  const family = isIP(address)
  return family === 4 ? !blocked.check(address,"ipv4") : family === 6 && global6.check(address,"ipv6") && !blocked.check(address,"ipv6")
}
export function normalizedFeedURL(raw: string): URL {
  const url = new URL(raw.trim().replace(/^webcal:/i,"https:"))
  if (url.protocol !== "https:" || url.username || url.password || url.port && url.port !== "443" || url.hash || url.hostname.length > 253) throw new Error("Use a public HTTPS subscription URL without a username or password")
  const host=url.hostname.toLowerCase().replace(/\.$/,"")
  if (!host.includes(".") || /(?:^|\.)(?:localhost|local|internal|lan|home|onion)$/.test(host) || isIP(host) && !publicAddress(host)) throw new Error("Private network calendars cannot be fetched")
  return url
}
export type ResolveCalendarHost = (host:string) => Promise<ReadonlyArray<{address:string;family:number}>>
/** Resolve and pin the socket IP to prevent DNS rebinding. Never follow redirects,
 * send browser cookies or forward instance/provider credentials to a feed host. */
export async function fetchCalendarFeed(raw: string, resolveHost: ResolveCalendarHost = host => lookup(host,{all:true,verbatim:true})): Promise<string> {
  const url = normalizedFeedURL(raw)
  let dnsTimeout:ReturnType<typeof setTimeout>|undefined
  const addresses = await Promise.race([resolveHost(url.hostname),new Promise<never>((_,reject)=>{dnsTimeout=setTimeout(()=>reject(new Error("Calendar DNS lookup timed out")),5000);dnsTimeout.unref()})]).finally(()=>clearTimeout(dnsTimeout))
  if (!addresses.length || addresses.some(a => !publicAddress(a.address))) throw new Error("Private calendar address rejected")
  const target = addresses[0]!
  const agent=new Agent({keepAlive:false})
  // Bun 1.3 rewrites custom-lookup HTTPS URLs to the IP before certificate
  // validation. A dedicated TLS socket pins the IP while preserving real SNI
  // and certificate hostname verification, without disabling TLS checks.
  agent.createConnection=()=>connect({host:target.address,port:443,servername:url.hostname,rejectUnauthorized:true})
  return new Promise((resolve,reject) => {
    let settled = false
    let deadline:ReturnType<typeof setTimeout>
    const finish = (error: Error | null, body?: string) => { if(settled)return;settled=true;clearTimeout(deadline);agent.destroy();error?reject(error):resolve(body!) }
    const request = get(url,{agent,headers:{Accept:"text/calendar", "User-Agent":"MeetingNotes-Calendar/1"},maxHeaderSize:16*1024,
    }, response => {
      if (response.statusCode !== 200) { response.destroy();finish(new Error("Calendar host did not return a feed"));return }
      const length=Number(response.headers["content-length"]??0)
      if(length>MAX_FEED_BYTES){response.destroy();finish(new Error("Calendar feed is too large"));return}
      const chunks:Buffer[]=[];let size=0
      response.on("data", (chunk:Buffer)=>{size+=chunk.length;if(size>MAX_FEED_BYTES){response.destroy();finish(new Error("Calendar feed is too large"))}else chunks.push(chunk)})
      response.on("end",()=>finish(null,Buffer.concat(chunks).toString("utf8")))
      response.on("error",()=>finish(new Error("Calendar feed could not be read")))
    })
    deadline=setTimeout(()=>{request.destroy();finish(new Error("Calendar fetch timed out"))},15_000)
    request.on("error",()=>finish(new Error("Calendar feed could not be fetched")))
  })
}
const hash=(value:string)=>createHash("sha256").update(value).digest("hex")
function uuid(value:string){const b=createHash("sha1").update(Buffer.from("d4fa2b127ae348c99db05a2207646140","hex")).update(value).digest().subarray(0,16);b[6]=(b[6]!&15)|80;b[8]=(b[8]!&63)|128;const h=b.toString("hex");return h.slice(0,8)+"-"+h.slice(8,12)+"-"+h.slice(12,16)+"-"+h.slice(16,20)+"-"+h.slice(20)}
const value=(v:unknown):string=>typeof v==="string"?v:typeof v==="object"&&v!==null&&"val"in v?String(v.val):""
function datePart(date:Date,zone:string|null){try{return new Intl.DateTimeFormat("en-CA",{timeZone:zone??"UTC",year:"numeric",month:"2-digit",day:"2-digit"}).format(date)}catch{return date.toISOString().slice(0,10)}}
function meetingLink(...values:unknown[]):string|null {
  for(const input of values)for(const candidate of value(input).match(/https?:\/\/[^\s<>"\\]+/g)??[]){
    try{const url=new URL(candidate.replace(/[).,;]+$/,""));if(url.protocol==="https:"&&!url.username&&!url.password&&/^(?:meet\.google\.com|(?:[\w-]+\.)?zoom\.us|teams\.microsoft\.com|teams\.live\.com)$/.test(url.hostname))return url.href}catch{}
  }
  return null
}
/** The UID as an identity: trimmed, and none at all when it is longer than the contract allows (a shortened one would name another meeting). */
const meetingUid=(uid:unknown)=>{const text=String(uid).trim();return text&&text.length<=2048?text:null}
/** The three forms of `occurrence` in the contract. Anything else (a date no calendar could mean) is sent as no occurrence, never as a malformed one. */
const OCCURRENCE=/^(single|date:\d{4}-\d{2}-\d{2}|instant:\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z)$/
export const wellFormedOccurrence=(occurrence:string):string|null=>OCCURRENCE.test(occurrence)?occurrence:null
const localDay=(date:Date)=>`${String(date.getFullYear()).padStart(4,"0")}-${String(date.getMonth()+1).padStart(2,"0")}-${String(date.getDate()).padStart(2,"0")}`
const utcMidnight=(date:Date)=>new Date(Date.UTC(date.getFullYear(),date.getMonth(),date.getDate()))
const address=(v:unknown)=>value(v).trim().replace(/^mailto:/i,"").toLowerCase()
/** How many other people are invited: the ATTENDEE lines that are people, without the organizer. For
 * anyone who is in the meeting that is the number of others: a feed that lists the organizer among the
 * attendees (Google) and one that does not (Exchange) give the same count. Rooms and equipment
 * (CUTYPE ROOM or RESOURCE) are left out. */
export function otherInvitees(attendees:unknown,organizer:unknown):number {
  const host=address(organizer),people=new Set<string>()
  for(const attendee of(Array.isArray(attendees)?attendees:attendees?[attendees]:[]).slice(0,2000)){
    const kind=typeof attendee==="object"&&attendee!==null&&"params"in attendee&&typeof attendee.params==="object"&&attendee.params!==null&&"CUTYPE"in attendee.params?String(attendee.params.CUTYPE).toUpperCase():""
    const who=address(attendee)
    if(who&&who!==host&&kind!=="ROOM"&&kind!=="RESOURCE")people.add(who)
  }
  return people.size
}
/** node-ical handles RRULE/EXDATE/RECURRENCE-ID/DST; bound source and expansion. */
export async function parseCalendarFeed(calendarId:string, source:string, now:Date):Promise<StoredCalendarEvent[]> {
  if(Buffer.byteLength(source)>MAX_FEED_BYTES||!source.includes("BEGIN:VCALENDAR")||(source.match(/BEGIN:VEVENT/g)?.length??0)>2000)throw new Error("Invalid or oversized calendar")
  const parsed=await ical.async.parseICS(source), events=new Map<string,StoredCalendarEvent>()
  const from=new Date(now.getTime()-31*86400000), to=new Date(now.getTime()+366*86400000)
  for(const event of Object.values(parsed)){
    if(!event||event.type!=="VEVENT"||event.status==="CANCELLED"||!event.uid||!(event.start instanceof Date)||!Number.isFinite(event.start.getTime()))continue
    const instances=ical.expandRecurringEvent(event,{from,to,expandOngoing:true,maxIterations:10_000})
    if(instances.length>5000)throw new Error("Too many calendar occurrences")
    for(const instance of instances){
      if(instance.event.status==="CANCELLED"||!Number.isFinite(instance.start.getTime())||!Number.isFinite(instance.end.getTime()))continue
      const original=instance.event.recurrenceid??instance.start
      const occurrence=!instance.isRecurring?"single":instance.isFullDay?original.toISOString().slice(0,10):original.toISOString()
      const sourceKey=hash(event.uid+"\n"+occurrence), meetingKey=sourceKey
      // The identity sent to the apps, in the one format both sources use (google.ts builds the same three
      // forms). `meetingKey` above keeps its older spelling: notes are bound to it.
      // node-ical gives an all-day date as midnight in this process's own time zone, so the day is read back
      // in that zone: the identity is the same wherever the server runs.
      const identity=!instance.isRecurring?"single":instance.isFullDay?"date:"+localDay(original):"instant:"+original.toISOString()
      const zone=instance.start.tz&&instance.start.tz!=="Etc/UTC"?instance.start.tz:null
      // An all-day event's times are sent as UTC midnights of its days (as the Google source sends them), wherever this server runs.
      const begin=instance.isFullDay?utcMidnight(instance.start):instance.start,finish=instance.isFullDay?utcMidnight(instance.end):instance.end
      const end=finish>begin?finish:new Date(begin.getTime()+(instance.isFullDay?86400000:0))
      events.set(sourceKey,{id:uuid(calendarId+":"+sourceKey),calendarId,sourceKey,meetingKey,title:(value(instance.summary)||"Untitled event").slice(0,500),startAt:begin.toISOString(),endAt:end.toISOString(),allDay:instance.isFullDay,startDate:datePart(begin,instance.isFullDay?"UTC":zone),endDate:datePart(end,instance.isFullDay?"UTC":zone),timeZone:zone,location:value(instance.event.location).slice(0,1000)||null,meetingUrl:meetingLink(instance.event.url,instance.event.location,instance.event.description),meetingUid:meetingUid(event.uid),occurrence:wellFormedOccurrence(identity),attendeeCount:otherInvitees(instance.event.attendee,instance.event.organizer)})
      if(events.size>5000)throw new Error("Too many calendar occurrences")
    }
  }
  return [...events.values()]
}
