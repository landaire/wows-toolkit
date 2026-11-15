use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rootcause::prelude::*;
use tokio_util::sync::CancellationToken;
use wows_mcp::GameDataSource;
use wows_mcp::McpServer;
use wows_mcp::Transport;
use wows_query::GameData;

/// MCP server answering World of Warships game data questions.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// wows-replay-data dump root (holding builds.toml) or one build directory
    /// (holding metadata.toml). The newest build of a root is served.
    #[arg(long)]
    data: PathBuf,

    /// Download the newest published build into `--data` before serving.
    #[arg(long)]
    download: bool,

    /// Cache root for a rebuilt GameParams cache, used when the dump's own
    /// cache was written by another version, and for the raw GameParams that
    /// gameparams_query reads. Each lives in `<kind>/<build>/` below it, where
    /// `<kind>` (`game-params-v<N>`, `raw-params-v<N>`) names the format.
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Serve streamable HTTP on this address (for example 127.0.0.1:8080)
    /// instead of stdio.
    #[arg(long)]
    http: Option<SocketAddr>,
}

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error("failed to load game data from {path}")]
    Load { path: PathBuf },

    #[error("failed to build the HTTP client")]
    HttpClient(#[source] reqwest::Error),

    #[error("failed to start the MCP server")]
    Build,

    #[error("the MCP server failed")]
    Serve,
}

async fn load(cli: &Cli) -> Result<GameData, Report<MainError>> {
    let dump = if cli.download {
        // Every TLS connection in this process uses graviola, so no C or
        // assembly crypto library is linked.
        let _ = rustls_graviola::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("wows-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|source| report!(MainError::HttpClient(source)))?;
        Some(
            wows_query::download::download_latest(&client, &cli.data)
                .await
                .context(MainError::Load { path: cli.data.clone() })?,
        )
    } else {
        None
    };

    // A download yields the build directory it wrote.
    let path = dump.unwrap_or_else(|| cli.data.clone());
    let cache_dir = cli.cache_dir.clone();
    tokio::task::spawn_blocking(move || {
        wows_mcp::load_dump(&path, cache_dir.as_deref()).context(MainError::Load { path: path.clone() })
    })
    .await
    .expect("game data load task panicked")
}

#[tokio::main]
async fn run() -> Result<(), Report<MainError>> {
    // Stdout carries the MCP protocol; logs go to stderr.
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).init();

    let cli = Cli::parse();
    let game = Arc::new(load(&cli).await?);
    tracing::info!(version = %game.version().to_path(), ships = game.ship_count(), "game data loaded");

    let transport = match cli.http {
        Some(address) => Transport::Http(address),
        None => Transport::Stdio,
    };
    let server = McpServer::builder(transport)
        .game_data(GameDataSource::Loaded(game))
        .build()
        .await
        .context(MainError::Build)?;
    if let Some(endpoint) = server.http_endpoint() {
        tracing::info!(url = %endpoint.url(), "MCP endpoint");
    }

    // HTTP serves until interrupted; stdio ends when the client disconnects.
    let shutdown = CancellationToken::new();
    let interrupt = shutdown.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.cancel();
        }
    });

    server.run(shutdown).await.context(MainError::Serve)
}

fn main() -> Result<(), Report<MainError>> {
    // gameparams_query runs jq in a copy of this program; in that copy this
    // evaluates and exits.
    wows_query::jq_worker::init();

    run()
}
