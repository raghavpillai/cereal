import { describe, expect, test } from "bun:test"
import { classifyOrigin, googleNeedsHttps } from "../src/transport"

const each = (expected: string, addresses: string[]) => { for (const address of addresses) expect([address, classifyOrigin(address)]).toEqual([address, expected]) }

describe("how a server address is reached", () => {
  test("HTTPS anywhere, and this machine over plain HTTP, are secure", () => {
    each("secure", [
      "https://notes.example.com", "https://10.30.1.77:3000", "https://cereal", "HTTPS://NOTES.EXAMPLE.COM/",
      "http://localhost:3000", "http://LOCALHOST", "http://localhost.", "http://app.localhost:3000", "http://127.0.0.1:3000", "http://127.8.9.10", "http://[::1]:3000",
      // Other spellings of 127.0.0.1 that a URL parser reads as that address.
      "http://2130706433", "http://0x7f.1", "http://0177.0.0.1", "http://[::ffff:127.0.0.1]",
    ])
  })

  test("plain HTTP on a private network is private", () => {
    each("private", [
      "http://10.30.1.77:3000", "http://10.0.0.1", "http://10.255.255.255", "http://172.16.0.1", "http://172.31.255.254:8080", "http://192.168.1.20:3000",
      "http://100.64.0.1", "http://100.94.42.50:18080", "http://100.127.255.255", "http://169.254.10.20",
      "http://[fd12:3456:789a::1]:3000", "http://[fc00::1]", "http://[fe80::1]", "http://[febf::1]", "http://[::ffff:192.168.1.20]", "http://[::ffff:c0a8:114]",
      "http://cereal.local:3000", "http://notes.internal", "http://nas.lan", "http://box.home.arpa", "http://CEREAL.LOCAL.", "http://deep.name.internal",
      // A name with no dot only resolves on the network it is on.
      "http://cereal:3000", "http://mac-mini",
      // Spelled another way, still 10.30.1.77.
      "http://0x0a.30.1.77", "http://169090381",
    ])
  })

  test("any other plain HTTP address is public, including ones dressed up to look private", () => {
    each("public-http", [
      "http://notes.example.com", "http://203.0.113.9:3000", "http://8.8.8.8", "http://[2001:db8::1]",
      // Just outside each private range.
      "http://9.255.255.255", "http://11.0.0.1", "http://172.15.255.255", "http://172.32.0.1", "http://192.167.1.1", "http://192.169.1.1",
      "http://100.63.255.255", "http://100.128.0.1", "http://169.253.1.1", "http://169.255.1.1", "http://126.255.255.255", "http://128.0.0.1",
      "http://[fbff::1]", "http://[fe00::1]", "http://[fec0::1]", "http://[::2]", "http://[::ffff:8.8.8.8]", "http://0.0.0.0", "http://[::]",
      // A private address or name used as a label of a public name proves nothing.
      "http://10.30.1.77.example.com", "http://192.168.1.20.nip.io", "http://127.0.0.1.example.com", "http://localhost.example.com", "http://example.com.localhost.evil.test",
      "http://internal.example.com", "http://notes.local.example.com", "http://evil.test/.local", "http://evil.test#.local", "http://evil.test?x=.internal",
      "http://10.30.1.77@evil.test", "http://localhost:pw@evil.test", "http://evil.test\\@10.0.0.1",
      "http://homearpa.example", "http://notes.locale", "http://lan.example.org",
    ])
    // A single-label name is private by rule, whatever it spells; a suffix only counts as a whole label.
    each("private", ["http://local", "http://notlocal", "http://xlan"])
  })

  test("anything that is not an http or https address gets the cautious answer, never a crash", () => {
    each("public-http", ["", " ", "notes.example.com", "10.30.1.77:3000", "ftp://10.0.0.1", "file:///etc/passwd", "javascript:alert(1)", "cereal://auth", "http://", "http://[fe80::1%25en0]", "http://[not-an-address]", "http://256.1.1.1", "http://1.2.3.4.5", "//10.0.0.1", "http://exa mple.com", "\u0000"])
    expect(() => classifyOrigin("x".repeat(100_000))).not.toThrow()
  })

  test("Google needs an address it will redirect to: HTTPS, or this machine", () => {
    expect(googleNeedsHttps("https://notes.example.com")).toBe(false)
    expect(googleNeedsHttps("http://localhost:3000")).toBe(false)
    expect(googleNeedsHttps("http://127.0.0.1:3000")).toBe(false)
    expect(googleNeedsHttps("http://10.30.1.77:3000")).toBe(true)
    expect(googleNeedsHttps("http://cereal.local")).toBe(true)
    expect(googleNeedsHttps("http://notes.example.com")).toBe(true)
  })
})
