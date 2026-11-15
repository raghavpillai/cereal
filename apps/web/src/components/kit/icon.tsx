import type { ReactNode } from 'react'

/** The desktop app's own glyphs (apps/desktop/assets/icons), drawn on its 24-unit
 * grid and painted in the current text colour, as the app paints them. */
const stroke = (width: number, body: ReactNode, join = true) => ({ body, attributes: { fill: 'none', stroke: 'currentColor', strokeWidth: width, strokeLinecap: 'round' as const, ...(join ? { strokeLinejoin: 'round' as const } : {}) } })

const ICONS = {
  document: stroke(1.5, <><path d="M13.75 2.75H7.25a2 2 0 0 0-2 2v14.5a2 2 0 0 0 2 2h9.5a2 2 0 0 0 2-2V7.75Z" /><path d="M13.75 2.75v3a2 2 0 0 0 2 2h3" /></>),
  search: stroke(1.5, <><circle cx="10.8" cy="10.8" r="7.2" /><path d="m16 16 5 5" /></>),
  'calendar-dots': { attributes: { fill: 'none' }, body: <><rect x="4" y="5" width="16" height="15.5" rx="3" stroke="currentColor" strokeWidth="1.5" /><path d="M4 9.75h16" stroke="currentColor" strokeWidth="1.5" /><path d="M8 3.25v3M16 3.250v3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" /><g fill="currentColor"><circle cx="8.5" cy="13.25" r="1" /><circle cx="12" cy="13.25" r="1" /><circle cx="15.5" cy="13.25" r="1" /><circle cx="8.5" cy="16.75" r="1" /><circle cx="12" cy="16.75" r="1" /></g></> },
  people: stroke(1.5, <><circle cx="9" cy="7.75" r="3.5" /><path d="M2.75 20.25v-1a4.5 4.5 0 0 1 4.5-4.5h3.5a4.5 4.5 0 0 1 4.5 4.5v1" /><path d="M15.5 4.6a3.5 3.5 0 0 1 0 6.3" /><path d="M18 14.9a4.4 4.4 0 0 1 3.25 4.35v1" /></>),
  folder: stroke(1.5, <path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />),
  'notes-lines': stroke(1.5, <path d="M4 6h16M4 12h16M4 18h9" />, false),
  sparkles: stroke(1.5, <path d="m12 3 2.7 6.3L21 12l-6.3 2.7L12 21l-2.7-6.3L3 12l6.3-2.7ZM20 2v4m-2-2h4" />),
  'sparkles-solid': { attributes: { fill: 'currentColor' }, body: <><path d="M10 3.5c.5 4.2 2.3 6 6.5 6.5-4.2.5-6 2.3-6.5 6.5-.5-4.2-2.3-6-6.5-6.5 4.2-.5 6-2.3 6.5-6.5Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /><path d="M18 14c.25 2.1 1.15 3 3.25 3.25-2.1.25-3 1.150-3.25 3.25-.25-2.100-1.150-3-3.250-3.250 2.100-.250 3-1.150 3.250-3.250Z" /></> },
  'chevron-down': stroke(1.5, <path d="m6 9 6 6 6-6" />),
  'chevron-left': stroke(1.5, <path d="m15 5-7 7 7 7" />),
  more: stroke(1.5, <><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>),
  'lock-solid': { attributes: { fill: 'none' }, body: <><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" /><rect x="4.5" y="10" width="15" height="11.5" rx="2.6" fill="currentColor" /></> },
  'link-chain': stroke(1.7, <><path d="M10.5 13.5a4 4 0 0 0 5.66 0l3.2-3.2a4 4 0 0 0-5.66-5.66l-1.2 1.2" /><path d="M13.5 10.5a4 4 0 0 0-5.66 0l-3.2 3.2a4 4 0 0 0 5.66 5.66l1.2-1.2" /></>),
  globe: stroke(1.5, <><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.500c2.500 2.300 3.750 5.150 3.750 8.500s-1.250 6.200-3.750 8.500c-2.500-2.300-3.750-5.150-3.750-8.500s1.250-6.200 3.750-8.500Z" /></>, false),
  lock: stroke(1.5, <><rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V6a4 4 0 0 1 8 0v4" /><circle cx="12" cy="15" r="1" /></>),
  copy: stroke(1.5, <><rect x="8" y="8" width="13" height="13" rx="2" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></>),
  file: stroke(1.5, <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Zm0 0v6h6" />),
  'footer-transcript': stroke(2, <path d="M7.5 9.5v5M12 4.5v15m4.5-12v9" />, false),
  close: stroke(1.5, <path d="m6 6 12 12M6 18 18 6" />),
  check: stroke(1.5, <path d="m5 12 4 4L19 6" />),
  sun: stroke(1.5, <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5" /></>),
  // The one glyph the desktop set has no drawing for: the web's own theme control.
  moon: stroke(1.5, <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" />),
  share: stroke(1.5, <path d="M12 16V3m-5 5 5-5 5 5M5 13v7a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-7" />),
  warning: stroke(1.6, <><path d="M10.3 4.6 3.2 17.2A2 2 0 0 0 4.9 20.200h14.2a2 2 0 0 0 1.700-3L13.7 4.600a2 2 0 0 0-3.400 0Z" /><path d="M12 9.500v4.500" /><path d="M12 17v.2" /></>),
} as const

export type IconName = keyof typeof ICONS

export function Icon({ name, size, className }: { name: IconName; size: number; className?: string }) {
  const icon = ICONS[name]
  return <svg className={className ? `icon ${className}` : 'icon'} width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" focusable="false" {...icon.attributes}>{icon.body}</svg>
}
