import {expect,test} from "bun:test"
import {request} from "node:https"
import {fetchClientMetadataResource} from "@better-auth/cimd/node"

test("Bun HTTPS honors the official CIMD transport's connection-pinning lookup boundary",async()=>{
 let called=false
 await new Promise<void>((resolve,reject)=>{
  const sentinel=new Error("fixture pinning boundary")
  const req=request("https://public-metadata.example.test",{agent:false,lookup:(_host,_options,callback)=>{called=true;callback(sentinel,"",4)}},()=>reject(new Error("Unexpected network connection")))
  req.once("error",()=>resolve());req.end()
 })
 expect(called).toBe(true)
})
test("official CIMD transport rejects private-address metadata and non-HTTPS before any private connection",async()=>{
 await expect(fetchClientMetadataResource("http://127.0.0.1/client.json")).rejects.toThrow("HTTPS")
 await expect(fetchClientMetadataResource("https://127.0.0.1/client.json")).rejects.toThrow("public-routable")
 await expect(fetchClientMetadataResource("https://[::1]/client.json")).rejects.toThrow()
})
