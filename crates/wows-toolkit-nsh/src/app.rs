use nsh_core::prelude::*;

use crate::chart_tab::chart_tab;
use crate::frames::FrameMode;
use crate::frames::tab::frames_tab;
use crate::replays_tab::replays_tab;
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Replays,
    Frames,
    Chart,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Replays, Tab::Frames, Tab::Chart];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Replays => "Replays",
            Tab::Frames => "Frames",
            Tab::Chart => "Chart",
        }
    }

    pub fn parse(label: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|tab| tab.label().eq_ignore_ascii_case(label))
    }
}

/// Startup choices for scripted runs, from `WTN_TAB` and `WTN_FRAME_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launch {
    pub tab: Tab,
    pub frame_mode: FrameMode,
}

impl Launch {
    pub fn from_env() -> Self {
        let tab = std::env::var("WTN_TAB").ok().and_then(|value| Tab::parse(&value));
        let frame_mode = std::env::var("WTN_FRAME_MODE").ok().and_then(|value| FrameMode::parse(&value));

        // Unset or unknown values start where a user would: the first tab.
        Self { tab: tab.unwrap_or(Tab::Replays), frame_mode: frame_mode.unwrap_or(FrameMode::Native) }
    }
}

#[component]
pub fn App() -> impl IntoElement {
    let launch = Launch::from_env();
    let tab = signal(launch.tab);

    view()
        .flex_1()
        .bg(rgb(0x101820))
        .child(view().flex_row().gap(px(8.0)).p(px(8.0)).children(
            Tab::ALL.map(|choice| ui::button(choice.label(), move || tab.get() == choice, move || tab.set(choice))),
        ))
        .child(dynamic(move || match tab.get() {
            Tab::Replays => replays_tab().into_element(),
            Tab::Frames => frames_tab(launch.frame_mode).into_element(),
            Tab::Chart => chart_tab().into_element(),
        }))
}
