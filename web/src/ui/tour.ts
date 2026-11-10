// The tour: six stops that point at the real page, one card at a time.
//
// It is not a modal. The ring dims everything but the region it names, and clicks
// pass through it, so a visitor can press Play on the board the card is describing.
// Escape leaves, the arrow keys move between stops while it is open, and focus goes
// back to whatever opened it.
//
// The words never quote a number: every number on this page names its source (the
// figure registry), and a tour sentence has no source to name.

export interface TourStop {
  /** The element the ring goes around, or `null` for a centred card. */
  readonly anchor: string | null;
  readonly title: string;
  readonly body: string;
}

export const TOUR_STOPS: readonly TourStop[] = [
  {
    anchor: null,
    title: "A search that sees every piece",
    body:
      "The seed fixes the whole game before the first move: the starting board and every piece to come. " +
      "A beam search plays it in this tab, and the Lab replays its answer and shows how it got there. " +
      "Six stops, about a minute.",
  },
  {
    anchor: "#centre",
    title: "The answer, replayed",
    body:
      "Each move the search chose drops onto the board. Play with Space, step with the arrow keys, or drag " +
      "along the strip to seek. An amber underline marks a move that cleared rows.",
  },
  {
    anchor: "#readout",
    title: "What the engine sees",
    body:
      "The score is the evaluator's one number for the board on screen, summed from eight weighted features. " +
      "Higher is better. Around it: the next pieces, the placement just made and every column's height.",
  },
  {
    anchor: "#inspector",
    title: "The search itself",
    body:
      "Each row is one step of the game. Each mark is a board the beam kept, placed by its score with the best " +
      "on the left. The amber line is the path that won. Tree drills into the top boards; Layers lists every step.",
  },
  {
    anchor: "#panel-playground",
    title: "Change the game",
    body:
      "Pick a preset or type a seed: every change re-solves at once. Narrow the beam width and watch the search " +
      "start to lose; widen it and it gets stronger and slower.",
  },
  {
    anchor: "#mode-measure",
    title: "Then measure it",
    body:
      "One win proves little. Measure solves a range of seeds with your Playground settings and reports the win " +
      "rate, or runs the committed benchmark beside the reference machine's results.",
  },
];

/** The flag that says this browser has seen the tour; storage may be missing. */
export const SEEN_KEY = "tetris-lab:tour-seen";

export function tourSeen(): boolean {
  try {
    return globalThis.localStorage?.getItem(SEEN_KEY) === "1";
  } catch {
    return false;
  }
}

export function markSeen(): void {
  try {
    globalThis.localStorage?.setItem(SEEN_KEY, "1");
  } catch {
    // Private windows and blocked storage: the tour still works, it just forgets.
  }
}

export interface TourOptions {
  /** Called before a stop is shown, so the page can put the right view on screen. */
  readonly prepare?: (stop: TourStop, index: number) => void;
  /** Called when the tour closes, whether finished or left. */
  readonly onClose?: () => void;
}

/** The gap between the ring and the card, and the card's distance from the edges. */
const GAP = 14;
const EDGE = 12;
/** Below this width the card docks to the bottom of the screen instead of floating. */
const DOCK_BELOW = 700;

export class Tour {
  private readonly options: TourOptions;
  private readonly ring: HTMLElement;
  private readonly card: HTMLElement;
  private readonly title: HTMLElement;
  private readonly body: HTMLElement;
  private readonly count: HTMLElement;
  private readonly backButton: HTMLButtonElement;
  private readonly nextButton: HTMLButtonElement;
  private readonly skipButton: HTMLButtonElement;
  private at = 0;
  private openNow = false;
  private returnTo: HTMLElement | null = null;
  private frame = 0;

  constructor(options: TourOptions = {}) {
    this.options = options;

    this.ring = document.createElement("div");
    this.ring.id = "tour-ring";
    this.ring.hidden = true;
    this.ring.setAttribute("aria-hidden", "true");

    this.card = document.createElement("section");
    this.card.id = "tour";
    this.card.hidden = true;
    this.card.setAttribute("role", "dialog");
    this.card.setAttribute("aria-modal", "false");
    this.card.setAttribute("aria-labelledby", "tour-title");
    this.card.setAttribute("aria-describedby", "tour-body");
    this.card.tabIndex = -1;

    this.title = document.createElement("h2");
    this.title.id = "tour-title";
    this.body = document.createElement("p");
    this.body.id = "tour-body";

    const actions = document.createElement("div");
    actions.className = "tour-actions";
    this.skipButton = button("Skip", "Leave the tour. Escape does the same.", "tour-skip");
    this.count = document.createElement("span");
    this.count.className = "tour-count";
    this.backButton = button("Back", "The previous stop (←).", "tour-back");
    this.nextButton = button("Next", "The next stop (→).", "tour-next");
    this.nextButton.classList.add("go");
    actions.append(this.skipButton, this.count, this.backButton, this.nextButton);
    this.card.append(this.title, this.body, actions);
    document.body.append(this.ring, this.card);

    this.skipButton.addEventListener("click", () => this.close());
    this.backButton.addEventListener("click", () => this.go(this.at - 1));
    this.nextButton.addEventListener("click", () => {
      if (this.at >= TOUR_STOPS.length - 1) this.close();
      else this.go(this.at + 1);
    });

    // Capture, so the page's own keyboard map never sees a key the tour used.
    document.addEventListener("keydown", (event) => this.key(event), true);
    globalThis.addEventListener("resize", () => this.schedule());
    document.addEventListener("scroll", () => this.schedule(), true);
  }

  get isOpen(): boolean {
    return this.openNow;
  }

  open(at = 0): void {
    if (!this.openNow) {
      const active = document.activeElement;
      this.returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
    }
    this.openNow = true;
    this.ring.hidden = false;
    this.card.hidden = false;
    this.go(at);
    this.nextButton.focus({ preventScroll: true });
  }

  close(): void {
    if (!this.openNow) return;
    this.openNow = false;
    this.ring.hidden = true;
    this.card.hidden = true;
    markSeen();
    this.options.onClose?.();
    const back = this.returnTo;
    this.returnTo = null;
    if (back?.isConnected) back.focus({ preventScroll: true });
  }

  private go(index: number): void {
    const at = Math.max(0, Math.min(TOUR_STOPS.length - 1, index));
    this.at = at;
    const stop = TOUR_STOPS[at];
    this.options.prepare?.(stop, at);
    this.title.textContent = stop.title;
    this.body.textContent = stop.body;
    this.count.textContent = `${at + 1} of ${TOUR_STOPS.length}`;
    this.backButton.disabled = at === 0;
    const last = at === TOUR_STOPS.length - 1;
    this.nextButton.textContent = last ? "Done" : at === 0 ? "Start" : "Next";
    this.nextButton.dataset.tip = last ? "Close the tour." : "The next stop (→).";
    const anchor = this.anchorOf(stop);
    if (anchor && globalThis.innerWidth < DOCK_BELOW) {
      // Docked, the card covers the bottom of the screen: the region's top goes to the
      // top, so what the card describes is above it rather than under it.
      const top = anchor.getBoundingClientRect().top;
      // Instant, so a quick second Next measures a settled page, not a scroll in flight.
      if (Math.abs(top - EDGE) > 1) globalThis.scrollTo({ top: globalThis.scrollY + top - EDGE, behavior: "instant" });
    } else if (anchor && !inView(anchor)) {
      anchor.scrollIntoView({ block: "center", inline: "nearest" });
    }
    // The page may have just changed view; place the card once layout has settled.
    this.schedule();
  }

  private anchorOf(stop: TourStop): HTMLElement | null {
    if (stop.anchor === null) return null;
    const found = document.querySelector<HTMLElement>(stop.anchor);
    if (!found) return null;
    const box = found.getBoundingClientRect();
    return box.width > 0 && box.height > 0 ? found : null;
  }

  private schedule(): void {
    if (!this.openNow) return;
    cancelAnimationFrame(this.frame);
    this.frame = requestAnimationFrame(() => this.place());
  }

  /** Put the ring around the anchor and the card beside it, inside the viewport. */
  private place(): void {
    if (!this.openNow) return;
    const width = globalThis.innerWidth;
    const height = globalThis.innerHeight;
    const anchor = this.anchorOf(TOUR_STOPS[this.at]);
    const docked = width < DOCK_BELOW;
    this.card.dataset.docked = String(docked);

    if (anchor === null) {
      this.ring.dataset.centred = "true";
      setBox(this.ring, width / 2, height / 2, 0, 0);
    } else {
      this.ring.dataset.centred = "false";
      const box = anchor.getBoundingClientRect();
      const pad = 4;
      const left = Math.max(2, box.left - pad);
      const top = Math.max(2, box.top - pad);
      const right = Math.min(width - 2, box.right + pad);
      const bottom = Math.min(height - 2, box.bottom + pad);
      setBox(this.ring, left, top, Math.max(0, right - left), Math.max(0, bottom - top));
    }

    if (docked) {
      this.card.style.left = "";
      this.card.style.top = "";
      return;
    }

    const card = this.card.getBoundingClientRect();
    const cw = card.width;
    const ch = card.height;
    let x = (width - cw) / 2;
    let y = (height - ch) / 2;
    if (anchor !== null) {
      const box = anchor.getBoundingClientRect();
      const fits = {
        right: width - box.right - GAP - EDGE >= cw,
        left: box.left - GAP - EDGE >= cw,
        below: height - box.bottom - GAP - EDGE >= ch,
        above: box.top - GAP - EDGE >= ch,
      };
      const middleY = clamp(box.top + box.height / 2 - ch / 2, EDGE, height - ch - EDGE);
      const middleX = clamp(box.left + box.width / 2 - cw / 2, EDGE, width - cw - EDGE);
      if (fits.right) [x, y] = [box.right + GAP, middleY];
      else if (fits.left) [x, y] = [box.left - GAP - cw, middleY];
      else if (fits.below) [x, y] = [middleX, box.bottom + GAP];
      else if (fits.above) [x, y] = [middleX, box.top - GAP - ch];
      else [x, y] = [middleX, clamp(box.bottom - ch - EDGE * 2, EDGE, height - ch - EDGE)];
    }
    this.card.style.left = `${Math.round(clamp(x, EDGE, width - cw - EDGE))}px`;
    this.card.style.top = `${Math.round(clamp(y, EDGE, height - ch - EDGE))}px`;
  }

  private key(event: KeyboardEvent): void {
    if (!this.openNow) return;
    const inCard = event.target instanceof Node && this.card.contains(event.target);
    // Not a modal: a field or slider the visitor is using keeps its own arrow keys.
    if (!inCard && editable(event.target) && event.key !== "Escape") return;
    let used = true;
    switch (event.key) {
      case "Escape":
        this.close();
        break;
      case "ArrowRight":
        if (this.at < TOUR_STOPS.length - 1) this.go(this.at + 1);
        break;
      case "ArrowLeft":
        this.go(this.at - 1);
        break;
      default:
        // Enter, Space and Tab belong to the card's own buttons.
        used = false;
    }
    if (used) {
      event.preventDefault();
      event.stopPropagation();
    } else if (inCard && event.key !== "Tab") {
      event.stopPropagation();
    }
  }
}

function button(words: string, tip: string, id: string): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.id = id;
  element.className = "tbtn";
  element.textContent = words;
  element.dataset.tip = tip;
  return element;
}

function setBox(element: HTMLElement, left: number, top: number, width: number, height: number): void {
  element.style.left = `${Math.round(left)}px`;
  element.style.top = `${Math.round(top)}px`;
  element.style.width = `${Math.round(width)}px`;
  element.style.height = `${Math.round(height)}px`;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), Math.max(low, high));
}

function editable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || target.matches("input, select, textarea");
}

function inView(element: HTMLElement): boolean {
  const box = element.getBoundingClientRect();
  return box.top >= 0 && box.bottom <= globalThis.innerHeight;
}
