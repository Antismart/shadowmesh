//! ShadowMesh CLI — manage your node from the command line.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

use shadowmesh_cli::client::NodeClient;
use shadowmesh_cli::commands;

#[derive(Parser)]
#[command(name = "shadowmesh-cli", about = "ShadowMesh node management CLI")]
struct Cli {
    /// Node API base URL
    #[arg(long, default_value = "http://127.0.0.1:3030", global = true)]
    node_url: String,

    /// Output raw JSON instead of formatted text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show node status
    Status,

    /// Show health checks
    Health,

    /// List connected peers
    Peers,

    /// Show node metrics
    Metrics,

    /// Upload a file to the node
    Upload {
        /// Path to the file to upload
        file: PathBuf,
    },

    /// Show storage statistics
    Storage,

    /// List stored content
    List,

    /// Show content details
    Get {
        /// Content ID (CID)
        cid: String,
    },

    /// Pin content to prevent garbage collection
    Pin {
        /// Content ID (CID)
        cid: String,
    },

    /// Unpin content
    Unpin {
        /// Content ID (CID)
        cid: String,
    },

    /// Delete content
    Delete {
        /// Content ID (CID)
        cid: String,
    },

    /// Fetch content from the P2P network
    Fetch {
        /// Content ID (CID)
        cid: String,
    },

    /// Run garbage collection
    Gc {
        /// Target free space in GB
        #[arg(long, default_value = "1.0")]
        target_gb: f64,
    },

    /// Show current configuration
    Config,

    /// Update configuration
    ConfigSet {
        /// Node name
        #[arg(long)]
        name: Option<String>,

        /// Maximum connected peers
        #[arg(long)]
        max_peers: Option<usize>,

        /// Maximum storage in GB
        #[arg(long)]
        storage_gb: Option<f64>,

        /// Maximum outbound bandwidth in Mbps
        #[arg(long)]
        bandwidth_mbps: Option<u64>,
    },

    /// Show bandwidth statistics
    Bandwidth,

    /// Show replication health
    Replication,

    /// Download content to a file
    Download {
        /// Content ID (CID)
        cid: String,
        /// Output file path (defaults to CID-based name)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Check node readiness
    Ready,

    /// Gracefully shutdown the node
    Shutdown,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let client = NodeClient::new(&cli.node_url);
    let json = cli.json;

    let result = match cli.command {
        Command::Status => commands::status(&client, json).await,
        Command::Health => commands::health(&client, json).await,
        Command::Peers => commands::peers(&client, json).await,
        Command::Metrics => commands::metrics(&client, json).await,
        Command::Upload { file } => commands::upload(&client, &file, json).await,
        Command::Storage => commands::storage(&client, json).await,
        Command::List => commands::list(&client, json).await,
        Command::Get { cid } => commands::get(&client, &cid, json).await,
        Command::Pin { cid } => commands::pin(&client, &cid).await,
        Command::Unpin { cid } => commands::unpin(&client, &cid).await,
        Command::Delete { cid } => commands::delete(&client, &cid).await,
        Command::Fetch { cid } => commands::fetch(&client, &cid, json).await,
        Command::Gc { target_gb } => commands::gc(&client, target_gb, json).await,
        Command::Config => commands::config(&client, json).await,
        Command::ConfigSet {
            name,
            max_peers,
            storage_gb,
            bandwidth_mbps,
        } => {
            commands::config_set(&client, name, max_peers, storage_gb, bandwidth_mbps, json).await
        }
        Command::Download { cid, output } => {
            commands::download(&client, &cid, output.as_deref()).await
        }
        Command::Ready => commands::ready(&client, json).await,
        Command::Bandwidth => commands::bandwidth(&client, json).await,
        Command::Replication => commands::replication(&client, json).await,
        Command::Shutdown => commands::shutdown(&client, json).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
