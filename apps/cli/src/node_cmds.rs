//! Node pool CLI subcommands (`node import`, `node list`).
//!
//! Extracted from `commands.rs` to keep both files under the 500-line hard
//! fuse (AGENTS.md rule #9). See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::commands::{ensure_db_dir, open_db};

/// Node management command container.
#[derive(Args)]
pub struct NodeArgs {
    #[command(subcommand)]
    pub command: NodeSubCommand,
}

/// Node subcommands.
#[derive(Subcommand)]
pub enum NodeSubCommand {
    /// Import nodes from a file or stdin.
    Import(NodeImportArgs),
    /// List nodes in the pool.
    List(NodeListArgs),
}

/// Arguments for `node import`.
#[derive(Args)]
pub struct NodeImportArgs {
    /// Input format. One of: auto, base64, uri_list, mihomo_yaml, singbox_json,
    /// xray_json, v2ray_json, shadowrocket.
    #[arg(long, default_value = "auto")]
    pub source_type: String,

    /// Read content from this file. Use `-` for stdin. If omitted, reads
    /// stdin.
    #[arg(long)]
    pub input: Option<String>,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `node list`.
#[derive(Args)]
pub struct NodeListArgs {
    /// Maximum number of nodes to print.
    #[arg(long, default_value = "50")]
    pub limit: u32,

    /// Include nodes marked missing from their source.
    #[arg(long)]
    pub include_missing: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

pub async fn node_import(args: NodeImportArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "importing nodes");

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let pool_repo = deve_sub_storage_sqlite::SqliteNodePoolRepository::new(pool);

    let content = match args.input.as_deref() {
        None | Some("-") => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read stdin")?;
            buf
        }
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("failed to read input file: {path}"))?,
    };

    if content.is_empty() {
        anyhow::bail!("input content is empty");
    }

    let source_type = args
        .source_type
        .parse::<deve_sub_domain::SourceType>()
        .map_err(|e| anyhow::anyhow!("invalid --source-type: {e}"))?;

    let parsed =
        deve_sub_application::source::parse_for_import(source_type, None, content.as_bytes())
            .map_err(|e| anyhow::anyhow!("parse failed: {e}"))?;

    let failed_count = parsed.failed.len();
    let result = deve_sub_application::source::import_nodes(&pool_repo, parsed.nodes)
        .await
        .map_err(|e| anyhow::anyhow!("import failed: {e}"))?;

    println!("Import completed:");
    println!("  new:       {}", result.new_nodes);
    println!("  duplicate: {}", result.duplicate_nodes);
    println!(
        "  failed:    {}",
        u64::try_from(failed_count).unwrap_or(u64::MAX)
    );
    Ok(())
}

pub async fn node_list(args: NodeListArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "listing nodes");

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let pool_repo = deve_sub_storage_sqlite::SqliteNodePoolRepository::new(pool);

    let params = deve_sub_application::source::ListNodesParams {
        protocol: None,
        region: None,
        include_missing: args.include_missing,
        include_inactive: false,
        cursor: None,
        limit: args.limit,
    };

    let entries = deve_sub_application::source::list_nodes(&pool_repo, params)
        .await
        .map_err(|e| anyhow::anyhow!("list failed: {e}"))?;

    if entries.is_empty() {
        println!("No nodes found.");
        return Ok(());
    }

    println!(
        "{:<28} {:<12} {:<28} {:<6} {:<10} {:<8}",
        "ID", "Protocol", "Host", "Port", "Region", "Active"
    );
    for e in &entries {
        println!(
            "{:<28} {:<12} {:<28} {:<6} {:<10} {:<8}",
            e.node.id.to_string(),
            e.node.protocol.to_string(),
            e.node.endpoint.host.uri_host(),
            e.node.endpoint.port,
            e.node.region.value.as_deref().unwrap_or(""),
            if e.is_active { "yes" } else { "no" },
        );
    }
    Ok(())
}
