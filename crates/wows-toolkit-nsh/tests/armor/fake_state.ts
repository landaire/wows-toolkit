import type { ArmorState, ExportDialogView, ZoneView } from "../../ui/armor/state";

const plate = (tenths: number, checked: boolean) => ({ tenths, label: `${tenths / 10} mm`, color: "#aa4444", checked });

/** A citadel with one of its two parts hidden. */
export const partialZone: ZoneView = {
  name: "Citadel",
  label: "Citadel",
  state: "partial",
  parts: [
    { name: "Belt", label: "Belt", state: "on", single: true, plates: [plate(1000, true)] },
    { name: "Deck", label: "Deck", state: "off", single: true, plates: [plate(500, false)] },
  ],
};

/** A loaded ship in one pane, with `zones` and `exportDialog`. */
export function fakeArmor(zones: ZoneView[], exportDialog: ExportDialogView | null): ArmorState {
  return {
    status: "ready",
    gpu: "ready",
    revealRequest: 0,
    shipLoaded: true,
    mirrorCameras: false,
    syncOptions: false,
    stackPanes: false,
    optionsOpen: true,
    penCheckOpen: false,
    activePane: 1,
    panes: [{
      id: 1,
      surface: "armor-pane-1",
      number: 1,
      active: true,
      ship: "Test Ship",
      paramIndex: "PTSB001",
      hasArmor: true,
      shipGeneration: 1,
      exportConfirm: false,
      hiddenPlates: 0,
      hits: 0,
      shotLog: [],
    }],
    active: {
      paneId: 1,
      hasArmor: true,
      ship: "Test Ship",
      showHidden: false,
      gaps: { shown: false, count: 0 },
      splash: { shown: false, boxesShown: false, hasData: false, zonesReached: 0 },
      trajectory: { shown: false, count: 0, continueOnRicochet: false, castRange: 10 },
      arcs: [],
      hasPlateOverrides: false,
      zones,
      hull: null,
      display: {
        legendVisible: true,
        plateEdges: false,
        waterline: false,
        waterlineOpacity: 0.5,
        zeroMm: false,
        shipCenter: false,
        armorOpacity: 1,
        markerOpacity: 1,
        hitsDrawn: false,
        roll: 0,
        rings: { available: false, shown: false, modes: [], fov: 30, height: 10, zoomPath: false, zoomPathAtFov: false, zoomPathAtMaxFov: false },
        perspective: { enabled: false, fov: 30, look: "game" },
        lighting: { enabled: false, flat: 0, intensity: 1, azimuth: 0, elevation: 45, rim: 0, specular: 0, shininess: 16, ambientColor: "#ffffff", keyColor: "#ffffff" },
      },
    },
    legend: { visible: false, collapsed: false, x: 0, y: 0, entries: [] },
    pen: { ifhe: false, search: "", results: [], ships: [] },
    exportDialog,
  };
}

/** The export dialog for a ship with two of its own camos, exporting `contents`. */
export function fakeExportDialog(contents: ExportDialogView["contents"]): ExportDialogView {
  return {
    ship: "Test Ship",
    contents,
    hulls: [],
    hullsDisabled: false,
    lodCount: 1,
    lod: 0,
    textured: true,
    resolution: "full",
    camos: {
      noneTicked: false,
      own: [{ id: 1, name: "Type 1", selected: true }, { id: 2, name: "Type 2", selected: false }],
      groups: [],
    },
    ready: true,
    size: { kind: "silent" },
  };
}
