/** The update's picture: the Cereal mark as a bowl that is poured full, a fifth for every step of the update.
 *
 * The same story the desktop app tells its own update with (apps/desktop/src/update_bowl.rs). Asked for, the
 * bowl is an outline and its two flakes dive in. Every step pours a measure of gold and drops a flake after
 * it; the surface rocks while it rises. While the server restarts the bowl holds, full, and the flakes go
 * round it. When the new version answers, the bowl pops, the flakes are flung out and settle, and a ring of
 * gold spreads and fades. A failure tips the bowl and lets it run out.
 *
 * The shapes are the mark's own (src/assets/mark.svg): its bowl and its two flakes, taken apart, never
 * redrawn. It is decoration: everything it shows is also said in words beside it, so it is hidden from
 * assistive technology, and with reduced motion each state is a still picture (the level stays, the movement
 * goes). Only transforms and opacity move. */
import { useEffect, useId, useRef, useState, type CSSProperties } from 'react'
import markSource from '../../assets/mark.svg?raw'
import { markParts, POURS, surfaceY, type BowlAct, type BowlTone } from '../../lib/update-scene'

const MARK = markParts(markSource)
const { x: MX, y: MY } = MARK.middle, R = MARK.radius

/** A ripple: half-waves of `length` and height `rise` along y = 0, closed far below. Wide enough to slide a
 * whole wave sideways without its ends showing in the bowl. */
function ripple(length: number, rise: number) {
  let path = 'M-48 0'
  for (let x = -48, up = true; x < 72; x += length / 2, up = !up) path += `q${length / 4} ${up ? -2 * rise : 2 * rise} ${length / 2} 0`
  return `${path}V40H-48Z`
}
const NEAR = ripple(8, 0.42), FAR = ripple(11, 0.5)
/** Where each step's flake falls: over the gold at the height that step fills to, clear of the ring's hole. */
const DROPS = [{ x: 9.4, r: 0.9 }, { x: 4.6, r: 0.75 }, { x: 4.1, r: 1 }, { x: 8.1, r: 0.8 }] as const
/** What runs out of a tipped bowl, from the lower lip of its bite. */
const SPILLS = [{ x: 15.5, r: 0.8 }, { x: 16.3, r: 0.55 }, { x: 15.1, r: 0.65 }, { x: 16, r: 0.9 }, { x: 15.6, r: 0.5 }] as const
const SPARKS = [8, 52, 97, 141, 188, 233, 278, 322]

export function UpdateBowl({ act, tone, level, pours }: { act: BowlAct; tone: BowlTone; level: number; pours: number }) {
  const clip = useId()
  // Flakes drop for the steps that finish while the page watches, not for those already done when it opened.
  const first = useRef(pours)
  // A bowl that is already empty has nothing to pour out: it tips once, when it had something in it.
  const [seen, setSeen] = useState({ act, level, quiet: act === 'spill' })
  const quiet = act === 'spill' && (seen.act === 'spill' ? seen.quiet : seen.level <= 0)
  if (seen.act !== act || seen.level !== level) setSeen({ act, level, quiet })
  useEffect(() => { if (pours < first.current) first.current = pours }, [pours])
  const shown: BowlAct = quiet ? 'rest' : act
  // They stay in the page once they have fallen (unseen), so that coming back to pouring after the server was away drops none of them again.
  const drops = DROPS.slice(first.current, Math.min(pours, POURS - 1)).map((drop, index) => ({ ...drop, step: first.current + index }))
  const style = { '--surface': `${surfaceY(level).toFixed(3)}px`, '--mx': `${MX}px`, '--my': `${MY}px` } as CSSProperties
  return <span className={`update-bowl is-${shown} is-${tone}`} data-kick={pours % 2} aria-hidden="true" style={style}>
    <svg viewBox={`${MX - 12} ${MY - 12} 24 24`} focusable="false">
      <defs><clipPath id={clip}><path d={MARK.bowl} /></clipPath></defs>
      <circle className="update-bowl-halo" cx={MX} cy={MY} r={R} />
      <circle className="update-bowl-halo is-late" cx={MX} cy={MY} r={R} />
      <g className="update-bowl-body">
        <path className="update-bowl-track" d={MARK.bowl} />
        <g clipPath={`url(#${clip})`}>
          {/* Turned back by as much as the bowl is tipped, so the surface stays level while the bowl leans. */}
          <g className="update-bowl-plumb">
            <g className="update-bowl-liquid">
              <g className="update-bowl-swell"><g className="update-bowl-rock">
                <path className="update-bowl-wave is-far" d={FAR} />
                <path className="update-bowl-wave" d={NEAR} />
              </g></g>
            </g>
          </g>
        </g>
      </g>
      {drops.map(drop => <circle key={drop.step} className="update-bowl-drop" cx={drop.x} cy="0" r={drop.r} />)}
      {shown === 'spill' && SPILLS.map((spill, index) => <circle key={index} className="update-bowl-spill" cx={spill.x} cy="21.4" r={spill.r} style={{ '--i': index } as CSSProperties} />)}
      {shown === 'pop' && SPARKS.map((turn, index) => <g key={turn} transform={`rotate(${turn} ${MX} ${MY})`}><circle className="update-bowl-spark" cx={MX} cy={MY - R - 0.6} r={index % 2 ? 0.42 : 0.62} style={{ '--i': index } as CSSProperties} /></g>)}
      {MARK.flakes.map((flake, index) => <g key={index} className={`update-bowl-flake is-${index ? 'second' : 'first'}`}
        // Into the bowl: a little either side of its middle, and a little below it.
        style={{ '--fx': `${flake.x}px`, '--fy': `${flake.y}px`, '--dx': `${(MX + (index ? 1.6 : -1.6) - flake.x).toFixed(3)}px`, '--dy': `${(MY + 1.5 - flake.y).toFixed(3)}px` } as CSSProperties}>
        <path d={flake.path} />
      </g>)}
    </svg>
  </span>
}
