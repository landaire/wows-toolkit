# Current Match Average PR Design

## Goal

Show team average personal rating beside team average win rate in the Current Match roster heading.

## Behavior

- Each team heading renders `Avg WR` followed by `Avg PR` when both aggregates are available. Either aggregate still renders by itself when the other is unavailable.
- Average PR is the arithmetic mean of rows with a resolved account PR. `PlayerStatsOut` has no per-ship PR, so this aggregate is intentionally identical in Overall and Ship modes, matching existing row PR behavior.
- Rows without a resolved PR are skipped rather than counted as zero.
- Average PR is absent when no row has a resolved PR.
- The rendered form is `{localized stat.avg_pr}: {value:.0}`.
- The displayed value is rounded to a whole number. Its color comes from `PersonalRatingCategory::from_pr` applied to the unrounded average.
- Existing average WR behavior is unchanged.

## Verification

The production-path egui_kittest test must assert the friendly and enemy average PR labels and that each follows its own team's average WR. Unit tests cover averaging, missing values, no values, mode invariance, and independent WR/PR availability in the heading.
