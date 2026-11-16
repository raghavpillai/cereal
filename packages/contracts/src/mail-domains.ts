/** Mail services anyone can sign up with. An allow-list entry for one of their whole domains ("@gmail.com")
 * would let anybody in, so the server refuses such an entry and the web app never offers "Anyone at <domain>"
 * for one. Shared by both, so they cannot disagree. Pure: no DNS, no network.
 *
 * Getting this wrong in either direction is cheap: a missed provider can still be refused by Google's `hd`
 * check at sign-in (a consumer account has no Workspace domain), and a company caught by a rule below can
 * still list its people's full addresses. */

/** Whole domains, matched exactly and with any subdomain ("mail.comcast.net"). */
const PROVIDERS = new Set([
  // Google, Microsoft, Apple, Yahoo and AOL (their regional domains are the families below)
  "gmail.com", "googlemail.com", "msn.com", "passport.com", "windowslive.com",
  "icloud.com", "me.com", "mac.com", "privaterelay.appleid.com",
  "ymail.com", "rocketmail.com", "aim.com", "verizon.net",
  // Privacy and independent providers
  "proton.me", "protonmail.com", "protonmail.ch", "pm.me", "tutanota.com", "tutanota.de", "tutamail.com", "tuta.io", "tuta.com",
  "duck.com", "fastmail.com", "fastmail.fm", "hey.com", "hushmail.com", "mailfence.com", "posteo.de", "posteo.net", "mailbox.org",
  "zoho.com", "zohomail.com", "mail.com", "email.com", "usa.com", "inbox.com", "gmx.net", "runbox.com", "disroot.org", "riseup.net",
  // Europe
  "web.de", "t-online.de", "freenet.de", "arcor.de", "online.de", "1und1.de",
  "orange.fr", "wanadoo.fr", "free.fr", "sfr.fr", "neuf.fr", "laposte.net", "club-internet.fr",
  "libero.it", "virgilio.it", "tiscali.it", "alice.it", "tin.it", "email.it",
  "btinternet.com", "sky.com", "virginmedia.com", "blueyonder.co.uk", "talktalk.net", "ntlworld.com", "tiscali.co.uk",
  "telenet.be", "skynet.be", "ziggo.nl", "kpnmail.nl", "home.nl", "planet.nl", "xs4all.nl", "bluewin.ch",
  "seznam.cz", "centrum.cz", "wp.pl", "o2.pl", "onet.pl", "interia.pl", "op.pl", "abv.bg", "freemail.hu", "sapo.pt",
  // Russia, Ukraine and around
  "mail.ru", "inbox.ru", "list.ru", "bk.ru", "internet.ru", "rambler.ru", "ya.ru", "ukr.net", "i.ua", "meta.ua",
  // Asia
  "naver.com", "daum.net", "hanmail.net", "kakao.com", "nate.com",
  "qq.com", "foxmail.com", "163.com", "126.com", "yeah.net", "sina.com", "sina.cn", "sohu.com", "aliyun.com", "139.com", "189.cn",
  "docomo.ne.jp", "ezweb.ne.jp", "softbank.ne.jp", "i.softbank.jp", "nifty.com", "rediffmail.com",
  // North America: the large internet providers
  "comcast.net", "xfinity.com", "att.net", "sbcglobal.net", "bellsouth.net", "cox.net", "charter.net", "spectrum.net",
  "earthlink.net", "optonline.net", "frontier.com", "frontiernet.net", "windstream.net", "centurylink.net", "q.com", "juno.com", "netzero.net",
  "rogers.com", "shaw.ca", "sympatico.ca", "bell.net", "videotron.ca", "telus.net",
  // Elsewhere
  "bigpond.com", "bigpond.net.au", "optusnet.com.au", "telstra.com", "xtra.co.nz",
  "uol.com.br", "bol.com.br", "terra.com.br", "ig.com.br", "prodigy.net.mx",
])

/** Families with a domain per country: yahoo.co.uk, hotmail.fr, outlook.de, live.co.uk, gmx.at, yandex.ru,
 * aol.co.uk, proton.ch… The name, then a country or generic ending of one or two parts. */
const FAMILIES = /^(?:yahoo|hotmail|outlook|live|gmx|yandex|aol|proton|protonmail|windowslive)\.(?:[a-z]{2,3}|(?:co|com|net|org|ne|or)\.[a-z]{2})$/

/** Whether a domain ("gmail.com", "Yahoo.co.uk") belongs to a mail service anyone can sign up with. */
export function personalMailDomain(raw: string): boolean {
  const domain = raw.trim().toLowerCase().replace(/^@/, "").replace(/\.$/, "")
  if (!domain) return false
  if (FAMILIES.test(domain)) return true
  for (let at = domain; at.includes("."); at = at.slice(at.indexOf(".") + 1)) if (PROVIDERS.has(at)) return true
  return false
}

/** Schools and universities: their addresses also go to students and alumni, so "anyone at" the domain is
 * more people than the organisation. Not refused as an entry, only never suggested. */
export function academicDomain(raw: string): boolean {
  const domain = raw.trim().toLowerCase().replace(/^@/, "").replace(/\.$/, "")
  return /(?:^|\.)edu$/.test(domain) || /(?:^|\.)(?:edu|ac|sch)\.[a-z]{2}$/.test(domain)
}
