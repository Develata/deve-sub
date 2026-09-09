#![allow(clippy::expect_used)]
use super::*;

#[tokio::test]
async fn version_output_is_bounded_and_hung_child_is_terminated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = dir.path().join("test-binary");
    std::fs::write(
        &binary,
        "#!/bin/sh\nwhile :; do printf '0123456789'; done\n",
    )
    .expect("script");
    set_executable(&binary).expect("chmod");
    let error = verify_binary_version(&binary, "1.0")
        .await
        .expect_err("output cap");
    assert!(error.to_string().contains("4096"));
    // exec keeps the child PID, allowing direct evidence that timeout kills it.
    let pid_path = dir.path().join("pid");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 60\n",
            pid_path.display()
        ),
    )
    .expect("script");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        verify_binary_version(&binary, "1.0"),
    )
    .await
    .expect("bounded version command")
    .expect_err("timeout");
    assert!(result.to_string().contains("timed out"));
    let pid = std::fs::read_to_string(&pid_path).expect("pid");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while Path::new(&format!("/proc/{}", pid.trim())).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed-out child was killed and reaped");
}
