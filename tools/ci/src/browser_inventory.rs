//! Browser preparation requires an existing exact asset, never a moved download.
use super::*;

pub(super) fn validate(browser: &Value, normal: &Value) -> Result<()> {
    for (job, condition) in [(browser, None), (normal, Some("matrix.lane != 'dns'"))] {
        let name = "Verify existing locked Playwright Chromium";
        let mut expected = json!({"name": name, "run": "node verify-locked-browser.cjs", "working-directory": "tests/e2e"});
        if let Some(condition) = condition {
            expected["if"] = json!(condition);
        }
        ensure!(
            named_step(job, name)? == &expected,
            "browser capability preflight cannot download, substitute or suppress a missing exact asset"
        );
        let steps = array(&job["steps"])?;
        let installs: Vec<_> = steps.iter().filter(|s| s["run"] == "npm ci").collect();
        ensure!(
            installs.len() == 1
                && installs[0]["env"] == json!({"PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1"}),
            "dependency installation must disable automatic browser acquisition"
        );
        ensure!(
            !steps.iter().any(|s| s["run"]
                .as_str()
                .is_some_and(|r| r.contains("playwright install") || r.contains("cli.js install"))),
            "browser acquisition is outside the permitted workflow"
        );
    }
    let steps = array(&browser["steps"])?;
    let position = |name: &str| {
        steps
            .iter()
            .position(|s| s["name"] == name)
            .with_context(|| format!("missing {name}"))
    };
    let preflight = position("Verify existing locked Playwright Chromium")?;
    ensure!(
        position("Verify native installer lifecycle")? < preflight
            && position("Verify browser process lifecycle")? < preflight
            && preflight < position("Run browser lane")?,
        "independent legacy checks precede browser capability preflight, which precedes execution"
    );
    Ok(())
}
