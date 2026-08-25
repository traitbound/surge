//! `surge` CLI end to end against a real in-process server on an ephemeral
//! port: token storage (INV-AUTH-4), status, bind --create (INV-DATA-1),
//! compile of the seeded pipeline (§04 report), dispatch refusal (INV-ERR-1)
//! and abort. The CLI binary is the real one (`CARGO_BIN_EXE_surge`).

use std::process::{Command, Output};
use surge_server::AppState;
use surge_store::tokens::TokenKind;

/// Boot the full router (seeded) on 127.0.0.1:0 and mint a session token the
/// way the claim flow would (INV-AUTH-5) — the store is the mint authority.
async fn start_server() -> (String, sqlx::SqlitePool, String) {
    let pool = surge_store::open_in_memory().await.unwrap();
    surge_server::bootstrap_seed(&pool).await.unwrap();
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();
    let state = AppState::new(pool.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, surge_server::app(state)).await.unwrap();
    });
    (base, pool, session)
}

/// The real binary, pointed at the test server, with config isolated to a
/// tempdir so the developer's own token file is never touched.
fn surge(base: &str, config_dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_surge"))
        .args(args)
        .env("SURGE_API", base)
        .env("SURGE_CONFIG_DIR", config_dir)
        .env_remove("SURGE_TOKEN")
        .output()
        .expect("surge binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_stores_the_token_owner_only_and_never_echoes_it() {
    let (base, _pool, session) = start_server().await;
    let cfg = tempfile::tempdir().unwrap();

    // `surge auth <token>` writes the machine-local file (INV-AUTH-4)…
    let out = surge(&base, cfg.path(), &["auth", &session]);
    assert!(out.status.success(), "auth failed: {}", stderr(&out));
    let token_file = cfg.path().join("token");
    assert_eq!(std::fs::read_to_string(&token_file).unwrap().trim(), session);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "token file must be owner-only");
    }

    // …and bare `surge auth` reports the source without printing the token.
    let out = surge(&base, cfg.path(), &["auth"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("configured"), "should report a configured token: {text}");
    assert!(!text.contains(&session), "must never echo the token back");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_prints_health_and_recent_audit() {
    let (base, _pool, session) = start_server().await;
    let cfg = tempfile::tempdir().unwrap();

    // Unauthenticated status still reaches /healthz.
    let out = surge(&base, cfg.path(), &["status"]);
    assert!(out.status.success(), "status failed: {}", stderr(&out));
    assert!(stdout(&out).contains("schema v"));
    assert!(stdout(&out).contains("not authenticated"));

    // With a token (env path this time) the audit tail appears — the seed
    // boot always leaves at least `library.seeded`.
    let out = Command::new(env!("CARGO_BIN_EXE_surge"))
        .args(["status"])
        .env("SURGE_API", &base)
        .env("SURGE_CONFIG_DIR", cfg.path())
        .env("SURGE_TOKEN", &session)
        .output()
        .unwrap();
    assert!(out.status.success(), "authed status failed: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("recent audit:"), "missing audit block: {text}");
    assert!(text.contains("library.seeded"), "missing seed entry: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bind_create_then_compile_prints_the_capability_report() {
    let (base, _pool, session) = start_server().await;
    let cfg = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    surge(&base, cfg.path(), &["auth", &session]);

    // bind --create: project row + surge.yaml, nothing else (INV-DATA-1).
    let repo_path = repo.path().to_str().unwrap();
    let out = surge(
        &base,
        cfg.path(),
        &["bind", "demo", "--create", "--name", "Demo", "--repo", repo_path],
    );
    assert!(out.status.success(), "bind failed: {}", stderr(&out));
    assert!(stdout(&out).contains("surge.yaml written"));
    assert!(repo.path().join("surge.yaml").is_file(), "bind must write surge.yaml");

    // Compile the seeded two-node pipeline: the §04 four lines + signature.
    let out = surge(&base, cfg.path(), &["compile", "demo", "pl_two_node_v1"]);
    assert!(out.status.success(), "compile failed: {}", stderr(&out));
    let text = stdout(&out);
    for line in ["writes:", "shell:", "network:", "egress:"] {
        assert!(text.contains(line), "report missing `{line}`: {text}");
    }
    assert!(text.contains("sha256:"), "missing materialization hash: {text}");
    assert!(text.contains("cache key:"), "missing cache key: {text}");
    assert!(repo.path().join(".claude").is_dir(), "compile must write .claude/");

    // Compiling an unknown pipeline is a refusal: reason + nonzero exit.
    let out = surge(&base, cfg.path(), &["compile", "demo", "pl_nope"]);
    assert!(!out.status.success(), "unknown pipeline must fail");
    assert!(stderr(&out).contains("unknown pipeline"), "got: {}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatch_refusal_and_abort_round_trip() {
    let (base, pool, session) = start_server().await;
    let cfg = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    surge(&base, cfg.path(), &["auth", &session]);
    surge(
        &base,
        cfg.path(),
        &["bind", "p1", "--create", "--name", "P1", "--repo", repo.path().to_str().unwrap()],
    );

    // An eligible issue with no fresh materialization → dispatch refused,
    // reason on stderr, nonzero exit (INV-ID-1, INV-ERR-1).
    let now = 1_i64;
    let issue = surge_domain::board::Issue {
        id: "is_1".into(),
        project_id: "p1".into(),
        title: "fixture".into(),
        wave: 1,
        phase: "phase-0".into(),
        status: surge_domain::board::OrchestrationStatus::Eligible,
        work_order_hash: "sha256:whatever".into(),
        gate2: surge_domain::board::Gate2State::Reviewed { by: "human".into(), at: now },
        lease: None,
        retry_count: 0,
        disposition: None,
        priority: 0,
        is_wave_integration: false,
        created_at: now,
    };
    surge_store::issues::insert(&pool, &issue).await.unwrap();
    let out = surge(&base, cfg.path(), &["dispatch", "is_1"]);
    assert!(!out.status.success(), "refused dispatch must exit nonzero");
    let err = stderr(&out);
    assert!(err.contains("no fresh materialization"), "got: {err}");
    assert!(err.contains("refusal run"), "should name the refusal run: {err}");

    // Abort a running run; aborting it again is a refusal.
    let run = surge_domain::observatory::Run {
        id: "run_live".into(),
        project_id: "p1".into(),
        issue_id: Some("is_1".into()),
        kind: surge_domain::observatory::RunKind::WorkOrder,
        materialization_hash: "sha256:whatever".into(),
        work_order_hash: Some("sha256:whatever".into()),
        status: surge_domain::observatory::RunStatus::Running,
        started_at: now,
        ended_at: None,
        cost: 0.0,
    };
    surge_store::observatory::insert_run(&pool, &run).await.unwrap();
    let out = surge(&base, cfg.path(), &["abort", "run_live"]);
    assert!(out.status.success(), "abort failed: {}", stderr(&out));
    assert!(stdout(&out).contains("aborted"));
    let out = surge(&base, cfg.path(), &["abort", "run_live"]);
    assert!(!out.status.success(), "double abort must fail");
    assert!(stderr(&out).contains("not running"), "got: {}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_token_points_at_the_claim_flow() {
    let (base, _pool, _session) = start_server().await;
    let cfg = tempfile::tempdir().unwrap();
    surge(&base, cfg.path(), &["auth", "st_bogus"]);

    let out = surge(&base, cfg.path(), &["dispatch", "is_1"]);
    assert!(!out.status.success(), "401 must exit nonzero");
    let err = stderr(&out);
    assert!(err.contains("claim URL"), "should point at the claim flow: {err}");
    assert!(err.contains("surge auth"), "should point at `surge auth`: {err}");
}
