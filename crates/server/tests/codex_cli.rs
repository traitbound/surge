//! Optional compatibility check against the installed CLI. No model request.
use surge_server::supervisor::SupervisorConfig;

#[test]
#[ignore = "requires installed Codex; run explicitly during runtime acceptance"]
fn default_worker_command_parses_on_installed_codex() {
    let cfg = SupervisorConfig::new("unused-for-parser-check".into());
    let output = std::process::Command::new(&cfg.worker_cmd[0])
        .args(&cfg.worker_cmd[1..cfg.worker_cmd.len() - 1])
        .arg("--help")
        .output()
        .expect("installed Codex CLI required");
    assert!(output.status.success(), "default worker command rejected: {}",
        String::from_utf8_lossy(&output.stderr));
}
