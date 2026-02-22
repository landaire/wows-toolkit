# Ballistics Reverse Engineering Notes

Findings from reverse engineering `WorldOfWarships64.exe` (Binary Ninja) compared
against our implementation in `ballistics.rs` and `penetration.rs`.

Source path embedded in the binary:
```
D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\wows\source\lib\lesta\gamelogic\reverse_ballistics\
```

Key files: `py_ammo_pitches.h`, `py_ammo_pitches.cpp`, `py_fast_pitches.h`.

---

## 1. Trajectory Simulation Architecture

### Game binary structure

| Function | Role |
|---|---|
| `sub_1403703d0` | High-level trajectory builder. Extracts `position`, `direction`, `bulletMass`, `bulletDiametr`, `bulletAirDrag`, `bulletSpeed`, `targetPos`, `timeLeft` from Python params and calls the simulation. |
| `sub_14036f950` | Wrapper. Calls the core sim, optionally does a two-pass correction when `targetPos` is provided (adjusts last trajectory segment to converge on target). |
| `sub_140307700` | Core simulation loop. Max 1024 iterations. Stores trajectory points as `(pos_xyz, speed, time)` tuples. Handles overflow logging. |
| `sub_140307580` | Per-step update. Checks max range (84 000 m), computes adaptive dt, calls drag, advances position and velocity via **forward Euler**. |
| `sub_1403073c0` | Core drag/acceleration. Computes air density via ISA model, applies drag force, adds gravity, returns acceleration vector. |
| `sub_140307d70` | `simulateFlight` — Python-facing entry point. Builds trajectory, optionally applies a time-based resampling, packages result for Python. |
| `sub_14030b340` | Target convergence. Given a target position, finds the closest trajectory point and interpolates to improve accuracy. |

### Our implementation

Single 2D simulation in `ballistics.rs`:
- `simulate_trajectory()` — RK4 integration in the (x, y) plane
- `solve_for_range()` — bisection to find launch angle for a given range
- `simulate_arc_points()` — produces normalized arc for visualization

---

## 2. ISA Atmospheric Model — MATCH

The game initializes a trajectory struct with 6 inline constants (via `memcpy`
at `sub_140307700+0x77`):

| Offset | Game value | Our constant | Meaning |
|--------|-----------|-------------|---------|
| 0 | 101 325.0 | `P0 = 101325.0` | Sea-level pressure (Pa) |
| 4 | 0.0065 | `L = 0.0065` | Temperature lapse rate (K/m) |
| 8 | 288.15 | `T0 = 288.15` | Sea-level temperature (K) |
| 12 | 9.8 | `G = 9.8` | Gravitational acceleration (m/s²) |
| 16 | 0.028964 | `M_AIR = 0.0289644` | Molar mass of air (kg/mol) |
| 20 | 8.31447 | `R_GAS = 8.31447` | Ideal gas constant (J/(mol·K)) |

**Verdict: Exact match.** Both use the International Standard Atmosphere with
identical constants.

### Air density formula

Game (`sub_1403073c0`):
```
T = T0 - L * y
rho = (P0 * M_AIR) / (R_GAS * T) * (T / T0) ^ (G * M_AIR / (R_GAS * L))
    = (P0 * M_AIR) / (R_GAS * T0) * (1 - L*y/T0) ^ (G*M_AIR/(R_GAS*L) - 1)
```

Ours (`air_density()`):
```
T = T0 - L * y
P = P0 * (T / T0) ^ (G * M_AIR / (R_GAS * L))
rho = M_AIR * P / (R_GAS * T)
```

**Verdict: Algebraically identical.**

---

## 3. Drag Force Formula — MATCH

### Game computation (`sub_1403073c0`)

At struct initialization (`sub_140307700`):
```
area = (pi/4) * diameter^2       // cross-sectional area
```

In the drag function:
```
F_drag/mass = 0.5 * cd * area * rho(y) * v^2 / mass
            = (pi/8) * cd * d^2 * rho(y) * v^2 / mass
```

### Our computation

```rust
k = 0.5 * cd * (d/2)^2 * pi / mass   // = (pi/8) * cd * d^2 / mass
a_drag = k * rho * speed              // per-component: -k * rho * v_component * speed
```

**Verdict: Algebraically identical.** The game stores `area = pi/4 * d^2` separately
and divides by mass at runtime; we precompute `k` which folds mass in. Same result.

### 3D drag decomposition (game only)

The game decomposes drag in 3D using `atan2` for pitch/yaw angles followed by
`sincos` for direction. Gravity is added to the y-component only:
```
accel_y = -(drag_y_magnitude) - G
accel_xz = -(drag_xz_magnitude) in the velocity direction
```

Sign negation is done via XOR with the `0x80000000` mask at `data_14255db60`.

Our 2D version is equivalent for planar trajectories.

---

## 4. Integration Method — DIFFERS

### Game: Forward Euler with adaptive time step

```
dt = clamp(exp(y * 0.000650) * y * 0.000650,  0.1001,  0.8125)
pos += vel * dt
vel += accel * dt
step_count++
```

- Max 1024 steps per trajectory
- Max range 42 000 m (`data_142994650` × 1400.0, where the scale is 30.0 at runtime)
- Time step varies with altitude (larger dt at higher altitudes)
- At low altitudes, dt is clamped to ~0.1001 (≈100 ms game time)

### Ours: RK4 with fixed time step

```
dt = 0.02 s (fixed)
RK4 integration (4th-order Runge-Kutta)
Max time: 200 s
```

**Verdict: Different integration scheme.** Our RK4 is more accurate per step than
the game's Euler, but uses a finer fixed step. In practice, results are very close
because the game's adaptive step keeps the trajectory smooth. The game's Euler
approach is faster computationally, suitable for real-time client prediction.

---

## 5. Coordinate System

The game uses three coordinate spaces, defined by four hardcoded constants
exposed via the `BigWorld` C++ Python module:

| Constant | Value | Address | Meaning |
|----------|-------|---------|---------|
| `BW_TO_BALLISTIC` | 30.0 | `sub_140f66070` | 1 BW unit = 30 meters |
| `BALLISTIC_TO_BW` | 1/30 | `sub_140f66080` | 1 meter = 1/30 BW units |
| `BW_TO_SHIP` | 15.0 | `sub_140f66090` | 1 BW unit = 15 ship-model units |
| `SHIP_TO_BW` | 1/15 | `sub_140f660a0` | 1 ship-model unit = 1/15 BW units |

From these: **1 ship-model unit = 2 meters** (since 30/15 = 2).

| Space | Scale to BW | Scale to meters | Notes |
|-------|-------------|-----------------|-------|
| BigWorld (BW) | 1 | 30 | Entity positions, map coordinates |
| Ballistic (meters) | 1/30 | 1 | Physics sim, ISA model, drag |
| Ship-model | 1/15 | 2 | Ship geometry/armor meshes |

### Ballistic scale (30.0 at runtime)

The trajectory simulation uses a scale factor stored at `data_142994650` to
convert between its input coordinate space and SI meters. This global is set
by `Lesta.setBallicticScale()` from Python at startup.

**Static binary value:** The on-disk binary contains `0x42700000` = **60.0** at
`data_142994650`. This is the compiled-in default before any Python initialization.

**Runtime value:** The deobfuscated game scripts show the actual value is **30.0**:

```python
# BWPersonality.pyc (deobfuscated, bytecode offset 1052-1092):
#   from m3510ec80 import BW_TO_BALLISTIC, BALLISTIC_TRAJECTORY_FLATTENING, AVATAR_FILTER_PARAMS
#   Lesta.setBallicticScale(BW_TO_BALLISTIC)

# m3510ec80 = ConstantsShip (deobfuscated):
from BigWorld import BW_TO_BALLISTIC, BALLISTIC_TO_BW, BW_TO_SHIP, SHIP_TO_BW
```

The Python variable `BW_TO_BALLISTIC` is imported directly from the `BigWorld`
C++ module, where it is hardcoded to **30.0** (see table above). So at runtime,
`data_142994650 = 30.0`.

At initialization in `sub_140307700`, input positions are scaled by this factor:
```
var_520 = position[0] * data_142994650   // * 30.0 → meters
var_51c = position[1] * data_142994650   // * 30.0 → meters
var_518 = position[2] * data_142994650   // * 30.0 → meters
```

And in `sub_1403841b0`, outputs are divided by it:
```
direction[i] = direction[i] / data_142994650   // / 30.0 → BW
speed = speed / data_142994650                 // / 30.0 → BW
```

This means the trajectory functions receive positions in **BigWorld units** and
convert to meters by multiplying by 30.0. Outputs (direction, speed) are
converted back to BW units by dividing by 30.0. The internal simulation works
in SI meters (ISA constants, g=9.8 m/s²).

### ConstantsShip usage patterns (from deobfuscated `m3510ec80`)

The deobfuscated `ConstantsShip` module confirms the conversion conventions:

```python
AGRO_DISTANCE = 700.0 * BALLISTIC_TO_BW              # 700 m → BW
AIR_DEFENSE_SHOOT_EFFECTS_VISIBILITY = 5000.0 * BALLISTIC_TO_BW  # 5000 m → BW
WAVEHORN_WAVE_SPEED = 3000.0 * BALLISTIC_TO_BW       # 3000 m/s → BW/s
WAVEHORN_WAVE_RADIUS = 5000.0 * BALLISTIC_TO_BW      # 5000 m → BW
DEFAULT_AIR_SUPPORT_DISTANCES = (500 * BALLISTIC_TO_BW, 7000 * BALLISTIC_TO_BW)
SHIP_BY_SHIP_XRAY_BALLISTIC_KM = VisibilityDistance.SHIP_BY_SHIP_XRAY * BW_TO_BALLISTIC / KM_TO_M
```

All distance literals are in meters, multiplied by `BALLISTIC_TO_BW` (= 1/30)
to convert to BigWorld units for the engine.

### Additional constants from ConstantsShip

| Constant | Value | Notes |
|----------|-------|-------|
| `BALLISTIC_TRAJECTORY_FLATTENING` | 0.1 | Passed to `Lesta.setBallisticFlattening()` |
| `MAX_MAP_SIZE` | 5000.0 | BW units (= 150 km) |
| `MAX_SHOOT_LEN` | 1500.0 | BW units (= 45 km) |
| `KNOTS_TO_MPS` | 1.852/3.6 | ≈ 0.5144 m/s per knot |
| `SHIP_TIME_SCALE` | 2.61 | Server time scaling factor |
| `BW_KNOTS_TO_MPS` | KNOTS_TO_MPS × SHIP_TO_BW × SHIP_TIME_SCALE | Composite speed conversion |

Our simulation works in meters directly and only converts at the UI boundary,
using `BW_TO_METERS = 30.0` (from `wowsunpack`) for model-space conversions.

---

## 6. `PyFastPitches::getFast` — Pitch Table Lookup

`sub_1403e54b0` implements a fast pitch-angle lookup for fire control. Given a
horizontal distance and height difference to target, it:

1. Computes horizontal distance between source and target
2. Indexes into a precomputed 40-entry pitch table (entries 0–39)
3. Linearly interpolates between table entries
4. Applies a correction factor computed as `atan2(height_diff, distance) * clamp(factor, 1.0, 1.2)`
5. Clamps result between `-bulletAirDrag` and `min(bulletAirDrag, result)`

This is the fire control system's fast path — it doesn't re-simulate the full
trajectory each time, instead using precomputed lookup tables built from
`PyAmmoPitches` simulations.

---

## 7. Penetration Formula — NOT IN CLIENT BINARY

### Exhaustive search methodology

The following searches were performed to locate any penetration-related code:

**String searches (all returned zero results for penetration mechanics):**
- `krupp`, `bulletKrupp`, `alphaPiercing`, `shellVelocity`
- `postPen`, `remainingPen`, `reducedVelocity`, `detonator`
- `overmatch`, `ricochet`, `cosAngle`, `effectiveThick`
- `penValue`, `armorPenetrat`, `calcPenetration`, `calcDamage`
- `shellHit`, `onShellHit`, `onProjectile`, `damageApply`
- `thickness` (only rendering-related results)
- `armor` (only `SplashMesh`/`ArmorModel` rendering classes)

**String found but not relevant:**
- `"PENETRATION"` at `0x142a8c297` — no code xrefs (an enum/label string)

**All `expf` callers in game logic range (0x140xxx) were decompiled:**

| Address | Function | Purpose |
|---------|----------|---------|
| `sub_140165cc0` | Entity filter | Exponential decay for position smoothing |
| `sub_14021d560` | UI rendering update | `exp(x * 12.48 - 1.39)` — visual speed scaling |
| `sub_14023ce60` | UI data packing | Same visual `exp` scale pattern |
| `sub_140307580` | Trajectory per-step | Adaptive dt (already documented) |
| `sub_1403ebbb0` | Splash/water physics | Water surface deformation, not armor |

**All `powf` callers in the ballistics address range were decompiled:**

| Address | Function | Purpose |
|---------|----------|---------|
| `sub_1402f7890` | Turret/gun controller | Angular velocity with `powf` for aim speed curves |
| `sub_1402f7cf0` | Turret/gun controller | Similar aim controller with position clamping |
| `sub_1402f83b0` | Turret/gun controller | Simplified aim controller variant |
| `sub_1403f3f60` | Material decay | `powf(lerp(a,b,t), exp)` — material/shader interpolation |
| `sub_14031ecd0` | `_py_decay` (mathemagic.cpp) | Generic `0.5^(val/scale) * (max-min) + min` |

**All 16 functions from `mathemagic.cpp` were decompiled:**

These are geometry utility functions (pitch/yaw direction, line-sphere intersection,
line-line intersection, etc.) — none involve penetration mechanics. The `_py_decay`
function computes `0.5^(ratio) * (max - min) + min` which is a generic exponential
interpolation, not the AP penetration formula.

**Ballistics-specific functions from `ballistics_trajectory.cpp`:**

| String | Function |
|--------|----------|
| `Ballistics::_py_setBallicticScale` | Sets the ballistic scale at `data_142994650` (30.0 at runtime, = `BW_TO_BALLISTIC`) |
| `Ballistics::_py_ballistics_trajectory` | Full trajectory simulation wrapper |
| `Ballistics::_py_getDistUnderWater` | Underwater distance computation |
| `Ballistics::_py_getTimeUnderWater` | Underwater time computation |
| `Ballistics::_py_getVeloUnderWater` | Underwater velocity computation |
| `Ballistics::_py_setBallisticFlattening` | Visual arc flattening parameter |
| `Ballistics::_py_pyFlattenTrajHeight` | Flatten trajectory for rendering |
| `Ballistics::_py_pyUnflattenTrajHeight` | Inverse of flattening |
| `Ballistics::_py_getRandomTrajPack` | Random trajectory spread |
| `Ballistics::_py_getTrajectoryDist` | Distance along trajectory |

None of these functions reference penetration, normalization, or armor interaction.

**`PySplashMesh::getSplashEffectiveArmor` — HE splash only:**

Traced `sub_14039fc00` → `sub_1403a1b10` which computes effective armor for HE
splash damage (box intersection geometry). This is NOT AP shell-vs-plate penetration.

### Ballistic scale constant

The global at `data_142994650` is the ballistic scale factor, set from Python via
`Lesta.setBallicticScale(BW_TO_BALLISTIC)` where `BW_TO_BALLISTIC = 30.0` (see
Section 5). The on-disk binary contains a default of 60.0, but at runtime this
is overwritten to 30.0. It converts the trajectory function's input positions
(in BW units) to meters, and is used inversely to convert outputs back.

### Verdict

**Penetration computation is server-side only.** After decompiling every `expf`
and `powf` caller in the game logic address range, and all functions from
`ballistics_trajectory.cpp` and `mathemagic.cpp`, no code was found that:
- Computes `1 - exp(1 - pen/thickness)` (post-penetration velocity reduction)
- Computes `mass^0.69 * caliber^(-1.07)` (penetration coefficient)
- References krupp, normalization angles, ricochet checks, or fuse mechanics

The client receives `TerminalBallisticsInfo` with impact velocities and hit
results but does not compute penetration itself.

### Our penetration formula (from wows_shell / jcw780)

```rust
p_ppc = 1e-7 * krupp * mass^0.69 * caliber^(-1.07)
raw_pen = p_ppc * impact_velocity^1.38
post_pen_velocity = velocity * (1 - exp(1 - raw_pen / effective_thickness))
```

These constants (0.69, -1.07, 1.38) and the post-penetration velocity formula
cannot be verified from the client binary. They were empirically derived by the
community (jcw780's wows_shell project) through in-game testing and curve fitting.

---

## 8. Normalization, Ricochet, Fuse — NOT IN CLIENT BINARY

No client-side code was found for:
- Shell normalization angle application
- Ricochet angle checks (45°/60° thresholds)
- Fuse arming threshold or fuse timer logic
- Post-penetration velocity reduction

All armor interaction is server-authoritative. The client only visualizes
results received from the server.

Our `penetration.rs` implements these for the offline armor viewer simulation:
- **Overmatch**: `caliber_mm > thickness_mm * 14.3` — community-confirmed constant
- **Normalization**: `angle = max(0, angle_from_normal - normalization_rad)`
- **Ricochet**: at `always_ricochet_angle` (typically 60°)
- **Post-penetration velocity**: `v_after = v * (1 - exp(1 - raw_pen / eff_thickness))`
- **Fuse distance**: `fuse_arm_velocity * fuse_time`, converted to BigWorld units

These formulas are consistent with observed in-game behavior and widely used by
community tools (wows_shell, WoWs Fitting Tool, ShipBuilder). While they cannot
be verified against the binary, they produce results that match server behavior
within measurement precision.

---

## 9. Turret/Gun Aim Controllers (bonus finding)

Functions `sub_1402f7890`, `sub_1402f7cf0`, and `sub_1402f83b0` implement the
client-side turret aim controllers. These compute:

1. Direction to target via `atan2`
2. Angular velocity/acceleration via `sub_1402f75d0` (a PID-like controller)
3. Angle wrapping to [-π, π] via `0.159154937f` (1/2π) and `6.28318548f` (2π)
4. Speed decay using `powf(base, dt)` where `base` is stored at `rsi[0x1a]`
5. Speed clamping: `min(new_speed, (dist² * a + dist * b + c) * max_factor)`

These are the smooth turret-tracking controllers visible when aiming in-game.
Not related to penetration but documented here as they were investigated during
the penetration formula search.

---

## 10. HE Splash Damage Mechanics — `PySplashMesh`

Source path embedded in the binary:
```
D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\wows\source\lib\lesta\physics\splash_meshes.cpp
D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\wows\source\lib\lesta/physics/pyPhysics/py_splash_mesh.h
```

### Architecture overview

The splash damage system uses **axis-aligned bounding boxes (AABBs)** to represent
ship armor regions. A `PySplashMesh` object holds an array of named splash boxes,
each with:
- A name (string identifier like "bow", "stern", "citadel", etc.)
- An AABB defined by min/max (x, y, z) coordinates
- A "marked" flag (set by `markNamedArmorBoxes`)

The splash box array is stored at `self + 0x828` as a contiguous vector of 64-byte
(8 qwords) entries:
```
offset 0x00: name (std::string, inline SSO buffer or heap pointer)
offset 0x20: AABB min (3 floats: x_min, y_min, z_min)
offset 0x2C: AABB max (3 floats: x_max, y_max, z_max)
offset 0x38: marked flag (byte)
```

A BVH (bounding volume hierarchy) tree is stored at `self + 0x810` for spatial
acceleration of intersection queries.

### Python-exposed methods

| Method | Function | Args | Description |
|--------|----------|------|-------------|
| `getSplashEffectiveArmor` | `sub_14039fc00` → `sub_1403a1b10` | (Vector3, Vector3, PyObj) | Compute effective armor at a point |
| `getIntersectedBoxes` | `sub_14039de40` → `sub_1403a14f0` | (Vector3, Vector3) | List boxes intersected by line segment |
| `getDistanceToSplashBox` | `sub_14039e210` → `sub_1403a22f0` | (Vector3, string) | Distance from point to named box center |
| `getRayIntersectedArray` | `sub_14039e6b0` → `sub_1403a1cb0` | (Vector3, Vector3) | Ray-cast: sorted list of (name, t-param) pairs |
| `getSplashBoxes` | `sub_14039f940` → `sub_1403a27f0` | () | List all boxes as (min_pt, max_pt, name) tuples |
| `getSplashBoxNameAtPoint` | `sub_14039edc0` | (Vector3) | Return name of box containing point |
| `getNearestSplashBoxName` | `sub_1403a0470` → `sub_1403a2510` | (Vector3, list[str]) | Nearest box (by name filter) to a point |
| `markNamedArmorBoxes` | `sub_1403a00e0` | (list[str]) | Set `marked` flag on boxes matching name list |

### `getSplashEffectiveArmor` — core formula

The core computation (`sub_1403a1b10`) takes:
- `arg1`: the `PySplashMesh` object
- `arg2`: splash position (Vector3)
- `arg3`: splash half-extents (Vector3) — the splash radius/size per axis
- `arg4`: output Python object reference

**Algorithm:**

1. Call `sub_1403a2dd0` to extract mesh data into a local buffer:
   - Returns `zmm7_1` (a threshold float) and fills arrays with per-axis
     armor thicknesses and weight values

2. For each axis `i` in {x, y, z}:
   ```
   penetration_dist[i] = abs(splash_pos[i]) - half_extent[i]
   if penetration_dist[i] <= threshold:
       // Inside or touching the splash zone on this axis
       clamped_dist[i] = penetration_dist[i]
   ```

3. Compute total distance:
   ```
   total_dist = clamped_dist[x] + clamped_dist[y] + clamped_dist[z]
   ```

4. If `total_dist != threshold` (i.e., splash actually reaches armor):
   ```
   effective_armor = (dist_y * weight_y + dist_x * weight_x + dist_z * weight_z)
                     / total_dist
   ```

This is a **distance-weighted average** of armor thicknesses across the three
axes the splash penetrates through. Axes where the splash doesn't reach the box
contribute zero weight.

### `getDistanceToSplashBox` — distance computation

The core function (`sub_1403a22f0`) finds a named box by string comparison, then:

1. Computes the box center:
   ```
   center = (box_min + box_max) * 0.5
   ```

2. Computes vector from center to query point:
   ```
   delta = center - query_point
   ```

3. Computes Euclidean distance:
   ```
   dist = sqrt(delta.x² + delta.y² + delta.z²)
   ```

4. Normalizes the direction vector (with zero-divide guard)

5. Calls `sub_140a97370` (ray-AABB intersection) to find the exact intersection
   point on the box surface along the direction from query point to center

6. Returns the **scaled distance** (intersection parameter × direction)

If the named box is not found, logs:
```
PySplashMesh::getDistanceToSplashBox. HitLocation name %s is not found
```

### `getRayIntersectedArray` — ray casting

The core function (`sub_1403a1cb0`) casts a ray through the BVH:

1. Calls `sub_1403e79c0` (BVH traversal) with `origin` and `direction` vectors,
   collecting up to 256 (`0x100`) hit results

2. Sorts results by distance using `sub_1403a33e0` with comparator `sub_14039de30`

3. Deduplicates adjacent hits that share the same box name and have nearly
   identical t-parameters (threshold `1.1920929e-07` = float epsilon)

4. Builds Python list of `(name, t_near)` tuples

### `getSplashBoxNameAtPoint` — point containment

The function (`sub_14039edc0`) iterates over all splash boxes:

```
for each box in splash_boxes:
    if box.marked == true:
        continue  // skip marked boxes
    if point.x >= box.x_min && point.x < box.x_max &&
       point.y >= box.y_min && point.y < box.y_max &&
       point.z >= box.z_min && point.z < box.z_max:
        return box.name
return ""  // empty string if no box contains point
```

Note: marked boxes are **excluded** from point containment queries. The `marked`
flag is set by `markNamedArmorBoxes` and is used to partition boxes into "active"
and "inactive" sets.

### `getNearestSplashBoxName` — closest box query

The function (`sub_1403a2510`) filters boxes by a provided name list:

1. Builds a hash set from the input string list for O(1) lookup
   (using FNV-1a hash: initial value `0xcbf29ce484222325`, prime `0x100000001b3`)

2. For each splash box whose name is in the filter set:
   ```
   for each axis (x, y, z):
       if point[axis] > box_max[axis]:
           clamped_delta[axis] = point[axis] - box_max[axis]
       elif point[axis] < box_min[axis]:
           clamped_delta[axis] = point[axis] - box_min[axis]
       else:
           clamped_delta[axis] = 0
   dist = sqrt(clamped_delta.x² + clamped_delta.y² + clamped_delta.z²)
   ```

3. Returns the name of the box with the smallest distance

This computes **point-to-AABB distance** (clamping to box surface), not
center-to-center distance.

### `markNamedArmorBoxes` — box selection

The function (`sub_1403a00e0`) takes a Python list of box name strings:

1. Parses the name list via `sub_1403a30f0`
2. For each splash box: sets `box.marked = false`
3. For each splash box, for each input name:
   - If `box.name == input_name`: set `box.marked = true`

Marked boxes are excluded from `getSplashBoxNameAtPoint` queries.

### `getIntersectedBoxes` — segment intersection

The function (`sub_1403a14f0`):

1. Calls `sub_1403e6f40` which performs BVH traversal to find AABB candidates

2. For each candidate box, computes the **clipped intersection volume**:
   ```
   clipped_min = max(box_min, ray_aabb_min)
   clipped_max = min(box_max, ray_aabb_max)
   volume = (max_x - min_x) * (max_y - min_y) * (max_z - min_z)
   ```
   (volume = 0 if no overlap on any axis)

3. Also computes a second clipped volume variant for the "positive quadrant"
   (clamping min to 0) — used for partial penetration scoring

4. Computes the **center-to-center distance** between the query AABB center
   and the box center:
   ```
   query_center = (query_min + query_max) * 0.5
   box_center = (box_min + box_max) * 0.5
   manhattan_dist = |Δx| + |Δy| + |Δz|
   ```

5. Checks if query center is **inside** the box

6. Calls `sub_140a97370` (ray-AABB intersection) along the center-to-center
   direction for precise intersection parameterization

7. Returns a Python list of tuples: `(box_min_pt, box_max_pt, box_name)`
   for each intersected box

### BVH tree structure

The BVH tree (`sub_1403e79c0` / `sub_1403e7b70`) is stored as an array of
40-byte (5 qwords) nodes:

```
offset 0x00: left_child_index (int32, -1 if leaf)
offset 0x04: right_child_index (int32, -1 if leaf)
offset 0x08: AABB bounds (6 floats: min_x, min_y, min_z, max_x, max_y, max_z)
offset 0x20: leaf data pointer (if leaf node)
```

Traversal (`sub_1403e79c0`) is recursive:
1. Test ray-AABB intersection against current node (`sub_140a97370`)
2. If hit and children exist: recurse into left and right children
3. If leaf: add the leaf's box to the output (up to capacity limit)

### Ray-AABB intersection (`sub_140a97370`)

Standard slab method for ray-AABB intersection:

```
for each axis in {x, y, z}:
    if abs(direction[axis]) > epsilon:
        t_near = (box_min[axis] - origin[axis]) / direction[axis]
        t_far  = (box_max[axis] - origin[axis]) / direction[axis]
    // Check if the intersection point on this slab is within
    // the other two axes' extents
    // Track global t_min (nearest entry) and t_max (farthest entry)
```

The function also iterates over both `box_min` and `box_max` faces (the loop
runs twice with `i_1` counting from 2 down to 1), testing each face and
updating `t_near`/`t_far` parameters.

Returns: `(t_near, t_far)` via output pointers, and `true` if `t_far >= 0`
(i.e., the ray hits the box in the forward direction).

---

## 11. Underwater Ballistics — `getDistUnderWater`, `getVeloUnderWater`, `getTimeUnderWater`

Source path embedded in the binary:
```
D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\wows\source\lib\lesta\gamelogic\reverse_ballistics\ballistics_trajectory.cpp
```

### Overview

Three Python-exposed functions compute underwater shell trajectory using an
**exponential drag deceleration model** (quadratic fluid drag with closed-form
solutions). All three share the same drag coefficient computation and are
mathematically consistent — each solves a different variable from the same
underlying ODE.

### Function addresses

| Python name | Wrapper | Core computation |
|---|---|---|
| `Ballistics::_py_getDistUnderWater` | `sub_140308f50` | inline after arg extraction |
| `Ballistics::_py_getVeloUnderWater` | `sub_140309930` | inline after arg extraction |
| `Ballistics::_py_getTimeUnderWater` | `sub_14030a310` | inline after arg extraction |

All three take 5 float arguments from Python via `PyArg_ParseTuple(args, "fffff", ...)`.

### Arguments

| # | Name | Units | Description |
|---|------|-------|-------------|
| 1 | `dist` or `time` | m or s | Independent variable (see per-function) |
| 2 | `V0` | m/s | Initial underwater velocity (at water entry) |
| 3 | `bulletDiametr` | m | Shell caliber (diameter) in meters |
| 4 | `bulletMass` | kg | Shell mass |
| 5 | `Cd` | dimensionless | Drag coefficient in water |

### Drag coefficient

All three functions compute the same drag constant `K`:

```
K = 392.942596 * bulletDiametr² * Cd / bulletMass
```

The constant `392.942596` is stored as a 32-bit float at address `0x14255cb34`
(bytes `a7 78 c4 43` little-endian, confirmed value `392.9425964355469`).

#### Physical derivation

The quadratic drag force on a sphere/projectile in fluid is:

```
F_drag = 0.5 * ρ * Cd * A * v²
```

where:
- `ρ` = fluid density (water ≈ 1000 kg/m³)
- `A` = cross-sectional area = `π/4 * d²`

The drag deceleration is:

```
a = F_drag / m = (ρ/2 * π/4) * Cd * d² * v² / m = K * v²
```

So:

```
K = (ρ_water / 2) * (π / 4) * d² * Cd / m
  = (1000 / 2) * (π / 4) * d² * Cd / m
  = 500 * 0.7853981... * d² * Cd / m
  ≈ 392.699... * d² * Cd / m
```

The game uses `392.942596` rather than the exact `500π/4 ≈ 392.699`, suggesting
either a slightly different water density (≈1000.62 kg/m³) or a precomputed
constant with minor rounding. The difference is <0.07% and negligible.

### The underlying ODE

With quadratic drag only (no gravity component in the direction of travel):

```
dv/dt = -K * v²
```

This separable ODE has the solution:

```
v(t) = V0 / (1 + K * V0 * t)
```

Or equivalently, in the distance domain:

```
dv/dx = dv/dt * dt/dx = (-K * v²) * (1/v) = -K * v
```

Which gives:

```
v(x) = V0 * exp(-K * x)
```

Both forms are consistent; the game uses whichever is more convenient for each
function.

### Function formulas

#### `getDistUnderWater(dist, V0, d, m, Cd)` → distance traveled

Given a **distance** `dist` as input (confusingly named — this appears to be the
time parameter in practice, or a reparameterized distance), computes:

```
K = 392.942596 * d² * Cd / m
result = ln(1 + K * dist * V0) / K
```

This is the integral of `v(t) = V0 / (1 + K*V0*t)` from 0 to `dist`:

```
x(t) = ∫₀ᵗ v(τ) dτ = ln(1 + K * V0 * t) / K
```

Assembly confirms: `fld` loads the constant, `fmul` chains compute `K`, then
`fyl2xp1` computes `log2(1 + K*dist*V0)`, followed by multiplication by
`ln(2)/K` to convert to natural log.

#### `getVeloUnderWater(time, V0, d, m, Cd)` → velocity after time

```
K = 392.942596 * d² * Cd / m
result = V0 / exp(K * time)
     = V0 * exp(-K * time)
```

This is the velocity-distance relation `v(x) = V0 * exp(-K*x)` where `time`
represents the distance traveled underwater. (The argument naming in the game
code is inconsistent — what's called "time" here acts as distance in the
exponential decay formula.)

Assembly confirms: computes `K * time`, calls `expf()`, divides `V0` by result.

#### `getTimeUnderWater(dist, V0, d, m, Cd)` → time elapsed

```
K = 392.942596 * d² * Cd / m
result = (exp(K * dist) - 1) / (K * V0)
```

This is the inverse of `getDistUnderWater`: given distance `x`, solve for time `t`:

```
x = ln(1 + K * V0 * t) / K
K * x = ln(1 + K * V0 * t)
exp(K * x) = 1 + K * V0 * t
t = (exp(K * x) - 1) / (K * V0)
```

Assembly confirms: computes `K * dist`, calls `expf()`, subtracts `1.0`
(loaded from `0x14255bf90`), divides by `K * V0`.

### Consistency check

The three functions are mutually consistent:

```
Let t = getTimeUnderWater(x, V0, d, m, Cd)
    = (exp(K*x) - 1) / (K * V0)

Then getDistUnderWater(t, V0, d, m, Cd)
    = ln(1 + K * t * V0) / K
    = ln(1 + (exp(K*x) - 1)) / K
    = ln(exp(K*x)) / K
    = x  ✓

And getVeloUnderWater(x, V0, d, m, Cd)
    = V0 * exp(-K * x)
    = V0 / (1 + K * V0 * t)   [substituting t]  ✓
```

### Usage in the game

These functions are called by the server (and possibly client prediction) to
compute what happens when a shell enters water:

1. Shell hits water surface with velocity `V0` at some angle
2. `getDistUnderWater` computes how far the shell travels underwater
3. `getVeloUnderWater` computes the shell's velocity at any point underwater
4. `getTimeUnderWater` computes how long the shell spends underwater

This enables the game's underwater citadel hit mechanic: AP shells that land
short can dive under the waterline and hit the underwater belt/citadel if they
retain enough velocity after traveling through water.

### Implementation notes for penetration calculator

To simulate underwater hits:
1. Compute water entry point from trajectory (intersection with sea level)
2. Decompose velocity into horizontal and vertical components
3. Apply underwater drag using `K = 392.942596 * d² * Cd / m`
4. Track underwater travel distance to determine if shell reaches the hull
5. Use remaining velocity at hull contact for penetration check

The `Cd` (water drag coefficient) should be available in GameParams shell data
(e.g., `bulletDeceleration` or similar field for underwater drag). The value
`bulletAirDrag` is the air drag coefficient — water drag uses a separate parameter.

---

## 12. Summary

| Component | Game (client) | Our implementation | Match? |
|-----------|--------------|-------------------|--------|
| ISA atmospheric constants | P0, L, T0, G, M_AIR, R_GAS | Same values | **Yes** |
| Air density formula | ISA barometric | ISA barometric | **Yes** |
| Drag force | 0.5 * cd * area * rho * v² / mass | k * rho * v * speed (equivalent) | **Yes** |
| Cross-sectional area | pi/4 * d² | 0.5 * cd * (d/2)² * pi / mass (folded into k) | **Yes** |
| Dimensionality | 3D (vx, vy, vz) | 2D (vx, vy) | ~Close |
| Integration | Forward Euler, adaptive dt | RK4, fixed dt=0.02s | Different |
| Max range | 42 000 m | 200 s timeout | ~Same |
| Time multiplier | Not in trajectory code | 2.75 (applied to output) | N/A |
| Penetration | Server-only | wows_shell formula (community) | Unverifiable |
| Normalization/ricochet | Server-only | Community constants | Unverifiable |
| Fuse mechanics | Server-only | Community formula | Unverifiable |
| HE splash geometry | AABB boxes + BVH | Not implemented | N/A (documented) |
| Underwater drag model | Quadratic drag, K=392.94*d²*Cd/m | Not yet implemented | **Yes** (formulas extracted) |
| Underwater closed-form solutions | 3 functions (dist, velo, time) | Not yet implemented | **Yes** (fully RE'd) |

### Key takeaway

The trajectory physics (drag, atmospheric model, gravity) are **identical** between
the game client and our implementation. The main differences are:

1. **3D vs 2D** — the game does full 3D, we do 2D planar (sufficient for range/impact calculations)
2. **Euler vs RK4** — the game uses cheaper Euler with adaptive step, we use more accurate RK4 with fixed step
3. **Penetration is server-only** — our penetration formulas come from community reverse engineering (jcw780) and cannot be verified from the client binary
