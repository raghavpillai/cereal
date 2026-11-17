/**
 * Clean-ups of the recogniser's text that do not change what was said.
 *
 * The language is detected by the provider (no language is forced: with English forced, Japanese and short
 * Spanish were dropped, measured 2026-11-16). Detection sometimes writes English with Chinese or Japanese
 * punctuation ("Test，test。", "I need you。"). Text that is mostly Latin script gets ASCII punctuation instead,
 * with the space after it that the full-width form leaves out. CJK text is left as it is.
 */
const FULL_WIDTH: Readonly<Record<string, string>> = { "。": ".", "，": ",", "、": ",", "？": "?", "！": "!", "：": ":", "；": ";", "（": "(", "）": ")" }
const PATTERN = /[。，、？！：；（）]/g, ANY = /[。，、？！：；（）]/

/** True when at least 70% of the letters are Latin script. */
export function mostlyLatin(text: string): boolean {
  let latin = 0, letters = 0
  for (const char of text) {
    if (/\p{L}/u.test(char)) { letters++; if (/\p{Script=Latin}/u.test(char)) latin++ }
  }
  return letters > 0 && latin / letters >= 0.7
}
/** `text` with full-width punctuation as ASCII, a space added after it where a word follows directly. */
export function asciiPunctuation(text: string): string {
  return text.replace(PATTERN, (mark, offset: number) => {
    const ascii = FULL_WIDTH[mark]!, next = text[offset + mark.length]
    return ascii === "(" || next === undefined || /\s/.test(next) || ANY.test(next) ? ascii : `${ascii} `
  })
}
/** Applies `asciiPunctuation` to the words of one utterance when the utterance is mostly Latin script. Word
 * texts carry their own trailing spacing; a word that ends in converted punctuation and is followed by another
 * word gets the space the full-width form left out. */
export function normaliseWords<W extends { text: string }>(words: ReadonlyArray<W>): W[] {
  const whole = words.map(word => word.text).join("")
  if (!ANY.test(whole) || !mostlyLatin(whole)) return [...words]
  return words.map((word, index) => {
    let text = asciiPunctuation(word.text)
    if (index < words.length - 1 && /[.,?!:;)]$/.test(text) && text !== word.text) text += " "
    return { ...word, text }
  })
}
export const normaliseText = (text: string) => ANY.test(text) && mostlyLatin(text) ? asciiPunctuation(text) : text
/** The writing system most letters of `text` are in: "latin", "cjk" (Chinese characters and kana), "hangul",
 * "cyrillic", "greek", "arabic", "hebrew", "devanagari", "thai", "other"; undefined when it has no letters. */
export function textScript(text: string): string | undefined {
  const counts = new Map<string, number>()
  for (const char of text) {
    if (!/\p{L}/u.test(char)) continue
    const script = /\p{Script=Latin}/u.test(char) ? "latin" : /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u.test(char) ? "cjk" : /\p{Script=Hangul}/u.test(char) ? "hangul"
      : /\p{Script=Cyrillic}/u.test(char) ? "cyrillic" : /\p{Script=Greek}/u.test(char) ? "greek" : /\p{Script=Arabic}/u.test(char) ? "arabic" : /\p{Script=Hebrew}/u.test(char) ? "hebrew"
      : /\p{Script=Devanagari}/u.test(char) ? "devanagari" : /\p{Script=Thai}/u.test(char) ? "thai" : "other"
    counts.set(script, (counts.get(script) ?? 0) + 1)
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0]
}
