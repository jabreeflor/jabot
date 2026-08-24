//! Candidate 05, Watchers: one, two or three eyes, and they follow the page.
//!
//! This is the only candidate whose status channel is not a face at all. A
//! watcher that needs you stops looking around and looks straight at you, and
//! that reads from across the sidebar without being read — you catch it in
//! peripheral vision the way you catch a person turning their head. The other
//! four say "needs you" with brows and a mouth, which you have to actually
//! look at first.
//!
//! Ported from `prototypes/jabot-avatars-characters.html`, renderer 05. The
//! eye plans, the radii, the pupil ratio and the brow arithmetic are the
//! page's, unedited; what changed is where the gaze lives, since the
//! prototype could hang two listeners off a page it owned and this has to be
//! a hook that survives mounting and unmounting a hundred avatars.
//!
//! The eye plan is dealt off the roster position and not hashed. The
//! prototype tried hashed first and both halves of a forced colour pair came
//! out with the same eyes — which is exactly the pair a person has to tell
//! apart, and the case a hash cannot see. Here the deal is `dealIndex`, the
//! incremental version, because a renderer is handed one bot and never the
//! crew.

import { useEffect, type JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { dealIndex, pick } from "./hash";

/** One eye: centre and radius, in the shared 24-unit box. */
type Eye = readonly [cx: number, cy: number, r: number];

/**
 * Five arrangements, and the count is doing most of the identifying: one eye,
 * two eyes and three eyes are three different creatures before you have
 * looked at any of them properly. The two-eyed plans then vary by asymmetry
 * — level, one high and one low — which is what keeps a crew of five from
 * being a crew of three.
 *
 * Order is load-bearing: the deal indexes into this array, so moving an entry
 * reassigns every bot's eyes.
 */
const EYE_PLAN: readonly (readonly Eye[])[] = [
  // cyclops
  [[12, 11.6, 4.2]],
  // a level pair
  [
    [8.4, 11.4, 3.2],
    [15.6, 11.4, 3.2],
  ],
  // two low, one up on the forehead
  [
    [8, 12, 2.7],
    [16, 12, 2.7],
    [12, 7.4, 2.3],
  ],
  // mismatched, one bigger and higher than the other
  [
    [9, 10.6, 3.6],
    [15.8, 12.6, 2.6],
  ],
  // a big middle eye flanked by two small ones
  [
    [7.8, 11, 2.4],
    [12, 12.4, 3.4],
    [16.2, 11, 2.4],
  ],
];

/**
 * The same five silhouettes candidate 01 draws. A body under these eyes is
 * backdrop rather than character — the eyes are the whole mark — so it is
 * hashed and on its own salt (`w2`), which is what stops a bot being drawn as
 * the same creature in both styles and making the switch look like it did
 * nothing.
 *
 * Copied rather than imported from `Moodblob`: the five candidates are peers
 * behind one switch, and a table reaching sideways into another candidate
 * would mean deleting the loser breaks the winner. If two of them survive the
 * decision this is the first thing to hoist.
 */
const WOBBLE: readonly string[] = [
  // tall
  "M12 2.4c4.7 0 7.9 3.4 7.9 9.4s-2.9 9.8-7.9 9.8-7.9-3.8-7.9-9.8 3.2-9.4 7.9-9.4z",
  // squat
  "M12 5c6.2 0 9.4 3 9.4 8s-3.2 8.4-9.4 8.4S2.6 18 2.6 13 5.8 5 12 5z",
  // leaning left
  "M11.2 3c5.4 0 9 3.6 9 8.8s-3.8 9.6-9.4 9.6-7.5-4.2-7.5-9.4S5.8 3 11.2 3z",
  // leaning right
  "M12.8 3c5.4 0 8.1 4 8.1 9.2s-1.9 9.2-7.5 9.2-9.4-4.4-9.4-9.6S7.4 3 12.8 3z",
  // pear
  "M12 3c4.4 0 7 3 7 7.4 0 4-3 4.6-3 7.2 0 2.2-1.6 3.6-4 3.6s-4-1.4-4-3.6c0-2.6-3-3.2-3-7.2C5 6 7.6 3 12 3z",
];

/* ---- the gaze -----------------------------------------------------------
   Two numbers on the document root, and every pupil on screen translates by
   them in CSS. The alternative — each avatar measuring its own position and
   doing the trigonometry to the pointer — is both a layout read per eye per
   mousemove and, more importantly, wrong: a room of characters all turn to
   look at the same thing, they do not each stare at you from their own angle.

   The listeners are shared the same way. Twelve bots in the sidebar plus the
   chat header plus an Inbox full of rows is a lot of avatars, and every one
   of them mounting its own `pointermove` would be a real cost on a surface
   that is meant to be idle. So the module keeps a count, installs on the
   first mount and removes on the last. */

let mounted = 0;
let detach: (() => void) | null = null;
let gx = 0;
let gy = 0;

/**
 * Ease towards the target rather than jumping to it. Half the remaining
 * distance per event is enough to take the jitter off a fast pointer without
 * the eyes lagging behind it; the rest of the smoothing is the CSS
 * transition.
 */
function setGaze(x: number, y: number): void {
  gx += (x - gx) * 0.5;
  gy += (y - gy) * 0.5;
  const root = document.documentElement;
  root.style.setProperty("--gx", gx.toFixed(2));
  root.style.setProperty("--gy", gy.toFixed(2));
}

function scrollTopOf(target: EventTarget | null): number {
  return target instanceof Element ? target.scrollTop : window.scrollY;
}

function attach(): () => void {
  const onPointer = (e: PointerEvent) => {
    setGaze(
      ((e.clientX / window.innerWidth) * 2 - 1) * 1.1,
      ((e.clientY / window.innerHeight) * 2 - 1) * 1.1,
    );
  };

  // The prototype watched `scrollY`, because the prototype was one long page.
  // Nothing in the app scrolls the window: the sidebar, the thread and the
  // Inbox each scroll themselves. Scroll events do not bubble but they do
  // capture, so one capturing listener on the window sees all three, and the
  // delta comes off whichever element fired. Switching panes resets the
  // baseline instead of reporting the difference between two unrelated
  // scrollTops as one enormous jump.
  let lastTarget: EventTarget | null = null;
  let lastTop = 0;
  const onScroll = (e: Event) => {
    const top = scrollTopOf(e.target);
    if (e.target !== lastTarget) {
      lastTarget = e.target;
      lastTop = top;
      return;
    }
    const dy = top - lastTop;
    lastTop = top;
    setGaze(0, Math.max(-1.2, Math.min(1.2, dy / 12)));
  };

  window.addEventListener("pointermove", onPointer, { passive: true });
  window.addEventListener("scroll", onScroll, { passive: true, capture: true });

  return () => {
    window.removeEventListener("pointermove", onPointer);
    window.removeEventListener("scroll", onScroll, { capture: true });
    // Put the eyes back where they started. Leaving --gx behind would mean
    // the next watcher to mount is born mid-glance, and in the tests it would
    // mean one case setting the gaze for every case after it.
    gx = 0;
    gy = 0;
    document.documentElement.style.removeProperty("--gx");
    document.documentElement.style.removeProperty("--gy");
  };
}

/**
 * Install the page's gaze for as long as at least one watcher is on screen.
 *
 * Reduced motion opts out by never installing, rather than by installing and
 * then declining to move: with no `--gx` on the root the pupils fall back to
 * the `0` in their own `calc`, so the eyes still look forward and still have
 * pupils. A style that goes blank under reduced motion would be a worse
 * accessibility story than the one it was avoiding.
 */
export function useGaze(): void {
  useEffect(() => {
    if (
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches
    ) {
      return;
    }
    mounted += 1;
    if (mounted === 1) detach = attach();
    return () => {
      mounted -= 1;
      if (mounted === 0 && detach) {
        detach();
        detach = null;
      }
    };
  }, []);
}

export function Watchers(props: CrewRenderProps): JSX.Element {
  useGaze();

  const plan = EYE_PLAN[dealIndex(props.id) % EYE_PLAN.length];
  const body = pick(WOBBLE, props.id, "w2");
  const closed = props.state === "failed";

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <g className="rig">
        <path className="body" d={body} />
        {/* Inside `.eyes` so the blink in base.css shuts them, and so the
            failed arcs get shut eyes that still twitch rather than a dead
            drawing. */}
        <g className="eyes">
          {plan.map(([cx, cy, r], i) =>
            closed ? (
              <path
                key={i}
                className="inkstroke"
                d={`M${cx - r * 0.6} ${cy}q${r * 0.6} ${r * 0.55} ${r * 1.2} 0`}
              />
            ) : (
              // The pupil is 0.46 of the eye. Small enough that moving it
              // reads as looking somewhere, large enough to survive the 28px
              // rung, where a smaller one turns into a grey smudge.
              <g key={i}>
                <circle className="sclera" cx={cx} cy={cy} r={r} />
                <circle className="pupil" cx={cx} cy={cy} r={r * 0.46} />
              </g>
            ),
          )}
        </g>
        {/* One brow per eye, sized to that eye, and not the shared pair the
            rest of the crew wears. A pair drawn to the face's centreline met
            in the middle of a cyclops and sat dead straight above the third
            eye of a three-eyed head — a brow only reads as a brow when it is
            the width of the thing it is over. */}
        {props.state === "waiting" &&
          plan.map(([cx, cy, r], i) => (
            <path
              key={i}
              className="inkstroke"
              d={`M${(cx - r * 0.9).toFixed(1)} ${(cy - r - 0.5).toFixed(1)}q${(r * 0.9).toFixed(1)} -${(r * 0.75).toFixed(1)} ${(r * 1.8).toFixed(1)} 0`}
            />
          ))}
        {/* Shut eyes on their own read as asleep, which is very nearly the
            opposite of failed. The mouth is what makes it a wince. */}
        {closed && <path className="inkstroke" d="M9.8 18.4q2.2-2 4.4 0" />}
      </g>
    </svg>
  );
}
