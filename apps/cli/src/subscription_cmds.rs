//! Subscription CLI subcommands
//! (`subscription add/list/get/update/delete/rotate-token`).
//!
//! See `docs/plan/milestones/M6-subscription-distribution.md` Slice 1.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use time::format_description::well_known::Rfc3339;

use crate::commands::{ensure_db_dir, open_db};

/// Format a [`Timestamp`] as an ISO 8601 string for CLI output.
fn ts_to_iso8601(ts: deve_sub_kernel::Timestamp) -> String {
    ts.as_offset_date_time()
        .format(&Rfc3339)
        .unwrap_or_else(|_| ts.as_offset_date_time().to_string())
}

/// Default node selection JSON for new subscriptions (dynamic mode, no
/// filters — includes all pool nodes).
const DEFAULT_NODE_SELECTION: &str = r#"{"mode":"dynamic","filters":[]}"#;

/// Subscription management command container.
#[derive(Args)]
pub struct SubscriptionArgs {
    #[command(subcommand)]
    pub command: SubscriptionSubCommand,
}

/// Subscription subcommands.
#[derive(Subcommand)]
pub enum SubscriptionSubCommand {
    /// Create a new subscription and show the delivery token once.
    Add(SubscriptionAddArgs),
    /// List subscriptions for an owner.
    List(SubscriptionListArgs),
    /// Show a subscription by ID.
    Get(SubscriptionGetArgs),
    /// Update a subscription's mutable fields.
    Update(SubscriptionUpdateArgs),
    /// Delete a subscription.
    Delete(SubscriptionDeleteArgs),
    /// Rotate the delivery token and show the new token once.
    RotateToken(SubscriptionRotateArgs),
}

/// Arguments for `subscription add`.
#[derive(Args)]
pub struct SubscriptionAddArgs {
    /// Human-readable subscription name.
    #[arg(long)]
    pub name: String,

    /// URL-safe slug, unique per owner.
    #[arg(long)]
    pub slug: String,

    /// Owner user ULID.
    #[arg(long)]
    pub owner_id: String,

    /// Template ULID to bind.
    #[arg(long)]
    pub template_id: String,

    /// Target output profile (kebab-case: mihomo, sing-box, xray, v2ray,
    /// shadowrocket, uri_list).
    #[arg(long)]
    pub profile: String,

    /// Node selection JSON (mode, filters, nodeIds, nodeRevision).
    #[arg(long, default_value = DEFAULT_NODE_SELECTION)]
    pub node_selection: String,

    /// Traffic limit in bytes. Omit for unlimited.
    #[arg(long)]
    pub traffic_limit: Option<u64>,

    /// Expiry time as an ISO 8601 string. Omit for never expires.
    #[arg(long)]
    pub expires_at: Option<String>,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,

    /// Master key file path (for token generation).
    #[arg(long, env = "DEVE_SUB_KEY_PATH", default_value = "data/master.key")]
    pub key_path: String,
}

/// Arguments for `subscription list`.
#[derive(Args)]
pub struct SubscriptionListArgs {
    /// Owner user ULID.
    #[arg(long)]
    pub owner_id: String,

    /// Maximum number of subscriptions to print.
    #[arg(long, default_value = "50")]
    pub limit: u32,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `subscription get`.
#[derive(Args)]
pub struct SubscriptionGetArgs {
    /// Subscription ID (ULID).
    #[arg(long)]
    pub id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `subscription update`.
#[derive(Args)]
pub struct SubscriptionUpdateArgs {
    /// Subscription ID (ULID).
    #[arg(long)]
    pub id: String,

    /// New human-readable name.
    #[arg(long)]
    pub name: String,

    /// New URL-safe slug.
    #[arg(long)]
    pub slug: String,

    /// Pinned template version. Omit to follow the active version.
    #[arg(long)]
    pub template_version_pin: Option<u64>,

    /// New target output profile.
    #[arg(long)]
    pub profile: String,

    /// New node selection JSON.
    #[arg(long, default_value = DEFAULT_NODE_SELECTION)]
    pub node_selection: String,

    /// New traffic limit in bytes. Omit for unlimited.
    #[arg(long)]
    pub traffic_limit: Option<u64>,

    /// New expiry time as ISO 8601. Omit for never expires.
    #[arg(long)]
    pub expires_at: Option<String>,

    /// Whether delivery is enabled (true/false). Omit to keep current.
    #[arg(long)]
    pub enabled: Option<bool>,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `subscription delete`.
#[derive(Args)]
pub struct SubscriptionDeleteArgs {
    /// Subscription ID (ULID).
    #[arg(long)]
    pub id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `subscription rotate-token`.
#[derive(Args)]
pub struct SubscriptionRotateArgs {
    /// Subscription ID (ULID).
    #[arg(long)]
    pub id: String,

    /// Grace period in seconds. -1 or omit for permanent grace. 0 for no grace.
    #[arg(long, default_value = "-1")]
    pub grace_seconds: i64,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,

    /// Master key file path.
    #[arg(long, env = "DEVE_SUB_KEY_PATH", default_value = "data/master.key")]
    pub key_path: String,
}

/// Load the master key, generating it if missing.
fn load_master_key(path: &str) -> Result<deve_sub_security::MasterKey> {
    deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(path))
        .context("failed to load master key")
}

pub async fn subscription_add(args: SubscriptionAddArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, name = %args.name, "adding subscription");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool.clone());
    let master_key = load_master_key(&args.key_path)?;

    let owner_id = deve_sub_kernel::UserId::parse(&args.owner_id)
        .context("invalid owner_id (expected ULID)")?;
    let template_id = deve_sub_kernel::TemplateId::parse(&args.template_id)
        .context("invalid template_id (expected ULID)")?;
    let node_selection: serde_json::Value =
        serde_json::from_str(&args.node_selection).context("invalid node_selection JSON")?;

    let params = deve_sub_application::subscription::CreateSubscriptionParams {
        name: args.name.clone(),
        slug: args.slug.clone(),
        owner_id,
        template_id,
        profile: args.profile.clone(),
        node_selection,
        traffic_limit: args.traffic_limit,
        expires_at: args.expires_at.clone(),
    };

    let result =
        deve_sub_application::subscription::create_subscription(&sub_repo, &master_key, params)
            .await
            .map_err(|e| anyhow::anyhow!("create failed: {e}"))?;

    println!("Subscription created successfully:");
    println!("  id:      {}", result.subscription.id);
    println!("  name:    {}", result.subscription.name);
    println!("  slug:    {}", result.subscription.slug);
    println!("  profile: {}", result.subscription.profile);
    println!("  token:   {}", result.token_plaintext);
    println!("\nStore the token securely — it will not be shown again.");
    Ok(())
}

pub async fn subscription_list(args: SubscriptionListArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "listing subscriptions");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool);

    let owner_id = deve_sub_kernel::UserId::parse(&args.owner_id)
        .context("invalid owner_id (expected ULID)")?;

    let subs = deve_sub_application::subscription::list_subscriptions(
        &sub_repo,
        owner_id,
        None,
        Some(args.limit),
    )
    .await
    .context("list failed")?;

    if subs.is_empty() {
        println!("No subscriptions found.");
        return Ok(());
    }

    println!(
        "{:<28} {:<20} {:<12} {:<8} {:<20}",
        "ID", "Name", "Profile", "Enabled", "Updated"
    );
    for s in &subs {
        println!(
            "{:<28} {:<20} {:<12} {:<8} {:<20}",
            s.id.to_string(),
            s.name,
            s.profile,
            if s.enabled { "yes" } else { "no" },
            ts_to_iso8601(s.updated_at),
        );
    }
    Ok(())
}

pub async fn subscription_get(args: SubscriptionGetArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "getting subscription");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool);

    let id = deve_sub_kernel::SubscriptionId::parse(&args.id)
        .context("invalid subscription id (expected ULID)")?;

    let sub = deve_sub_application::subscription::get_subscription(&sub_repo, id)
        .await
        .context("get failed")?
        .ok_or_else(|| anyhow::anyhow!("subscription '{id}' not found"))?;

    println!("Subscription:");
    println!("  id:                    {}", sub.id);
    println!("  name:                  {}", sub.name);
    println!("  slug:                  {}", sub.slug);
    println!("  owner_id:              {}", sub.owner_id);
    println!("  template_id:           {}", sub.template_id);
    println!("  template_version_pin:  {:?}", sub.template_version_pin);
    println!("  profile:               {}", sub.profile);
    println!("  traffic_limit:         {:?}", sub.traffic_limit);
    println!(
        "  expires_at:            {:?}",
        sub.expires_at.map(ts_to_iso8601)
    );
    println!("  enabled:               {}", sub.enabled);
    println!("  created_at:            {}", ts_to_iso8601(sub.created_at));
    println!("  updated_at:            {}", ts_to_iso8601(sub.updated_at));
    println!(
        "\n  node_selection: {}",
        serde_json::to_string_pretty(&sub.node_selection)?
    );
    Ok(())
}

pub async fn subscription_update(args: SubscriptionUpdateArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "updating subscription");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool);

    let id = deve_sub_kernel::SubscriptionId::parse(&args.id)
        .context("invalid subscription id (expected ULID)")?;
    let node_selection: serde_json::Value =
        serde_json::from_str(&args.node_selection).context("invalid node_selection JSON")?;

    let params = deve_sub_application::subscription::UpdateSubscriptionParams {
        id,
        name: args.name.clone(),
        slug: args.slug.clone(),
        template_version_pin: args.template_version_pin,
        profile: args.profile.clone(),
        node_selection,
        traffic_limit: args.traffic_limit,
        expires_at: args.expires_at,
        enabled: args.enabled,
    };

    let sub = deve_sub_application::subscription::update_subscription(&sub_repo, params)
        .await
        .map_err(|e| anyhow::anyhow!("update failed: {e}"))?;

    println!("Subscription updated successfully:");
    println!("  id:      {}", sub.id);
    println!("  name:    {}", sub.name);
    println!("  slug:    {}", sub.slug);
    println!("  profile: {}", sub.profile);
    Ok(())
}

pub async fn subscription_delete(args: SubscriptionDeleteArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "deleting subscription");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool);

    let id = deve_sub_kernel::SubscriptionId::parse(&args.id)
        .context("invalid subscription id (expected ULID)")?;

    deve_sub_application::subscription::delete_subscription(&sub_repo, id)
        .await
        .map_err(|e| anyhow::anyhow!("delete failed: {e}"))?;

    println!("Subscription {id} deleted.");
    Ok(())
}

pub async fn subscription_rotate(args: SubscriptionRotateArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "rotating subscription token");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let sub_repo = deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(pool.clone());
    let token_repo = deve_sub_storage_sqlite::SqliteSubscriptionTokenRepository::new(pool);
    let master_key = load_master_key(&args.key_path)?;

    let id = deve_sub_kernel::SubscriptionId::parse(&args.id)
        .context("invalid subscription id (expected ULID)")?;

    let grace = if args.grace_seconds < 0 {
        None
    } else {
        Some(time::Duration::seconds(args.grace_seconds))
    };

    let result = deve_sub_application::subscription::rotate_token(
        &sub_repo,
        &token_repo,
        &master_key,
        id,
        grace,
    )
    .await
    .map_err(|e| anyhow::anyhow!("rotate failed: {e}"))?;

    println!("Token rotated successfully:");
    println!("  token_id: {}", result.token_id);
    println!("  token:    {}", result.token_plaintext);
    println!("\nStore the token securely — it will not be shown again.");
    Ok(())
}
