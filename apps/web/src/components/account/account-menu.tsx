import { Link } from '@tanstack/react-router'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { LogOut, Plug, Settings2, ShieldCheck } from 'lucide-react'
import { HeaderActions } from '../kit/header-slot'
import { Menu, MenuItem } from '../kit/menu'
import { initials } from '../../lib/notes'

/** The signed-in person's menu, placed in the site header by any gated page. */
export function AccountMenu({ session, signOut }: { session: AuthSessionDTO; signOut: () => Promise<void> }) {
  const { user } = session, name = user.name.trim() || user.email
  return <HeaderActions>
    <Menu label="Account" align="end" buttonClassName="avatar-button pressable" buttonLabel={`Account menu for ${name}`} button={<span className="avatar" aria-hidden="true">{initials(name)}</span>}>
      {close => <>
        <div className="menu-label"><strong>{name}</strong>{user.name.trim() && user.email}</div>
        <div className="menu-separator" role="separator" />
        <Link to="/account" className="menu-item" role="menuitem" tabIndex={-1} onClick={close}><Settings2 aria-hidden="true" /><span>Account</span></Link>
        <Link to="/mcp/connect" className="menu-item" role="menuitem" tabIndex={-1} onClick={close}><Plug aria-hidden="true" /><span>Connections</span></Link>
        {user.role !== 'user' && <Link to="/admin" className="menu-item" role="menuitem" tabIndex={-1} onClick={close}><ShieldCheck aria-hidden="true" /><span>Instance settings</span></Link>}
        <div className="menu-separator" role="separator" />
        <MenuItem icon={<LogOut aria-hidden="true" />} onSelect={() => { close(); void signOut() }}>Sign out</MenuItem>
      </>}
    </Menu>
  </HeaderActions>
}
