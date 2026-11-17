/**
 * A TCP proxy between the desktop client and the harness server, for the client's side of the network:
 *
 *  - `down(ms)`: every connection is cut without a closing handshake and new ones are refused for `ms`
 *    (a lost network);
 *  - `hold(true)`: nothing is forwarded either way, bytes wait (a laptop lid closed: the peers' TCP
 *    stacks keep the connection, nothing moves); `hold(false)` lets them through;
 *  - `throttle(bytesPerSecond)`: the uplink carries at most this much (null lifts it).
 */
import type { Socket, TCPSocketListener } from "bun"

interface Pair { client: Socket<Side>; server?: Socket<Side>; up: Uint8Array[]; down: Uint8Array[]; closed: boolean }
interface Side { pair: Pair; role: "client" | "server" }

export function startProxy(target: () => number) {
  const pairs = new Set<Pair>()
  let downUntil = 0, holding = false, rate: number | null = null, credit = 0, creditAt = performance.now()
  const stats = { connections: 0, cuts: 0, bytesUp: 0 }
  const end = (pair: Pair) => { if (pair.closed) return; pair.closed = true; pairs.delete(pair); try { pair.client.terminate() } catch { /* gone */ } try { pair.server?.terminate() } catch { /* gone */ } }
  const write = (socket: Socket<Side>, chunks: Uint8Array[], limit = Infinity) => {
    let sent = 0
    while (chunks.length && sent < limit) {
      const head = chunks[0]!, take = Math.min(head.byteLength, limit - sent), written = socket.write(head.subarray(0, take))
      sent += written
      if (written < head.byteLength) { chunks[0] = head.subarray(written); if (written < take) break } else chunks.shift()
    }
    return sent
  }
  const pump = () => {
    const now = performance.now()
    if (rate !== null) { credit = Math.min(rate / 4, credit + (now - creditAt) / 1000 * rate) }
    creditAt = now
    if (holding) return
    for (const pair of pairs) {
      if (pair.server && pair.up.length) {
        const sent = write(pair.server, pair.up, rate === null ? Infinity : Math.max(0, Math.floor(credit)))
        stats.bytesUp += sent
        if (rate !== null) credit -= sent
      }
      if (pair.down.length) write(pair.client, pair.down)
    }
  }
  const timer = setInterval(pump, 5)
  const listener: TCPSocketListener<Side> = Bun.listen<Side>({
    hostname: "127.0.0.1", port: 0,
    socket: {
      open(client) {
        const pair: Pair = { client, up: [], down: [], closed: false }
        client.data = { pair, role: "client" }
        if (performance.now() < downUntil) { pair.closed = true; client.terminate(); return }
        pairs.add(pair); stats.connections++
        void Bun.connect<Side>({ hostname: "127.0.0.1", port: target(), socket: {
          open(server) { server.data = { pair, role: "server" }; if (pair.closed) { server.terminate(); return } pair.server = server; pump() },
          data(_, chunk) { pair.down.push(new Uint8Array(chunk)); pump() },
          drain() { pump() },
          close() { end(pair) }, error() { end(pair) }, connectError() { end(pair) },
        } }).catch(() => end(pair))
      },
      data(client, chunk) { const pair = client.data.pair; pair.up.push(new Uint8Array(chunk)); pump() },
      drain() { pump() },
      close(client) { end(client.data.pair) },
      error(client) { end(client.data.pair) },
    },
  })
  return {
    port: listener.port, stats,
    down(ms: number) { downUntil = performance.now() + ms; stats.cuts++; for (const pair of [...pairs]) end(pair) },
    hold(on: boolean) { holding = on; if (!on) pump() },
    throttle(bytesPerSecond: number | null) { rate = bytesPerSecond; credit = 0; creditAt = performance.now() },
    stop() { clearInterval(timer); for (const pair of [...pairs]) end(pair); listener.stop(true) },
  }
}
export type Proxy = ReturnType<typeof startProxy>
