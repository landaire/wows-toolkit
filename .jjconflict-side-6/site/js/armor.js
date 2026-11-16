// Armor zones assigned to the 3DBenchy parts, and a simplified shell model
// used by the background scene's armor mode. The numbers imitate the game's
// rules (overmatch, ricochet angles, normalization, fuse arming) closely
// enough to illustrate what the desktop armor viewer computes.

// The game's thickness buckets (max mm, color), from ArmorConstants.py. This
// matches ARMOR_COLOR_SCALE in wowsunpack's gltf_export.rs.
export const ARMOR_SCALE = [
    [14, [110, 209, 176]],
    [16, [149, 210, 127]],
    [24, [170, 201, 102]],
    [26, [192, 193, 80]],
    [28, [226, 195, 62]],
    [33, [225, 171, 54]],
    [75, [227, 144, 49]],
    [160, [230, 115, 49]],
    [399, [220, 78, 48]],
    [999, [185, 47, 48]],
];

export function armorColor(mm) {
    for (const [max, rgb] of ARMOR_SCALE) {
        if (mm <= max) return rgb;
    }
    return ARMOR_SCALE[ARMOR_SCALE.length - 1][1];
}

export const ZONES = [
    { id: 0, zone: "Citadel", plate: "Main Belt", mm: 305 },
    { id: 1, zone: "Citadel", plate: "Lower Belt", mm: 32 },
    { id: 2, zone: "Casemate", plate: "Upper Belt", mm: 152 },
    { id: 3, zone: "Bow", plate: "Plating", mm: 32 },
    { id: 4, zone: "Stern", plate: "Plating", mm: 27 },
    { id: 5, zone: "Citadel", plate: "Armored Deck", mm: 60 },
    { id: 6, zone: "Bow", plate: "Deck", mm: 25 },
    { id: 7, zone: "Hull", plate: "Bulwark", mm: 19 },
    { id: 8, zone: "Citadel", plate: "Magazine", mm: 406 },
    { id: 9, zone: "Superstructure", plate: "Bridge", mm: 16 },
    { id: 10, zone: "Superstructure", plate: "Roof", mm: 13 },
    { id: 11, zone: "Superstructure", plate: "Funnel", mm: 19 },
    { id: 12, zone: "Superstructure", plate: "Funnel Cap", mm: 10 },
    { id: 13, zone: "Superstructure", plate: "Door", mm: 6 },
    { id: 14, zone: "Superstructure", plate: "Window", mm: 6 },
    { id: 15, zone: "Stern", plate: "Name Plate", mm: 26 },
    { id: 16, zone: "Conning Tower", plate: "Wheel", mm: 457 },
    { id: 17, zone: "Superstructure", plate: "Rangefinder", mm: 13 },
    { id: 18, zone: "Bow", plate: "Hawsepipe", mm: 25 },
    { id: 19, zone: "Stern", plate: "Deck", mm: 25 },
];

const WATERLINE = 5;

// Zone of a triangle, from its part and its centroid in model space
// (millimetres, bow toward +x, keel at y = 0).
export function zoneFor(part, cx, cy) {
    switch (part) {
        case "hull":
            if (cx > 16) return 3;
            if (cx < -21) return 4;
            if (cy < WATERLINE - 2) return 1;
            if (cy < WATERLINE + 6) return 0;
            return 2;
        case "deck":
            if (cx > 14) return 6;
            if (cx < -20) return 19;
            return 5;
        case "gunwale":
            return 7;
        case "cargo_box":
            return 8;
        case "bridge_walls":
            return 9;
        case "bridge_roof":
            return 10;
        case "chimney":
            return 11;
        case "chimney_top":
            return 12;
        case "door_port":
        case "door_starboard":
            return 13;
        case "window":
        case "stern_window":
            return 14;
        case "name_plate":
            return 15;
        case "wheel":
            return 16;
        case "rod_holder":
            return 17;
        default:
            return 18;
    }
}

export const SHELLS = {
    ap203: { label: "203 mm AP", kind: "ap", caliber: 203, pen0: 340, ricStart: 45, ricAlways: 60 },
    ap460: { label: "460 mm AP", kind: "ap", caliber: 460, pen0: 880, ricStart: 45, ricAlways: 60 },
    he152: { label: "152 mm HE", kind: "he", caliber: 152, pen0: 25 },
};

export function penetrationAt(shell, rangeKm) {
    if (shell.kind === "he") return shell.pen0;
    return shell.pen0 * Math.exp(-rangeKm / 28);
}

export function fallAngleDeg(rangeKm) {
    return 2 + rangeKm * 1.2;
}

// Distance the shell travels in model units after its fuse arms. One model
// unit is roughly 4 m at the scale of a 250 m ship.
const FUSE_UNITS = 4;

// Resolves a shell against the ordered entry and exit hits along its path.
// Each hit: { t, entry, zone, angleDeg, part }.
// Returns { plates, outcome, detonationT, stopT, ricochetIndex }.
export function resolveShot(shell, rangeKm, hits) {
    let pen = penetrationAt(shell, rangeKm);
    const plates = [];
    const inside = [];
    let armedAt = null;
    let outcome = null;
    let detonationT = null;
    let stopT = null;
    let ricochetIndex = null;

    for (const hit of hits) {
        if (armedAt !== null && hit.t >= armedAt + FUSE_UNITS) {
            detonationT = armedAt + FUSE_UNITS;
            break;
        }
        if (!hit.entry) {
            const i = inside.lastIndexOf(hit.part);
            if (i >= 0) inside.splice(i, 1);
            continue;
        }
        const z = ZONES[hit.zone];
        const row = { ...z, angle: hit.angleDeg, t: hit.t, effective: 0, result: "" };
        plates.push(row);

        if (shell.kind === "he") {
            row.effective = z.mm;
            if (z.mm <= pen) {
                row.result = "PEN";
                outcome = "HE_PEN";
            } else {
                row.result = "SHATTER";
                outcome = "SHATTER";
            }
            detonationT = hit.t;
            break;
        }

        const overmatch = shell.caliber / 14.3 > z.mm;
        if (!overmatch && hit.angleDeg >= shell.ricAlways) {
            row.result = "RICOCHET";
            outcome = "RICOCHET";
            stopT = hit.t;
            ricochetIndex = plates.length - 1;
            break;
        }
        // Normalization turns the shell up to 6 degrees toward the plate normal.
        const norm = Math.max(0, hit.angleDeg - 6);
        const eff = z.mm / Math.cos((norm * Math.PI) / 180);
        row.effective = eff;
        if (!overmatch && hit.angleDeg >= shell.ricStart) {
            const chance = Math.round(((hit.angleDeg - shell.ricStart) / (shell.ricAlways - shell.ricStart)) * 100);
            row.note = `RICOCHET ${chance}%`;
        }
        if (overmatch) row.note = "OVERMATCH";
        if (eff > pen) {
            row.result = "NO PEN";
            outcome = "NO_PEN";
            stopT = hit.t;
            break;
        }
        pen -= eff;
        row.result = "PEN";
        row.remaining = pen;
        inside.push(hit.part);
        if (armedAt === null && z.mm >= shell.caliber / 6) armedAt = hit.t;
    }

    if (outcome === null) {
        if (detonationT !== null) {
            outcome = inside.includes("cargo_box") ? "CITADEL" : "PEN";
        } else if (plates.length === 0) {
            outcome = "MISS";
        } else {
            outcome = armedAt === null ? "OVERPEN" : "OVERPEN";
        }
    }
    return { plates, outcome, detonationT, stopT, ricochetIndex };
}

export const OUTCOME_LABEL = {
    CITADEL: "CITADEL",
    PEN: "PENETRATION",
    HE_PEN: "HE PENETRATION",
    OVERPEN: "OVERPENETRATION",
    RICOCHET: "RICOCHET",
    NO_PEN: "NO PENETRATION",
    SHATTER: "SHATTER",
    MISS: "MISS",
};
