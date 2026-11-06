mod server;

use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rootcause::prelude::*;
use wows_query::GameData;

use crate::server::GameDataServer;

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

    /// Directory for a rebuilt GameParams cache, used when the dump's own
    /// cache was written by another version.
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Serve streamable HTTP on this address (for example 127.0.0.1:8080)
    /// instead of stdio.
    #[arg(long)]
    http: Option<SocketAddr>,
}

#[derive(Debug, thiserror::Error)]
enum ServeError {
    #[error("failed to load game data from {path}")]
    Load { path: PathBuf },

    #[error("failed to build the HTTP client")]
    HttpClient(#[source] reqwest::Error),

    #[error("failed to serve MCP on stdio")]
    Stdio,

    #[error("failed to listen on {address}")]
    Listen {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

async fn load(cli: &Cli) -> Result<GameData, Report<ServeError>> {
    let dump = if cli.download {
        // Every TLS connection in this process uses graviola, so no C or
        // assembly crypto library is linked.
        let _ = rustls_graviola::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("wows-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|source| report!(ServeError::HttpClient(source)))?;
        Some(
            wows_query::download::download_latest(&client, &cli.data)
                .await
                .context(ServeError::Load { path: cli.data.clone() })?,
        )
    } else {
        None
    };

    let data = cli.data.clone();
    let cache_dir = cli.cache_dir.clone();
    tokio::task::spawn_blocking(move || {
        let loaded = match dump {
            Some(dir) => GameData::load_dump(&dir, cache_dir.as_deref()),
            None if is_build_dir(&data) => GameData::load_dump(&data, cache_dir.as_deref()),
            None => GameData::load_latest(&data, cache_dir.as_deref()),
        };
        loaded.context(ServeError::Load { path: data })
    })
    .await
    .expect("game data load task panicked")
}

fn is_build_dir(path: &Path) -> bool {
    path.join("metadata.toml").is_file()
}

async fn serve_http(server: GameDataServer, address: SocketAddr) -> Result<(), Report<ServeError>> {
    let service: StreamableHttpService<GameDataServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(server.clone()),
        Default::default(),
        StreamableHttpServerConfig::default(),
    );
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| report!(ServeError::Listen { address, source }))?;
    tracing::info!(%address, "serving MCP over streamable HTTP");

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "failed to accept a connection");
                continue;
            }
        };
        let service = TowerToHyperService::new(service.clone());
        tokio::spawn(async move {
            if let Err(error) =
                auto::Builder::new(TokioExecutor::new()).serve_connection(TokioIo::new(stream), service).await
            {
                tracing::debug!(%peer, %error, "connection ended with an error");
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Report<ServeError>> {
    // Stdout carries the MCP protocol; logs go to stderr.
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).init();

    let cli = Cli::parse();
    let game = Arc::new(load(&cli).await?);
    tracing::info!(version = %game.version().to_path(), ships = game.ship_count(), "game data loaded");
    let server = GameDataServer::new(game);

    match cli.http {
        Some(address) => serve_http(server, address).await,
        None => {
            let running = server.serve(stdio()).await.context(ServeError::Stdio)?;
            running.waiting().await.context(ServeError::Stdio)?;
            Ok(())
        }
    }
}
