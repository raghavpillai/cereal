import type { Transport } from '@meeting-notes/contracts/transport'

/** Said where a password is typed or settings are kept, when the server is reached over plain HTTP at an
 * address that is not on a private network. A warning, not a wall: nothing is disabled. */
export function TransportWarning({ transport }: { transport: Transport | undefined }) {
  if (transport !== 'public-http') return null
  return <div className="transport-warning" role="note">
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false"><path d="M4.5 7V4.8a3.5 3.5 0 0 1 6.6-1.6M3.4 7h9.2a.9.9 0 0 1 .9.9v4.7a.9.9 0 0 1-.9.9H3.4a.9.9 0 0 1-.9-.9V7.9a.9.9 0 0 1 .9-.9ZM8 9.7v1.6" /></svg>
    <p><strong>This address is not encrypted.</strong> Passwords and notes travel in the clear. That is fine on a private network; on the internet put Cereal behind HTTPS.</p>
  </div>
}

/** The one quiet line the admin panel shows for plain HTTP on a private address. */
export const PRIVATE_TRANSPORT_LINE = 'Served over plain HTTP on a private address'
