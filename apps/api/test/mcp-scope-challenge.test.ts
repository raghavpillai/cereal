import {test,expect} from "bun:test"
import {extractWWWAuthenticateParams} from "@modelcontextprotocol/client"
import type {Database} from "@meeting-notes/database"
import type {AuthenticationEngine} from "../src/auth"
import {makeMcpAuthentication,MCP_OAUTH_SCOPES} from "../src/mcp-auth"
import {MCP_SCOPES} from "../src/mcp-tools"

test("unauthorized MCP advertises the full requested connection scope without granting or accessing account data",async()=>{
 const baseURL="http://127.0.0.1:3000"
 // Missing credentials fail inside the official verifier before any JWKS or DB
 // lookup. This exercises the actual challenge and official SDK parsing.
 const auth={$context:Promise.resolve({baseURL:baseURL+"/api/auth",internalAdapter:{}})} as unknown as AuthenticationEngine
 let executed=false
 const response=await makeMcpAuthentication({} as Database,auth,baseURL).protect(new Request(baseURL+"/mcp"),async()=>{executed=true;return new Response()})
 expect(response.status).toBe(401)
 const challenge=extractWWWAuthenticateParams(response)
 expect(new Set(challenge.scope?.split(" "))).toEqual(new Set([...MCP_SCOPES,"offline_access"]))
 expect(challenge.resourceMetadataUrl?.href).toBe(baseURL+"/.well-known/oauth-protected-resource/mcp")
 // The endpoint's own address is /api/mcp; a request there is pointed at that address's metadata.
 const current=await makeMcpAuthentication({} as Database,auth,baseURL).protect(new Request(baseURL+"/api/mcp"),async()=>{executed=true;return new Response()})
 expect(current.status).toBe(401)
 expect(extractWWWAuthenticateParams(current).resourceMetadataUrl?.href).toBe(baseURL+"/.well-known/oauth-protected-resource/api/mcp")
 expect(new Set(extractWWWAuthenticateParams(current).scope?.split(" "))).toEqual(new Set([...MCP_SCOPES,"offline_access"]))
 expect(executed).toBe(false)
 expect(MCP_OAUTH_SCOPES).toContain("offline_access")
})
