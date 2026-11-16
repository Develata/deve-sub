//! Repository-only CI policy checks; never linked into the production binary.

mod gate;
mod inventory;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    env, fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "inventory" => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let workflow = deve_sub_yaml::parse_source_json(&fs::read_to_string(
                root.join(".github/workflows/ci.yml"),
            )?)?;
            let packages = inventory::workspace(&metadata(&root)?)?;
            let shards = inventory::validate(&workflow, &packages)?;
            println!(
                "CI non-browser inventory: {shards} Rust shards, {} packages, HTTP API15, lifecycle3, DNS1; local browser151 required separately",
                packages.len()
            );
        }
        [command, profile_flag, profile, output_flag, output]
            if command == "gate" && profile_flag == "--profile" && output_flag == "--output" =>
        {
            write_gate(output, profile)?;
        }
        [command, output_flag, output] if command == "gate" && output_flag == "--output" => {
            write_gate(output, "full")?;
        }
        _ => bail!("usage: deve-sub-ci inventory | gate --output PATH"),
    }
    Ok(())
}

fn write_gate(output: &str, profile: &str) -> Result<()> {
    let needs: Value = serde_json::from_str(&env::var("CI_NEEDS")?)?;
    let event = env::var("GITHUB_EVENT_NAME")?;
    let report = if profile == "full" {
        gate::evaluate(&needs, &event)
    } else {
        gate::evaluate_profile(&needs, &event, profile)
    };
    let path = Path::new(output);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    ensure!(report["status"] == "pass", "{profile} CI profile failed");
    Ok(())
}

fn metadata(root: &Path) -> Result<Value> {
    // WHY: a file avoids pipe backpressure while retaining a finite child wait.
    let output = tempfile::NamedTempFile::new()?;
    let mut child = Command::new("cargo")
        .args(["metadata", "--locked", "--no-deps", "--format-version=1"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output.reopen()?)
        .spawn()
        .context("start cargo metadata")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "cargo metadata failed: {status}");
            return Ok(serde_json::from_slice(&fs::read(output.path())?)?);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("cargo metadata exceeded 60 seconds");
        }
        thread::sleep(Duration::from_millis(25));
    }
}
