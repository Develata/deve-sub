//! Template CLI subcommands (`template add/list/get/update/delete/rollback`).
//!
//! Extracted from `commands.rs` to keep both files under the 500-line hard
//! fuse (AGENTS.md rule #9). See
//! `docs/plan/milestones/M5-generator-and-v3-template.md` Slice 1.

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

/// Template management command container.
#[derive(Args)]
pub struct TemplateArgs {
    #[command(subcommand)]
    pub command: TemplateSubCommand,
}

/// Template subcommands.
#[derive(Subcommand)]
pub enum TemplateSubCommand {
    /// Create a new V3 subscription template.
    Add(TemplateAddArgs),
    /// List templates.
    List(TemplateListArgs),
    /// Show a template by ID or name.
    Get(TemplateGetArgs),
    /// Update a template, creating a new version.
    Update(TemplateUpdateArgs),
    /// Delete a template.
    Delete(TemplateDeleteArgs),
    /// List version history for a template.
    Versions(TemplateVersionsArgs),
    /// Rollback a template to a specific version.
    Rollback(TemplateRollbackArgs),
}

/// Arguments for `template add`.
#[derive(Args)]
pub struct TemplateAddArgs {
    /// Human-readable template name.
    #[arg(long)]
    pub name: String,

    /// Optional description.
    #[arg(long, default_value = "")]
    pub description: String,

    /// Path to the V3 template YAML file. Use `-` for stdin.
    #[arg(long)]
    pub spec_file: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template list`.
#[derive(Args)]
pub struct TemplateListArgs {
    /// Maximum number of templates to print.
    #[arg(long, default_value = "50")]
    pub limit: u32,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template get`.
#[derive(Args)]
pub struct TemplateGetArgs {
    /// Template ID (ULID) or name.
    #[arg(long)]
    pub id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template update`.
#[derive(Args)]
pub struct TemplateUpdateArgs {
    /// Template ID (ULID).
    #[arg(long)]
    pub id: String,

    /// New human-readable name.
    #[arg(long)]
    pub name: String,

    /// New description.
    #[arg(long, default_value = "")]
    pub description: String,

    /// Path to the new V3 template YAML file. Use `-` for stdin.
    #[arg(long)]
    pub spec_file: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template delete`.
#[derive(Args)]
pub struct TemplateDeleteArgs {
    /// Template ID (ULID).
    #[arg(long)]
    pub id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template versions`.
#[derive(Args)]
pub struct TemplateVersionsArgs {
    /// Template ID (ULID).
    #[arg(long)]
    pub id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Arguments for `template rollback`.
#[derive(Args)]
pub struct TemplateRollbackArgs {
    /// Version ID (ULID) to activate.
    #[arg(long)]
    pub version_id: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Read a spec YAML from a file path or stdin (`-`).
fn read_spec(path: &str) -> Result<String> {
    if path == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("failed to read spec file: {path}"))
    }
}

/// Resolve a template by ID or name. Tries ULID parse first, then name lookup.
async fn resolve_template(
    repo: &deve_sub_storage_sqlite::SqliteTemplateRepository,
    id_or_name: &str,
) -> Result<deve_sub_domain::SubscriptionTemplate> {
    if let Ok(id) = deve_sub_kernel::TemplateId::parse(id_or_name)
        && let Some(t) = deve_sub_application::template::get_template(repo, id)
            .await
            .context("template lookup failed")?
    {
        return Ok(t);
    }
    if let Some(t) = deve_sub_application::template::get_template_by_name(repo, id_or_name)
        .await
        .context("template name lookup failed")?
    {
        return Ok(t);
    }
    anyhow::bail!("template '{id_or_name}' not found")
}

pub async fn template_add(args: TemplateAddArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, name = %args.name, "adding template");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let template_repo = deve_sub_storage_sqlite::SqliteTemplateRepository::new(pool.clone());
    let version_repo = deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(pool);

    let spec_yaml = read_spec(&args.spec_file)?;
    if spec_yaml.is_empty() {
        anyhow::bail!("spec file is empty");
    }

    let params = deve_sub_application::template::CreateTemplateParams {
        name: args.name.clone(),
        description: args.description.clone(),
        spec_yaml,
    };

    let result =
        deve_sub_application::template::create_template(&template_repo, &version_repo, params)
            .await
            .map_err(|e| anyhow::anyhow!("create failed: {e}"))?;

    println!("Template created successfully:");
    println!("  id:             {}", result.template.id);
    println!("  name:           {}", result.template.name);
    println!("  active_version: {}", result.template.active_version);
    println!("  version_id:     {}", result.version.id);
    Ok(())
}

pub async fn template_list(args: TemplateListArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "listing templates");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let template_repo = deve_sub_storage_sqlite::SqliteTemplateRepository::new(pool);

    let templates =
        deve_sub_application::template::list_templates(&template_repo, None, Some(args.limit))
            .await
            .context("list failed")?;

    if templates.is_empty() {
        println!("No templates found.");
        return Ok(());
    }

    println!(
        "{:<28} {:<24} {:<8} {:<20}",
        "ID", "Name", "Version", "Updated"
    );
    for t in &templates {
        println!(
            "{:<28} {:<24} {:<8} {:<20}",
            t.id.to_string(),
            t.name,
            t.active_version,
            ts_to_iso8601(t.updated_at),
        );
    }
    Ok(())
}

pub async fn template_get(args: TemplateGetArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "getting template");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let template_repo = deve_sub_storage_sqlite::SqliteTemplateRepository::new(pool.clone());
    let version_repo = deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(pool);

    let template = resolve_template(&template_repo, &args.id).await?;
    let active = deve_sub_application::template::get_active_version(&version_repo, template.id)
        .await
        .context("active version lookup failed")?;

    println!("Template:");
    println!("  id:             {}", template.id);
    println!("  name:           {}", template.name);
    println!("  description:    {}", template.description);
    println!("  active_version: {}", template.active_version);
    println!("  created_at:     {}", ts_to_iso8601(template.created_at));
    println!("  updated_at:     {}", ts_to_iso8601(template.updated_at));

    if let Some(v) = active {
        println!("\nActive version:");
        println!("  version_id: {}", v.id);
        println!("  version:    {}", v.version);
        println!("  created_at: {}", ts_to_iso8601(v.created_at));
        println!("\nSpec YAML:");
        println!("{}", v.spec_yaml);
    }
    Ok(())
}

pub async fn template_update(args: TemplateUpdateArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "updating template");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let template_repo = deve_sub_storage_sqlite::SqliteTemplateRepository::new(pool.clone());
    let version_repo = deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(pool);

    let id = deve_sub_kernel::TemplateId::parse(&args.id)
        .context("invalid template id (expected ULID)")?;

    let spec_yaml = read_spec(&args.spec_file)?;
    if spec_yaml.is_empty() {
        anyhow::bail!("spec file is empty");
    }

    let params = deve_sub_application::template::UpdateTemplateParams {
        id,
        name: args.name.clone(),
        description: args.description.clone(),
        spec_yaml,
    };

    let result =
        deve_sub_application::template::update_template(&template_repo, &version_repo, params)
            .await
            .map_err(|e| anyhow::anyhow!("update failed: {e}"))?;

    println!("Template updated successfully:");
    println!("  id:             {}", result.template.id);
    println!("  name:           {}", result.template.name);
    println!("  active_version: {}", result.template.active_version);
    println!("  version_id:     {}", result.version.id);
    Ok(())
}

pub async fn template_delete(args: TemplateDeleteArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "deleting template");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let template_repo = deve_sub_storage_sqlite::SqliteTemplateRepository::new(pool);

    let id = deve_sub_kernel::TemplateId::parse(&args.id)
        .context("invalid template id (expected ULID)")?;

    deve_sub_application::template::delete_template(&template_repo, id)
        .await
        .map_err(|e| anyhow::anyhow!("delete failed: {e}"))?;

    println!("Template {id} deleted.");
    Ok(())
}

pub async fn template_versions(args: TemplateVersionsArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, id = %args.id, "listing versions");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let version_repo = deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(pool);

    let id = deve_sub_kernel::TemplateId::parse(&args.id)
        .context("invalid template id (expected ULID)")?;

    let versions = deve_sub_application::template::list_versions(&version_repo, id, Some(100))
        .await
        .context("list versions failed")?;

    if versions.is_empty() {
        println!("No versions found.");
        return Ok(());
    }

    println!(
        "{:<28} {:<8} {:<8} {:<20}",
        "Version ID", "Version", "Active", "Created"
    );
    for v in &versions {
        println!(
            "{:<28} {:<8} {:<8} {:<20}",
            v.id.to_string(),
            v.version,
            if v.is_active { "yes" } else { "no" },
            ts_to_iso8601(v.created_at),
        );
    }
    Ok(())
}

pub async fn template_rollback(args: TemplateRollbackArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, version_id = %args.version_id, "rolling back template");

    ensure_db_dir(&args.db_path)?;
    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let version_repo = deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(pool);

    let version_id = deve_sub_kernel::TemplateVersionId::parse(&args.version_id)
        .context("invalid version id (expected ULID)")?;

    let version = deve_sub_application::template::rollback_template(&version_repo, version_id)
        .await
        .map_err(|e| anyhow::anyhow!("rollback failed: {e}"))?;

    println!("Rollback successful:");
    println!("  version_id: {}", version.id);
    println!("  version:    {}", version.version);
    println!("  active:     {}", version.is_active);
    Ok(())
}
