import { useState } from 'react'

/** What goes inside a person's round avatar: their initial, and over it their picture when the
 * server has one (Google's, as of their last sign-in). The initial stays underneath, so a picture
 * that is slow, missing or refused leaves the letter and never an empty circle. The address is
 * asked for without a referrer: the picture's host does not learn which page showed it. */
export function AccountPicture({ image, name, fallback = '?' }: { image: string | undefined; name: string; fallback?: string }) {
  const [failed, setFailed] = useState<string | null>(null)
  return <>
    <span aria-hidden="true">{[...name.trim()][0]?.toLocaleUpperCase() ?? fallback}</span>
    {image && failed !== image && <img className="account-picture" src={image} alt="" referrerPolicy="no-referrer" decoding="async" draggable={false} onError={() => setFailed(image)} />}
  </>
}
