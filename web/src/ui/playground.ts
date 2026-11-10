// The Playground: the four game fields, the two presets, and the expert width —
// the controls whose whole job is to change the game and re-solve it.
//
// §2.1.7 is the requirement that carries the ticket: "Changing seed, goal, pieces or
// starting height must change the board on screen. In the prototype the sliders
// moved and nothing happened; that is the product's core promise." So every control
// here commits a *game*, not a number: this file clamps what a visitor typed
// before it is passed on (`core/game.ts`), and hands the page one `GameInput` per
// commit. The page writes the URL and posts the solve; this file never posts one
// itself, which keeps "what re-solves" in one place.
//
// Two tiers, because a drag is not a decision (see `ui/coalesce.ts`):
//
//   * every `input` event updates the control's own readout synchronously, so the
//     number and the tick never lag the thumb;
//   * `input` also restarts a 150 ms trailing timer, while `change`, a preset click
//     and an arrow-key step flush it immediately. A keyboard visitor is never made
//     to wait out a debounce the mouse needed.
//
// There is no visual system here on purpose: ticket 14 owns the shell, so the
// controls are plain labelled form elements and every string that a person reads is
// a constant in `PLAYGROUND_TEXT` below. What ticket 14 must find intact is
// `fill` / `current` / the commit callback — not the geometry.
//
// Accessibility, in the smallest space that is honest: a `<label>` per control, a
// `title` on every control that says what it does (§2.1.2), and one `aria-valuetext`
// per slider so a screen reader hears "25 of 800 · default 25" rather than "25".

import type { Bounds, GameField } from "../core/game.ts";
import { applyField, boundOf, defaultGame, fieldsOf } from "../core/game.ts";
import type { GameInput } from "../core/protocol.ts";
import { COMMIT_DELAY_MS, createCoalescer, type Coalescer, type TimerHandle } from "./coalesce.ts";
import { PRESETS, presetGame } from "./presets.ts";
import { widthTooltip } from "./width-copy.ts";

/** Every string a visitor reads, in one place. Ticket 14 restyles, not rewrites. */
export const PLAYGROUND_TEXT = {
  legend: "Playground",
  presets: "Presets",
  /** The visible label and the tooltip, per game field. */
  fields: {
    seed: {
      label: "Seed",
      tip: "The whole game comes from the seed: the same seed is the same board and the same piece sequence.",
    },
    goal: {
      label: "Goal",
      tip: "How many lines the solve must clear to win. Changing it re-solves.",
    },
    pieces: {
      label: "Pieces",
      tip: "How many pieces the sequence holds — how long the game can run.",
    },
    cap: {
      label: "Starting height",
      tip: "How tall the pre-filled stack is; the piece that would cross it is never placed.",
    },
    beam: {
      label: "Beam width",
      tip: "Search width: how many states survive each layer. Narrower is cheaper and weaker.",
    },
  },
  expert: "Expert",
  /** The beam slider's readout: the value, the slider's end, and where the default sits. */
  beamReadout: (value: number, bounds: Bounds) =>
    `${value} of ${bounds.widthCap} · default ${bounds.defaultBeam}`,
  unit: { seed: "", goal: "lines", pieces: "pieces", cap: "rows", beam: "states per layer" },
} as const;

/** What the page gives the Playground, and the two things it may call back. */
export interface PlaygroundOptions {
  bounds: Bounds;
  /** One commit: a clamped game the visitor asked for. */
  onSolve(game: GameInput): void;
  /** Trailing delay for the commit tier. Defaults to `COMMIT_DELAY_MS`. */
  delayMs?: number;
  setTimer?(handler: () => void, ms: number): TimerHandle;
  clearTimer?(handle: TimerHandle): void;
}

/** A node the Playground just built, or a loud failure rather than a `!`. */
function mustBuild<T extends Element>(root: Element, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (!element) throw new Error(`the Playground did not build ${selector}`);
  return element;
}

/** One number field and the game field it edits. */
interface FieldView {
  readonly input: HTMLInputElement;
  readonly field: GameField;
}

export class Playground {
  private readonly bounds: Bounds;
  private readonly fields: Record<GameField, FieldView>;
  private readonly beamOutput: HTMLOutputElement;
  private readonly beamTick: HTMLElement;
  private readonly commit: Coalescer<GameInput>;
  private game: GameInput;

  constructor(root: HTMLElement, options: PlaygroundOptions) {
    this.bounds = options.bounds;
    this.game = defaultGame(options.bounds);
    this.commit = createCoalescer<GameInput>({
      delayMs: options.delayMs ?? COMMIT_DELAY_MS,
      setTimer: options.setTimer,
      clearTimer: options.clearTimer,
      onFire: (game) => options.onSolve(game),
    });

    root.replaceChildren();
    root.dataset.player = "playground";
    const legend = document.createElement("p");
    legend.className = "legend";
    legend.textContent = PLAYGROUND_TEXT.legend;
    root.append(legend);

    // ── presets ────────────────────────────────────────────────────────────
    const presets = document.createElement("div");
    presets.className = "presets";
    presets.setAttribute("role", "group");
    presets.setAttribute("aria-label", PLAYGROUND_TEXT.presets);
    const presetsLabel = document.createElement("span");
    presetsLabel.className = "presets-label";
    presetsLabel.textContent = PLAYGROUND_TEXT.presets;
    presets.append(presetsLabel);
    for (const preset of PRESETS) {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.preset = preset.id;
      button.textContent = preset.label;
      button.title = preset.tooltip(this.bounds);
      button.addEventListener("click", () => {
        // A preset commits at once: one click is one decision, not a drag.
        this.fill(presetGame(preset, this.bounds));
        this.commit.push(this.game);
        this.commit.flush();
      });
      presets.append(button);
    }
    root.append(presets);

    // ── the four game fields ───────────────────────────────────────────────
    const grid = document.createElement("div");
    grid.className = "fields";
    this.fields = {
      seed: this.numberField(grid, "seed"),
      goal: this.numberField(grid, "goal"),
      pieces: this.numberField(grid, "pieces"),
      cap: this.numberField(grid, "cap"),
      beam: this.rangeField(grid),
    };
    root.append(grid);

    this.beamOutput = mustBuild<HTMLOutputElement>(grid, "#beam-readout");
    this.beamTick = mustBuild<HTMLElement>(grid, "#beam-tick");
    this.refresh();
  }

  /** The game the controls currently name. */
  current(): GameInput {
    return this.game;
  }

  /** Put every control on a game (on load, and after a preset). */
  fill(game: GameInput): void {
    this.game = game;
    const values = fieldsOf(game);
    for (const { field, input } of Object.values(this.fields)) {
      input.value = String(values[field]);
    }
    this.refresh();
  }

  /** Fire a pending commit now: called when the page is about to be left. */
  flush(): void {
    this.commit.flush();
  }

  /** One labelled number field: the four values a visitor types. */
  private numberField(parent: HTMLElement, field: GameField): FieldView {
    const limit = boundOf(field, this.bounds);
    const text = PLAYGROUND_TEXT.fields[field];
    const id = `pg-${field}`;

    const label = document.createElement("label");
    label.htmlFor = id;
    label.textContent = text.label;
    label.title = text.tip;

    const input = document.createElement("input");
    input.type = "number";
    input.id = id;
    input.dataset.field = field;
    input.min = String(limit.min);
    input.max = String(limit.max);
    input.step = "1";
    input.title = text.tip;
    input.setAttribute("aria-label", `${text.label} (${limit.min} to ${limit.max})`);
    input.addEventListener("input", () => {
      // Display tier: the game follows the field immediately; the solve waits.
      this.game = applyField(this.game, field, input.value, this.bounds);
      this.paintReadouts();
      this.commit.push(this.game);
    });
    input.addEventListener("change", () => {
      // Commit tier: clamp what was typed, show the clamped value, and solve now.
      this.game = applyField(this.game, field, input.value, this.bounds);
      this.refresh();
      this.commit.push(this.game);
      this.commit.flush();
    });

    // One cell per field, label above its control, so the rail lays four of them
    // out as two by two rather than one tall column.
    const cell = document.createElement("div");
    cell.className = "field";
    cell.append(label, input);
    parent.append(cell);
    return { input, field };
  }

  /**
   * The expert width: a slider that opens at `DEFAULT_BEAM`, runs to `WIDTH_CAP`,
   * carries a labelled tick where the default sits, and explains the difference in
   * one tooltip line (§2.2, ticket bullet 7).
   */
  private rangeField(parent: HTMLElement): FieldView {
    const field: GameField = "beam";
    const limit = boundOf(field, this.bounds);
    const text = PLAYGROUND_TEXT.fields.beam;
    const id = "pg-beam";

    const label = document.createElement("label");
    label.htmlFor = id;
    label.textContent = text.label;
    label.title = widthTooltip();

    const wrap = document.createElement("div");
    wrap.className = "beam";

    const input = document.createElement("input");
    input.type = "range";
    input.id = id;
    input.dataset.field = field;
    input.min = String(limit.min);
    input.max = String(limit.max);
    input.step = "1";
    input.value = String(this.bounds.defaultBeam);
    input.title = widthTooltip();
    input.setAttribute("aria-label", `${text.label}, ${limit.min} to ${limit.max}`);
    input.addEventListener("input", () => {
      this.game = applyField(this.game, field, input.value, this.bounds);
      this.paintReadouts();
      this.commit.push(this.game);
    });
    input.addEventListener("change", () => {
      this.game = applyField(this.game, field, input.value, this.bounds);
      this.refresh();
      this.commit.push(this.game);
      this.commit.flush();
    });

    const tick = document.createElement("b");
    tick.id = "beam-tick";
    tick.className = "tick";
    tick.title = `Default ${this.bounds.defaultBeam}`;

    const output = document.createElement("output");
    output.id = "beam-readout";
    output.htmlFor = id;
    output.title = widthTooltip();

    wrap.append(input, tick);
    const cell = document.createElement("div");
    cell.className = "field field-beam";
    cell.append(label, wrap, output);
    parent.append(cell);
    return { input, field };
  }

  /**
   * Put every control on `this.game`. Only for a *programmatic* change — loading, a
   * preset, or the normalising `change` commit — because writing the value a visitor
   * is in the middle of typing would fight them.
   */
  private refresh(): void {
    const values = fieldsOf(this.game);
    for (const { field, input } of Object.values(this.fields)) {
      input.value = String(values[field]);
    }
    this.paintReadouts();
  }

  /** The display tier: the beam readout and the tick, on the frame the value moved. */
  private paintReadouts(): void {
    const values = fieldsOf(this.game);
    const beam = values.beam;
    this.fields.beam.input.value = String(beam);
    const readout = PLAYGROUND_TEXT.beamReadout(beam, this.bounds);
    this.beamOutput.textContent = readout;
    this.fields.beam.input.setAttribute("aria-valuetext", readout);
    // `(value - min) / (max - min)` is 0/0 for a one-width slider, so the tick is
    // omitted rather than placed at NaN (notes 13 §2.2's degenerate case).
    const span = this.bounds.widthCap - 1;
    if (span < 1) {
      this.beamTick.hidden = true;
      return;
    }
    this.beamTick.hidden = false;
    const fraction = (this.bounds.defaultBeam - 1) / span;
    this.beamTick.style.left = `${Math.min(1, Math.max(0, fraction)) * 100}%`;
    this.beamTick.textContent = `default ${this.bounds.defaultBeam}`;
  }
}
