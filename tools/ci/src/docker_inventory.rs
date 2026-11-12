//! Exact native Docker builds without privileged builder helpers or emulation.
use super::*;

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const HOST_CHECK: &str = "test \"$(uname -m)\" = \"$NATIVE_MACHINE\"\ncase \"$(docker info --format '{{.Architecture}}')\" in\n  x86_64|amd64) docker_arch=amd64 ;;\n  aarch64|arm64) docker_arch=arm64 ;;\n  *) exit 1 ;;\nesac\ntest \"$docker_arch\" = \"$NATIVE_ARCH\"\nbuilder_info=$(docker buildx inspect default)\ntest \"$(printf '%s\\n' \"$builder_info\" | awk '$1 == \"Driver:\" {print $2}')\" = docker\n";

pub(super) fn validate(docker: &Value, multiarch: &Value) -> Result<()> {
    for (job, extra) in [(docker, &[][..]), (multiarch, &["if", "strategy"][..])] {
        reject_overrides(job)?;
        let keys: BTreeSet<_> = job
            .as_object()
            .context("Docker job must be an object")?
            .keys()
            .map(String::as_str)
            .collect();
        let allowed: BTreeSet<_> = ["name", "runs-on", "timeout-minutes", "needs", "steps"]
            .into_iter()
            .chain(extra.iter().copied())
            .collect();
        ensure!(
            keys == allowed && job["needs"] == "rust-gate" && job["timeout-minutes"] == 60,
            "Docker jobs require the reviewed native environment and bounded full build"
        );
    }
    ensure!(
        docker["runs-on"] == "ubuntu-24.04",
        "ordinary Docker build must use the native amd64 runner"
    );
    let checkout = json!({"uses": CHECKOUT, "with": {"persist-credentials": false}});
    let host = |arch: &str, machine: &str| {
        json!({"name": "Verify native Docker host", "env": {
        "DOCKER_CONTEXT": "default", "NATIVE_ARCH": arch, "NATIVE_MACHINE": machine}, "run": HOST_CHECK})
    };
    let ordinary = json!([
        checkout.clone(), host("amd64", "x86_64"),
        {"name": "Build Docker image from source", "env": {"DOCKER_CONTEXT": "default"},
         "run": "docker build --builder default --platform linux/amd64 --tag deve-sub:ci ."},
        {"name": "Verify environment administrator bootstrap", "env": {"DOCKER_CONTEXT": "default"},
         "run": "python3 scripts/tests/test_docker_bootstrap.py --image deve-sub:ci"},
        {"name": "Verify bounded container log rotation", "env": {"DOCKER_CONTEXT": "default"},
         "run": "python3 scripts/tests/test_docker_logging.py --image deve-sub:ci"},
        {"name": "Verify image architecture, healthcheck and Web runtime", "env": {"DOCKER_CONTEXT": "default"},
         "run": "python3 scripts/tests/test_docker_health.py --image deve-sub:ci --platform linux/amd64"}
    ]);
    ensure!(
        docker["steps"] == ordinary,
        "ordinary Docker steps must build and boot the original source without helper containers"
    );
    let rows = matrix(multiarch)?;
    ensure!(
        rows == &vec![
            json!({"arch": "amd64", "platform": "linux/amd64", "runner": "ubuntu-24.04", "machine": "x86_64"}),
            json!({"arch": "arm64", "platform": "linux/arm64", "runner": "ubuntu-24.04-arm", "machine": "aarch64"})
        ],
        "both native runner/platform pairs must occur exactly once"
    );
    ensure!(
        multiarch["runs-on"] == "${{ matrix.runner }}"
            && multiarch["strategy"]
                .as_object()
                .is_some_and(|s| s.len() == 3)
            && multiarch["strategy"]["max-parallel"] == 2,
        "native Docker matrix must use its exact runner and bounded parallelism"
    );
    let matching = json!([
        checkout, host("${{ matrix.arch }}", "${{ matrix.machine }}"),
        {"name": "Build matching native image from source", "env": {"DOCKER_CONTEXT": "default",
          "NATIVE_ARCH": "${{ matrix.arch }}", "NATIVE_PLATFORM": "${{ matrix.platform }}"},
         "run": "docker build --builder default --platform \"$NATIVE_PLATFORM\" --tag \"deve-sub-$NATIVE_ARCH\" ."},
        {"name": "Verify matching native image health and Web", "env": {"DOCKER_CONTEXT": "default",
          "NATIVE_ARCH": "${{ matrix.arch }}", "NATIVE_PLATFORM": "${{ matrix.platform }}"},
         "run": "python3 scripts/tests/test_docker_health.py --image \"deve-sub-$NATIVE_ARCH\" --platform \"$NATIVE_PLATFORM\""}
    ]);
    ensure!(
        multiarch["steps"] == matching,
        "each native Docker lane must build and boot its matching image without helper containers"
    );
    Ok(())
}
