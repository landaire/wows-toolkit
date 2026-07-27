# Fire Chance Reverse Engineering Notes

How World of Warships decides whether a shell hit starts a fire, recovered from the
deobfuscated client scripts (`G:\deob\scripts`, build 12668706 / 15.6.0), stage4
bytecode for constants the decompiler zeroes, and live `GameParams.json`.

Scope: the attacker-side burn chance, the defender-side fire resistance, the burn
node (fire section) model, and which parts are and are not knowable from a replay.
Written to support an "effective fire chance" stat in the replay inspector.

---

## 1. Two independent halves

The per-hit probability is built from an attacker term and a defender term. They
live in different places and are combined server side.

```
P(fire | hit) = attacker_burn_chance * defender_node_probability
```

- `attacker_burn_chance` = `ModifiersApply.calculateBurnChance(...)`, a function of
  the shell's `burnProb`, the firing ship's tier, and the firing player's build.
- `defender_node_probability` = `hull.burnNodes[i][0] * defender_modifier.burnProb`,
  a per-hull balance stat times the target player's survivability build.

The multiplication itself is server side and is not in the client scripts. It is
inferred: the client computes and displays only the attacker half (the port stat
card), and stores the defender half per burn node without ever rolling against it
(`BurnNode.probability` is only surfaced in the `Burn And Flood` ImGui dev panel
tooltip, `m1d2e83a2/ImGui/BurnAndFloodPanel/BurnAndFloodPanel.py:31`).

---

## 2. Attacker side: `calculateBurnChance`

`Modifiers/ModifiersApply.py:39-66`:

```python
def calculateBurnChance(ammoOwnerLevel, ammoParams, modifier, initialBurnProb):
    species = ammoParams.typeinfo.species
    if species == ProjectileTypes.ARTILLERY:
        if ammoOwnerLevel > MAX_SMALL_CATEGORY_LEVEL:
            initialBurnProb *= modifier.burnChanceFactorHighLevel
        else:
            initialBurnProb *= modifier.burnChanceFactorLowLevel
        initialBurnProb *= modifier.burnChanceGMGSMultiplier
        initialBurnProb += modifier.artilleryBurnChanceBonus
    elif species == ProjectileTypes.ROCKET:
        initialBurnProb += modifier.rocketBurnChanceBonus
    elif species in ProjectileTypes.PLANE_BOMBS:
        initialBurnProb += modifier.bombBurnChanceBonus

    initialBurnProb *= modifier.burnChanceMultiplier
    initialBurnProb += modifier.burnChanceBonus

    if isSmallProjectile(ammoParams):
        initialBurnProb += modifier.burnChanceFactorSmall
    else:
        initialBurnProb += modifier.burnChanceFactorBig

    return max(initialBurnProb, 0)
```

`initialBurnProb` is the projectile param `burnProb`. `ammoOwnerLevel` is the firing
ship's tier.

This is already transcribed in the toolkit:
`wowsunpack/src/game_params/ttx/weapon_tables.rs::calculate_burn_chance`, which also
returns an ordered `Vec<AppliedModifier>` describing every step. That provenance list
is what a per-ship formula breakdown should render.

### 2.1 Constants

The straight decompile zeroes compiled-module floats (`me658a8e4.py` lines 11-19 are
all `0.0`). Real values recovered from stage4 bytecode
(`G:\dev\wowsdeob\output\m3510ec80_stage4_deob.pyc`):

| Constant | Real value | Use |
|---|---|---|
| `SMALL_PROJECTILE_MAX_DIAMETER` | `0.16` m | `isSmallProjectile`: picks `burnChanceFactorSmall` vs `Big` |
| `SMALL_SHELL_MAX_DIAMETER` | `0.149` m | `isSmallGun` (unrelated to fire) |
| `HEAVY_CRUISER_SHELL_DIAMETER` | `0.19` m | AP damage coeff gate (unrelated to fire) |
| `MAX_SMALL_CATEGORY_LEVEL` | `7` (int, not zeroed) | tier gate for `burnChanceFactor{Low,High}Level` |

`Modifiers/__init__.py:5`:

```python
def isSmallProjectile(ammoParams):
    if ammoParams.typeinfo.species in ProjectileTypes.PLANE_BOMBS:
        return False
    if hasattr(ammoParams, 'bulletDiametr'):
        return ammoParams.bulletDiametr <= SMALL_PROJECTILE_MAX_DIAMETER
    return True
```

Note 160 mm, not the 139 mm / 149 mm thresholds used elsewhere. This matches the
signal flags: Victor Lima and India X-Ray give `+0.005` to guns up to 160 mm and
`+0.01` above it.

**Known defect:** `wowsunpack/src/game_params/ttx/factories.rs:597` still carries the
zeroed placeholder `SMALL_PROJECTILE_MAX_DIAMETER_M: f32 = 0.0`, so every shell is
classified "big" and the port stat card overstates the Victor Lima / India X-Ray
bonus by 0.5 pp for guns of 160 mm and under.

### 2.2 Where each modifier comes from (live GameParams)

| Modifier | Source | Value | Op |
|---|---|---|---|
| `artilleryBurnChanceBonus` | `HeFireProbability` skill (Demolition Expert, `skillType` 8) | `+0.01` for BB/CA/DD owners, `0.0` for CV/Sub | add |
| `burnChanceFactorLowLevel` / `HighLevel` | `HePenetration` skill (IFHE, `skillType` 33) | `0.5` both | multiply |
| `burnChanceFactorSmall` | `PCEF017_VL_SignalFlag`, `PCEF018_IX_SignalFlag` | `+0.005` | add |
| `burnChanceFactorBig` | same two flags | `+0.01` | add |
| `rocketBurnChanceBonus` | `HeFireProbabilityCv` skill (`skillType` 46) | `+0.01` | add |
| `bombBurnChanceBonus` | same skill | `+0.05` | add |
| `burnChanceMultiplier` | `PCOM005_HEBurnChance`, the Arms Race / battle-buff HE fire-chance powerup | `1.0` at level 0 rising to `2.2` at level 6 | multiply |
| `burnChanceGMGSMultiplier`, `burnChanceBonus` | no live user | identity | multiply / add |

`burnChanceMultiplier` is carried by a `species: Modifier` param (the `PCOM*`
battle-buff family), not by a skill, upgrade or signal. Those params each embed
the full modifier template with every name at identity, which is why the name
appears about 1100 times in GameParams while only `PCOM005_HEBurnChance` moves
it off `1.0`. Searching only top-level `.modifiers` on skills, upgrades and
exteriors misses it.

Names that exist in the modifier vocabulary but have no live user anywhere in
GameParams or the deob scripts: `artilleryBurnChanceBonusMain`,
`artilleryBurnChanceBonusSec`, `chanceToSetOnFireBonusMain`,
`chanceToSetOnFireBonusSecondary`, `chanceToSetOnFireBonusAll`,
`mainGaugeBurnProbabilityForCapture`. The engine therefore has the vocabulary to
split fire modifiers between main battery and secondaries, and currently uses
none of it. See section 2.5.

`artilleryBurnChanceBonus` is a per-species dict keyed by the **owner's** ship class
(`maa3520d6.py:3985`), resolved through the existing `get_for_species` path.

### 2.3 Projectile `burnProb`

Per-projectile in GameParams. Observed ranges:

- HE artillery: `0.0` to about `0.5`.
- AP and SAP (`CS`): the sentinel `-0.5`. `max(x, 0)` clamps it to zero, and the
  `-0.5` magnitude deliberately absorbs every additive bonus in the game (Demolition
  Expert `+0.01` plus a flag `+0.01`) so AP and SAP can never be given fire chance.
- Torpedoes: `0.0`.

So "can this projectile start a fire at all" is `calculate_burn_chance(...) > 0`, not
an ammo-type test. That is the correct gate and it is version independent.

### 2.5 Secondaries take the same fire modifiers as the main battery

Nothing in the client distinguishes secondary fire chance from main-battery fire
chance, on either side of the calculation.

**Attacker side, one code path.** `ma6320f36/ttx/FactoryArtillery.py:147`
(`createAmmoTTX`) is called for both main battery and secondaries, separated by
an `isATBA` flag:

```python
def createAmmoTTX(preprocessedAmmo, prepared, isATBA=False, altModernization=None, isManualATBA=False):
    q = altModernization or prepared.modernization
    ...
    if preprocessedAmmo.type == SHELL_TYPES.HE:
        burnChance = ModifiersApply.calculateBurnChance(prepared.level, ammo, q, ammo.burnProb)
        ...
        if isATBA:
            pen = q.GSPenetrationCoeffHE          # secondary HE penetration
        else:
            pen = q.GMPenetrationCoeffHE          # main HE penetration
```

`isATBA` branches HE *penetration* and nothing else. The burn chance call is
identical, with the same modifier bundle `q`. And `calculateBurnChance` itself
only branches on `ammoParams.typeinfo.species == ProjectileTypes.ARTILLERY`,
which is true of secondary shells too; it has no ATBA branch.

Consequence: IFHE's `burnChanceFactorLowLevel`/`HighLevel` of `0.5` halves
secondary fire chance exactly as it halves main-battery fire chance, and
Demolition Expert and the Victor Lima / India X-Ray flags add to secondaries
too. `burnChanceGMGSMultiplier` is named for both batteries (GM and GS) and sits
in the same shared branch.

**Defender side, structurally cannot discriminate.** The target's fire reduction
is folded into the burn node when the ship is built
(`_createBurnNode(owner, hullNodeProb * modifier.burnProb, ...)`, section 3.2).
A node holds one probability, fixed at construction, with no knowledge of what
will later hit it. There is no per-shot hook where a defender-side reduction
could be made weapon-aware. So Fire Prevention Expert and Damage Control System
Modification 1 apply to secondary-set fires the same as any other.

Caveat: this is all client code. The roll itself is server side. The attacker
half is strong evidence, since it is the same function the port stat card shows.
The defender half is inference from the node's shape, but that shape is dictated
by replicated state (`burningFlags` and the node set), not by display code.

### 2.4 Legacy formula (0.6.x)

`G:\deob\0.6.13_296659\ModifiersApply.py:10` is a different function with different
modifier names:

```python
def getBurnProb(ammoParams, modernization, crewModifiers):
    p = ammoParams.burnProb
    if modernization:
        p += (modernization.burnChanceFactorSmall if isSmallProjectile(ammoParams)
              else modernization.burnChanceFactorBig) - 1.0
    if crewModifiers:
        if ammoParams.typeinfo.species == ARTILLERY:
            p += (crewModifiers.chanceToSetOnFireBonusSmall if isSmallBullet(ammoParams)
                  else crewModifiers.chanceToSetOnFireBonusBig)
        p += crewModifiers.probabilityFireBonus
    return max(p, 0)
```

Differences that matter for old replays:

- Upgrade factors were centred on `1.0` and folded in as `value - 1.0`.
- Crew bonuses used `chanceToSetOnFireBonus{Small,Big}` and `probabilityFireBonus`,
  none of which exist today.
- `isSmallBullet` used a hard-coded `0.139` m, distinct from `isSmallProjectile`.

Anything computing an attacker burn chance for a pre-0.7 replay must read the
modifier names present in that build's GameParams rather than today's names.

---

## 3. Defender side: burn nodes

### 3.1 Hull params

Every hull carries `burnNodes`, a list of triples:

```
burnNodes = [(probability, damage, duration), ...]
```

Iowa `A_Hull`: `[[0.6004, 0.3, 60.0], ...]` repeated four times.

The length is the ship's fire-section count and is **not** always four, despite
`MAX_BURN_NODES_COUNT = 4`. Across live GameParams:

| Species | Nodes | Hulls |
| --- | --- | --- |
| Battleship, Cruiser, AirCarrier | 4 | all |
| Destroyer | 4 | 374 |
| Destroyer | 1 | 1 |
| Submarine | 1 | 38 |
| Submarine | 4 | 5 |
| Auxiliary | 4 / 2 / 1 | 14 / 10 / 3 |

Anything reading this must take the count from `burnNodes.len()`. Most submarines
have a single fire section.

- `probability` is the ship's hidden fire-resistance coefficient. Observed live range
  is `0.2` to `1.0` (plus `9.0` on some non-combat / bot hulls). It varies per ship
  and per hull, correlates loosely with tier, and is not exposed anywhere in the
  client UI.
- `damage` is fire damage as a fraction of max HP per second (`0.3` on Iowa reads as
  0.3 %/s, the documented battleship fire DPS).
- `duration` is the base burn time in seconds (`60.0` on Iowa).

All entries are identical within a hull on every hull inspected.

### 3.2 Node construction

`m09838fe6/m0700235d.py:317` (`HitLocationEffects._createNodes`):

```python
totalDuration = modifier.hlCritTimeCoeff * modifier.burnTime
burnProbMod   = modifier.burnProb            # defender's modifier bundle
for i, (prob, damage, duration) in enumerate(hull.params.burnNodes):
    name = 'fireResistance' if (modifier.fireResistanceEnabled and i == 1) else 'fire%d' % (i + 1)
    nodes.append(BurnNode(owner, prob * burnProbMod, damage, duration * totalDuration, name))
```

So the node probability the client stores is
`hull.burnNodes[i][0] * defender_modifier.burnProb`.

### 3.3 Defender modifiers

| Modifier | Source | Value |
|---|---|---|
| `burnProb` | `PCM020_DamageControl_Mod_I` (Damage Control System Modification 1) | `0.95` |
| `burnProb` | `DefenceFireProbability` skill (Fire Prevention Expert, `skillType` 14) | `0.9` |
| `fireResistanceEnabled` | same skill | `true` |

They stack multiplicatively (`0.95 * 0.9 = 0.855`), which is how the existing
`ModifierSet` / `ModifierBundle` combine unlisted coefficient modifiers already.

`burnTime` (`1.25` on the `ApDamageBb` skill) and `vulnerabilityBurn` (`0.9` on the
same skill) affect fire duration and fire damage, not fire chance.

### 3.4 `fireResistanceEnabled` removes node index 2

`m09838fe6/m0700235d.py:344` (`setBurningFlags`):

```python
for i, node in enumerate(self.nodes):
    if fireResistanceEnabled and i == 2:
        active = False
    else:
        active = bool(flags & (1 << i))
```

With Fire Prevention Expert, bit 2 of `burningFlags` is forced off, i.e. the third
burn node can never burn. That is the mechanical implementation of "reduces the
maximum number of fires from 4 to 3".

Note the off-by-one against `_createNodes`, which renames node index **1** to the
`fireResistance` effect group while `setBurningFlags` suppresses index **2**. The
rename is only about which particle effect plays; the suppression is the mechanic.

---

## 4. `burningFlags`: the observable state

`ma779114d` (resistant to decompilation; constants recovered by disassembling the
module code object with CPython 2.7 `marshal` + `dis`):

| Constant | Value |
|---|---|
| `MAX_BURN_NODES_COUNT` | 4 |
| `MAX_FLOOD_NODES_COUNT` | 4 |
| `MAX_ACID_NODES_COUNT` | 1 |
| `MAX_WILD_FIRE_NODES_COUNT` | 1 |
| `BURN_MASK` | `0x000F` (bits 0-3) |
| `FLOOD_MASK` | `0x00F0` (bits 4-7) |
| `ACID_MASK` | `0x0100` (bit 8) |
| `WILD_FIRE_MASK` | `0x0200` (bit 9) |
| `BURN_NODES_SHIFT` | 0 |
| `BURN_NODES_RUN_ON_DEATH` | `(1, 3)` |
| `NODE_BURN_AFTER_DEATH_TIME` | 10000 |
| `INFINITY_WORK` | `-1.0` |

`burningFlags` is a replicated Vehicle property, already parsed into
`VehicleProps::burning_flags` (`wows-replays`, `controller.rs`). Bits 0-3 are the
four burn nodes. This is the only direct, per-section, server-authoritative view of
a target's fire state available from a replay.

On death the server lights 1 to 3 extra burn nodes for effect
(`HitLocationEffects.onEntityDeath`), so `burningFlags` after a ship dies is not a
combat signal.

---

## 5. Node selection: unresolved

Which of the four nodes a successful roll lights is decided server side and is
**not** present in any client script. Searched: `hitLocationEffects`, `Vehicle.py`,
`ModelArmor`, the `Moduls`/`StateSystem` fire handlers, the 0.6.13 and 0.7.3 dumps,
and the whole deob tree for any position-to-node mapping. Nothing.

What is known:

- Each node has a concrete position on the ship, and the positions partition the
  hull lengthwise. See section 5.1.
- HE damage is applied as a splash event carrying a per-hit `burn` field and an
  `hlName` hit-location name (`Avatar.py:248` `dev_dmgLogSplashKeys`), so the fire
  roll is per shell explosion and is aware of what it hit.
- Nodes are homogeneous (identical probability), so nothing in the data distinguishes
  them other than position.

### 5.1 Where the nodes are

`A_Hull.effects.fire{i+1}` names the emitter node for burn node `i`, e.g. Iowa's
`fire1` is `[["HP_FX_Fire_1", "particles/vehicles/Fire_big_2.xml"]]`.

The model does not contain that name. GameParams uses an `HP_FX_` prefix; the
model node is the same suffix under `EP_` (`EP_Fire_1`). The nodes are not in the
`.visual` records at all: they live in the per-section **skeleton extenders**,
`<ship>_<Section>_ep.skel_ext`, already parsed by
`wowsunpack::models::skeleton_extender`. Each is parented to `Scene Root`, so all
sections share one hull-local space and no mount composition is needed.

A hull is built from section parts. Iowa's root visual carries `HP_Bow`,
`HP_MidFront`, `HP_MidBack` and `HP_Stern`, and exactly one fire node lives in
each section's extender:

| Node | Section | Local z | Meters |
| --- | --- | --- | --- |
| `EP_Fire_1` | Bow | +6.489 | +97.3 |
| `EP_Fire_2` | MidFront | +1.317 | +19.8 |
| `EP_Fire_3` | MidBack | -2.480 | -37.2 |
| `EP_Fire_4` | Stern | -6.912 | -103.7 |

Monotonic bow to stern, one per hull section. Model space is right-handed with
+Z toward the bow.

**Model space is a fixed 15 meters per unit.** There is no scale anywhere in the
exported hierarchy to compose: on every hull checked, `Scene Root`, `export` and
each `HP_<Section>` node are identity with unit column lengths, so the raw node
translation is already hull-local and only needs multiplying.

The constant is 15, measured across the roster. For each of 1158 ships the root
visual's z extent times 15 was compared against `A_Hull.size[0]`:

| | ratio |
| --- | --- |
| p5 | 1.001 |
| p25 | 1.009 |
| median | 1.014 |
| p75 | 1.021 |
| p95 | 1.070 |

The model is consistently a little longer than the published length, which is
what a bounding box over the whole hull should be: it covers bow and stern
overhang that `size[0]` excludes. The top of the band is carriers, whose flight
decks project past the hull at both ends (Langley 1.166, Hosho 1.152, Bogue
1.220). The tail below 1.0 is event and joke hulls (Battle Duck, Crab
Battleship, Transylvania) and ships whose GameParams points at a stand-in model.

The three validation ships:

| Ship | `size[0]` | z extent | extent x 15 | ratio |
| --- | --- | --- | --- | --- |
| `PASB018_Iowa_1944` | 262.1 m | 18.326 | 274.9 m | 1.049 |
| `PFSD110_Kleber` | 141.0 m | 9.567 | 143.5 m | 1.018 |
| `PASS110_Balao` | 94.99 m | 6.391 | 95.9 m | 1.009 |

Iowa is the loosest of the three because its `size[0]` is the waterline length;
against the real 270.4 m overall the model is 1.7% long, in line with the rest.

Deriving the scale per hull instead (`size[0]` over the z extent) would give
14.30 for Iowa and 14.86 for Balao, and would absorb a carrier's flight-deck
overhang into a 15% scale error. It is the wrong model: the scale is a property
of the engine, not of the hull.

The same 15 was already recovered independently, from the main-battery
dispersion formula against published port values, as
`wowsunpack::game_params::ttx::constants::BW_TO_SHIP`. Two unrelated derivations
agreeing is why `crates/wows-core/src/units.rs` now documents
`ShipModelDistance` as 15 m per unit; its previous claim of 2 m
(`BW_TO_METERS / BW_TO_SHIP`, 30/15) was arithmetic on a misreading and had no
callers.

The resolver is `wowsunpack::models::fire_nodes::resolve_fire_sections`, which
returns positions in meters so consumers never handle the scale. Across the
whole roster 1197 of 1199 hulls resolve; the two that do not are a ship whose
model is absent from `assets.bin` and one event submarine whose GameParams
claims four burn nodes for a model carrying two.

`EP_Fire_5` and `EP_Fire_5_1` also exist on 452 hulls. These are **not** burn
nodes: they are the extra emitters of the `fireResistance` effect group, which
replaces `fire2`'s group when the target has Fire Prevention Expert. Only
`EP_Fire_1..N` for `N = burnNodes.len()` are burn nodes.

Surveyed across every `_ep.skel_ext` in the live build: 1027 models carry four
distinct fire nodes, 452 carry five (four plus the `fireResistance` extra), and
the rest are submarines and special entities carrying one or two, consistent with
their `burnNodes` length.

Two hypotheses remain open:

1. **Positional.** The impact position selects the section; a hit on a burning
   section cannot start a fire there.
2. **Free-node.** The roll is position independent and a success lights any node that
   is not already burning.

They differ materially: under (1) a hit is only "fire-capable" when *its* section is
free, under (2) whenever *any* node is free.

The geometry in 5.1 is strong circumstantial evidence for (1): the nodes sit one
per named hull section, evenly spaced bow to stern, and the section count tracks
`burnNodes.len()` down to submarines having a single node covering the whole
boat. That is the layout a positional rule needs and a position-independent rule
would not.

This is empirically resolvable from replay data: correlate observed
`burningFlags` bit transitions against the impact positions of the shells that
preceded them, over a corpus.

`docs/superpowers/specs/2026-07-27-effective-fire-chance-design.md` adopts
hypothesis (1) with nearest-hardpoint assignment, and carries the correlation
above as a measured agreement rate so the assumption reports on itself.

---

## 6. Damage Control Party

`crashCrew` in GameParams, e.g. `PCY009_CrashCrewPremium.B_Gold`:

```json
{"consumableType": "crashCrew", "numConsumables": -1, "workTime": 15.0,
 "reloadTime": 80.0, "preparationTime": 0.0, "periodical": true}
```

The client's only fire-related logic is the activation gate
(`CommonConsumables/Logic/Ship.py:197`, `CrashCrew.canActivate`), which requires
`needCrashCrew(burningFlags)`, a pending crit, an oil leak, or a resettable ping.
The immunity itself is server side and is not in the scripts, but its effect is
directly observable: while a ship's Damage Control Party is working, its
`burningFlags` burn bits stay clear and no new fire appears.

For replay analysis this consumable is fully observable:

- `consumableUsed` / `onConsumableUsed` is an entity method on the *using* Vehicle,
  so activations are visible for any ship in the recording client's AOI.
- `work_time` and `reload_time` are already resolved per player with build modifiers
  applied in `ConsumableInventory` (`wows-replays` `state.rs`).

Therefore, for a ship observed continuously, "is Damage Control Party running at time
t" is known exactly. Across an observation gap it is only known when the gap is
shorter than the remaining cooldown from the last observed activation, since a ship
cannot reactivate inside `reload_time`.

That cooldown argument fails for ships that can refund or reset consumable charges
early (Valparaiso, San Martin). `ConsumableInventory`'s doc comment already flags
this. A robust detector is behavioural rather than a name list: if any two observed
activations of the same slot are closer together than the modelled `reload_time`,
that ship's cooldown model is unreliable for the rest of the match.

---

## 7. What a replay can and cannot establish

Available per shell hit (`ResolvedShotHit`, already produced by `wows-battle-world`):
game clock, impact world position, victim entity, victim position/yaw/pitch/roll at
impact, hit type, and the originating salvo (shooter, params id, so the shell param
and therefore `burnProb`).

Available per target: `burningFlags` over time, consumable activations, the resolved
build (`ResolvedBuild` with a populated `ModifierSet`) and therefore the defender's
`burnProb` coefficient and `fireResistanceEnabled`.

Available per attacker: the same `ResolvedBuild`, plus ship tier, so
`calculate_burn_chance` is computable exactly.

Available after the match: server-authoritative per attacker/victim fire counts in
the battle results (`playersPublicInfo[attacker].interactions[victim].fires`),
already resolved in `wows-replay-insights::battle_report`.

Not available:

- Which node a fire started in as a function of the hit (section 5).
- Any state at all for a target outside the recording client's AOI. Every "we know"
  claim has to be scoped to observation windows.
- Attribution of a `burningFlags` transition to a specific attacker when several are
  hitting the same target in the same instant. Post-battle results give exact totals
  per attacker/victim pair and are the reliable numerator.

---

## 8. Recovery techniques used here

For future work in this area:

- Compiled-module float constants read `0.0` in the decompiled `.py`. Recover them by
  disassembling the corresponding stage4 module from `G:\dev\wowsdeob\output`:
  `python2 -c "import marshal,dis; f=open(p,'rb'); f.read(8); dis.dis(marshal.load(f))"`,
  then read the `LOAD_CONST` feeding the `STORE_NAME`.
- Modules that resist decompilation entirely (`ma779114d`) still disassemble; module
  level `LOAD_CONST` / `STORE_NAME` pairs give every constant.
- GameParams is fastest to interrogate with `jaq` against
  `E:\WoWsStable\World_of_Warships\GameParams.json`, top level under `.[0]`.
