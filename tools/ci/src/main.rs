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
            let workflow: Value =
                serde_yaml::from_str(&fs::read_to_string(root.join(".github/workflows/ci.yml"))?)?;
            let packages = inventory::workspace(&metadata(&root)?)?;
            let shards = inventory::validate(&workflow, &packages)?;
            println!(
                "Full static baseline: {shards} Rust shards, {} packages, 4 browser lanes",
                packages.len()
            );
        }
        [command, output_flag, output] if command == "gate" && output_flag == "--output" => {
            let needs: Value = serde_json::from_str(&env::var("CI_NEEDS")?)?;
            let report = gate::evaluate(&needs, &env::var("GITHUB_EVENT_NAME")?);
            let path = Path::new(output);
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(&report)?;
            fs::write(path, format!("{json}\n"))?;
            println!("{json}");
            ensure!(report["status"] == "pass", "full baseline failed");
        }
        _ => bail!("usage: deve-sub-ci inventory | gate --output PATH"),
    }
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
