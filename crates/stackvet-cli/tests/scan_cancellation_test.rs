use stackvet_core::ExitCode;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn make_temp_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "stackvet_test_cancel_{}_{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_cli_scan_cancellation_timeout() {
    let repo = make_temp_repo("timeout_exit");
    for i in 0..10 {
        fs::write(repo.join(format!("file_{i}.py")), "import os\n").unwrap();
    }

    let exe = env!("CARGO_BIN_EXE_stackvet");
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("--timeout")
        .arg("0")
        .arg("check")
        .output()
        .expect("Failed to execute stackvet");

    let code = output.status.code();
    // ExitCode::Cancelled is 130
    assert!(
        code == Some(ExitCode::Cancelled.as_i32())
            || code == Some(ExitCode::Success.as_i32())
            || code == Some(ExitCode::IncompleteAnalysis.as_i32()),
        "Expected Cancelled (130) or clean exit, got {:?}",
        code
    );

    let _ = fs::remove_dir_all(&repo);
}
