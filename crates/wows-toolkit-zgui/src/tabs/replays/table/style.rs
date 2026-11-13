//! The player table's styles.

use zgui::css;

pub const SHEET: &str = "wt-player-table";

pub const CSS: &str = css!(
    ".wt-pt {
        display: flex;
        flex: 1;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
    }
    .wt-pt-grid { flex: 1; min-height: 0; }
    .wt-pt-grid .lui-grid__group { border-top: 1px solid var(--lui-color-border); }
    .wt-pt-trigger { display: flex; flex: 1; min-width: 0; }
    .wt-pt-cell {
        display: flex;
        flex: 1;
        position: relative;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
        white-space: nowrap;
        color: var(--wt-pt-ink, var(--lui-color-text));
    }
    .wt-pt-cell[data-figures] { font-variant-numeric: tabular-nums; }
    .wt-pt-text {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        user-select: text;
    }
    .wt-pt-text[data-chip] {
        padding: 0 4px;
        border-radius: 3px;
    }
    .wt-pt-share {
        position: absolute;
        left: -4px;
        top: -2px;
        bottom: -2px;
        background-color: color-mix(in srgb, var(--wt-pt-ink, var(--lui-color-text)) 13%, transparent);
    }
    .wt-pt-figure {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: 1px;
        min-width: 0;
    }
    .wt-pt-mix {
        display: flex;
        flex-direction: row;
        height: 4px;
        overflow: hidden;
    }
    .wt-pt-mix__segment { height: 4px; flex-basis: 0; }
    .wt-pt-listing {
        display: flex;
        flex-direction: row;
        gap: 8px;
        align-items: flex-start;
        font-size: 0.75rem;
    }
    .wt-pt-listing__column { display: flex; flex-direction: column; gap: 1px; }
    .wt-pt-listing__column[data-figures] { align-items: flex-end; font-variant-numeric: tabular-nums; }
    .wt-pt-listing__key { display: flex; flex-direction: row; align-items: center; gap: 4px; }
    .wt-pt-swatch { width: 8px; height: 8px; flex: none; }
    .wt-pt-hover {
        display: flex;
        flex-direction: column;
        font-size: 0.75rem;
        font-family: var(--lui-font-mono, monospace);
    }
    .wt-pt-name {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
        padding-right: 28px;
        flex: 1;
    }
    .wt-pt-stripe {
        position: absolute;
        left: 0;
        top: 0;
        bottom: 0;
        width: 3px;
    }
    .wt-pt-grid .lui-grid__mark[data-mark=\"owner\"] { background-color: var(--lui-color-accent); }
    .wt-pt-stripe[data-team=\"ally\"], .wt-pt-grid .lui-grid__mark[data-mark=\"ally\"] {
        background-color: var(--wt-color-win);
    }
    .wt-pt-stripe[data-team=\"enemy\"], .wt-pt-grid .lui-grid__mark[data-mark=\"enemy\"] {
        background-color: var(--wt-color-loss);
    }
    .wt-pt-caret {
        display: flex;
        flex: none;
        width: 16px;
        height: 16px;
        align-items: center;
        justify-content: center;
        color: var(--lui-color-text-dim);
        transition: transform 200ms ease-out;
    }
    .wt-pt-caret[data-open] { transform: rotate(90deg); }
    .wt-pt-species { font-size: 0.75rem; flex: none; }
    .wt-pt-class {
        flex: none;
        width: 9px;
        height: 16px;
        margin: 0 3px;
        transform: rotate(90deg);
    }
    .wt-pt-flag { flex: none; width: 20.125px; height: 14px; }
    .wt-pt-clan { color: var(--wt-clan-light); }
    @media (prefers-color-scheme: dark) {
        .wt-pt-clan { color: var(--wt-clan-dark); }
    }
    .wt-pt-team {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        font-size: 0.75rem;
        font-weight: 400;
    }
    .wt-pt-team__name { font-weight: 700; }
    .wt-pt-fill { flex: 1; }
    .wt-pt-marker { flex: none; }
    .wt-pt-offline {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 2px;
        padding: 0 2px;
        border-radius: 3px;
        font-size: 0.75rem;
        background-color: color-mix(
            in srgb,
            var(--wt-color-error) var(--wt-pt-depth, 0%),
            color-mix(in srgb, var(--lui-color-text) 8%, transparent)
        );
    }
    .wt-pt-offline-ink { color: var(--wt-offline-light); }
    @media (prefers-color-scheme: dark) {
        .wt-pt-offline-ink { color: var(--wt-offline-dark); }
    }
    .wt-pt-actions {
        position: absolute;
        right: 0;
        top: 0;
        bottom: 0;
        width: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        visibility: hidden;
    }
    .lui-grid__row:hover .wt-pt-actions, .wt-pt-actions[data-open] { visibility: visible; }
    .wt-pt-heading { display: flex; flex-direction: row; align-items: center; gap: 4px; white-space: nowrap; }
    .wt-pt-heading__ship { color: var(--lui-color-text-dim); }
    .wt-pt-item-icon { display: flex; flex: none; }"
);
