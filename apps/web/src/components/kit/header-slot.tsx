import { createContext, useContext, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

/** The site header is static and knows nothing about the session or the page.
 * Signed-in pages place the account control into it, and a note page places its
 * own actions at its trailing edge, so the shell itself never carries account state. */
export interface HeaderSlots { account: HTMLElement | null; page: HTMLElement | null }
export const HeaderSlotContext = createContext<HeaderSlots>({ account: null, page: null })

/** Who is signed in: the administration link and the initials. */
export function HeaderActions({ children }: { children: ReactNode }) {
  const slot = useContext(HeaderSlotContext).account
  return slot ? createPortal(children, slot) : null
}

/** What the page on screen can do: where the desktop app's title bar keeps a note's actions. */
export function HeaderPageActions({ children }: { children: ReactNode }) {
  const slot = useContext(HeaderSlotContext).page
  return slot ? createPortal(children, slot) : null
}
