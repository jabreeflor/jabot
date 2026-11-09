/**
 * The GUI scale from `--ui-scale` (tokens.css). `html` is zoomed by it, so
 * pointer coordinates (`clientX`, `getBoundingClientRect`) are in screen
 * pixels while CSS lengths set from script are in zoomed pixels. Divide a
 * screen-pixel value by this before writing it to a style, and multiply a
 * CSS length by it before comparing it to a pointer. jsdom has no
 * stylesheet, so a missing or bad value reads as 1.
 */
export function uiScale(): number {
  if (typeof document === "undefined") return 1;
  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue("--ui-scale")
    .trim();
  const scale = Number.parseFloat(raw);
  return Number.isFinite(scale) && scale > 0 ? scale : 1;
}
