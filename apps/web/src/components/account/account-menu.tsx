import { Link } from '@tanstack/react-router'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { HeaderActions } from '../kit/header-slot'
import { Popover } from '../kit/popover'
import { AccountPicture } from './account-picture'

/** The signed-in person in the site header: their picture, or their initial, which opens who they are
 * and Sign out, the page for connecting an AI client, and for administrators a link to
 * the instance's settings. Everything else about an account is done in the desktop and iPhone apps. */
export function AccountMenu({ session, signOut }: { session: AuthSessionDTO; signOut: () => Promise<void> }) {
  const { user } = session, name = user.name.trim() || user.email
  const admin = user.role !== 'user'
  return <HeaderActions>
    <Link to="/download" className="site-link" activeProps={{ 'aria-current': 'page' }}><span>Download</span></Link>
    {admin && <Link to="/admin" className="site-link" activeProps={{ 'aria-current': 'page' }}><span>Admin</span></Link>}
    <Popover label="Account" className="account-popover" buttonClassName="site-avatar" buttonLabel={`Signed in as ${name}`} button={<AccountPicture image={user.image} name={name} />}>
      {close => <>
        <div className="app-menu-who"><span>{name}</span>{user.name.trim() && <small>{user.email}</small>}</div>
        <div className="app-separator" role="separator" />
        {admin && <Link to="/admin" className="app-menu-row site-link-row" role="menuitem" onClick={close}><span>Admin</span></Link>}
        <Link to="/download" className="app-menu-row site-link-row" role="menuitem" onClick={close}><span>Download</span></Link>
        <Link to="/mcp/connect" className="app-menu-row" role="menuitem" onClick={close}><span>Connect an AI client</span></Link>
        <button type="button" className="app-menu-row" role="menuitem" onClick={() => { close(); void signOut() }}><span>Sign out</span></button>
      </>}
    </Popover>
  </HeaderActions>
}
