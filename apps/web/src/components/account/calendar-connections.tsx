import { AccountMenu } from './account-menu'
import { GoogleCalendars } from './google-calendars'
import { SessionGate } from './session-gate'

export function CalendarConnections() {
  return <div className="admin-page is-account"><SessionGate>{(session, client, expired, signOut) => <div key={session.user.id}>
    <AccountMenu session={session} signOut={signOut} />
    <div className="admin-heading"><div><h1>Google Calendar</h1><p>Your meetings, in every Cereal app.</p></div></div>
    <GoogleCalendars client={client} expired={expired} />
  </div>}</SessionGate></div>
}
