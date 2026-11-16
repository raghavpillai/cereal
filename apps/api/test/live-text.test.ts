import { expect, test } from "bun:test"
import { mostlyLatin, normaliseText, normaliseWords } from "../src/live/text"

test("English the detector wrote with CJK punctuation gets ASCII punctuation and its spaces back", () => {
  // As Scribe v3 Live returned them with language detection on (2026-11-16).
  expect(normaliseText("Test，test。")).toBe("Test, test.")
  expect(normaliseText("Test，test，test，test，test。")).toBe("Test, test, test, test, test.")
  expect(normaliseText("I need you。")).toBe("I need you.")
  expect(normaliseText("Really？Yes！Note：this；that")).toBe("Really? Yes! Note: this; that")
  expect(normaliseWords([{ text: "Test，" }, { text: "test。" }]).map(word => word.text).join("")).toBe("Test, test.")
})
test("CJK text and text without full-width marks are left exactly as they are", () => {
  for (const text of ["来週の会議は水曜日の午後三時に変更になりました。", "资料，请在周五之前发送。", "Hola a todos. ¿Y tú?", "Guten Morgen."]) expect(normaliseText(text)).toBe(text)
  expect(mostlyLatin("Okay, sounds good 好的")).toBe(true)
  expect(mostlyLatin("好的 ok")).toBe(false)
})
