// Converts the multi-part 3DBenchy STL set into the compact mesh the site's
// background scene loads (site/assets/benchy.bin).
//
// Usage:
//   npm install --prefix /tmp/benchy-tools meshoptimizer
//   NODE_PATH=/tmp/benchy-tools/node_modules node scripts/site/build_benchy_mesh.mjs \
//       <3DBenchy-master/Multi-part dir> site/assets/benchy.bin
//
// 3DBenchy by Creative Tools is public domain (CC0 1.0):
// https://github.com/CreativeTools/3DBenchy
//
// Output layout (little endian):
//   u32 magic "BNCH", u32 header byte length, header JSON (padded to 4 bytes),
//   then per part: i16x3 positions quantized to the header's bounds, u16 indices.
// Positions are converted from the STL's Z-up millimetres to Y-up with the bow
// facing +X.

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { MeshoptSimplifier } = require("meshoptimizer");

// Part name, source file stem, and the fraction of triangles to keep.
const PARTS = [
    ["hull", "Hull", 0.06],
    ["deck", "Deck surface", 0.25],
    ["gunwale", "Gunwale", 0.06],
    ["cargo_box", "Cargo box", 0.5],
    ["bridge_walls", "Bridge walls", 0.25],
    ["bridge_roof", "Bridge roof", 0.12],
    ["chimney", "Chimney body", 0.25],
    ["chimney_top", "Chimney top", 0.12],
    ["door_port", "Doorframe port", 0.4],
    ["door_starboard", "Doorframe starboard", 0.4],
    ["window", "Window", 0.6],
    ["stern_window", "Stern window", 0.4],
    ["name_plate", "Stern name plate", 0.06],
    ["wheel", "Wheel", 0.4],
    ["rod_holder", "Fishing-rod-holder", 0.12],
    ["hawse_port", "Hawsepipe port", 0.3],
    ["hawse_starboard", "Hawsepipe starboard", 0.3],
];

function readStl(file) {
    const buf = fs.readFileSync(file);
    const count = buf.readUInt32LE(80);
    const map = new Map();
    const positions = [];
    const indices = new Uint32Array(count * 3);
    for (let t = 0; t < count; t++) {
        const base = 84 + t * 50 + 12;
        for (let v = 0; v < 3; v++) {
            const o = base + v * 12;
            const x = buf.readFloatLE(o);
            const y = buf.readFloatLE(o + 4);
            const z = buf.readFloatLE(o + 8);
            // Weld at 0.05 mm. The STL repeats seam vertices with float noise,
            // which leaves open borders the simplifier refuses to collapse.
            const key = `${Math.round(x * 20)},${Math.round(y * 20)},${Math.round(z * 20)}`;
            let id = map.get(key);
            if (id === undefined) {
                id = positions.length / 3;
                map.set(key, id);
                // Z-up to Y-up. The STL's bow points along -X; rotate it to +X.
                positions.push(x, z, -y);
            }
            indices[t * 3 + v] = id;
        }
    }
    return { positions: new Float32Array(positions), indices };
}

function compact(positions, indices) {
    const remap = new Map();
    const out = [];
    const idx = new Uint32Array(indices.length);
    for (let i = 0; i < indices.length; i++) {
        let n = remap.get(indices[i]);
        if (n === undefined) {
            n = out.length / 3;
            remap.set(indices[i], n);
            const s = indices[i] * 3;
            out.push(positions[s], positions[s + 1], positions[s + 2]);
        }
        idx[i] = n;
    }
    return { positions: new Float32Array(out), indices: idx };
}

async function main() {
    const [srcDir, outFile] = process.argv.slice(2);
    if (!srcDir || !outFile) {
        console.error("usage: build_benchy_mesh.mjs <Multi-part dir> <out.bin>");
        process.exit(2);
    }
    await MeshoptSimplifier.ready;

    const parts = [];
    for (const [name, stem, keep] of PARTS) {
        const file = path.join(srcDir, `#3DBenchy - Multi-part - Single - ${stem} - (3DBenchy.com).stl`);
        const mesh = readStl(file);
        const target = Math.max(3, Math.floor((mesh.indices.length * keep) / 3) * 3);
        const [simplified] = MeshoptSimplifier.simplify(mesh.indices, mesh.positions, 3, target, 0.02, []);
        const c = compact(mesh.positions, simplified);
        if (c.positions.length / 3 > 65535) throw new Error(`${name} has too many vertices for u16 indices`);
        parts.push({ name, ...c });
        console.log(`${name}: ${mesh.indices.length / 3} -> ${c.indices.length / 3} triangles`);
    }

    // Centre the hull's footprint on the origin and put the keel at y = 0.
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (const p of parts) {
        for (let i = 0; i < p.positions.length; i++) {
            min[i % 3] = Math.min(min[i % 3], p.positions[i]);
            max[i % 3] = Math.max(max[i % 3], p.positions[i]);
        }
    }
    const offset = [-(min[0] + max[0]) / 2, -min[1], -(min[2] + max[2]) / 2];
    const lo = min.map((v, i) => v + offset[i]);
    const hi = max.map((v, i) => v + offset[i]);

    const header = { units: "mm", bounds: { min: lo, max: hi }, parts: [] };
    const blobs = [];
    let cursor = 0;
    for (const p of parts) {
        const q = new Int16Array(p.positions.length);
        for (let i = 0; i < p.positions.length; i++) {
            const a = i % 3;
            const t = (p.positions[i] + offset[a] - lo[a]) / (hi[a] - lo[a]);
            q[i] = Math.round(t * 65534 - 32767);
        }
        const ix = Uint16Array.from(p.indices);
        const posBytes = Buffer.from(q.buffer);
        const idxBytes = Buffer.from(ix.buffer);
        const pad = (4 - ((posBytes.length + idxBytes.length) % 4)) % 4;
        header.parts.push({
            name: p.name,
            vertices: q.length / 3,
            indices: ix.length,
            offset: cursor,
        });
        blobs.push(posBytes, idxBytes, Buffer.alloc(pad));
        cursor += posBytes.length + idxBytes.length + pad;
    }

    let json = Buffer.from(JSON.stringify(header));
    json = Buffer.concat([json, Buffer.alloc((4 - (json.length % 4)) % 4, 0x20)]);
    const head = Buffer.alloc(8);
    head.write("BNCH", 0, "ascii");
    head.writeUInt32LE(json.length, 4);
    fs.mkdirSync(path.dirname(outFile), { recursive: true });
    fs.writeFileSync(outFile, Buffer.concat([head, json, ...blobs]));
    console.log(`wrote ${outFile}: ${8 + json.length + cursor} bytes`);
}

main();
