/** A CSS time as milliseconds: "270ms", ".27s", "0.27s". A build may rewrite one form as the other (270ms
 * leaves the minifier as .27s), so a bare parseFloat reads a quarter of a second as a quarter of a millisecond. */
export function cssMilliseconds(value: string | null | undefined, fallback: number): number {
  const match = /^\s*(-?(?:\d+\.?\d*|\.\d+))(ms|s)\s*$/i.exec(value ?? '')
  if (!match) return fallback
  const amount = Number(match[1]) * (match[2]!.toLowerCase() === 's' ? 1000 : 1)
  return Number.isFinite(amount) && amount >= 0 ? amount : fallback
}
