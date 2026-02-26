# Team Advantage Scoring System

The minimap renderer evaluates which team has a stronger position at each frame
of the replay. This document describes how the scoring works, what factors
contribute, and how the final verdict is determined.

## Overview

The system awards **non-negative points** to each team across three independent
factors. The team with more total points has the advantage, and the gap between
totals determines the strength level.

| Factor             | Max Points | Description                                   |
|--------------------|------------|-----------------------------------------------|
| Score Projection   | 10         | Who is winning or projected to win on points   |
| Fleet Power        | 10         | Class-weighted HP and ship count advantage      |
| Strategic Threat   | 5          | DD/SS survival, class diversity, CV advantage   |
| **Total**          | **25**     |                                                 |

All values in the breakdown are tuples `(team0_pts, team1_pts)` where both
elements are >= 0. There are no negative numbers anywhere in the output.

## Factor 1: Score Projection (max 10 pts)

Evaluates which team is winning or projected to win on points. Combines three
sub-factors:

### Current Score Gap (up to 4 pts)

Points awarded to the team with a higher score, scaled by how large the gap is
relative to the win score (typically 1000).

```
gap_pts = min(score_gap / win_score * 4, 4)
```

A 500-point lead in a 1000-point game awards 2 pts.

### Time-to-Win (up to 3 pts)

Based on how quickly each team can reach the win score from cap income alone.

- If only one team can reach the win score before time runs out: 3 pts to that team.
- If both can reach it: points to whoever gets there first, scaled by time difference
  (3 pts if >30s apart, 2 pts if >10s, 1 pt if >3s, 0 if very close).

### Projected Final Score (up to 3 pts)

Projects each team's final score assuming current cap income continues for the
remaining match time. Points to the team with a higher projection:

- Gap >= 300: 3 pts
- Gap >= 150: 2 pts
- Gap >= 50: 1 pt

### Cap Income (Points Per Second)

Cap income is calculated from uncontested capture points only. A capture point
is uncontested if no enemy ships are inside it (`hasInvaders == false`).

```
pps = uncontested_caps * hold_reward / hold_period
```

Typical values: `hold_reward = 3`, `hold_period = 5.0` (0.6 pps per cap).

## Factor 2: Fleet Power (max 10 pts)

Evaluates combat strength through class-weighted ship counts and HP.
**Only calculated when HP data is reliable** (all ships on both teams have known
entity data).

### Class Weights

Not all ship classes contribute equally to a team's combat power:

| Class       | Weight | Rationale                                        |
|-------------|--------|--------------------------------------------------|
| Destroyer   | 1.5    | High spotting value, torpedo threat, cap contesting |
| Cruiser     | 1.0    | Baseline — versatile but not dominant              |
| Battleship  | 1.0    | Tanky but less strategically decisive              |
| Submarine   | 1.3    | Very hard to kill, spotting denial                 |
| Carrier     | 1.2    | Spotting + long-range damage projection            |

### Calculation

For each team, fleet power is computed as:

```
team_power = sum over all classes of:
    class_weight * alive_count * (class_hp / class_max_hp)
```

The `hp_fraction` term means damaged ships contribute less than full-health
ships. A destroyer at 50% HP contributes `1.5 * 1 * 0.5 = 0.75` instead of
`1.5`.

Points are then split proportionally across the 10-point budget:

```
team0_pts = (team0_power / total_power) * 10
team1_pts = (team1_power / total_power) * 10
```

This naturally handles fleet size context:
- **12 vs 6 ships**: Massive power gap, roughly 6.7 vs 3.3 pts.
- **2 vs 1 ships**: Much closer — if the 1 ship has full HP and the 2 ships are
  damaged, the gap could be small.

## Factor 3: Strategic Threat (max 5 pts)

Evaluates late-game strategic resilience — which team has the tools to come back
or close out the game. **Only calculated when HP data is reliable.**

### DD/SS Survival (up to 2.5 pts)

Destroyers and submarines are strategically critical:
- Destroyers can contest caps, spot enemies, and deliver torpedo strikes.
- Submarines are extremely hard to kill and deny area control.

```
dd_ss_score = min(dd_alive * 1.0 + ss_alive * 0.8, 2.5) * time_weight
```

The `time_weight` scales from 0.2 (no time left) to 1.0 (5+ minutes remaining).
Strategic threats matter less when the clock is about to expire.

### Class Diversity (up to 1.5 pts)

A team with multiple ship classes alive is harder to eliminate because each class
requires different tactics to counter.

| Classes Alive | Points |
|---------------|--------|
| 0-1           | 0.0    |
| 2             | 0.5    |
| 3             | 1.0    |
| 4+            | 1.5    |

### CV Advantage (up to 1.0 pts)

A carrier advantage means superior spotting, which is critical for finding
destroyers and submarines. 1 point to whichever team has more carriers alive.

## Advantage Levels

The gap between total points determines the strength label:

| Level    | Gap Required | Interpretation                              |
|----------|-------------|----------------------------------------------|
| Even     | < 1.0       | No meaningful advantage                      |
| Weak     | >= 1.0      | Slight edge, easily reversible               |
| Moderate | >= 3.0      | Clear advantage in one or more areas         |
| Strong   | >= 6.0      | Dominant position across multiple factors    |
| Absolute | >= 10.0     | Overwhelming advantage or team eliminated    |

The maximum possible gap is 25 (one team gets all points in all factors), so
reaching Absolute requires dominance across multiple areas simultaneously.

### Special Case: Team Eliminated

If all ships on one team are destroyed, the surviving team automatically receives
an **Absolute** advantage with the maximum possible points (25). The breakdown
is marked with `team_eliminated = true`.

## Team Perspective Normalization (Swap)

### Why It's Needed

In replay files, teams are indexed as **team 0** and **team 1**. The replay
owner (the player who recorded the replay) can be on either team. The UI always
displays the friendly team on the left (green) and the enemy team on the right
(red).

The advantage calculation works in raw team indices (0 and 1). But the renderer
needs to display results from the viewer's perspective: "friendly advantage" vs
"enemy advantage."

### How It Works

When the replay owner is on **team 0**, no transformation is needed — team 0 is
already the friendly team.

When the replay owner is on **team 1**, all per-team data must be swapped so
that index 0 in the output means "friendly" (the replay owner's team):

```rust
// Before swap: team0 = raw team 0, team1 = raw team 1
// After swap:  team0 = friendly,    team1 = enemy

swap_breakdown(&mut result.breakdown);
// This swaps all (f32, f32) tuples and the pps values
```

With non-negative tuple scoring, the swap is just element swaps — no sign
changes needed. The `TeamAdvantage` enum (Team0/Team1/Even) is also swapped so
that `Team0` always means "the friendly team has the advantage."

### What Gets Swapped

- `score_projection: (f32, f32)` — elements swapped
- `fleet_power: (f32, f32)` — elements swapped
- `strategic_threat: (f32, f32)` — elements swapped
- `total: (f32, f32)` — elements swapped
- `team0_pps` / `team1_pps` — values swapped

After the swap, `total.0` is always the friendly team's total points and
`total.1` is always the enemy team's total points, regardless of which raw team
the replay owner was on.

## Design Rationale

### Why Non-Negative Tuples?

An earlier version used signed values where positive meant team 0's advantage
and negative meant team 1's. This had two problems:

1. **Confusing output**: Breakdown values could be negative, which was hard to
   interpret ("what does -3.2 fleet power mean?").
2. **Fragile swap logic**: Converting between team perspectives required negating
   signed values, which was error-prone.

The tuple approach `(team0_pts, team1_pts)` is clearer: each team's contribution
is always a non-negative number, and swapping perspectives is just swapping
elements.

### Why Class Weights?

Treating all ships equally doesn't reflect gameplay reality. A surviving
destroyer is strategically more valuable than a surviving battleship because it
can:
- Contest and capture points (win condition)
- Spot enemies for the team
- Deliver torpedo strikes from concealment
- Is small and hard to hit

Similarly, submarines are nearly impossible to kill in the late game without
specialized ASW ships, making their survival a significant strategic factor.

### Why Fleet Size Context Matters

The proportional fleet power split naturally handles fleet size:
- In a 12v6 situation, the team with 12 ships has roughly twice the weighted
  power, getting ~6.7 of 10 points.
- In a 2v1 situation, the ratio might be 2:1 in raw count, but if the solo ship
  has full HP and the two ships are damaged, the actual power ratio could be much
  closer (e.g., 5.5 vs 4.5).

This prevents the system from treating a 2v1 endgame the same as a 12v6
midgame blowout.
