// One tooltip for the whole page.
//
// §2.1.2 asks for a tooltip on every control; the reason this is a module rather than
// a `::after` on `[data-tip]` (the mockup's way) is that a CSS pseudo-element is
// clipped by whatever the control sits inside. The rail scrolls, the inspector
// scrolls, the transport is 40 px tall — a tooltip drawn inside those boxes would be
// cut off exactly where it is needed most. So: one fixed-position element, delegated
// from one listener pair, and the tip is always readable and always on screen.
//
// Three behaviours that matter more than the styling:
//
//   * **`title` is migrated, not duplicated.** Ticket 13's Playground sets `title` on
//     every one of its controls (and so do the Move strip's cells, which are created
//     during playback). Showing both a native tooltip and this one would be two
//     surfaces disagreeing, so the first time an element is hovered its `title` is
//     moved into `data-tip` and removed. A sweep at install does the same for
//     everything already in the document, which is why the headless check can assert
//     that every control in a region has `data-tip`.
//   * **It never takes a pointer.** `pointer-events: none`, no focus trap, no delay
//     long enough to feel like a broken hover: 120 ms of fade-in (notes/14 §7).
//   * **It closes on Escape, on scroll and on blur**, so a tooltip cannot outlive its
//     anchor and sit over the board.

const TIP_ID = "tip";

/** Move `title` into `data-tip` for one element, if it still has one. */
function adopt(element: HTMLElement): void {
  const title = element.getAttribute("title");
  if (title === null) return;
  if (!element.dataset.tip) element.dataset.tip = title;
  element.removeAttribute("title");
}

/** Sweep the whole document once, so a pre-existing `title` becomes a page tooltip. */
export function adoptTitles(root: ParentNode = document): void {
  for (const element of root.querySelectorAll("[title]")) {
    if (element instanceof HTMLElement) adopt(element);
  }
}

/**
 * Install the tooltip. Idempotent: a second call rebinds nothing.
 *
 * `getComputedStyle` is not consulted; the whole look is in `shell.css` under
 * `#tip[data-open]`.
 */
export function installTooltips(root: Document = document): void {
  const tip = root.getElementById(TIP_ID);
  if (!tip || tip.dataset.installed === "true") return;
  tip.dataset.installed = "true";
  adoptTitles(root);

  let anchor: HTMLElement | null = null;

  const hide = (): void => {
    anchor = null;
    delete tip.dataset.open;
  };

  const place = (element: HTMLElement): void => {
    const text = element.dataset.tip;
    if (!text) return hide();
    adopt(element);
    anchor = element;
    tip.textContent = text;
    tip.dataset.open = "true";
    // Measure after the text is in, so the box is the size it will be.
    const rect = element.getBoundingClientRect();
    const box = tip.getBoundingClientRect();
    const margin = 8;
    const above = rect.top - box.height - margin;
    const top = above > margin ? above : Math.min(rect.bottom + margin, window.innerHeight - box.height - margin);
    const centred = rect.left + rect.width / 2 - box.width / 2;
    const left = Math.max(margin, Math.min(centred, window.innerWidth - box.width - margin));
    tip.style.top = `${Math.round(top)}px`;
    tip.style.left = `${Math.round(left)}px`;
    tip.dataset.anchor = element.dataset.figure ?? element.dataset.field ?? element.id ?? element.tagName.toLowerCase();
  };

  const hovered = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const host = target.closest<HTMLElement>("[data-tip], [title]");
    if (host && host !== anchor) place(host);
    else if (!host) hide();
  };

  root.addEventListener("pointerover", hovered, { passive: true });
  root.addEventListener("pointerdown", hide, { passive: true, capture: true });
  root.addEventListener("focusin", (event) => {
    const target = event.target;
    if (target instanceof HTMLElement && target.matches("[data-tip], [title]")) place(target);
  });
  root.addEventListener("focusout", hide);
  root.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hide();
  });
  root.defaultView?.addEventListener("scroll", hide, { passive: true, capture: true });
  root.defaultView?.addEventListener("blur", hide);

  // A tip anchored to something that has just gone away (a strip cell replaced by a
  // new game) must not be left floating over the board.
  const observer = new MutationObserver(() => {
    if (anchor && !anchor.isConnected) hide();
  });
  observer.observe(root.body, { childList: true, subtree: true });
}
