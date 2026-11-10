//! The values the TSX view names stats views, charts, ships and modes by.

use std::fmt;
use std::num::ParseIntError;

use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;

use crate::js_newtype::js_newtype;

/// Names one open stats view: `session` or `directory-N`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeName(String);

js_newtype!(ScopeName, String);

impl ScopeName {
    const SESSION: &'static str = "session";

    pub(crate) fn new(name: String) -> Self {
        Self(name)
    }

    pub(crate) fn session() -> Self {
        Self(Self::SESSION.to_owned())
    }

    pub(crate) fn is_session(&self) -> bool {
        self.0 == Self::SESSION
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A chart's id as the TSX view numbers it, in 32 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewChartId(u32);

js_newtype!(ViewChartId, u32);

impl ViewChartId {
    /// `None` for an id too large for the view to address.
    pub(crate) fn from_chart(id: ChartId) -> Option<Self> {
        u32::try_from(id.0).ok().map(Self)
    }

    /// `None` for an id no chart can have on this target.
    pub(crate) fn chart(self) -> Option<ChartId> {
        usize::try_from(self.0).ok().map(ChartId)
    }
}

impl fmt::Display for ViewChartId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A game param id as decimal text: a u64 does not fit a JS number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParamIdText(String);

js_newtype!(ParamIdText, String);

impl ParamIdText {
    pub(crate) fn from_id(id: GameParamId) -> Self {
        Self(id.raw().to_string())
    }

    pub(crate) fn parse(&self) -> Result<GameParamId, ParseIntError> {
        self.0.parse::<u64>().map(GameParamId::from)
    }
}

impl fmt::Display for ParamIdText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A match group as the games record it, such as `pvp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchGroup(String);

js_newtype!(MatchGroup, String);

impl MatchGroup {
    pub(crate) fn new(name: String) -> Self {
        Self(name)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
