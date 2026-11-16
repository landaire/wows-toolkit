// Background scene: the 3DBenchy on a swell that the reader's cursor
// disturbs, with an armor mode that colors the hull by plate thickness and
// traces simulated shells through it. Everything is drawn into an offscreen
// target and passed through an ordered-dither pass.

import * as THREE from "../vendor/three/three.module.min.js";
import { ARMOR_SCALE, ZONES, zoneFor, SHELLS, fallAngleDeg, resolveShot } from "./armor.js";

THREE.ColorManagement.enabled = false;

// Upper bound on the energy cursor movement can add to the swell.
const MAX_ENERGY = 2.5;
const WATER_SIZE = 900;
const WATER_SEGMENTS = 320;
const WATERLINE = 5;
const TRANSITION_MS = 2200;
// Zone attribute value telling the boat shader to resolve the zone per fragment.
const HULL_PER_FRAGMENT = 100;
const AXES = [
    ["x", new THREE.Vector3(1, 0, 0)],
    ["y", new THREE.Vector3(0, 1, 0)],
    ["z", new THREE.Vector3(0, 0, 1)],
];

const PALETTES = {
    dark: { bg: 0x07090d, ink: 0xc9d6e8, dim: 0x3a475a, accent: 0xffb020, cool: 0x5ad1ff },
    light: { bg: 0xebe7dd, ink: 0x17191d, dim: 0x9a9483, accent: 0xd9481a, cool: 0x1f5fbf },
};

// Swell shared by the water shader and the boat's buoyancy samples. uChop
// rises with the energy the reader's cursor puts into the sea and adds
// shorter, steeper waves on top of the base swell.
const SWELL_GLSL = /* glsl */ `
uniform float uTime;
uniform float uSwellAmp;
uniform float uChop;

float swell(vec2 p, float t) {
    float h = 0.0;
    h += 1.10 * sin(dot(p, vec2(0.031, 0.012)) + t * 0.90);
    h += 0.55 * sin(dot(p, vec2(-0.017, 0.043)) + t * 1.30);
    h += 0.28 * sin(dot(p, vec2(0.071, -0.052)) + t * 2.10);
    h += 0.12 * sin(dot(p, vec2(-0.13, -0.09)) + t * 3.20);
    h += uChop * 0.45 * sin(dot(p, vec2(0.052, 0.061)) + t * 1.90);
    h += uChop * 0.22 * sin(dot(p, vec2(-0.094, 0.071)) + t * 2.70);
    return h * uSwellAmp;
}
`;

function swellJs(x, z, t, amp, chop) {
    let h = 0;
    h += 1.1 * Math.sin(x * 0.031 + z * 0.012 + t * 0.9);
    h += 0.55 * Math.sin(x * -0.017 + z * 0.043 + t * 1.3);
    h += 0.28 * Math.sin(x * 0.071 + z * -0.052 + t * 2.1);
    h += 0.12 * Math.sin(x * -0.13 + z * -0.09 + t * 3.2);
    h += chop * 0.45 * Math.sin(x * 0.052 + z * 0.061 + t * 1.9);
    h += chop * 0.22 * Math.sin(x * -0.094 + z * 0.071 + t * 2.7);
    return h * amp;
}

const WATER_VERT = /* glsl */ `
${SWELL_GLSL}
uniform float uDrop;
varying vec3 vWorld;
varying float vH;
void main() {
    vec3 p = position;
    float h = swell(p.xz, uTime);
    p.y += h - uDrop;
    vH = h;
    vec4 w = modelMatrix * vec4(p, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
}
`;

const WATER_FRAG = /* glsl */ `
uniform vec3 uBg;
uniform vec3 uInk;
uniform vec3 uDim;
uniform vec3 uAccent;
uniform vec3 uCool;
uniform float uArmor;
uniform float uDark;
uniform vec3 uCam;
varying vec3 vWorld;
varying float vH;

float lineMask(float coord, float spacing, float widthPx) {
    float d = abs(fract(coord / spacing - 0.5) - 0.5) * spacing;
    float w = fwidth(coord);
    return 1.0 - smoothstep(w * widthPx * 0.5, w * (widthPx * 0.5 + 1.0), d);
}

void main() {
    float dist = distance(vWorld.xz, uCam.xz);
    float fog = smoothstep(90.0, 430.0, dist);

    // Ridge lines run across the view; the grid lines along it stay faint.
    float rows = lineMask(vWorld.z, mix(7.0, 10.0, uArmor), 1.1);
    float cols = lineMask(vWorld.x, 10.0, 0.8) * mix(0.18, 0.55, uArmor);
    float crest = smoothstep(-0.5, 2.6, vH);

    vec3 lineColor = mix(uDim, uInk, crest * (1.0 - uArmor) + 0.25);
    lineColor = mix(lineColor, uAccent, smoothstep(1.6, 3.4, vH) * (1.0 - uArmor));
    float lines = max(rows * mix(0.55, 1.0, crest), cols);

    vec3 base = uBg + (uInk - uBg) * (0.018 + 0.02 * crest) * (1.0 - uArmor);
    vec3 col = mix(base, lineColor, lines * mix(0.85, 0.5, uArmor));
    col = mix(col, uBg, fog);
    gl_FragColor = vec4(col, 1.0);
}
`;

const BOAT_VERT = /* glsl */ `
attribute float aThickness;
attribute float aZone;
varying vec3 vNormalW;
varying vec3 vWorld;
varying vec3 vLocal;
varying float vThickness;
varying float vZone;
void main() {
    vLocal = position;
    vThickness = aThickness;
    vZone = aZone;
    vec4 w = modelMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    vNormalW = normalize(mat3(modelMatrix) * normal);
    gl_Position = projectionMatrix * viewMatrix * w;
}
`;

const ARMOR_GLSL = /* glsl */ `
vec3 armorColor(float mm) {
${ARMOR_SCALE.map(([max, [r, g, b]], i) => `    ${i === 0 ? "" : "else "}if (mm <= ${max.toFixed(1)}) return vec3(${(r / 255).toFixed(4)}, ${(g / 255).toFixed(4)}, ${(b / 255).toFixed(4)});`).join("\n")}
    return vec3(0.7255, 0.1843, 0.1882);
}
`;

const BOAT_FRAG = /* glsl */ `
${ARMOR_GLSL}
uniform vec3 uBg;
uniform vec3 uInk;
uniform vec3 uAccent;
uniform vec3 uCool;
uniform float uArmor;
uniform float uDark;
uniform vec3 uCam;
uniform vec4 uSection;
uniform float uHoverZone;
uniform float uSweep;
uniform float uTime;
varying vec3 vNormalW;
varying vec3 vWorld;
varying vec3 vLocal;
varying float vThickness;
varying float vZone;

void main() {
    if (uSection.w < 900.0 && dot(vLocal, uSection.xyz) > uSection.w) discard;

    vec3 n = normalize(vNormalW);
    vec3 v = normalize(uCam - vWorld);
    if (!gl_FrontFacing) n = -n;
    vec3 key = normalize(vec3(0.45, 0.8, 0.35));
    float lambert = max(dot(n, key), 0.0);
    float fill = max(dot(n, normalize(vec3(-0.6, 0.2, -0.5))), 0.0);
    float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0);

    // Sea look: a drafting model, dark body with a lit rim at night and
    // paper with graphite shading by day.
    vec3 sea;
    if (uDark > 0.5) {
        sea = mix(uBg, uInk, 0.05 + 0.16 * lambert + 0.05 * fill) + uAccent * rim * 0.35;
    } else {
        sea = mix(uBg * 1.02, uInk, 0.1 + 0.32 * (1.0 - lambert)) + uCool * rim * 0.08;
    }
    // Waterline stripe.
    float wl = 1.0 - smoothstep(0.0, 0.35, abs(vLocal.y - ${WATERLINE.toFixed(1)}));
    sea = mix(sea, uAccent, wl * 0.6);

    // Hull triangles span zone boundaries, so their zone is resolved per
    // fragment from the same rules as zoneFor() in armor.js.
    float zoneId = vZone;
    float mm = vThickness;
    if (vZone > 99.0) {
        if (vLocal.x > 16.0) { zoneId = 3.0; mm = ${ZONES[3].mm.toFixed(1)}; }
        else if (vLocal.x < -21.0) { zoneId = 4.0; mm = ${ZONES[4].mm.toFixed(1)}; }
        else if (vLocal.y < ${(WATERLINE - 2).toFixed(1)}) { zoneId = 1.0; mm = ${ZONES[1].mm.toFixed(1)}; }
        else if (vLocal.y < ${(WATERLINE + 6).toFixed(1)}) { zoneId = 0.0; mm = ${ZONES[0].mm.toFixed(1)}; }
        else { zoneId = 2.0; mm = ${ZONES[2].mm.toFixed(1)}; }
    }
    vec3 plate = armorColor(mm);
    vec3 armor = plate * (0.5 + 0.5 * lambert + 0.15 * fill) + rim * 0.08;
    if (!gl_FrontFacing) armor *= 0.45;
    float hover = step(abs(zoneId - uHoverZone), 0.25);
    armor = mix(armor, vec3(1.0), hover * (0.25 + 0.15 * sin(uTime * 6.0)));

    // The armor coloring sweeps from bow to stern behind a scan line, with a
    // band of contour lines running just ahead of it.
    float bx = 34.0 - uSweep * 70.0;
    float a = smoothstep(bx - 0.6, bx + 0.6, vLocal.x);
    float moving = step(0.001, uSweep) * step(uSweep, 0.999);
    float ahead = (1.0 - smoothstep(0.0, 9.0, bx - vLocal.x)) * step(vLocal.x, bx) * moving;
    float contour = 1.0 - smoothstep(0.0, 0.18, abs(fract(vLocal.y * 0.8) - 0.5) - 0.32);
    sea = mix(sea, uCool, ahead * contour * 0.7);
    vec3 col = mix(sea, armor, a);
    float scan = (1.0 - smoothstep(0.0, 0.7, abs(vLocal.x - bx))) * moving;
    col = mix(col, uAccent, scan);
    gl_FragColor = vec4(col, 1.0);
}
`;

const EDGE_VERT = /* glsl */ `
varying vec3 vLocal;
void main() {
    vLocal = position;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const EDGE_FRAG = /* glsl */ `
uniform vec3 uColor;
uniform float uAlphaSea;
uniform float uAlphaArmor;
uniform float uSweep;
uniform vec4 uSection;
varying vec3 vLocal;
void main() {
    if (uSection.w < 900.0 && dot(vLocal, uSection.xyz) > uSection.w) discard;
    float bx = 34.0 - uSweep * 70.0;
    float a = smoothstep(bx - 0.6, bx + 0.6, vLocal.x);
    gl_FragColor = vec4(uColor, mix(uAlphaSea, uAlphaArmor, a));
}
`;

const POST_VERT = /* glsl */ `
varying vec2 vUv;
void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

// Ordered dither. Low-chroma pixels are quantized to four tones between the
// background and ink colors; saturated pixels (armor colors, the accent) are
// quantized per channel so the thickness scale keeps its hues.
const POST_FRAG = /* glsl */ `
uniform sampler2D tScene;
uniform vec2 uRes;
uniform float uCell;
uniform vec3 uBg;
uniform vec3 uInk;
varying vec2 vUv;

float bayer4(vec2 p) {
    vec2 q = mod(floor(p), 4.0);
    float i = q.x + q.y * 4.0;
    float m[16];
    m[0]=0.0;  m[1]=8.0;  m[2]=2.0;  m[3]=10.0;
    m[4]=12.0; m[5]=4.0;  m[6]=14.0; m[7]=6.0;
    m[8]=3.0;  m[9]=11.0; m[10]=1.0; m[11]=9.0;
    m[12]=15.0;m[13]=7.0; m[14]=13.0;m[15]=5.0;
    for (int k = 0; k < 16; k++) { if (float(k) == i) return (m[k] + 0.5) / 16.0; }
    return 0.5;
}

void main() {
    vec2 p = floor(gl_FragCoord.xy / uCell);
    vec3 s = texture2D(tScene, (p * uCell + uCell * 0.5) / uRes).rgb;
    float th = bayer4(p);
    float chroma = max(s.r, max(s.g, s.b)) - min(s.r, min(s.g, s.b));
    vec3 col;
    if (chroma > 0.22) {
        col = floor(s * 3.0 + th) / 3.0;
    } else {
        vec3 d = uInk - uBg;
        float t = clamp(dot(s - uBg, d) / dot(d, d), 0.0, 1.0);
        col = mix(uBg, uInk, floor(t * 3.0 + th) / 3.0);
    }
    gl_FragColor = vec4(col, 1.0);
}
`;

async function loadBenchy(url) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`benchy mesh: HTTP ${res.status}`);
    const buf = await res.arrayBuffer();
    const view = new DataView(buf);
    const magic = String.fromCharCode(...new Uint8Array(buf, 0, 4));
    if (magic !== "BNCH") throw new Error("benchy mesh: bad magic");
    const headerLen = view.getUint32(4, true);
    const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 8, headerLen)));
    const base = 8 + headerLen;
    const { min, max } = header.bounds;
    const parts = [];
    for (const p of header.parts) {
        const q = new Int16Array(buf, base + p.offset, p.vertices * 3);
        const idx = new Uint16Array(buf, base + p.offset + p.vertices * 6, p.indices);
        // Expand to unindexed triangles so every face gets a flat normal and
        // its own zone, with no interpolation across plate boundaries.
        const tris = p.indices / 3;
        const pos = new Float32Array(p.indices * 3);
        const thick = new Float32Array(p.indices);
        const zone = new Float32Array(p.indices);
        const faceZone = new Uint8Array(tris);
        const deq = (i, a) => min[a] + ((q[i * 3 + a] + 32767) / 65534) * (max[a] - min[a]);
        for (let t = 0; t < tris; t++) {
            let cx = 0;
            let cy = 0;
            for (let k = 0; k < 3; k++) {
                const vi = idx[t * 3 + k];
                const o = (t * 3 + k) * 3;
                pos[o] = deq(vi, 0);
                pos[o + 1] = deq(vi, 1);
                pos[o + 2] = deq(vi, 2);
                cx += pos[o];
                cy += pos[o + 1];
            }
            const z = zoneFor(p.name, cx / 3, cy / 3);
            faceZone[t] = z;
            for (let k = 0; k < 3; k++) {
                thick[t * 3 + k] = ZONES[z].mm;
                zone[t * 3 + k] = p.name === "hull" ? HULL_PER_FRAGMENT : z;
            }
        }
        const geom = new THREE.BufferGeometry();
        geom.setAttribute("position", new THREE.BufferAttribute(pos, 3));
        geom.setAttribute("aThickness", new THREE.BufferAttribute(thick, 1));
        geom.setAttribute("aZone", new THREE.BufferAttribute(zone, 1));
        geom.computeVertexNormals();
        geom.computeBoundingSphere();
        geom.computeBoundingBox();
        parts.push({ name: p.name, geom, faceZone });
    }
    return parts;
}

function easeInOut(t) {
    return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

export async function createScene(canvas, hooks = {}) {
    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: "high-performance" });
    renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
    renderer.setClearColor(0x000000, 1);

    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(30, 1, 1, 2000);

    const colors = {
        bg: new THREE.Color(),
        ink: new THREE.Color(),
        dim: new THREE.Color(),
        accent: new THREE.Color(),
        cool: new THREE.Color(),
    };
    const targetColors = {};
    for (const k of Object.keys(colors)) targetColors[k] = new THREE.Color();
    const dark = { value: 1 };
    scene.background = colors.bg;

    const shared = {
        uTime: { value: 0 },
        uBg: { value: colors.bg },
        uInk: { value: colors.ink },
        uDim: { value: colors.dim },
        uAccent: { value: colors.accent },
        uCool: { value: colors.cool },
        uArmor: { value: 0 },
        uDark: dark,
        uCam: { value: camera.position },
    };

    const waterGeom = new THREE.PlaneGeometry(WATER_SIZE, WATER_SIZE, WATER_SEGMENTS, WATER_SEGMENTS);
    waterGeom.rotateX(-Math.PI / 2);
    const waterMat = new THREE.ShaderMaterial({
        vertexShader: WATER_VERT,
        fragmentShader: WATER_FRAG,
        uniforms: {
            ...shared,
            uSwellAmp: { value: 1 },
            uChop: { value: 0 },
            uDrop: { value: 0 },
        },
    });
    const water = new THREE.Mesh(waterGeom, waterMat);
    water.frustumCulled = false;
    scene.add(water);

    const section = new THREE.Vector4(0, 0, 1, 1000);
    const hoverZone = { value: -10 };
    const sweep = { value: 0 };
    const anchor = new THREE.Vector3();
    const axisTmp = new THREE.Vector3();
    const boatMat = new THREE.ShaderMaterial({
        vertexShader: BOAT_VERT,
        fragmentShader: BOAT_FRAG,
        side: THREE.DoubleSide,
        uniforms: { ...shared, uSection: { value: section }, uHoverZone: hoverZone, uSweep: sweep },
    });
    const edgeMat = new THREE.ShaderMaterial({
        vertexShader: EDGE_VERT,
        fragmentShader: EDGE_FRAG,
        transparent: true,
        depthWrite: false,
        uniforms: {
            uColor: { value: new THREE.Color() },
            uAlphaSea: { value: 0.75 },
            uAlphaArmor: { value: 0.15 },
            uSweep: sweep,
            uSection: { value: section },
        },
    });

    const boat = new THREE.Group();
    const hull = new THREE.Group();
    boat.add(hull);
    scene.add(boat);

    const parts = await loadBenchy(new URL("../assets/benchy.bin", import.meta.url));
    const meshes = [];
    for (const p of parts) {
        const mesh = new THREE.Mesh(p.geom, boatMat);
        mesh.userData = { part: p.name, faceZone: p.faceZone };
        hull.add(mesh);
        meshes.push(mesh);
        const edges = new THREE.LineSegments(new THREE.EdgesGeometry(p.geom, 28), edgeMat);
        edges.raycast = () => {};
        hull.add(edges);
    }

    // Shell trajectories drawn in armor mode, in hull space.
    const shotGroup = new THREE.Group();
    hull.add(shotGroup);

    const rt = new THREE.WebGLRenderTarget(4, 4, { depthBuffer: true, samples: 4 });
    const postMat = new THREE.ShaderMaterial({
        vertexShader: POST_VERT,
        fragmentShader: POST_FRAG,
        uniforms: {
            tScene: { value: rt.texture },
            uRes: { value: new THREE.Vector2(1, 1) },
            uCell: { value: 2 },
            uBg: shared.uBg,
            uInk: shared.uInk,
        },
        depthTest: false,
        depthWrite: false,
    });
    const postScene = new THREE.Scene();
    const postQuad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), postMat);
    postQuad.frustumCulled = false;
    postScene.add(postQuad);
    const postCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

    const state = {
        mode: "sea",
        armorT: 0,
        armorFrom: 0,
        armorTo: 0,
        armorStart: 0,
        width: 1,
        height: 1,
        focusX: 0.68,
        pointer: new THREE.Vector2(0, 0),
        pointerNdc: new THREE.Vector2(0, 0),
        energy: 0,
        lastMove: null,
        orbitAz: 0.9,
        orbitEl: 0.3,
        orbitDist: 200,
        dragging: null,
        lastInteraction: 0,
        heave: 0,
        roll: 0,
        pitch: 0,
        heaveV: 0,
        rollV: 0,
        pitchV: 0,
        time: 0,
        shell: SHELLS.ap203,
        rangeKm: 12,
        hover: null,
        frame: 0,
        dimmed: false,
    };

    const raycaster = new THREE.Raycaster();
    const seaPlane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
    const tmpV = new THREE.Vector3();

    function setPalette(name, immediate) {
        const p = PALETTES[name];
        for (const k of Object.keys(colors)) {
            targetColors[k].setHex(p[k]);
            if (immediate) colors[k].copy(targetColors[k]);
        }
        dark.value = name === "dark" ? 1 : 0;
    }

    function resize() {
        const w = canvas.clientWidth || window.innerWidth;
        const h = canvas.clientHeight || window.innerHeight;
        const dpr = Math.min(window.devicePixelRatio || 1, state.dimmed ? 1 : 1.75);
        state.width = w;
        state.height = h;
        renderer.setPixelRatio(dpr);
        renderer.setSize(w, h, false);
        rt.setSize(Math.floor(w * dpr), Math.floor(h * dpr));
        postMat.uniforms.uRes.value.set(Math.floor(w * dpr), Math.floor(h * dpr));
        // Dither cells of about 1.5 CSS pixels at any display density.
        postMat.uniforms.uCell.value = Math.max(2, Math.round(dpr * 1.5));
    }

    function frameCamera() {
        const narrow = state.width < 820;
        const armor = easeInOut(state.armorT);
        const seaFocus = narrow ? 0.5 : 0.7;
        const armorFocus = narrow ? 0.5 : 0.58;
        state.focusX = seaFocus + (armorFocus - seaFocus) * armor;

        // On narrow screens the copy sits below the model, so the model is
        // placed in the upper third.
        const focusY = narrow ? 0.3 : 0.5;

        // Larger virtual frame so the boat sits at (focusX, focusY) on the canvas.
        const fullW = state.width * 2 * Math.max(state.focusX, 1 - state.focusX);
        const offsetX = state.focusX >= 0.5 ? 0 : fullW - state.width;
        const fullH = state.height * 2 * Math.max(focusY, 1 - focusY);
        const offsetY = focusY >= 0.5 ? 0 : fullH - state.height;
        camera.aspect = fullW / fullH;
        camera.setViewOffset(fullW, fullH, offsetX, offsetY, state.width, state.height);

        const px = state.pointerNdc.x;
        const py = state.pointerNdc.y;
        const seaAz = 0.62 + px * 0.08 + Math.sin(state.time * 0.05) * 0.06;
        const seaEl = 0.2 + py * 0.035;
        // Portrait screens show less width at the same vertical field of view.
        const fit = Math.min(3, Math.max(1, 1.25 / (state.width / state.height)));
        const seaDist = narrow ? 150 * fit : 190;
        const az = seaAz + (state.orbitAz - seaAz) * armor;
        const el = seaEl + (state.orbitEl - seaEl) * armor;
        const dist = seaDist + ((narrow ? 165 * fit : state.orbitDist) - seaDist) * armor;
        // The hull rises 16 units in armor view; aim at its middle.
        const target = tmpV.set(0, 10 + 26 * armor, 0);
        camera.position.set(
            target.x + dist * Math.cos(el) * Math.sin(az),
            target.y + dist * Math.sin(el),
            target.z + dist * Math.cos(el) * Math.cos(az),
        );
        camera.lookAt(target);
        camera.updateProjectionMatrix();
    }

    function waterHeight(x, z) {
        const u = waterMat.uniforms;
        return swellJs(x, z, state.time, u.uSwellAmp.value, u.uChop.value);
    }

    function updateBoat(dt) {
        const armor = easeInOut(state.armorT);
        // Buoyancy from four hull samples, in boat-yaw space.
        const yaw = boat.rotation.y;
        const c = Math.cos(yaw);
        const s = Math.sin(yaw);
        const sample = (lx, lz) => waterHeight(lx * c + lz * s, -lx * s + lz * c);
        const bow = sample(24, 0);
        const stern = sample(-24, 0);
        const port = sample(0, -12);
        const star = sample(0, 12);
        const heave = (bow + stern + port + star) / 4;
        const pitch = Math.atan2(bow - stern, 48);
        const roll = Math.atan2(port - star, 24);

        // Damped springs so the hull feels heavy.
        const k = 18;
        const d = 7;
        state.heaveV += ((heave - state.heave) * k - state.heaveV * d) * dt;
        state.pitchV += ((pitch - state.pitch) * k - state.pitchV * d) * dt;
        state.rollV += ((roll * 1.4 - state.roll) * k - state.rollV * d) * dt;
        state.heave += state.heaveV * dt;
        state.pitch += state.pitchV * dt;
        state.roll += state.rollV * dt;

        const lift = 16 * armor;
        boat.position.y = (state.heave - WATERLINE) * (1 - armor) + lift;
        hull.rotation.set(state.roll * (1 - armor), 0, state.pitch * (1 - armor));
        boat.rotation.y = -0.15 + Math.sin(state.time * 0.11) * 0.08 * (1 - armor);
    }

    function updateColors(dt) {
        const a = 1 - Math.exp(-dt * 6);
        for (const k of Object.keys(colors)) colors[k].lerp(targetColors[k], a);
        const edgeColor = edgeMat.uniforms.uColor.value;
        edgeColor.copy(colors.ink);
        edgeMat.uniforms.uAlphaSea.value = dark.value > 0.5 ? 0.75 : 0.8;
        edgeMat.uniforms.uAlphaArmor.value = dark.value > 0.5 ? 0.15 : 0.25;
    }

    function zoneIndex(object, faceIndex, point) {
        const data = object.userData;
        if (data.part !== "hull") return data.faceZone[faceIndex];
        const local = hull.worldToLocal(point.clone());
        return zoneFor("hull", local.x, local.y);
    }

    function zoneHit(intersection) {
        return ZONES[zoneIndex(intersection.object, intersection.faceIndex, intersection.point)];
    }

    function visibleHits(hits) {
        if (section.w >= 900) return hits;
        return hits.filter((h) => {
            const local = hull.worldToLocal(h.point.clone());
            return local.dot(tmpV.set(section.x, section.y, section.z)) <= section.w;
        });
    }

    function hoverArmor(clientX, clientY) {
        const rect = canvas.getBoundingClientRect();
        state.pointer.set(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1);
        raycaster.setFromCamera(state.pointer, camera);
        const hit = visibleHits(raycaster.intersectObjects(meshes, false))[0];
        if (!hit) {
            hoverZone.value = -10;
            state.hover = null;
            hooks.onHover?.(null);
            return;
        }
        const z = zoneHit(hit);
        const n = hit.face.normal.clone().transformDirection(hit.object.matrixWorld);
        const angle = (Math.acos(Math.min(1, Math.abs(n.dot(raycaster.ray.direction)))) * 180) / Math.PI;
        hoverZone.value = z.id;
        state.hover = { zone: z, angle, x: clientX, y: clientY };
        hooks.onHover?.(state.hover);
    }

    function clearShots() {
        for (const child of [...shotGroup.children]) {
            child.geometry?.dispose();
            child.material?.dispose();
            shotGroup.remove(child);
        }
    }

    function fire(clientX, clientY) {
        const rect = canvas.getBoundingClientRect();
        state.pointer.set(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1);
        raycaster.setFromCamera(state.pointer, camera);
        const aim = visibleHits(raycaster.intersectObjects(meshes, false))[0];
        if (!aim) return;

        // The shell arrives from the camera's bearing at the fall angle for
        // the chosen range, aimed at the clicked point.
        const fall = (fallAngleDeg(state.rangeKm) * Math.PI) / 180;
        const flat = raycaster.ray.direction.clone().setY(0).normalize();
        const dir = flat.multiplyScalar(Math.cos(fall)).add(new THREE.Vector3(0, -Math.sin(fall), 0)).normalize();
        const start = aim.point.clone().addScaledVector(dir, -160);

        raycaster.set(start, dir);
        const raw = visibleHits(raycaster.intersectObjects(meshes, false));
        const hits = raw.map((h) => {
            const n = h.face.normal.clone().transformDirection(h.object.matrixWorld);
            const cos = n.dot(dir);
            return {
                t: h.distance,
                entry: cos < 0,
                zone: zoneIndex(h.object, h.faceIndex, h.point),
                angleDeg: (Math.acos(Math.min(1, Math.abs(cos))) * 180) / Math.PI,
                part: h.object.userData.part,
                point: h.point,
                normal: n,
            };
        });
        const shot = resolveShot(state.shell, state.rangeKm, hits);

        clearShots();
        const toLocal = (p) => hull.worldToLocal(p.clone());
        const end =
            shot.detonationT ?? shot.stopT ?? (hits.length ? hits[hits.length - 1].t + 18 : aim.distance + 40);
        // Draw only the end of the flight; the full ray starts near the camera.
        const firstT = hits.length ? hits[0].t : 160;
        const drawFrom = start.clone().addScaledVector(dir, Math.max(0, firstT - 60));
        const path = [toLocal(drawFrom), toLocal(start.clone().addScaledVector(dir, end))];
        const accent = colors.accent.clone();
        const tube = new THREE.Mesh(
            new THREE.TubeGeometry(new THREE.LineCurve3(path[0], path[1]), 1, 0.35, 6, false),
            new THREE.MeshBasicMaterial({ color: accent, depthTest: false, transparent: true, opacity: 0.95 }),
        );
        tube.renderOrder = 10;
        shotGroup.add(tube);

        if (shot.outcome === "RICOCHET" && shot.ricochetIndex !== null) {
            const entry = hits.filter((h) => h.entry)[shot.ricochetIndex];
            const out = dir.clone().reflect(entry.normal);
            const p0 = toLocal(entry.point);
            const p1 = toLocal(entry.point.clone().addScaledVector(out, 40));
            const bounce = new THREE.Mesh(
                new THREE.TubeGeometry(new THREE.LineCurve3(p0, p1), 1, 0.25, 6, false),
                new THREE.MeshBasicMaterial({ color: colors.cool.clone(), depthTest: false, transparent: true, opacity: 0.9 }),
            );
            bounce.renderOrder = 10;
            shotGroup.add(bounce);
        }

        const markerGeom = new THREE.OctahedronGeometry(0.9);
        for (const h of hits) {
            if (!h.entry || h.t > end + 0.01) continue;
            const m = new THREE.Mesh(
                markerGeom,
                new THREE.MeshBasicMaterial({ color: colors.ink.clone(), depthTest: false, transparent: true }),
            );
            m.position.copy(toLocal(h.point));
            m.renderOrder = 11;
            shotGroup.add(m);
        }
        if (shot.detonationT !== null) {
            const boom = new THREE.Mesh(
                new THREE.OctahedronGeometry(2.4),
                new THREE.MeshBasicMaterial({ color: accent, depthTest: false, transparent: true, wireframe: true }),
            );
            boom.position.copy(toLocal(start.clone().addScaledVector(dir, shot.detonationT)));
            boom.renderOrder = 12;
            boom.userData.spin = true;
            shotGroup.add(boom);
        }
        hooks.onShot?.(shot);
    }

    function clearHover() {
        hoverZone.value = -10;
        state.hover = null;
        state.pendingHover = null;
        hooks.onHover?.(null);
    }

    function setMode(mode) {
        if (mode === state.mode) return;
        state.mode = mode;
        state.armorFrom = state.armorT;
        state.armorTo = mode === "armor" ? 1 : 0;
        state.armorStart = performance.now();
        if (reduceMotion) state.armorT = state.armorTo;
        if (mode === "sea") {
            clearHover();
            clearShots();
        } else {
            state.orbitAz = 0.9;
            state.orbitEl = 0.3;
        }
    }

    // Input.
    const onMove = (e) => {
        const rect = canvas.getBoundingClientRect();
        state.pointerNdc.set(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
        state.lastInteraction = performance.now();

        if (state.dragging) {
            const dx = e.clientX - state.dragging.x;
            const dy = e.clientY - state.dragging.y;
            state.dragging.moved += Math.abs(dx) + Math.abs(dy);
            state.orbitAz -= dx * 0.008;
            state.orbitEl = Math.min(1.3, Math.max(-0.2, state.orbitEl + dy * 0.006));
            state.dragging.x = e.clientX;
            state.dragging.y = e.clientY;
            return;
        }

        if (state.mode === "armor") {
            // Raycast at most once per frame; tick() handles the latest position.
            if (hooks.canInteract?.(e) ?? true) state.pendingHover = { x: e.clientX, y: e.clientY };
            else if (state.hover) clearHover();
            return;
        }
        // Cursor movement over the sea feeds energy into the whole swell.
        if (state.lastMove) {
            const d = Math.hypot(e.clientX - state.lastMove.x, e.clientY - state.lastMove.y);
            state.energy = Math.min(MAX_ENERGY, state.energy + Math.min(d * 0.0015, 0.2));
        }
        state.lastMove = { x: e.clientX, y: e.clientY };
    };

    const onDown = (e) => {
        if (!(hooks.canInteract?.(e) ?? true)) return;
        state.lastInteraction = performance.now();
        if (state.mode === "armor") {
            state.dragging = { x: e.clientX, y: e.clientY, moved: 0 };
            return;
        }
        state.energy = Math.min(MAX_ENERGY, state.energy + 0.9);
    };

    const onUp = (e) => {
        if (state.dragging) {
            const clicked = state.dragging.moved < 6;
            state.dragging = null;
            if (clicked && state.mode === "armor") fire(e.clientX, e.clientY);
        }
    };

    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", () => (state.dragging = null));
    window.addEventListener("resize", resize);

    setPalette(hooks.theme ?? "dark", true);
    resize();

    let last = performance.now();

    function tick(now) {
        requestAnimationFrame(tick);
        // When the reader has scrolled away, draw every other frame. The step
        // below then spans both frames, so it is capped higher.
        if (state.dimmed && (state.frame++ & 1)) return;
        const dt = Math.min(state.dimmed ? 0.1 : 0.05, (now - last) / 1000);
        last = now;
        // Energy decays back to the calm swell; while it lasts the sea
        // moves faster and higher.
        state.energy *= Math.exp(-dt * 0.35);
        if (!reduceMotion) state.time += dt * (1 + 0.35 * state.energy);
        shared.uTime.value = state.time;

        if (state.armorT !== state.armorTo) {
            const k = Math.min(1, (now - state.armorStart) / TRANSITION_MS);
            state.armorT = state.armorFrom + (state.armorTo - state.armorFrom) * k;
        }
        const armor = easeInOut(state.armorT);
        shared.uArmor.value = armor;
        // The camera and lift use the whole transition; the color sweep runs
        // through its middle.
        sweep.value = easeInOut(Math.min(1, Math.max(0, (state.armorT - 0.2) / 0.65)));
        waterMat.uniforms.uSwellAmp.value = (1 + 0.45 * state.energy) * (1 - 0.85 * armor);
        waterMat.uniforms.uChop.value = state.energy;

        if (state.mode === "armor" && !state.dragging && !reduceMotion && now - state.lastInteraction > 5000) {
            state.orbitAz += dt * 0.12;
        }

        updateColors(dt);
        updateBoat(dt);
        frameCamera();
        if (state.pendingHover && state.mode === "armor" && !state.dragging) {
            hoverArmor(state.pendingHover.x, state.pendingHover.y);
            state.pendingHover = null;
        }
        for (const c of shotGroup.children) if (c.userData.spin) c.rotation.y += dt * 2;

        renderer.setRenderTarget(rt);
        renderer.render(scene, camera);
        renderer.setRenderTarget(null);
        renderer.render(postScene, postCam);

        if (hooks.onAnchor) {
            anchor.set(0, 54, 0);
            hull.localToWorld(anchor).project(camera);
            hooks.onAnchor(((anchor.x + 1) / 2) * state.width, ((1 - anchor.y) / 2) * state.height);
        }
        // Measurement overlay in the style of the desktop model viewer: a
        // length ruler along the starboard waterline and an axis gizmo.
        if (hooks.onOverlay && armor > 0.01) {
            const toScreen = (v) => [((v.x + 1) / 2) * state.width, ((1 - v.y) / 2) * state.height];
            const ruler = [];
            if (armor > 0.5) {
                for (let x = -30; x <= 30; x += 10) {
                    anchor.set(x, WATERLINE, 17);
                    hull.localToWorld(anchor).project(camera);
                    ruler.push({ at: toScreen(anchor), label: `${x + 30} mm`, behind: anchor.z > 1 });
                }
            }
            const axes = {};
            const origin = anchor.copy(camera.position).add(camera.getWorldDirection(axisTmp).multiplyScalar(10));
            const base = toScreen(origin.clone().project(camera));
            for (const [name, dir] of AXES) {
                const tip = toScreen(origin.clone().add(dir).project(camera));
                const dx = tip[0] - base[0];
                const dy = tip[1] - base[1];
                const len = Math.hypot(dx, dy) || 1;
                axes[name] = [dx / len, dy / len];
            }
            hooks.onOverlay({ armor, ruler, axes });
        }
    }
    requestAnimationFrame(tick);

    return {
        setMode,
        setTheme(name) {
            setPalette(name, false);
        },
        setShell(key) {
            state.shell = SHELLS[key] ?? SHELLS.ap203;
        },
        setRange(km) {
            state.rangeKm = km;
        },
        setSection(fraction) {
            // fraction 1 shows the whole ship; lower values cut away the
            // starboard side down to the centreline.
            if (fraction >= 0.999) section.w = 1000;
            else section.set(0, 0, 1, 15.5 * (fraction * 2 - 1));
        },
        clearShots,
        setDimmed(dimmed) {
            if (dimmed === state.dimmed) return;
            state.dimmed = dimmed;
            resize();
        },
    };
}
