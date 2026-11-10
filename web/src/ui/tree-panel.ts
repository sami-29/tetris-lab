// The inspector's three views: the beam (the default), the tree (the drill-down) and
// the per-layer table. Each takes the whole column while it is the view.
//
// This file is the wiring between `tree-model.ts` (the picture as data) and the
// page: it owns the view toggle, the tree's **own** bound control, the canvas, the
// legend and the selected state's card. It solves nothing, counts nothing and
// re-derives nothing — every number on screen comes from the decoded trace the band
// already read, which is what makes the drill-down free (§2.3: "the band and the
// tree come from the engine's `solve_traced`, not from re-running anything in JS").
//
// Two rules are structural here, not conventions:
//
//   1. **The bound is the tree's number, never the beam's.** The control lives in
//      the tree's own section, it is clamped to `min(60, band)`, and moving it
//      rebuilds the model from the trace already in memory: no `solve`, no URL
//      write, no worker message. Nothing in this file can reach the solver.
//   2. **The band is the default view.** The tree is opened on demand and the
//      toggle's words are "Beam" and "Tree" — the band *is* the beam, so it is
//      never called a band on screen.
//
// The selected state's Board is the one thing the trace cannot carry (§4.4 is five
// `i32`): `boardAt` is the page's answer, and it can only answer for Boards it
// already holds. When it cannot, the card says so in words — never a blank
// rectangle, never a plausible-looking wrong Board.

import { BoardView } from "./board.ts";
import { type Trace } from "./trace.ts";
import {
  TREE_BOUND_DEFAULT,
  TREE_COPY,
  buildTree,
  clampTreeBound,
  pickState,
  stateCard,
  stateCardLayout,
  treeBoundLimit,
  type TreeModel,
  type TreeRef,
} from "./tree-model.ts";
import { TreeView } from "./tree-canvas.ts";

export type TreeViewName = "beam" | "tree" | "layers";

/** Where a state's Board can come from, when anything holds it. */
export type BoardAt = (layer: number, rank: number) => ArrayLike<number> | null;

export interface TreePanelOptions {
  /** The band's own section, shown while "Beam" is the view. */
  bandPanel: HTMLElement;
  /** The view buttons, which sit beside the tree's section rather than in it. */
  views: HTMLElement;
  /** The per-layer table's section, shown while "Layers" is the view. */
  layersPanel?: HTMLElement;
  /** The selected state moved; the page may keep other views in step. */
  onSelection?: (ref: TreeRef | null) => void;
}

/** The inspector's views and the tree's controls, as one object. */
export class TreePanel {
  readonly root: HTMLElement;
  private readonly options: TreePanelOptions;
  private readonly beamButton: HTMLButtonElement;
  private readonly treeButton: HTMLButtonElement;
  private readonly layersButton: HTMLButtonElement | null;
  private readonly boundSlider: HTMLInputElement;
  private readonly boundReadout: HTMLOutputElement;
  private readonly caption: HTMLElement;
  private readonly legend: HTMLElement;
  private readonly facts: HTMLElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly boardCanvas: HTMLCanvasElement;
  private readonly treeView: TreeView;
  private readonly boardView: BoardView;

  private trace: Trace | null = null;
  private sequence: string | undefined;
  private boardAt: BoardAt | null = null;
  private bound = TREE_BOUND_DEFAULT;
  private selection: TreeRef | null = null;
  private model: TreeModel | null = null;
  private view: TreeViewName = "beam";

  constructor(root: HTMLElement, options: TreePanelOptions) {
    this.root = root;
    this.options = options;
    this.beamButton = must(options.views, "[data-view='beam']");
    this.treeButton = must(options.views, "[data-view='tree']");
    this.layersButton = options.views.querySelector<HTMLButtonElement>("[data-view='layers']");
    this.boundSlider = must(root, "#tree-bound");
    this.boundReadout = must(root, "#tree-bound-readout");
    this.caption = must(root, "#tree-caption");
    this.legend = must(root, "#tree-legend");
    this.facts = must(root, "#tree-facts");
    this.canvas = must(root, "#tree");
    this.boardCanvas = must(root, "#tree-board");
    this.treeView = new TreeView(this.canvas);
    this.boardView = new BoardView(this.boardCanvas);

    this.beamButton.textContent = TREE_COPY.views[0];
    this.treeButton.textContent = TREE_COPY.views[1];
    this.beamButton.addEventListener("click", () => this.setView("beam"));
    this.treeButton.addEventListener("click", () => this.setView("tree"));
    this.layersButton?.addEventListener("click", () => this.setView("layers"));

    this.boundSlider.min = String(Math.min(TREE_BOUND_DEFAULT, 60));
    this.boundSlider.max = "60";
    this.boundSlider.step = "2";
    this.boundSlider.value = String(TREE_BOUND_DEFAULT);
    this.boundSlider.title = TREE_COPY.boundTooltip;
    this.boundSlider.setAttribute("aria-label", TREE_COPY.boundLabel);
    this.boundSlider.addEventListener("input", () => this.setBound(Number(this.boundSlider.value)));

    this.legend.replaceChildren(
      ...TREE_COPY.legend.map((words) => {
        const item = document.createElement("li");
        item.textContent = words;
        return item;
      }),
    );
    this.caption.textContent = TREE_COPY.emptyTrace;
    this.facts.textContent = "click a state to see its Board";
    this.canvas.addEventListener("pointerdown", (event) => this.pick(event));
    this.setView("beam");
  }

  /** The view on screen: the beam by default, the tree on demand. */
  get viewName(): TreeViewName {
    return this.view;
  }

  /** The tree's bound — its own number, never the beam's. */
  get treeBound(): number {
    return this.bound;
  }

  get selectionRef(): TreeRef | null {
    return this.selection;
  }

  get treeModel(): TreeModel | null {
    return this.model;
  }

  /**
   * Hand the panel the trace the band just read. Same `decodeTrace` output, so the
   * two views cannot disagree about a record. No solve, no worker, no wasm.
   */
  setTrace(trace: Trace | null, sequence: string | undefined, boardAt: BoardAt | null): void {
    this.trace = trace;
    this.sequence = sequence;
    this.boardAt = boardAt;
    // The bound survives a new solve (it is the visitor's setting), but the band is
    // the ceiling, so a narrower trace lowers it rather than lying about records.
    this.bound = clampTreeBound(this.bound, trace === null ? TREE_BOUND_MAX_SAFE : trace.band);
    const limit = treeBoundLimit(trace === null ? 60 : trace.band);
    this.boundSlider.max = String(limit);
    this.boundSlider.min = String(Math.min(4, limit));
    this.boundSlider.value = String(this.bound);
    if (this.selection !== null && trace !== null && !hasState(trace, this.selection)) {
      this.selection = null;
      this.options.onSelection?.(null);
    }
    this.draw();
  }

  /** Switch views. The band is the default; the tree and the table are on demand. */
  setView(view: TreeViewName): void {
    const next = view === "layers" && !this.options.layersPanel ? "beam" : view;
    this.view = next;
    this.options.bandPanel.hidden = next !== "beam";
    this.root.hidden = next !== "tree";
    if (this.options.layersPanel) this.options.layersPanel.hidden = next !== "layers";
    const buttons: Array<[HTMLButtonElement | null, TreeViewName]> = [
      [this.beamButton, "beam"],
      [this.treeButton, "tree"],
      [this.layersButton, "layers"],
    ];
    for (const [button, name] of buttons) {
      if (!button) continue;
      button.setAttribute("aria-selected", String(next === name));
      button.dataset.selected = String(next === name);
    }
    if (next === "tree") this.draw();
  }

  /** Move the bound and rebuild from the trace in memory. Calls nothing. */
  setBound(bound: number): void {
    const limit = treeBoundLimit(this.trace?.band ?? 60);
    this.bound = clampTreeBound(bound, this.trace?.band ?? 60);
    this.boundSlider.value = String(this.bound);
    this.boundSlider.max = String(limit);
    this.draw();
  }

  /** Select a state, or clear the selection when it is already selected. */
  select(ref: TreeRef | null): void {
    const same = ref !== null && this.selection !== null && this.selection.layer === ref.layer && this.selection.rank === ref.rank;
    this.selection = same || ref === null ? null : { layer: ref.layer, rank: ref.rank };
    this.options.onSelection?.(this.selection);
    this.draw();
  }

  clearSelection(): void {
    if (this.selection === null) return;
    this.selection = null;
    this.options.onSelection?.(null);
    this.draw();
  }

  resize(): void {
    this.treeView.resize();
    this.boardView.resize();
  }

  /** Rebuild the model from the measured canvas and repaint everything. */
  draw(): void {
    if (this.view !== "tree" || this.root.hidden) {
      // The tree is not on screen, so there is no canvas to measure — but the words
      // that describe it must still be the trace's, or opening the view would show
      // the previous game's sentence for a frame.
      this.summarise();
      return;
    }
    this.treeView.resize();
    this.model = this.trace === null ? null : buildTree(this.trace, {
      bound: this.bound,
      selection: this.selection,
      width: Math.max(1, Math.floor(this.canvas.getBoundingClientRect().width)),
      height: Math.max(1, Math.floor(this.canvas.getBoundingClientRect().height)),
      sequence: this.sequence,
    });
    this.treeView.draw(this.model);
    const model = this.model;
    if (model === null) {
      this.caption.textContent = TREE_COPY.emptyTrace;
      this.boundReadout.textContent = "—";
      return;
    }
    this.caption.textContent = `${model.caption} · ${model.label}`;
    this.boundReadout.textContent = model.label;
    this.boundSlider.setAttribute("aria-valuetext", model.label);
    this.drawCard();
  }

  /** The tree's one-line description, from the trace alone: no canvas needed. */
  private summarise(): void {
    const trace = this.trace;
    if (trace === null || trace.records.length === 0) {
      this.caption.textContent = TREE_COPY.emptyTrace;
      this.boundReadout.textContent = "—";
      return;
    }
    const widest = Math.max(...trace.perLayerCounts);
    const drawn = trace.perLayerCounts.reduce((sum, count) => sum + Math.min(this.bound, count), 0);
    this.caption.textContent = `the tree · top ${this.bound} of the ${widest} kept per Layer · traced at band ${trace.band} · ${trace.records.length} records over ${trace.layerCount} Layers`;
    this.boundReadout.textContent = `top ${this.bound} of the ${widest} kept · ${drawn} states drawn`;
  }

  /** The selected state's Board and facts, or the invitation to pick one. */
  private drawCard(): void {
    const model = this.model;
    const box = this.root.getBoundingClientRect();
    // What is left for the card after the controls, the caption, the legend and the
    // columns: the card yields height first, never the tree's 24 rows.
    const cell = stateCardLayout(box.width || 420, box.height - 150);
    this.boardCanvas.style.width = `${cell.width}px`;
    this.boardCanvas.style.height = `${cell.height}px`;
    if (model === null || this.selection === null) {
      this.boardCanvas.hidden = true;
      this.facts.textContent = "click a state to see its Board";
      return;
    }
    const card = stateCard(model, this.selection, { boardAt: this.boardAt ?? undefined });
    this.boardCanvas.hidden = card.board === null;
    if (card.board !== null) {
      this.boardView.resize();
      this.boardView.draw(Uint32Array.from(card.board));
    }
    const divergence =
      card.divergence === null
        ? ""
        : ` · this path left the lineage at layer ${card.divergence.layer} (rank ${card.divergence.rank}; the lineage took ${card.divergence.lineageRank})`;
    this.facts.textContent = card.boardNote === null ? `${card.facts}${divergence}` : `${card.facts}${divergence} · ${card.boardNote}`;
  }

  /** A click resolves to the nearest state within the hit radius. */
  private pick(event: PointerEvent): void {
    if (this.model === null) return;
    const rect = this.canvas.getBoundingClientRect();
    const ref = pickState(this.model, event.clientX - rect.left, event.clientY - rect.top);
    this.select(ref);
  }
}

/** The largest bound the control offers, so a trace-less panel still has a range. */
const TREE_BOUND_MAX_SAFE = 60;

function must<T extends Element>(root: HTMLElement, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (!element) throw new Error(`index.html is missing ${selector} inside the tree panel`);
  return element;
}

function hasState(trace: Trace, ref: TreeRef): boolean {
  if (ref.layer < 0 || ref.layer >= trace.layerCount) return false;
  return ref.rank >= 0 && ref.rank < trace.perLayerCounts[ref.layer];
}
