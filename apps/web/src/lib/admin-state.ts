import type { AdminSettingsDTO, CalendarDTO, CalendarEventDTO, UpdateAdminSettingsInput, UpdateCalendarInput } from '@meeting-notes/contracts'

export interface SettingsDraft {
  instanceName: string
  languageModel: string
  calendarRefreshMinutes: string
  calendarLookAheadDays: string
  openRouterApiKey: string
  elevenLabsApiKey: string
  removeOpenRouterKey: boolean
  removeElevenLabsKey: boolean
}
export function settingsDraft(settings: AdminSettingsDTO): SettingsDraft {
  return { instanceName: settings.instanceName, languageModel: settings.languageModel,
    calendarRefreshMinutes: String(settings.calendarRefreshMinutes), calendarLookAheadDays: String(settings.calendarLookAheadDays),
    openRouterApiKey: '', elevenLabsApiKey: '', removeOpenRouterKey: false, removeElevenLabsKey: false }
}
export function settingsPatch(draft: SettingsDraft, saved: AdminSettingsDTO): UpdateAdminSettingsInput {
  const patch: { -readonly [K in keyof UpdateAdminSettingsInput]: UpdateAdminSettingsInput[K] } = {}
  const name = draft.instanceName.trim(), model = draft.languageModel.trim()
  if (name !== saved.instanceName) patch.instanceName = name
  if (model !== saved.languageModel) patch.languageModel = model
  const refresh = Number(draft.calendarRefreshMinutes), ahead = Number(draft.calendarLookAheadDays)
  if (refresh !== saved.calendarRefreshMinutes) patch.calendarRefreshMinutes = refresh
  if (ahead !== saved.calendarLookAheadDays) patch.calendarLookAheadDays = ahead
  if (draft.removeOpenRouterKey) patch.openRouterApiKey = null
  else if (draft.openRouterApiKey.trim()) patch.openRouterApiKey = draft.openRouterApiKey.trim()
  if (draft.removeElevenLabsKey) patch.elevenLabsApiKey = null
  else if (draft.elevenLabsApiKey.trim()) patch.elevenLabsApiKey = draft.elevenLabsApiKey.trim()
  return patch
}
export function settingsProblem(draft: SettingsDraft): string | null {
  if (!draft.instanceName.trim() || draft.instanceName.trim().length > 100) return 'Use an instance name between 1 and 100 characters.'
  if (!/^[A-Za-z0-9_.:/-]{1,150}$/.test(draft.languageModel.trim())) return 'Use a model identifier such as openai/gpt-4o-mini.'
  const refresh = Number(draft.calendarRefreshMinutes), ahead = Number(draft.calendarLookAheadDays)
  if (!Number.isInteger(refresh) || refresh < 5 || refresh > 1440) return 'Calendar refresh must be between 5 and 1440 minutes.'
  if (!Number.isInteger(ahead) || ahead < 1 || ahead > 90) return 'The look-ahead window must be between 1 and 90 days.'
  for (const [key, remove] of [[draft.openRouterApiKey, draft.removeOpenRouterKey], [draft.elevenLabsApiKey, draft.removeElevenLabsKey]] as const) {
    if (!remove && key.trim() && (key.trim().length < 10 || key.trim().length > 1000 || /[\r\n]/.test(key))) return 'Provider keys must be a single value between 10 and 1000 characters.'
  }
  return null
}
export interface CalendarDraft { name: string; url: string; color: string; enabled: boolean }
export function calendarDraft(calendar?: CalendarDTO): CalendarDraft {
  return { name: calendar?.name ?? '', url: '', color: calendar?.color ?? '#be9737', enabled: calendar?.enabled ?? true }
}
export function calendarProblem(draft: CalendarDraft, editing: boolean): string | null {
  if (!draft.name.trim() || draft.name.trim().length > 200) return 'Use a calendar name between 1 and 200 characters.'
  if (!/^#[0-9a-f]{6}$/i.test(draft.color)) return 'Choose a calendar color.'
  if (!draft.url.trim() && editing) return null
  try {
    const value = draft.url.trim()
    const url = new URL(value.startsWith('webcal:') ? `https:${value.slice(7)}` : value)
    if (url.protocol !== 'https:' || url.username || url.password || value.length > 4096) return 'Use a public HTTPS or webcal subscription URL without embedded account credentials.'
  } catch { return 'Enter a valid HTTPS or webcal calendar subscription URL.' }
  return null
}
export function calendarPatch(draft: CalendarDraft, saved: CalendarDTO): UpdateCalendarInput {
  return { ...(draft.name.trim() !== saved.name ? { name: draft.name.trim() } : {}),
    ...(draft.color !== saved.color ? { color: draft.color } : {}),
    ...(draft.enabled !== saved.enabled ? { enabled: draft.enabled } : {}),
    ...(draft.url.trim() ? { url: draft.url.trim() } : {}) }
}
export function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}
export function eventWindow(startDate: string, days: number): { from: string; to: string } {
  const from = new Date(`${startDate}T00:00:00`)
  if (!Number.isFinite(from.getTime()) || localDateKey(from) !== startDate || !Number.isInteger(days) || days < 1 || days > 90) throw new Error('Invalid calendar window')
  const to = new Date(from); to.setDate(to.getDate() + days)
  return { from: from.toISOString(), to: to.toISOString() }
}
export function eventDay(event: CalendarEventDTO): string { return event.allDay ? event.startDate : localDateKey(new Date(event.startAt)) }
export function formatEventTime(event: CalendarEventDTO, locale?: string): string {
  if (event.allDay) {
    if (event.startDate === event.endDate) return 'All day'
    const start = new Date(`${event.startDate}T12:00:00`), last = new Date(`${event.endDate}T12:00:00`)
    last.setDate(last.getDate() - 1)
    return last <= start ? 'All day' : `All day · through ${new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(last)}`
  }
  const formatter = new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' })
  return `${formatter.format(new Date(event.startAt))} – ${formatter.format(new Date(event.endAt))}`
}
export function safeMeetingLink(value: string | null): string | undefined {
  if (!value || /[\u0000-\u0020]/.test(value)) return
  try { const url = new URL(value); if (url.protocol === 'https:' && !url.username && !url.password) return url.href } catch { /* untrusted calendar URL stays inert */ }
}
