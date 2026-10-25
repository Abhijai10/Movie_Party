/** A clipped or obscured official player must pause instead of running behind UI. */
export function intersects(
  a: Pick<DOMRect, "left" | "right" | "top" | "bottom">,
  b: Pick<DOMRect, "left" | "right" | "top" | "bottom">,
): boolean {
  return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
}
export function unobscured(slot: HTMLElement): boolean {
  const rect = slot.getBoundingClientRect();
  for (let parent = slot.parentElement; parent; parent = parent.parentElement) {
    const style = getComputedStyle(parent);
    if (/hidden|auto|scroll|clip/.test(`${style.overflowX} ${style.overflowY}`)) {
      const clip = parent.getBoundingClientRect();
      if (
        rect.left < clip.left ||
        rect.right > clip.right ||
        rect.top < clip.top ||
        rect.bottom > clip.bottom
      )
        return false;
    }
  }
  for (const overlay of document.querySelectorAll<HTMLElement>(
    '.camera-card,.chat-compose,.chat-bubble-stack,.reaction-float-layer,.ready-cinema-transition,.lobby-social-dock,[role="dialog"]',
  )) {
    const style = getComputedStyle(overlay);
    if (
      style.visibility === "hidden" ||
      Number(style.opacity) === 0 ||
      !overlay.getClientRects().length
    )
      continue;
    let clipped = overlay.getBoundingClientRect();
    for (let parent = overlay.parentElement; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent);
      if (
        /hidden|auto|scroll|clip/.test(`${style.overflowX} ${style.overflowY}`) ||
        style.contain.includes("paint")
      ) {
        const clip = parent.getBoundingClientRect();
        clipped = new DOMRect(
          Math.max(clipped.left, clip.left),
          Math.max(clipped.top, clip.top),
          Math.max(0, Math.min(clipped.right, clip.right) - Math.max(clipped.left, clip.left)),
          Math.max(0, Math.min(clipped.bottom, clip.bottom) - Math.max(clipped.top, clip.top)),
        );
      }
    }
    if (clipped.width > 0 && clipped.height > 0 && intersects(rect, clipped)) return false;
  }
  return true;
}
