//! Static workflow coverage for normal capability execution, not a scheduler.

use super::*;

pub(super) fn validate(normal: &Value, gate: &Value) -> Result<()> {
    reject_overrides(normal)?;
    let rows = matrix(normal)?;
    let expected = BTreeSet::from([
        "protocol-1",
        "protocol-2",
        "protocol-3",
        "protocol-4",
        "api",
        "browser",
        "themes-1",
        "themes-2",
        "themes-3",
        "themes-4",
        "dns",
    ]);
    let actual = rows
        .iter()
        .map(|row| string(&row["lane"]))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        rows.len() == 1 && actual == BTreeSet::from(["dns"]),
        "CI normal inventory is explicitly DNS-only; full catalogue remains separate"
    );
    ensure!(
        normal["strategy"]["max-parallel"] == 4,
        "normal lanes must be bounded at four runners"
    );
    ensure!(
        normal["needs"] == json!(["rust-gate", "web-wasm", "build-release"]),
        "normal runtime requires the full Rust gate and both producer artifacts"
    );
    let name = "Verify normal candidate artifacts";
    ensure!(
        named_step(normal, name)?
            == &json!({
                "name": name,
                "env": {
                    "BINARY_MANIFEST_SHA256": "${{ needs.build-release.outputs.manifest-sha256 }}",
                    "WASM_MANIFEST_SHA256": "${{ needs.web-wasm.outputs.manifest-sha256 }}",
                },
                "run": "python3 scripts/ci/artifact.py verify --kind binary --root target/release --manifest-sha256 \"$BINARY_MANIFEST_SHA256\"\npython3 scripts/ci/artifact.py verify --kind wasm --root apps/web/dist --manifest-sha256 \"$WASM_MANIFEST_SHA256\"\nchmod +x target/release/deve-sub\n",
            }),
        "normal runtime must verify both independent producer manifest digests"
    );
    let name = "Run complete normal lane";
    ensure!(
        named_step(normal, name)?
            == &json!({
                "name": name,
                "env": {"NORMAL_LANE": "${{ matrix.lane }}", "DEVE_SUB_VALIDATORS": "${{ runner.temp }}/validators"},
                "run": "python3 scripts/ci/normal.py run --profile ci-non-browser --lane \"$NORMAL_LANE\" --output \"$RUNNER_TEMP/normal\"",
            }),
        "normal execution cannot be filtered, skipped or suppressed"
    );
    ensure!(
        gate["if"] == "always()"
            && gate["needs"]
                == json!([
                    "normal-non-browser",
                    "build-release",
                    "web-wasm",
                    "rust-gate"
                ]),
        "normal gate must always evaluate its matrix and producers"
    );
    let name = "Require complete non-browser normal scope";
    ensure!(
        named_step(gate, name)?
            == &json!({
                "name": name, "if": "always()", "env": {"CI_NORMAL_NEEDS": "${{ toJSON(needs) }}"},
                "run": "python3 scripts/ci/normal.py gate --profile ci-non-browser --reports \"$RUNNER_TEMP/normal-results\" --output \"$RUNNER_TEMP/normal-result.json\"",
            }),
        "normal gate must require actual complete case evidence"
    );
    let catalogue: Value =
        serde_json::from_str(include_str!("../../../tests/acceptance/normal-ci.json"))?;
    let lanes = catalogue["lanes"]
        .as_object()
        .context("normal catalogue lanes")?;
    ensure!(
        lanes.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected,
        "normal catalogue and workflow lanes differ"
    );
    let mut cases = BTreeSet::new();
    for lane in lanes.values() {
        let entries = array(&lane["cases"])?;
        ensure!(!entries.is_empty(), "empty normal lane");
        for entry in entries {
            ensure!(cases.insert(string(entry)?), "duplicate normal case owner");
        }
    }
    ensure!(cases.len() >= 40, "normal catalogue lost baseline cases");
    Ok(())
}
