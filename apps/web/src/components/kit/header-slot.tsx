import { createContext, useContext, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

/** The site header is static and knows nothing about the session. Signed-in
 * pages place their account controls into it through this slot, so the public
 * shell and the anonymous shared reader never carry account state. */
export const HeaderSlotContext = createContext<HTMLElement | null>(null)

export function HeaderActions({ children }: { children: ReactNode }) {
  const slot = useContext(HeaderSlotContext)
  return slot ? createPortal(children, slot) : null
}
