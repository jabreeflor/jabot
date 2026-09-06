/** Stable appearance IDs retain the host's existing `color` wire/storage slot.
 * Do not reorder the mapping: saved bots and older clients must round-trip it.
 * This catalog is vendored into the web companion by `sync:ui`.
 */
export const BOT_ICONS = [
  { value: "b-teal", id: "classic", name: "Classic", motion: "Gentle bounce", body: "M16 17H32Q36 17 36 21V31Q36 35 32 35H16Q12 35 12 31V21Q12 17 16 17Z", eyes: "M20 24V27M28 24V27", antenna: "M24 17V12", tipY: 10 },
  { value: "b-yellow", id: "scout", name: "Scout", motion: "Curious tilt", body: "M18 17H30L37 24V31Q37 35 33 35H15Q11 35 11 31V24Z", eyes: "M19 24V27M29 24V27", antenna: "M24 17V12", tipY: 10 },
  { value: "b-purple", id: "buddy", name: "Buddy", motion: "Soft bob", body: "M18 17H30C37 17 39 35 30 35H18C9 35 11 17 18 17Z", eyes: "M20 24V27M28 24V27", antenna: "M24 17V12", tipY: 10 },
  { value: "b-violet", id: "pixel", name: "Pixel", motion: "Playful steps", body: "M15 17H33V20H36V32H33V35H15V32H12V20H15Z", eyes: "M19 24V27M29 24V27", antenna: "M24 17V12", tipY: 10 },
  { value: "b-blue", id: "relay", name: "Relay", motion: "Antenna flick", body: "M17 18H31Q34 18 34 21V31Q34 34 31 34H17Q14 34 14 31V21Q14 18 17 18ZM14 24H10V29H14M34 24H38V29H34", eyes: "M20 24V27M28 24V27", antenna: "M24 18V12", tipY: 10 },
  { value: "b-orange", id: "mini", name: "Mini", motion: "Little nod", body: "M20 15H28Q32 15 32 19V33Q32 37 28 37H20Q16 37 16 33V19Q16 15 20 15Z", eyes: "M21 24V27M27 24V27", antenna: "M24 15V10", tipY: 8 },
  { value: "b-pink", id: "wide", name: "Wide", motion: "Sideways glance", body: "M15 20H33Q39 20 39 26Q39 33 33 33H15Q9 33 9 26Q9 20 15 20Z", eyes: "M19 25V28M29 25V28", antenna: "M24 20V15", tipY: 13 },
  { value: "b-green", id: "visor", name: "Visor", motion: "Quiet sway", body: "M17 17H31Q36 17 36 22V30Q36 35 31 35H17Q12 35 12 30V22Q12 17 17 17ZM17 22H31Q33 22 33 26Q33 30 31 30H17Q15 30 15 26Q15 22 17 22Z", eyes: "M20 25V27M28 25V27", antenna: "M24 17V12", tipY: 10 },
] as const;
export type BotIcon = (typeof BOT_ICONS)[number];
export function botIcon(value: string): BotIcon {
  return BOT_ICONS.find((entry) => entry.value === value) ?? BOT_ICONS[0];
}
/** All interpolated values come from the closed catalog above, never user input. */
export function botIconSvg(value: string): string {
  const b = botIcon(value);
  return `<svg class="bot-mark" data-character="${b.id}" viewBox="4 3 40 40" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><g class="bot-pose"><g class="bot-antenna"><path d="${b.antenna}"/><circle cx="24" cy="${b.tipY}" r="2"/></g><path class="bot-head" d="${b.body}"/><g class="bot-face"><path class="bot-eyes" d="${b.eyes}"/><path class="bot-happy" d="M18.5 26Q20 23.5 21.5 26M26.5 26Q28 23.5 29.5 26"/><path class="bot-error" d="M18 24L22 28M22 24L18 28M26 24L30 28M30 24L26 28"/></g></g></svg>`;
}
