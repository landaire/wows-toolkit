import { signal } from "nsh";
import { Armor } from "wows-toolkit/native";
import * as toast from "../shell/toast";

export type Phase = "notConfigured" | "pending" | "failed" | "ready";
export type TriState = "off" | "partial" | "on";

export type CamoView = { id: number; name: string; selected: boolean };
export type CamoGroup = { heading: string; schemes: CamoView[] };

export type PlateView = { tenths: number; label: string; color: string; checked: boolean };
export type PartView = { name: string; label: string; state: TriState; single: boolean; plates: PlateView[] };
export type ZoneView = { name: string; label: string; state: TriState; parts: PartView[] };

export type ArcView = {
  number: number;
  color: string;
  heading: string;
  summary: string;
  range: number;
  isolatingPlates: boolean;
  isolatingZones: boolean;
  outcome: string;
  outcomeKind: "detonated" | "overpenetrated" | "ricocheted" | "shattered" | "stopped" | "notSimulated";
  canStepDown: boolean;
  canStepUp: boolean;
};

export type HullView = {
  opaque: boolean;
  groups: { name: string; state: TriState; meshes: { name: string; visible: boolean }[] }[];
  upgrades: { key: string; label: string; selected: boolean }[];
  modules: { component: string; options: { name: string; label: string; selected: boolean }[] }[];
  camo?: { selected?: number; own: CamoView[]; groups: CamoGroup[] } | null;
  lods: { lod: number; selected: boolean }[];
};

export type DisplayView = {
  legendVisible: boolean;
  plateEdges: boolean;
  waterline: boolean;
  waterlineOpacity: number;
  zeroMm: boolean;
  shipCenter: boolean;
  armorOpacity: number;
  markerOpacity: number;
  hitsDrawn: boolean;
  roll: number;
  rings: {
    available: boolean;
    shown: boolean;
    modes: { name: string; selected: boolean }[];
    fov: number;
    height: number;
    zoomPath: boolean;
    zoomPathAtFov: boolean;
    zoomPathAtMaxFov: boolean;
  };
  perspective: { enabled: boolean; fov: number; look: "game" | "throughCenter" };
  lighting: {
    enabled: boolean;
    flat: number;
    intensity: number;
    azimuth: number;
    elevation: number;
    rim: number;
    specular: number;
    shininess: number;
    ambientColor: string;
    keyColor: string;
  };
};

export type ActiveView = {
  paneId: number;
  hasArmor: boolean;
  ship?: string | null;
  showHidden: boolean;
  gaps: { shown: boolean; count: number };
  splash: { shown: boolean; boxesShown: boolean; hasData: boolean; zonesReached: number };
  trajectory: { shown: boolean; count: number; continueOnRicochet: boolean; castRange: number };
  arcs: ArcView[];
  hasPlateOverrides: boolean;
  zones: ZoneView[];
  hull?: HullView | null;
  display: DisplayView;
};

export type PaneView = {
  id: number;
  surface: string;
  number: number;
  active: boolean;
  ship?: string | null;
  paramIndex?: string | null;
  loading?: string | null;
  loadError?: string | null;
  hasArmor: boolean;
  exportConfirm: boolean;
  hiddenPlates: number;
  health?: number | null;
  hits: number;
  shotLog: { at: string; outcome: string }[];
  readout?: string | null;
};

export type ShellVerdict = "penetrates" | "shatters" | "angleDependent" | "noData" | "awaitingPlate";

export type PenShipView = {
  paramIndex: string;
  label: string;
  firing: boolean;
  shells: { index: number; label: string; pen: string; verdict: ShellVerdict; verdictText: string; firing: boolean }[];
};

export type PenView = {
  ifhe: boolean;
  search: string;
  results: { paramIndex: string; label: string }[];
  plate?: { mm: number; zone: string } | null;
  ships: PenShipView[];
  firingShip?: string | null;
  firingShell?: number | null;
};

export type HealthStripView = {
  line: { x: number; y: number }[];
  hits: number[];
  cursor?: number | null;
  firstSeconds: number;
  lastSeconds: number;
};

export type IncomingView = {
  strip?: HealthStripView | null;
  tracked: string;
  simUnchecked?: string | null;
  secondaries: boolean;
  attackers: { entity?: number | null; label: string; selected: boolean }[];
  salvos: {
    attacker: string;
    clock: string;
    seconds: number;
    count: string;
    shells: { text: string; sim?: { label: string; standing: "agrees" | "deferred" | "disagrees" } | null }[];
  }[];
};

export type ExportContents = "mesh" | "armor" | "meshAndArmor";
export type TextureResolution = "full" | "px2048" | "px1024" | "px512";

export type ExportDialogView = {
  ship: string;
  contents: ExportContents;
  hulls: { key: string; label: string; selected: boolean }[];
  hullsDisabled: boolean;
  lodCount: number;
  lod: number;
  textured: boolean;
  resolution: TextureResolution;
  camos: { noneTicked: boolean; own: CamoView[]; groups: CamoGroup[] };
  ready: boolean;
  error?: string | null;
  size: { kind: "waiting" | "silent" | "known" | "incomplete"; estimate?: string | null; breakdown?: string | null; incomplete?: string | null };
};

export type LegendView = { visible: boolean; collapsed: boolean; x: number; y: number; entries: { label: string; color: string }[] };

export type ArmorState = {
  status: Phase;
  statusReason?: string | null;
  gpu: "initializing" | "ready" | "failed";
  gpuError?: string | null;
  revealRequest: number;
  shipLoaded: boolean;
  mirrorCameras: boolean;
  syncOptions: boolean;
  stackPanes: boolean;
  optionsOpen: boolean;
  penCheckOpen: boolean;
  activePane: number;
  panes: PaneView[];
  active: ActiveView;
  legend: LegendView;
  pen: PenView;
  incoming?: IncomingView | null;
  exportDialog?: ExportDialogView | null;
};

export type HoverView = {
  paneId?: number | null;
  x: number;
  y: number;
  plate?: {
    zone: string;
    material: string;
    tenths: number;
    mm: number;
    color: string;
    zoneLabel: string;
    materialLabel: string;
    hidden: boolean;
    layers: { mm: number; color: string; current: boolean }[];
  } | null;
  ring?: string[] | null;
};

export type CatalogShip = { paramIndex: string; name: string; tier: number; roman: string; label: string };
export type CatalogView = {
  nations: { nation: string; label: string; classes: { species: string; label: string; ships: CatalogShip[] }[] }[];
};

export type ExportResult = { ship: string; path: string; bytes?: number | null; size?: string | null };

/** The latest view of the armor service; null until its first post. */
export const armor = signal<ArmorState | null>(null);
/** The plate or camera ring under the pointer. */
export const hover = signal<HoverView | null>(null);
/** The full ship catalog, read once the service reports it ready. */
export const catalog = signal<CatalogView | null>(null);

const revealListeners: (() => void)[] = [];
let lastReveal: number | null = null;
let watching = false;
let started = false;
let catalogRead = false;

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Runs one service command; a refusal becomes a failure toast. */
export function call(command: Promise<unknown>) {
  void command.catch((error) => toast.failed(errorText(error)));
}

/** Tells the shell to show the Armor Viewer tab. */
export function revealArmor() {
  for (const listener of revealListeners) listener();
}

function receive(next: ArmorState) {
  armor.set(next);
  if (lastReveal !== null && next.revealRequest !== lastReveal) revealArmor();
  lastReveal = next.revealRequest;
  // A new game build loads the catalog again; read it again once it is ready.
  if (next.status !== "ready") {
    catalogRead = false;
  } else if (!catalogRead) {
    catalogRead = true;
    void Armor.catalog().then((read: CatalogView) => catalog.set(read)).catch((error: unknown) => {
      catalogRead = false;
      toast.failed(errorText(error));
    });
  }
}

/** Follows the service's views from now on; the views read it whether or not the tab shows. */
export function ensureWatching() {
  if (watching) return;
  watching = true;
  call(Armor.watch(receive));
  call(Armor.watchHover((next: HoverView) => hover.set(next)));
}

/** Starts the viewport device and the catalog load, the first time the tab shows. */
export function ensureStarted() {
  if (started) return;
  started = true;
  void Armor.start().then(receive).catch((error: unknown) => {
    started = false;
    toast.failed(errorText(error));
  });
}

/** Calls `listener` whenever the Armor Viewer asks to be shown: a replay's request, or a ship picked in the palette. */
export function onArmorReveal(listener: () => void) {
  ensureWatching();
  revealListeners.push(listener);
}

/** Folds accents and case the way the service folds ship names. */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
}
