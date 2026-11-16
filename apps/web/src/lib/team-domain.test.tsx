import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { SignInFields } from '../components/onboarding/steps'
import { personalMailDomain } from '@meeting-notes/contracts/mail-domains'
import { teamDomain, withTeamDomain, type SignInDraft } from './instance-settings'

describe('team domain', () => {
  test('an organisation address offers its domain', () => {
    expect(teamDomain('Ada@NorthWind.com')).toBe('northwind.com')
    expect(teamDomain('a@eng.example.co.uk')).toBe('eng.example.co.uk')
  })
  test('personal mail providers and non-addresses offer nothing', () => {
    for (const email of ['someone@gmail.com', 'x@outlook.com', 'y@icloud.com', '', null, undefined, 'not an address']) expect(teamDomain(email)).toBeNull()
  })
  test('regional and less common personal providers, ISPs and relays offer nothing either', () => {
    // The ones the validator found offered, and their families.
    for (const domain of ['yahoo.co.uk', 'yahoo.co.jp', 'yahoo.com.br', 'yahoo.fr', 'hotmail.co.uk', 'hotmail.fr', 'live.co.uk', 'live.nl', 'outlook.de', 'outlook.com.au',
      'gmx.de', 'gmx.at', 'gmx.net', 'web.de', 'mail.ru', 'yandex.ru', 'yandex.com.tr', 'naver.com', '126.com', '163.com', 'qq.com', 'tutanota.com', 'duck.com',
      'privaterelay.appleid.com', 'icloud.com', 'me.com', 'mac.com', 'aol.com', 'aol.co.uk', 'proton.me', 'proton.ch', 'protonmail.com', 'pm.me',
      'comcast.net', 'btinternet.com', 'orange.fr', 'libero.it', 'sbcglobal.net', 'att.net', 'verizon.net', 'mail.comcast.net'])
      expect(teamDomain('someone@' + domain), domain).toBeNull()
    expect(personalMailDomain('@Yahoo.CO.UK')).toBe(true)
  })
  test('schools are not offered, but they are not refused as entries', () => {
    for (const domain of ['stanford.edu', 'cs.stanford.edu', 'ox.ac.uk', 'unimelb.edu.au', 'tsinghua.edu.cn']) {
      expect(teamDomain('someone@' + domain), domain).toBeNull()
      expect(personalMailDomain(domain), domain).toBe(false)
    }
  })
  test('organisation domains that only look like a provider are still offered', () => {
    for (const domain of ['northwind.com', 'yahooinc.com', 'outlookcapital.com', 'live-events.co.uk', 'mymail.com', 'gmail-tools.dev', 'education.com', 'eng.example.co.uk'])
      expect(teamDomain('someone@' + domain), domain).toBe(domain)
  })
  test('adding and removing the domain keeps the rest of the list', () => {
    const list = ['a@other.com', '@else.com']
    expect(withTeamDomain(list, 'northwind.com', true)).toEqual(['a@other.com', '@else.com', '@northwind.com'])
    expect(withTeamDomain(['@northwind.com', ...list], 'northwind.com', true)).toEqual(['@northwind.com', ...list])
    expect(withTeamDomain(['a@other.com', '@northwind.com', '@else.com'], 'northwind.com', false)).toEqual(list)
  })
})

describe('who may join', () => {
  const draft: SignInDraft = { password: false, google: true, allowedEmails: [], googleClientId: '', googleClientSecret: '', removeGoogleSecret: false }
  test('offers "anyone at" the owner\'s organisation domain, off until chosen', () => {
    const off = renderToStaticMarkup(<SignInFields draft={draft} owner="ada@northwind.com" change={() => {}} />)
    expect(off).toContain('Anyone at northwind.com can join')
    expect(off).toContain('aria-checked="false"')
    const on = renderToStaticMarkup(<SignInFields draft={{ ...draft, allowedEmails: ['@northwind.com'] }} owner="ada@northwind.com" change={() => {}} />)
    expect(on).toContain('aria-checked="true"')
  })
  test('no such switch for a personal address', () => {
    expect(renderToStaticMarkup(<SignInFields draft={draft} owner="someone@gmail.com" change={() => {}} />)).not.toContain('Anyone at')
  })
})
