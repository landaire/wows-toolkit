use zgui::css;

/// What the tree rules are installed under.
pub(crate) const SHEET: &str = "landaire-ui-tree";

pub(crate) const CSS: &str = css!(
    ".lui-tree-area {
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .lui-tree {
        display: block;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow-x: hidden;
        overflow-y: auto;
        position: relative;
        outline: none;
        color: var(--lui-color-text);
        font-size: var(--lui-font-size);
    }
    .lui-tree__pane {
        display: flex;
        flex-direction: column;
    }
    .lui-tree__row {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        box-sizing: border-box;
        height: var(--lui-tree-row);
        padding: 0 4px;
        overflow: hidden;
    }
    .lui-tree__row:hover {
        background-color: var(--lui-color-widget-hot);
    }
    .lui-tree__row[data-selected=\"true\"] {
        background-color: var(--lui-color-selection);
    }
    .lui-tree:focus-visible .lui-tree__row[data-cursor=\"true\"],
    .lui-tree:focus .lui-tree__row[data-cursor=\"true\"][data-selected=\"false\"] {
        box-shadow: inset 0 0 0 1px var(--lui-color-accent);
    }
    .lui-tree__chevron {
        display: flex;
        flex: none;
        width: 16px;
        height: 16px;
        align-items: center;
        justify-content: center;
        transform: rotate(0deg);
        transition: transform var(--lui-motion-turn) var(--lui-motion-ease);
    }
    .lui-tree__row[data-expanded=\"true\"] .lui-tree__chevron {
        transform: rotate(90deg);
    }
    .lui-tree__spacer {
        flex: none;
        width: 16px;
    }
    .lui-tree__content {
        display: flex;
        flex-direction: row;
        flex: 1 1 auto;
        align-items: center;
        min-width: 0;
        height: 100%;
        overflow: hidden;
    }"
);
