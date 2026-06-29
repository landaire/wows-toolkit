# Changelog

All notable changes to this project will be documented in this file.

## [unreleased]

### 🚀 Features

- *(wowsunpack)* Expose unit ucType + game-params min CBOR output
- *(wows-data-mgr)* Add CasVfs read-only filesystem over manifest + CAS
- *(wows-data-mgr)* Add BuildCas resolver with legacy fallback and conservative symlink pruning
- *(wows-data-mgr)* Stop materializing symlink trees on download
- *(wows-toolkit)* Load dumped builds through CasVfs instead of symlink tree
- *(wows-toolkit)* Borrow GUI assets from dumped builds via CasVfs
- *(wowsunpack)* Describable trait, return types, gap-safe modifier rendering
- *(wowsunpack)* Describable for Modernization
- *(wowsunpack)* Describable for Exterior; DRY translate_exterior core
- *(wowsunpack)* Describable for CrewSkill
- *(wowsunpack)* Describable name+description for Unit and Ability
- *(wowsunpack)* Delegating Describable for Param
- *(wowsunpack)* Retain raw consumable effect field map on AbilityCategory
- *(wowsunpack)* Per-flavor consumable effect modifiers via AbilityCategory
- *(wowsunpack)* Ttx unit constants and recovered BigWorld scales
- *(wowsunpack)* Ttx ShipStats model and stat newtypes
- *(wowsunpack)* Ttx ModifierBundle aggregation
- *(wowsunpack)* Extract typed TTX hull/engine base stats at parse time
- *(wowsunpack)* Ttx durability and mobility factories
- *(wowsunpack)* Ttx torpedo-protection and submarine battery stats
- *(wowsunpack)* Ttx armor min/max stat
- *(wowsunpack)* Retain torpedo fields (speed/damage/visibility) on Projectile
- *(wowsunpack)* Extract torpedo launcher base stats at parse time
- *(wowsunpack)* Ttx torpedo factory (damage/speed/range/visibility/reload)
- *(wowsunpack)* Extract artillery gun/component base stats at parse time
- *(wowsunpack)* Recover BigWorld dispersion constants and dispersion formula
- *(wowsunpack)* Shell projectile fields and weapon-type damage/burn tables
- *(wowsunpack)* Ttx main-battery artillery factory
- *(wowsunpack)* Ttx secondary-battery (ATBA) factory
- *(wowsunpack)* Extract fire-control maxDistCoef at parse time
- *(wowsunpack)* Extract visibility base fields; transcribe FactoryVisibility
- *(wowsunpack)* Ttx visibility (concealment) factory
- *(wowsunpack)* Ttx ship_stats orchestration entry point
- *(wowsunpack)* Ttx stat label sourcing from global.mo
- *(wowsunpack)* ATBA-specific stat labels for secondaries
- *(wowsunpack)* Effective consumable stats with modifiers applied; ModifierBundle::apply
- *(wowsunpack)* Surface per-type consumable effect fields (smoke/fighters/regen) with modifiers
- *(wows-core)* Add Version::base_eq (friendly-only equality); matches reuses it
- *(wowsunpack)* Display on stat newtypes; StatValue + ShipStats::rows() enumeration
- *(wowsunpack)* Map shell muzzle speed (MetersPerSecond) and submarine battery into ShipStats::rows()
- *(wows-replays)* Nested property-path update decoding + TUPLE arg support
- *(wowsunpack)* Add TranslationKey newtype for IDS_* catalog keys
- *(wowsunpack)* Typed dispersion ellipse API (horizontal + vertical semi-axes)
- *(wowsunpack)* Parse gun dispersion-curve fields + dispersion_curve() builder
- *(wowsunpack)* Surface vertical dispersion as a TTX stat
- *(wowsunpack)* Loadout-level artillery_dispersion profile (ellipse at any range)
- *(wowsunpack)* TTX stat render + diff layer (StatLine/StatDelta)
- *(wowsunpack)* Per-module ShipStats enumeration (module_options)
- *(wowsunpack)* Always-valid TTX stat labels (humanized field_key fallback)
- *(wowsunpack)* Effects engine types (Loadout/Effects/EffectsState/EffectiveModifiers)
- *(wowsunpack)* Enumerate a loadout's effects (skills/triggers/consumables)
- *(wowsunpack)* Resolve effects to a modifier bundle (+ adrenaline formula)
- *(wowsunpack)* Apply adrenaline reload + spotter range via EffectiveModifiers::stats
- *(wowsunpack)* Parse damageValue/countToModifier/interpolators; bump cache to 12
- *(wowsunpack)* Parse ship innate skills (HP-breakpoint adrenaline)
- *(wowsunpack)* Add Stacks activation + stacking effect kinds
- *(wowsunpack)* Enumerate stacking triggers (Furious + potential-damage)
- *(wowsunpack)* Resolve stacking effects into the modifier bundle
- *(wowsunpack)* Clamped piecewise-linear Interpolator::eval + max_x
- *(wowsunpack)* Expose modifier_identity (per-name fold identity)
- *(wowsunpack)* Heat activation/kind + enumerate atbaHeat
- *(wowsunpack)* Resolve heat effects (lerp identity->configured by ratio)
- *(wowsunpack)* InnateAdrenaline kind + enumerate ship innate skills
- *(wowsunpack)* Resolve innate adrenaline (clamp/lerp HP breakpoints)
- *(wowsunpack)* CrewSkillLogicTrigger consumable_type/duration accessors
- *(wowsunpack)* Typed TriggerCondition on binary effects
- *(wowsunpack)* SituationFacts + TriggerCondition::holds
- *(wowsunpack)* Effects::situation_state derives state from SituationFacts
- *(wowsunpack)* Split main-gun and secondary range detection into distinct visibility slots
- *(wowsunpack)* Add TTX stat-attribution provenance data model
- *(wowsunpack)* Add ModifierSources side-channel and zero-cost Recorder
- *(wowsunpack)* Tag modifier sources per upgrade/skill in Effects::resolve
- *(wowsunpack)* Record durability/mobility/battery stat provenance
- *(wowsunpack)* Record armor stat provenance (hull module base)
- *(wowsunpack)* Record main-battery artillery and shell provenance
- *(wowsunpack)* Record secondary-battery provenance
- *(wowsunpack)* Record torpedo armament provenance
- *(wowsunpack)* Record visibility provenance
- *(wowsunpack)* Add ship_stats_explained and provenance coverage/replay tests
- *(wowsunpack)* Add TTX attribution render layer
- *(wowsunpack)* Add SecondaryBattery/SecondaryMount types and secondary grouping helpers
- *(wowsunpack)* Model secondaries as per-caliber sub-batteries
- *(wowsunpack)* Add StatKey and derived_from upstream links to attribution
- *(wowsunpack)* Link derived stats (rotation time, on-fire/range detection) to upstream
- *(wowsunpack)* Attribute spotter and adrenaline-reload coefficients to their input
- *(wowsunpack)* Surface derived_from in render and guard every changed stat is explained
- *(wowsunpack)* Attribute shell damage and fire chance to real inputs (decompose net factors)
- *(wowsunpack)* Add consumable TtxStat variants, ConsumableCard, and rows
- *(wowsunpack)* Effective_consumable reports applied modifiers for attribution
- *(wowsunpack)* Enumerate and attribute ship consumables in orchestration
- *(wowsunpack)* Per-input stat contribution deltas, isolated values, and waterfall
- *(wowsunpack)* Surface raw delta and running value on ContributorLine
- *(wowsunpack)* Link dispersion provenance to max range
- *(wowsunpack)* Surface inherited contributors from derived_from upstream stats
- *(wows-toolkit)* Enable the AV1 video export codec

### 🐛 Bug Fixes

- *(wowsunpack)* Import game_params_to_pickle without vfs gate; bump to 0.36.1
- *(wowsunpack)* Surface per-species modifiers without ship context as Unresolved
- *(wowsunpack)* Include tacticalParams in consumable effect field map
- *(wowsunpack)* Merge logic.modifiers into consumable effect fields
- *(wowsunpack)* Render -1 consumable counts as infinity label via modifier translations
- *(wowsunpack)* Guard ttx modifier classification for absent-additive names and accessor mismatch
- *(wowsunpack)* Use real GTRotationSpeed modifier name for torpedo traverse
- *(wowsunpack)* Parse shell timeFactor; document artillery deferrals
- *(wowsunpack)* Match weapon hardpoints nation-agnostically
- *(wowsunpack)* Bump cache FORMAT_VERSION for TTX schema; strip divider/arrow in types
- *(wowsunpack)* Warn on dropped TTX ammo rows; drop guessed species default
- *(wowsunpack)* Restore CrewPersonality/Crew getters dropped before 0.37.0
- *(wowsunpack)* Fail-open modifier gating to latest; classify returns Result instead of defaulting to multiply
- *(wows-core)* Normalize Account.def stripped versions to replay-header friendly form
- *(wows-core)* Gate friendly_from_account_def_parts behind parsing feature
- *(wowsunpack)* Classify 6 captain-skill modifier table gaps
- *(wowsunpack)* Handle pickled Value::Weak in cbor/json converters
- *(replays)* Entity spec sort order bug with nullable/dynamic sized payloads
- *(wowsunpack)* Treat USER_TYPE as variable-size for exposed-method ordering
- *(wowsunpack)* Tolerate absent crew-skill LogicTrigger fields
- *(wowsunpack)* Dedup duplicate client methods to match engine exposed-method ids
- *(wowsunpack)* Resolve <=0.11.x SHIP_CONFIG fixed-dict shape
- *(wowsunpack)* Record secondary shells under Secondary* TtxStat variants
- *(wowsunpack)* Drop inert visibilityFactor/visibilityFactorByPlane reads
- *(wowsunpack)* Document consumable version assumption, species fallback, and warn on charge-kind mismatch
- *(replay-renderer)* Make any ship right-clickable and keep its context menu open
- *(minimap-renderer)* Drop zero-byte encoder packets when muxing
- *(minimap-renderer)* Default single replays to the stats panel

### ⚡ Performance

- *(wowsunpack)* Intern VFS volume filenames to cut ~382K allocations
- *(wowsunpack)* Shrink VFS directory child lists after dedup
- *(wows-toolkit)* Build the file-browser file list lazily
- *(wows-toolkit)* Memory-map CJK/Thai fallback fonts instead of reading them
- *(wowsunpack)* Cut GameParams parse peak ~52% via pickled sorted-Vec Dict

## [0.1.69] - 2026-06-11

### 🚀 Features

- Add support for WASM compilation
- *(wows-data-mgr)* New command for updating derived assets, gc, and add compressed gameparams + english translations
- *(replays)* Merge multiple per-player replays into a single rendered view
- *(toolkit)* Propagate merged perspectives through the live replay renderer
- *(data-mgr)* Backfill constants.json via refresh-derived; auto-gc
- *(insights)* Add wows-replay-insights crate for build resolution
- *(replays)* Consumable inventory tracking + team roster panels
- *(replays)* Merged-replay roster polish, zoom decoupling, default tweaks
- *(renderer)* Multi-codec video stack (gpu-video, rav1e AV1, muxide)
- *(replays)* Seed Vehicle entities from onArenaStateReceived
- *(replays)* Consumables/heals in skills expansion, disconnect highlight, terse perspective label
- *(renderer)* Expose bitrate config; default to ~10 MiB file-size target
- *(data)* Download old-version game data on demand
- *(data)* Check for and update cached game data
- *(wowsunpack)* Add `pkgs` command to resolve paths to required .pkg files
- *(data-mgr)* Audit dump VFS coverage, hard-fail incomplete dumps
- *(scripts)* Minimal cross-depot download in bulk_archive
- *(wowsunpack)* Typed, version-aware GUI asset resolver
- *(data-mgr)* Add verify, migrate-cas, and update commands
- *(data-mgr)* Complete-build to add maps without re-downloading basecontent
- *(bulk_archive)* Download via steamroom daemon to cut rate limiting
- *(wowsunpack,toolkit)* Version-aware consumable id resolution
- *(wowsunpack)* Extend consumable id layouts back to 0.7.0
- *(toolkit)* Show the equipped module loadout in the build panel
- *(toolkit)* Borrow current-build class icons for pre-12.0 replays
- *(wgcheck)* Add WGCheck .gch report parser
- *(toolkit)* Validate the game data cache against the remote repository
- *(renderer)* Skip pre-battle by default and fix old-replay assets/ranges
- *(battle-world)* Scaffold crate with bevy_ecs
- *(battle-world)* Ingest option types
- *(battle-world)* Entity components
- *(battle-world)* Resources and entity index
- *(battle-world)* World lifecycle and analyzer skeleton
- *(battle-world)* Entity lifecycle and position ingestion with parity tests
- *(battle-world)* Vehicle property and aim ingestion with parity tests
- *(battle-world)* Kills, damage, and self-stat ingestion with parity tests
- *(battle-world)* Player construction and chat ingestion with parity tests
- *(battle-world)* Consumable ingestion with parity tests
- *(battle-world)* Plane and ward ingestion with parity tests
- *(battle-world)* Projectile and shot-hit ingestion with parity tests
- *(battle-world)* Capture point, score, zone, and smoke ingestion with parity tests
- *(battle-world)* Match state and finalization ingestion with parity tests
- *(battle-world)* Score-card battle report with parity tests
- *(battle-world)* ECS-native read API with cached query states
- *(minimap-renderer)* Read battle state from BattleView (ECS-native)
- *(battle-world)* Port MergedReplays to wrap BattleWorld
- *(typedefs)* Preserve def semantic type names via ArgType::Named
- *(typedefs)* Collect semantic names across the type tree
- *(wowsunpack)* Def-name to newtype registry
- *(replayshark)* Audit-types subcommand for def newtype coverage
- *(packet2)* Non-fatal payload leftover diagnostics with semantic name
- *(error)* Attach def semantic name to RpcValueParseFailed
- *(minimap)* Dead-reckon visible ship positions during playback
- *(skills)* Generated modern captain skill grid table
- *(renderer)* Show full captain skill grid in build popover
- *(skill-grid)* Add pre-rework grids and unified skill_grid API
- *(wows-core)* Add NormalizedPos::lerp
- *(battle-world)* Position timeline types and merge helper
- *(battle-world)* Scan_replay driver and ScanCollector trait
- *(battle-world)* MetadataCollector for self-team, duration, battle-start
- *(battle-world)* PositionTimelineCollector (world + minimap samples)
- *(minimap)* Interpolate ship positions from shared timeline; retire dead-reckon
- *(minimap)* Per-frame render clock in advance_clock for smooth export
- *(toolkit)* Install shared position timeline in export and playback
- *(minimap)* Rename TurretDirection to CameraDirection and origin it from the ship icon position
- *(battle-world)* World-aware scan_replay_world driver and WorldScanCollector trait
- *(minimap)* Add shared ammo_type_color helper keyed on AmmoType
- *(minimap)* Add ShotTracerTip draw command and render
- *(minimap)* Emit ammo-colored tip per main-battery shot
- *(renderer)* Plumb battle_end clock into shared renderer state
- *(renderer)* Draw match start/end ticks on the playback timeline
- *(constants)* Resolve constants build via repo manifest by friendly version
- *(constants)* Fetch version-matched constants for cross-region replays
- *(minimap)* Scale tracer length by caliber, size tip to line width, fade timeline ticks
- *(core)* Add GunId/GunBits newtypes with bitmask expansion
- *(decode)* Decode shootOnClient/shootATBAGuns into WeaponFired payload
- *(merge)* Treat secondary fire methods as cross-perspective
- *(params)* Collect secondary battery ammo names from atba mounts
- *(params)* Add secondary_ammo_param resolver
- *(battle-world)* Add SecondaryShotState component and order resource
- *(ingest)* Record secondary shots from secondary fire events
- *(view)* Expose active_secondary_shots
- *(renderer)* Draw secondary battery dots from shooter to target
- *(params)* Extract per-gun secondary ammo ordered by hardpoint
- *(secondary)* Pace dots by per-gun shell via firing GunId
- *(params)* Add CrewSkillType newtype, bump cache format to 5
- *(params)* Add SkillPointCost newtype for captain skill tiers
- *(params)* Add CrewSkillName newtype for skill string identity
- *(skills)* Recognize IFHE/Dazzle by name, fix stale IFA id
- *(assets)* Add CrewSkill GuiAsset and crew_skill_icon_slug
- *(params)* Extract shared build_skill_grid for skill layout
- *(data)* Add lazy crew-skill icon cache
- *(replays)* Inspector build carries full captain skill grid
- *(replays)* Render captain skill grid with icons in inspector
- *(params)* Parse modernization slots + applicability; slot-count
- *(data)* Add modernization + signal icon caches; fix mod icon path
- *(replays)* Inspector build carries upgrade slots + signals
- *(replays)* Render upgrade slots + signals as icons in inspector
- *(replays)* Dedup abilities into consumables fallback
- *(wowsunpack)* Read per-ship camera orbit trajectories
- *(wowsunpack)* Expose camera trajectories on ship config
- *(armor-viewer)* Camera orbit ellipse geometry builder
- *(armor-viewer)* Hold camera trajectories and ellipse UI state
- *(armor-viewer)* Upload camera orbit ellipse overlay
- *(armor-viewer)* Camera orbit checkbox and mode selector
- *(viewport)* Camera animation and orthographic view presets
- *(viewport)* Navigation gizmo geometry and hit-test
- *(viewport)* Navigation gizmo draw, interact, and Viewport3D API
- *(armor-viewer)* Show navigation gizmo over the 3D viewport
- *(params)* Codegen version-gated modifier settings table
- *(params)* Format modifiers into description fragments
- *(params)* Port common skill modifier value transforms
- *(replays)* Generate captain skill descriptions from modifiers
- *(params)* Expand KnownCrewSkill to full catalog; fix IFA mapping
- *(replays)* Generate upgrade/signal descriptions from modifiers
- *(armor-viewer)* Toggleable ship-center marker overlay
- *(wowsunpack)* Camera trajectory carries inner/outer states + resolve(fov,height)
- *(armor-viewer)* FOV + camera-height knobs with inner/outer envelope
- *(armor-viewer)* Camera-rings section, slider resets, reorder ship-center
- *(armor-viewer)* Move camera-rings group to the bottom of display settings
- *(renderer)* Add EncoderWorker background encode thread
- *(renderer)* Drive VideoEncoder through the async EncoderWorker
- *(minimap-renderer)* Shared panel math helpers for stats/roster
- *(minimap-renderer)* Load_ribbon_icons for stats-panel ribbons
- *(minimap-renderer)* Ribbon icon keys, healable HP, stable ribbon order
- *(renderer)* Plumb ribbon/subribbon icons to desktop + video backends
- *(collab)* Transport ribbon/subribbon icons to web clients
- *(renderer)* Compact stats header + white healable silhouette meter
- *(renderer)* Wrapping ribbon icon grid in stats panel
- *(renderer)* Team-roster top HP bar with current/max text
- *(replays)* Decode and render modern-replay ribbons
- *(armor)* Inner/outer camera orbit rings with per-ring hover
- *(minimap)* Dim secondary fire and shell tips, retire ATBA dot path
- *(ui)* Rework replay inspector stats and add open-data-dir button
- *(armor)* Sidebar common settings popover and synced display overlays
- *(wgpu)* Widen backends and bias DX12 on Windows
- *(renderer)* Heal-state + regenerationHealth-based healable pool
- *(renderer)* Pixmap heal-state coloring + charcoal silhouette
- *(collab-egui)* Heal-state coloring + charcoal silhouette
- *(renderer)* Per-consumable availability with reload cooldown
- *(battle-world)* Add Division component with in-game labels
- *(gameparams)* Extract Repair Party heal rate into AbilityCategory
- *(insights)* Carry modifier-applied Repair Party heal rate to ConsumableInventory
- *(renderer)* Split healable region into per-charge and remaining pool
- Obfuscate Discord invite with ROT13
- *(armor)* Add zoom-path spoke mesh builder
- *(armor)* Add zoom-path toggle fields to ArmorPane
- *(armor)* Upload zoom-path spokes in the camera overlay
- *(armor)* Add zoom-path toggle UI and strings
- *(viewport)* Add ArcballCamera::set_eye_and_target
- *(armor)* Add camera_perspective geometry module
- *(armor)* Add perspective state, enter/exit, and sync
- *(armor)* Drive viewport camera from perspective lock
- *(armor)* Add camera perspective UI controls and strings
- *(armor)* Add water-aim geometry for perspective camera
- *(armor)* Aim perspective camera at the water plane
- *(armor)* Add perspective projection mode and far-side clamp
- *(armor)* Add water aim-point marker mesh builder
- *(armor)* Track perspective aim marker and sync look mode
- *(armor)* Perspective aim marker, projection selector, rings-independent toggle
- *(viewport_3d)* Add LightingSettings value type with presets
- *(armor)* Store lighting and detach flag on pane and defaults
- *(viewport_3d)* Half-Lambert hull lighting via per-mesh lit flag
- *(armor)* Light hull meshes and feed pane lighting to the viewport
- *(armor)* Hull lighting controls and presets in display settings
- *(armor)* Detach display settings into a floating egui window
- *(armor)* Persist hull lighting in armor viewer defaults
- *(armor)* Relabel directional slider as Light intensity

### 🐛 Bug Fixes

- Wowsunpack extract permission denied on Linux/macOS (#35)
- Use DirectX as default rendering backend instead of vulkan
- Updates to symlinking / version detection for CAS
- Update replayshark / replay parser to use updated manifest format
- *(replays)* Parsing of legacy packets now uses correct legacy packet mapping
- *(minimap-renderer)* Use full-precision world positions for smooth ship motion
- *(minimap-renderer)* Wrap detected-teammate outline fully around ship tips
- *(toolkit)* Move alt-perspective load button into the replay's own action row
- *(toolkit)* Thread the tab's Replay Arc into build_replay_view; clear stale cancel signal on every step start
- *(toolkit)* Coalesce slider-drag Seeks and skip frame publish on cancelled steps
- *(replays)* Suppress enemy spotted outline in single-replay sessions
- *(ui)* Species name fallback + drop redundant scroll wrappers
- *(wowsunpack)* Parse pre-rework GameParams without dropping params
- *(scripts)* Download per-build translation catalogs for the dump
- *(scripts)* Get translations from the client depot, not localization depot
- *(scripts)* Tolerate nix "Git tree is dirty" warnings in tool output
- *(wowsunpack)* Reject out-of-bounds pkg ranges instead of panicking
- *(wows-replays)* Parse old replays; tolerate sparse arena state, fail fast on missing roster
- *(bulk_archive)* Tolerate dirty-tree nix warning in pkg resolution
- *(bulk_archive)* Host Steam token auth + retry transient download failures
- *(bulk_archive)* Use atomic downloads and tolerate cold nix start
- *(bulk_archive)* Survive unicode errors, drop delta-removal, bind daemon account
- *(replay)* Use team color when a replay has no clanColor
- *(replay)* Only bridge versioned constants forward to newer builds
- *(wows-replays)* Map full player FixedDict for pre-0.10.7 replays
- *(wows-replays)* Complete player_key_map for 0.10.7-0.12.7
- *(wowsunpack)* Tolerate truncated ship-config blobs from older clients
- *(wows-replays)* Decode pre-rework captain skills (0.9.x bitmask)
- *(minimap-renderer)* Fall back to a system font when the game has no TTF
- *(bulk_archive)* Fetch translations from the localization depot (552994)
- *(toolkit)* Don't panic on malformed achievement result entries
- *(wows-replays)* Correct old-replay battle-state decoding
- *(toolkit)* Render old replays with fallback assets and correct builds
- *(wowsunpack)* Read consumable detection ranges from the category root for old clients
- *(wowsunpack)* Gate game_assets on the vfs feature for wasm
- *(wows-replays)* Resolve chat/voiceline sender names across versions
- *(renderer)* Keep chat overlay inside the minimap
- *(wows-replays)* Correct packet decoding flagged by game-script audit
- *(battle-world)* Address M1 ECS review (plane/ward indices, reset inventory preservation, dead ships, shot-tracking default, entity lifetime)
- *(battle-world)* Load test game data from dumped build archives with per-build caching
- *(battle-world)* Model weapon type as enum and track selected ammo per weapon type
- *(battle-world)* Close M2 parity gaps (arena health, spawned players, chat fallback) and remove buff-zone sentinel
- *(battle-world)* Close M3 parity gaps (cap-point index gaps, weather ordering, connection info)
- *(battle-world)* Resolve captain at entity-create time for report parity
- Make explicit maxHealth updates authoritative over the highest-health fallback
- *(nested-prop)* Peel ArgType::Named before structural match
- *(packet2)* Drop false-positive method leftover diagnostic; wire property diagnostics to replayshark
- *(game-params)* Make changePriorityTargetPenalty optional
- *(minimap)* Pace shell tracers by server time-to-impact
- *(renderer)* Borrow fonts from dump builds newest-first
- *(playback)* Drive playback at display rate, not snapshot rate
- *(renderer)* Load pre-rework crew skill icons from big/small subdirs
- *(renderer)* Load crew skill icons from the replay's own build
- *(minimap)* Per-variant position tracks so in-AOI motion lerps instead of snapping
- *(minimap)* Pace shell tracers by learned per-salvo impact time
- *(minimap)* Install learned salvo flight times in export, playback, and CLI
- *(minimap)* Color salvos/torpedoes by owner relation, skip unknown instead of defaulting to enemy
- *(decode)* Fall through to EntityMethod on malformed secondary fire args
- *(params)* Pick secondary ammo deterministically for mixed-caliber ships
- *(armor-viewer)* Waterline at model Y=0, drop dockYOffset shift
- *(armor-viewer)* Inline camera mode selector to keep popover open
- *(replays)* Restore SVG ship-class + ribbon icons (revert roster icons off PNG texture cache)
- *(viewport)* Mark viewport dirty and mirror on gizmo and animation camera changes
- *(viewport)* Foreshorten gizmo axis arms instead of normalizing
- *(params)* Tag modifiers with unresolved values as label-only
- *(params)* Modifier number sign follows the delta, not the value type
- *(params)* Triggered skills use trigger modifiers + trigger-type sentence
- *(armor-viewer)* Clamp navigation gizmo to the visible viewport
- *(replays)* Load entity specs on the extracted-dir path and resolve EntityCreate bounds safely
- *(minimap-renderer)* Group BULGE after main-caliber cluster in ribbon order
- *(renderer)* Char-safe ribbon label truncation; trim const doc
- *(net)* Honor the OS trust store for HTTPS instead of bundled roots
- *(net)* Add HTTP timeouts, retries, and root-cause logging
- *(net)* Validate wows-numbers data before caching it
- *(net)* Build the GitHub client inside the network runtime
- *(replays)* Populate regenerationHealth from entity props
- *(render)* Prefer Vulkan over DX12 on Windows
- *(renderer)* Hold heal per-charge target fixed while healing
- *(armor)* Drop unused extern crate in camera_perspective
- *(armor)* Refresh perspective saved camera on ship reload
- *(armor)* Flip normal on back faces to remove double-sided hull lighting seam
- *(armor)* Fade directional hull lighting with transparency to hide see-through seam
- Add description to wows-battle-world

### ⚡ Performance

- *(replays)* Load replays off the UI thread
- *(replays)* Resolve skill-grid icons in two passes
- *(replays)* Cache decoded icon textures per build
- *(renderer)* Tile rav1e AV1 encoding for multi-core parallelism
- *(renderer)* AV1 speed preset 8 + 16-way tiling for ~30s render
- *(collab)* Replace 100ms session-event poll with egui_inbox
- *(armor)* Decouple lighting changes from mesh re-upload; add fading light-source marker
- *(settings)* Cache game-data cache-dir stats instead of walking every frame

### ◀️ Revert

- *(scripts)* Drop fix_gui_holes.py -- nearest-build copy was unsound

## [0.1.68] - 2026-04-09

### 🚀 Features

- Add content-addressed storage deduplication + better metadata tracking for dumped builds

### 🐛 Bug Fixes

- Another stab at fixing memory consumption

## [0.1.67] - 2026-04-08

### 🐛 Bug Fixes

- Bump pickled dep to reduce RAM usage when converting GameParams

## [0.1.66] - 2026-04-07

### 🚀 Features

- Persist armor thickness legend position/collapse state

### 🐛 Bug Fixes

- Windows would change size actively

## [0.1.65] - 2026-04-07

### 🚀 Features

- Minimap renderer now respects chat/kill feed enable/disable options
- Add `max_duration`, `played_duration`, and `extra_duration` fields to exported match metadata
- Add MSI installer to releases
- Add batch replay export
- Add --recreate-game-params arg to minimap renderer CLI to recreate the cache if the internal format changes
- Add application setting to enable persisting data required for old replay compatibility

### 🐛 Bug Fixes

- *(wowsunpack)* Update to pickled 2.0-alpha4 to fix bugs surfaced in WoWs v15.3 + delete flaky tests
- Window sizes would shrink/grow after restarting the app when a non-1.0 scaling factor was applied
- Wows-data-mgr now exports the `gui/ships_silhouettes` directory so that CLI renderers can render the HP bar silhouette
- Rendered videos did not appropriately display CJK text
- Updating the application language will update any parsed replays' text
- Protocol tests were failing

## [0.1.64] - 2026-03-20

### 🚀 Features

- *(minimap-renderer)* Added option to dump all frames
- Allow test ships to be seen in armor viewer

### 🐛 Bug Fixes

- App defaults for updates/log files were incorrect
- I think I FINALLY fixed settings not saving for some people
- File > Check for Updates now does not respect the main app setting for checking for updates on startup

## [0.1.63] - 2026-03-17

### 🐛 Bug Fixes

- Some settings weren't being properly persisted (#30)

## [0.1.62] - 2026-03-15

### 🐛 Bug Fixes

- UI scaling was not being persisted

## [0.1.61] - 2026-03-14

### 🐛 Bug Fixes

- Main app window position was not being restored on startup
- Completely remove logic involving bundled constants data, except for using it as a fallback

## [0.1.60] - 2026-03-14

### 🐛 Bug Fixes

- Settings were not properly persisted on change
- Constants fallback sometimes was unreliable, which might break constants data updates

## [0.1.58] - 2026-03-08

### 🚀 Features

- Multi-language support

## [0.1.57] - 2026-03-05

### 🚀 Features

- Add replay sessions for group replay reviews
- Add tactics board
- Add tactics web page

### 🐛 Bug Fixes

- Ensure native windows do not depend on each other for repainting

## [0.1.55] - 2026-03-02

### 🚀 Features

- Add utility function for parsing game version from its data + update tests

## [0.1.54] - 2026-02-26

### 🐛 Bug Fixes

- Refactoring from wows-replays
- Ribbon icons were not properly loading

## [0.1.53] - 2026-02-25

### 🚀 Features

- Rudimentary realtime armor viewer
- Add hull upgrades to armor viewer

### 🐛 Bug Fixes

- Refactor child window repainting for perf / accuracy
- Come very close to real turret positions -- TODO rework

## [0.1.50] - 2026-02-18

### 🚀 Features

- Sign the toolkit binary

### 🐛 Bug Fixes

- Make it so searching doesn't make all nodes expanded when not searching

## [0.1.49] - 2026-02-18

### 🐛 Bug Fixes

- Armor viewer nation list was not scrollable + broke armor viewer

## [0.1.48] - 2026-02-18

### 🚀 Features

- Armor viewer
- *(armor)* Fix turret transform + sync options across panes
- *(armor)* Allow clicking to select armor regions, right-click to disable
- *(armor)* Show stacked plates

### 🐛 Bug Fixes

- Refactor to use Vfs
- Operations now load
- Game params are not reloaded when loading ShipAssets

## [0.1.47] - 2026-02-16

### 🚀 Features

- Add buttons for opening replays in game
- Holding ALT on with expanded replay details will show inverse damage dealt details (fixes #24)
- Show confirmation dialogs for destructive or annoying actions
- *(renderer)* Use fonts from game and fix scaling + show torpedo ranegs

### 🐛 Bug Fixes

- Improve error message visibility in UI
- Refactor updating WoWs game dir to ensure state is properly cleaned up
- Ensure that unsupported game versions surface errors
- *(renderer)* Make overlapping ship config labels rotate around their circle to avoid collisions

## [0.1.47-beta2] - 2026-02-16

### 🚀 Features

- Add progress callback for video export + ensure we can set prefer_cpu
- *(renderer)* Refactor how ship range filters applied

### 🐛 Bug Fixes

- Refactor networking logic to occur in background thread and re-enable logging

## [0.1.47-beta1] - 2026-02-14

### 🚀 Features

- *(renderer)* Add disable ship ranges button
- *(renderer)* Add chat overlay + enhance window title
- Show warning when GPU renderer cannot be used and fallback to CPU renderer

### 🐛 Bug Fixes

- *(renderer)* Fix smooth scrolling
- *(renderer)* Fix ghost trails
- Attempt to handle constants data updates better + surface errors better
- Update bundled constants data
- Mitigate against GPU mem leak when app is minimized

## [0.1.47-alpha6] - 2026-02-14

### 🚀 Features

- *(renderer)* Add speed trails, improved score bar, and kill feed
- *(renderer)* Add keyboard shortcuts and make the UI a bit prettier
- *(renderer)* Add options for disabling end-of-battle text + buff counters

## [0.1.47-alpha5] - 2026-02-13

### 🐛 Bug Fixes

- Performance improvements + hopefully fix deadlock causing app to crash

## [0.1.47-alpha4] - 2026-02-13

### 🚀 Features

- Check for constants file updates when loading replays
- *(renderer)* Add event timeline
- Add cap capture events
- Support multiple versions of the game, so long as they are installed

### 🐛 Bug Fixes

- Players that were never spotted now show on the replay results

## [0.1.47-alpha2] - 2026-02-11

### 🚀 Features

- *(renderer)* Add right-click option + options for showing/hiding dead ship info

### 🐛 Bug Fixes

- If the app state cannot be deserialized, reset it

## [0.1.47-alpha1] - 2026-02-11

### 🚀 Features

- Add achievement icons
- Add option to limit session stats

## [0.1.46] - 2026-02-06

### 🚀 Features

- Show player ribbons when row is expanded

### 🐛 Bug Fixes

- Cumulative -> average
- Maybe fix icon not working in CI builds?

## [0.1.45] - 2026-02-05

### 🚀 Features

- Add charts to session stats
- Allow taking screenshots of charts

## [0.1.44] - 2026-01-30

### 🚀 Features

- Switch graphics renderer from glow to wgpu
- Add manual secondaries

### 🐛 Bug Fixes

- Restore the stream sniper tech
- Remove plain bomb from damage to prevent double-counting
- Refactoring to fix clippy lints
- Auto-updater should write the new exe in same directory as old exe
- Incomplete match results warning was broken
- Some session stats weren't been shown. hopefully finally fixed?

## [0.1.43] - 2026-01-28

### 🚀 Features

- Add ability to update session stats from multiple selected items in replay list
- Show players who disconnect early from battle
- Show warning when replay has incomplete results
- Add error text when the toolkit fails to check for updates

### 🐛 Bug Fixes

- Tomato.gg no longer supports WoWs
- Newly parsed replays did not correctly sort by PR
- Bump wows_replays version to hopefully fix never-spotted players not being listed in score
- Re-parse replay on modification
- PR colors match WoWs-Numbers (thanks janatan)

## [0.1.42] - 2026-01-21

### 🚀 Features

- Add ship name to interaction player name hover text
- Add PR calculation (thanks WoWs Numbers)
- Reverse the damage interaction details to show ship name normally and player name on hover

### 🐛 Bug Fixes

- Auto-updater had a bug in how it renamed files
- Replay details no longer cause crash in matches with bots
- Only show achievements header when player has achievements

## [0.1.41] - 2026-01-21

### 🚀 Features

- Rudimentary session stats

### 🐛 Bug Fixes

- Restore replay view's context menus for grouped items
- Remove air support bomb to prevent damage double-counting

## [0.1.40] - 2026-01-20

### 🚀 Features

- Add replay grouping by ship/date + show win/loss

### 🐛 Bug Fixes

- Rework error propagation for better error info and app resiliance during updates

## [0.1.39] - 2026-01-20

### 🚀 Features

- Add damage breakdowns by player
- Add damage dealt/received breakdowns to tooltip and expanded info
- Add damage interactions to exported data

### 🐛 Bug Fixes

- Update embedded contants file

## [0.1.32] - 2025-03-27

### 🐛 Bug Fixes

- Application icon is now embedded in binary/shows when pinned to taskbar

## [0.1.31] - 2025-03-18

### 🚀 Features

- *(replays)* Allow exporting as CSV

## [0.1.30] - 2025-03-17

### 🚀 Features

- *(replays)* Test ship players can see their own stats

### 🐛 Bug Fixes

- *(replays)* Replay export filename replaces all characters which may bug filename

## [0.1.30-alpha2] - 2025-03-17

### 🚀 Features

- *(replays)* Add data export
- *(replays)* Add data auto-export in settings tab
- *(replays)* Data export provides module and skill names
- *(replays)* Show build info when player details are expanded

### 🐛 Bug Fixes

- *(replays)* Only make one attempt to parse historical replays
- *(replays)* Fix inconsistencies between auto data export and manual export

## [0.1.29] - 2025-03-05

### 🐛 Bug Fixes

- *(replays)* Constants data was not being loaded from disk

## [0.1.28] - 2025-03-05

### 🚀 Features

- *(replays)* Add fires/floods/cits/crits
- *(replays)* Add icons for IFA/Dazzle builds
- *(replays)* Add damage received and distance traveled
- *(replays)* Move column filters to replay tab
- *(replays)* Support file drag and drop
- Refactor tables
- *(replays)* Allow double clicking a table row to expand it
- *(replays)* Improvements to the player listing table
- *(replays)* Add skill info hover text to expanded row
- Show data collection consent window

### 🐛 Bug Fixes

- *(replays)* Decode HTML entities in chat messages
- *(replays)* Fix broken potential damage breakdown
- *(replays)* Refactor background replay parsing logic to prevent possible panics
- *(replays)* Fix hover labels for received damage
- *(replays)* Fixed long damage hover text
- *(replays)* Get rid of hardcoded stats indices

## [0.1.27] - 2024-11-24

### 🚀 Features

- Update prompt window renders markdown
- *(replays)* Implement sortable columns in replay viewer
- Expose player on GameMessage
- *(replays)* Player clan is now shown with chat message

### 🐛 Bug Fixes

- Map.bin was being written to disk by the replay parser lib by accident
- Adjustments to stream sniper detection
- *(player_tracker)* Fix filtering by player name

## [0.1.26] - 2024-11-20

### 🚀 Features

- Expose clan color and make your own div gold

### 🐛 Bug Fixes

- *(replays)* Fix stream sniper detection in replay parser
- Default settings were not properly applied

## [0.1.25] - 2024-11-17

### 🚀 Features

- *(player_tracker)* Only consider ranked / random battles
- Add twitch integration to detect stream snipers
- *(player_tracker)* Ignore players in division
- *(player_tracker)* Add more time ranges for time filter
- *(player_tracker)* Add players from current match with stream sniper detection
- *(settings)* Allow customizing which twitch channel to watch for player tracker

### 🐛 Bug Fixes

- Bug with loading game data when no locale is set

## [0.1.24] - 2024-11-15

### 🚀 Features

- *(player_tracker)* Add editable player notes

### 🐛 Bug Fixes

- *(player_tracker)* Fix bug with sorting encounters in time range
- *(player_tracker)* Colors stopped for high numbers
- Dark mode did not work for system-wide light mode users

## [0.1.23] - 2024-11-15

### 🚀 Features

- *(replays)* Add base xp
- *(replays)* Add checkbox to auto-load most recent replay
- *(replays)* Colorize base XP and damage
- Add new player tracker tab
- *(replays)* Add hover text to break down damage by damage type

### 🐛 Bug Fixes

- *(replays)* Fixed total damage numbers reflecting incorrect teams
- *(replays)* Fix operation replays failing to load

## [0.1.21] - 2024-11-12

### 🚀 Features

- *(replays)* Show which division a player was in (div letters probably don't match in-game)
- Default wows dir was previously broken, now should work

### 🐛 Bug Fixes

- Resolved application hang when first using the application

## [0.1.20] - 2024-11-11

### 🚀 Features

- *(replays)* Add total damage dealt in a match between the teams
- *(replays)* Selected replay will be highlighted in sidebar
- *(replays)* Add indicator for if a player disconnected from match
- *(replays)* Add action button to see raw player metadata

### 🐛 Bug Fixes

- Log file rotates hourly to reduce total log file size
- *(replays)* Airstrike and plane potential damage are the same

## [0.1.19] - 2024-11-10

### 🚀 Features

- Show actual damage numbers
- Add button for showing raw battle results
- Add potential and spotting damage + fixed some labels

## [0.1.18] - 2024-09-14

### 🚀 Features

- *(replays)* Add more statuses to indicate some action was done

### 🐛 Bug Fixes

- *(replays)* Fix bug where app would crash if it was focused at the end of a match
- *(settings)* Setting WoWs directory didn't work so well
- *(replays)* Chat is visually more appealing, easier to read (fixes #3)
- *(app)* Only show update window if there's a build to download

## [0.1.17] - 2024-09-05

### 🐛 Bug Fixes

- *(replays)* Watch replays directory only

## [0.1.16] - 2024-09-05

### 🚀 Features

- *(file_unpacker)* Add support for serializing as JSON/CBOR, including for WoWs Toolkit's internal representation
- Game version updates are auto-detected and new files will be auto-loaded
- *(replays)* Add support for ranked and sending ranked builds back to ShipBuild
- *(replays)* Consolidate the manual replay loading into a single button

## [0.1.15] - 2024-09-03

### 🚀 Features

- *(replays)* Add button for exporting game chat
- *(replays)* Add support for sending replays that were created when app was closed

### 🐛 Bug Fixes

- *(settings)* Sending replay data was not enabled by default
- Log files were not cleared
- *(replays)* Fix ci compilation error

## [0.1.13] - 2024-08-30

### 🐛 Bug Fixes

- *(replays)* Replays would not show any data when parsing

## [0.1.12] - 2024-08-30

### 🚀 Features

- *(resource_unpacker)* Add button for dumping GameParams.json
- Automatically send builds to ShipBuilds.com

## [0.1.10] - 2024-04-02

### 🐛 Bug Fixes

- *(replays)* Fix incompatability with 13.2.0

## [0.1.9] - 2024-03-11

### 🐛 Bug Fixes

- *(replays)* Replays in build-specific dirs should now work

## [0.1.8] - 2024-03-10

### 🚀 Features

- Add support for tomato.gg

### 🐛 Bug Fixes

- *(replays)* Double processing of replays
- Ensure replays dir is correctly reset if wows dir changes
- Improve perf for file listing filter + regression from egui update
- Ensure the found replays dir is used for loading replay files

## [0.1.0] - 2024-01-03

<!-- generated by git-cliff -->
