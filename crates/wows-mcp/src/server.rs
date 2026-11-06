use std::sync::Arc;

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::Implementation;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncReadExt;
use wows_query::GameData;
use wows_query::QueryError;
use wows_query::consumables::ShipConsumablesArgs;
use wows_query::penetration::PenetrationSearchArgs;
use wows_query::ships::FindShipsArgs;
use wows_query::ships::ShipArgs;
use wows_query::skills::CaptainSkillArgs;
use wows_query::skills::SkillTreeArgs;
use wows_query::stats::ShipStatsArgs;
use wows_query::upgrades::UpgradeArgs;

const INSTRUCTIONS: &str = "World of Warships game data for one game build. Ship arguments take the English ship name as players write it. Answer from tool output; numbers come from GameParams.";

/// Arguments for `replay_summary`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReplaySummaryArgs {
    /// Path of a .wowsreplay file on the server's file system.
    pub path: String,
}

/// MCP tools over one loaded game build.
#[derive(Clone)]
pub struct GameDataServer {
    game: Arc<GameData>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl GameDataServer {
    pub fn new(game: Arc<GameData>) -> Self {
        Self { game, tool_router: Self::tool_router() }
    }

    #[tool(
        description = "List ships by name fragment, tier, class or nation. Returns name, GameParams index, tier, class, nation and group."
    )]
    async fn find_ships(&self, Parameters(args): Parameters<FindShipsArgs>) -> Result<String, String> {
        self.run(move |game| Ok::<_, QueryError>(game.find_ships(&args))).await
    }

    #[tool(
        description = "Price in credits, doubloons and research XP, research tree predecessors and successors, group and upgrade slot count of one ship."
    )]
    async fn ship_overview(&self, Parameters(args): Parameters<ShipArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_overview(&args)).await
    }

    #[tool(
        description = "Armor of one ship: per hull zone the thinnest and thickest plate above the waterline, and every plate by collision material (Bow_*, St_* stern, Cit_* citadel, Cas_* casemate, SS_* superstructure) with thickness in mm."
    )]
    async fn ship_armor(&self, Parameters(args): Parameters<ShipArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_armor(&args)).await
    }

    #[tool(
        description = "Main and secondary battery shells of one ship: ammo type, caliber, HE/SAP penetration with and without Inertia Fuse for HE Shells, the plate thickness AP overmatches, alpha damage, muzzle velocity and fire chance."
    )]
    async fn ship_shells(&self, Parameters(args): Parameters<ShipArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_shells(&args)).await
    }

    #[tool(
        description = "Find ships whose shells defeat a plate outright: HE or SAP penetration at least the thickness, or AP whose caliber overmatches it (caliber > 14.3 x thickness). Without ammo, returns separate HE, SAP and AP overmatch groups. With a target ship and no tiers, searches the target's tier +-2."
    )]
    async fn penetration_search(&self, Parameters(args): Parameters<PenetrationSearchArgs>) -> Result<String, String> {
        self.run(move |game| game.penetration_search(&args)).await
    }

    #[tool(
        description = "Port stat card of one ship with stock modules. With captain skills and upgrades, lists the stats they change, from and to, and which skill or upgrade caused each change."
    )]
    async fn ship_stats(&self, Parameters(args): Parameters<ShipStatsArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_stats(&args)).await
    }

    #[tool(
        description = "One captain skill by name or acronym (IFHE, SI, CE, AR, DE, BoS): description, point cost and effects per ship class."
    )]
    async fn captain_skill(&self, Parameters(args): Parameters<CaptainSkillArgs>) -> Result<String, String> {
        self.run(move |game| game.captain_skill(&args)).await
    }

    #[tool(description = "The captain skill tree of one ship class, skill names by point cost.")]
    async fn skill_tree(&self, Parameters(args): Parameters<SkillTreeArgs>) -> Result<String, String> {
        self.run(move |game| Ok::<_, QueryError>(game.skill_tree(&args))).await
    }

    #[tool(
        description = "One upgrade (modernization) by name: slot, kind, credit price, effects per class and which tiers, nations and ships it fits."
    )]
    async fn upgrade(&self, Parameters(args): Parameters<UpgradeArgs>) -> Result<String, String> {
        self.run(move |game| game.upgrade(&args)).await
    }

    #[tool(description = "Every upgrade that fits one ship, by slot, with price and effects.")]
    async fn ship_upgrades(&self, Parameters(args): Parameters<ShipArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_upgrades(&args)).await
    }

    #[tool(
        description = "Consumables of one ship by slot with charges, reload, action time, detection ranges and heal rate, optionally with captain skills and upgrades applied."
    )]
    async fn ship_consumables(&self, Parameters(args): Parameters<ShipConsumablesArgs>) -> Result<String, String> {
        self.run(move |game| game.ship_consumables(&args)).await
    }

    #[tool(
        description = "Summarize a .wowsreplay file on the server: recording player, map, mode, client version, date, duration and every player's ship and team."
    )]
    async fn replay_summary(&self, Parameters(args): Parameters<ReplaySummaryArgs>) -> Result<String, String> {
        // Replays are tens of MB; the bound keeps a device or FIFO path from
        // exhausting memory.
        const REPLAY_LIMIT: u64 = 256 * 1024 * 1024;

        let file = tokio::fs::File::open(&args.path).await.map_err(|_| format!("cannot read replay {}", args.path))?;
        let mut bytes = Vec::new();
        file.take(REPLAY_LIMIT + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| format!("cannot read replay {}", args.path))?;
        if bytes.len() as u64 > REPLAY_LIMIT {
            return Err(format!("{} is larger than any replay", args.path));
        }

        self.run(move |game| game.replay_summary(&bytes)).await
    }

    /// Runs a query off the async workers and renders it as compact JSON.
    async fn run<T, F>(&self, query: F) -> Result<String, String>
    where
        T: Serialize,
        F: FnOnce(&GameData) -> Result<T, QueryError> + Send + 'static,
        T: Send + 'static,
    {
        let game = Arc::clone(&self.game);
        let output = tokio::task::spawn_blocking(move || query(&game))
            .await
            .map_err(|error| format!("query task failed: {error}"))?
            .map_err(|error| error.to_string())?;

        serde_json::to_string(&output).map_err(|error| format!("failed to encode the answer: {error}"))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for GameDataServer {
    fn get_info(&self) -> ServerConfig {
        let version = self.game.version();
        let description = format!("World of Warships {} game data", version.to_path());

        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).with_description(description),
            )
            .with_instructions(INSTRUCTIONS)
    }
}
