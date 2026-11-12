use zgui::css;

/// What the grid's rules are installed under.
pub(crate) const SHEET: &str = "landaire-ui-grid";

pub(crate) const CSS: &str = css!(
    ".lui-grid {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        color: var(--lui-color-text);
        font-size: var(--lui-font-size);
        font-family: var(--lui-font-family);
    }
    .lui-grid > .zui-context-menu__area {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .lui-grid__port {
        display: block;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: auto;
        outline: none;
    }
    .lui-grid__table {
        display: grid;
        align-content: start;
        width: max-content;
        min-width: 100%;
    }
    .lui-grid__row { display: contents; }
    .lui-grid__head, .lui-grid__cell, .lui-grid__filler {
        display: flex;
        flex-direction: row;
        align-items: center;
        box-sizing: border-box;
        min-width: 0;
        overflow: hidden;
        white-space: nowrap;
        background-color: var(--lui-color-panel);
        transition: translate var(--lui-motion-rearrange) var(--lui-motion-ease);
    }
    .lui-grid[data-instant] .lui-grid__head, .lui-grid[data-instant] .lui-grid__cell {
        transition: none;
    }
    .lui-grid__head, .lui-grid__head-filler {
        position: sticky;
        top: 0;
        z-index: 2;
        border-bottom: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-panel);
    }
    .lui-grid__head {
        gap: 4px;
        padding: 4px;
        font-weight: 600;
        color: var(--lui-color-text-bright);
        user-select: none;
    }
    .lui-grid__head[data-sortable] { cursor: pointer; }
    .lui-grid__head:focus-visible {
        outline: 2px solid var(--lui-color-accent);
        outline-offset: -2px;
    }
    .lui-grid__label {
        flex: 0 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .lui-grid__head[data-align=\"end\"] { justify-content: flex-end; }
    .lui-grid__head[data-align=\"center\"] { justify-content: center; }
    .lui-grid__sort { flex: none; color: var(--lui-color-text-dim); }
    .lui-grid__grip {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        width: 6px;
        cursor: col-resize;
        background-color: transparent;
    }
    .lui-grid__grip:hover, .lui-grid__grip[data-active] {
        background-color: var(--lui-color-border-bright);
    }
    .lui-grid__grip:focus-visible { background-color: var(--lui-color-accent); }
    .lui-grid__cell { padding: 2px 4px; }
    .lui-grid__cell[data-align=\"end\"] { justify-content: flex-end; }
    .lui-grid__cell[data-align=\"center\"] { justify-content: center; }
    .lui-grid__head[data-pinned], .lui-grid__cell[data-pinned] {
        position: sticky;
        left: var(--lui-grid-left);
    }
    .lui-grid__head[data-pinned] { z-index: 3; }
    .lui-grid__cell[data-pinned] { z-index: 1; }
    .lui-grid__head[data-pin-edge], .lui-grid__cell[data-pin-edge] {
        box-shadow: inset -1px 0 0 0 var(--lui-color-border);
    }
    .lui-grid__head[data-carried], .lui-grid__cell[data-carried] { opacity: 0.35; }
    .lui-grid__row[data-stripe] > .lui-grid__cell, .lui-grid__row[data-stripe] > .lui-grid__filler,
    .lui-grid__row[data-stripe] > .lui-grid__detail {
        background-color: var(--lui-color-faint);
    }
    .lui-grid__row:hover > .lui-grid__cell, .lui-grid__row:hover > .lui-grid__filler {
        background-color: var(--lui-color-widget-hot);
    }
    .lui-grid__row[data-selected] > .lui-grid__cell, .lui-grid__row[data-selected] > .lui-grid__filler,
    .lui-grid__row[data-selected] > .lui-grid__detail {
        background-color: var(--lui-color-selection);
    }
    .lui-grid__port:focus-visible .lui-grid__row[data-cursor] > .lui-grid__cell,
    .lui-grid__port:focus-visible .lui-grid__row[data-cursor] > .lui-grid__filler {
        box-shadow: inset 0 1px 0 0 var(--lui-color-accent), inset 0 -1px 0 0 var(--lui-color-accent);
    }
    .lui-grid[data-fixed] .lui-grid__cell, .lui-grid[data-fixed] .lui-grid__filler {
        height: var(--lui-grid-row);
    }
    .lui-grid__spacer {
        grid-column: 1 / -1;
        height: var(--lui-grid-spacer, 0px);
    }
    .lui-grid__detail {
        grid-column: 1 / -1;
        overflow: clip;
        height: 0;
        background-color: var(--lui-color-panel);
        transition: height var(--lui-motion-reveal) var(--lui-motion-ease);
    }
    .lui-grid__detail[data-state=\"open\"] { height: var(--lui-grid-detail-height, auto); }
    .lui-grid__detail-body {
        position: sticky;
        left: 0;
        display: flex;
        flex-direction: column;
        box-sizing: border-box;
        width: var(--lui-grid-view, auto);
        padding: 4px 8px 6px 8px;
    }
    .lui-grid__zone {
        position: absolute;
        top: 0;
        bottom: 0;
        left: 0;
        z-index: 4;
        pointer-events: none;
        background-color: color-mix(in srgb, var(--lui-color-accent) 8%, transparent);
        border-right: 2px solid var(--lui-color-accent);
    }
    .lui-grid__zone[data-refused] {
        background-color: color-mix(in srgb, var(--lui-color-text-dim) 8%, transparent);
        border-right-color: var(--lui-color-border);
    }
    .lui-grid__ghost {
        position: absolute;
        top: 0;
        z-index: 5;
        display: flex;
        align-items: center;
        gap: 4px;
        box-sizing: border-box;
        padding: 4px;
        overflow: hidden;
        white-space: nowrap;
        pointer-events: none;
        font-weight: 600;
        color: var(--lui-color-text);
        background-color: var(--lui-color-card);
        border: 1px solid var(--lui-color-accent);
        box-shadow: var(--lui-shadow-overlay);
    }"
);
