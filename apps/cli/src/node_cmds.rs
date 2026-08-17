//! Node pool CLI subcommands (`node import`, `node list`).
//!
//! Extracted from `commands.rs` to keep both files under the 500-line hard
//! fuse (AGENTS.md rule #9). See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

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

    /// Master key path for at-rest encryption of node credentials.
    #[arg(long, env = "DEVE_SUB_KEY_PATH", default_value = "data/master.key")]
    pub key_path: String,
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

    /// Output format. `table` (default) for human-readable output, `uri`
    /// for one URI per line (CLI-003), `json` for machine-readable JSON
    /// (CLI-004).
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,

    /// Master key path for decrypting node credentials at rest.
    #[arg(long, env = "DEVE_SUB_KEY_PATH", default_value = "data/master.key")]
    pub key_path: String,
}

/// Output format for `node list`.
#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table (default).
    Table,
    /// One URI per line (CLI-003).
    Uri,
    /// JSON array for automation scripts (CLI-004).
    Json,
}

/// JSON-serializable node summary for `--format json` (CLI-004).
#[derive(Serialize)]
struct NodeJson {
    id: String,
    protocol: String,
    host: String,
    port: u16,
    region: Option<String>,
    active: bool,
    missing_from_source: bool,
}

pub async fn node_import(args: NodeImportArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "importing nodes");

    ensure_db_dir(&args.db_path)?;
    ensure_db_dir(&args.key_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let master_key = Arc::new(
        deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(&args.key_path))
            .context("failed to load master key")?,
    );
    let pool_repo = deve_sub_storage_sqlite::SqliteNodePoolRepository::new_with_key(
        pool,
        Arc::clone(&master_key),
    );

    let content: Vec<u8> = match args.input.as_deref() {
        None | Some("-") => {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .context("failed to read stdin")?;
            buf
        }
        Some(path) => {
            use std::io::Read;
            let file = std::fs::File::open(path)
                .with_context(|| format!("failed to open input file: {path}"))?;
            let mut reader = std::io::BufReader::new(file);
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .with_context(|| format!("failed to read input file: {path}"))?;
            buf
        }
    };

    if content.is_empty() {
        anyhow::bail!("input content is empty");
    }

    let source_type = args
        .source_type
        .parse::<deve_sub_domain::SourceType>()
        .map_err(|e| anyhow::anyhow!("invalid --source-type: {e}"))?;

    let parsed = deve_sub_application::source::parse_for_import(source_type, None, &content)
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
    ensure_db_dir(&args.key_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let master_key = Arc::new(
        deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(&args.key_path))
            .context("failed to load master key")?,
    );
    let pool_repo = deve_sub_storage_sqlite::SqliteNodePoolRepository::new_with_key(
        pool,
        Arc::clone(&master_key),
    );

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
        match args.format {
            OutputFormat::Json => println!("[]"),
            OutputFormat::Uri => {}
            OutputFormat::Table => println!("No nodes found."),
        }
        return Ok(());
    }

    match args.format {
        OutputFormat::Table => print_table(&entries),
        OutputFormat::Uri => print_uris(&entries)?,
        OutputFormat::Json => print_json(&entries)?,
    }
    Ok(())
}

fn print_table(entries: &[deve_sub_domain::NodePoolEntry]) {
    println!(
        "{:<28} {:<12} {:<28} {:<6} {:<10} {:<8}",
        "ID", "Protocol", "Host", "Port", "Region", "Active"
    );
    for e in entries {
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
}

fn print_uris(entries: &[deve_sub_domain::NodePoolEntry]) -> Result<()> {
    for e in entries {
        match deve_sub_emitter::emit_uri(&e.node) {
            Ok(uri) => println!("{uri}"),
            Err(deve_sub_emitter::EmitError::NoEmitter(proto)) => {
                tracing::warn!(protocol = %proto, "skipping node without URI emitter");
            }
            Err(err) => {
                anyhow::bail!("failed to emit URI for node {}: {err}", e.node.id);
            }
        }
    }
    Ok(())
}

fn print_json(entries: &[deve_sub_domain::NodePoolEntry]) -> Result<()> {
    let summary: Vec<NodeJson> = entries
        .iter()
        .map(|e| NodeJson {
            id: e.node.id.to_string(),
            protocol: e.node.protocol.to_string(),
            host: e.node.endpoint.host.uri_host().to_owned(),
            port: e.node.endpoint.port,
            region: e.node.region.value.clone(),
            active: e.is_active,
            missing_from_source: e.missing_from_source,
        })
        .collect();
    let json =
        serde_json::to_string_pretty(&summary).context("failed to serialize node list as JSON")?;
    println!("{json}");
    Ok(())
}
