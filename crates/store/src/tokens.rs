//! Token repository (design §04). Plaintext tokens are minted here, returned
//! once, and only their SHA-256 lands in the store. Prefixes: `st_` session,
//! `rt_` runtime, `cl_` claim.

use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Session,
    Runtime,
    Claim,
}

impl TokenKind {
    fn as_str(self) -> &'static str {
        match self {
            TokenKind::Session => "session",
            TokenKind::Runtime => "runtime",
            TokenKind::Claim => "claim",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            TokenKind::Session => "st_",
            TokenKind::Runtime => "rt_",
            TokenKind::Claim => "cl_",
        }
    }
}

/// A valid, unrevoked credential as seen by the middleware.
///
/// A runtime identity carries the run it was minted for when the supervisor
/// minted it (`Some`), and nothing when a human minted it for the project
/// (`None`). That distinction is the whole of the scope story: comparing
/// project ids alone let any live runtime token in project P act on any run
/// or issue in P — worker A refreshing dead worker B's lease forever (auth
/// review 2026-08-26, the heartbeat hijack). `server::runtime_api` says what
/// each shape may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Human,
    Runtime {
        project_id: String,
        /// `Some` = supervisor-spawned, scoped to that run and its issue;
        /// `None` = the project token (interactive / curl), project-scoped.
        run_id: Option<String>,
    },
}

/// What a presented plaintext resolved to. Expiry is distinguished from
/// absence so the refusal can name which one it was, rather than telling an
/// operator whose project token aged out that it was "invalid" (INV-ERR-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenLookup {
    Active(Identity),
    Expired,
    Unknown,
}

/// How long a human-minted project runtime token lives (smoke walk 5, F1).
/// Long enough for the manual curl session that is its only real purpose,
/// short enough that one forgotten mid-walk is dead before the walk ends —
/// the walker's exploit token was 17 minutes old.
pub const PROJECT_RUNTIME_TTL_MS: i64 = 15 * 60 * 1000;

pub fn hash(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

fn generate(kind: TokenKind) -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{}{}", kind.prefix(), hex::encode(bytes))
}

/// Mint an instance-level credential — session or claim — and return the
/// plaintext (the only time it exists).
///
/// Runtime credentials deliberately cannot be minted here. Every one of them
/// must carry a lifecycle: a run binding ([`mint_for_run`], revoked when the
/// supervisor observes the worker exit) or an expiry
/// ([`rotate_project_runtime`]). F1 was exactly the credential this refusal
/// makes unrepresentable — minted with `run_id: None` and no expiry,
/// revocable by nothing, still claiming fresh leases 17 minutes after its run
/// went terminal (smoke walk 5).
pub async fn mint<'e, E>(
    executor: E,
    kind: TokenKind,
    project_id: Option<&str>,
    now: i64,
) -> anyhow::Result<String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    anyhow::ensure!(
        kind != TokenKind::Runtime,
        "runtime credentials need a lifecycle: mint_for_run (run-bound) or \
         rotate_project_runtime (expiring) — never a bare mint (F1)"
    );
    insert_token(executor, kind, project_id, None, None, now).await
}

/// Mint a credential bound to one run. The binding is what makes revocation
/// possible at all: the supervisor discards the plaintext at spawn, so a
/// token with no `run_id` can never be revoked by the lifecycle that created
/// it (smoke walk 4, S2).
///
/// No expiry: a run-bound token must stay live exactly as long as its worker
/// does — an abort lands at the worker's next status poll (§06-06) and that
/// poll needs a live token — so its clock is the observed process exit, never
/// a timestamp.
pub async fn mint_for_run<'e, E>(
    executor: E,
    kind: TokenKind,
    project_id: Option<&str>,
    run_id: Option<&str>,
    now: i64,
) -> anyhow::Result<String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    debug_assert_eq!(kind == TokenKind::Runtime, project_id.is_some());
    insert_token(executor, kind, project_id, run_id, None, now).await
}

async fn insert_token<'e, E>(
    executor: E,
    kind: TokenKind,
    project_id: Option<&str>,
    run_id: Option<&str>,
    expires_at: Option<i64>,
    now: i64,
) -> anyhow::Result<String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let plaintext = generate(kind);
    let token_hash = hash(&plaintext);
    let kind_s = kind.as_str();
    sqlx::query!(
        "INSERT INTO token (kind, token_hash, project_id, run_id, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        kind_s,
        token_hash,
        project_id,
        run_id,
        expires_at,
        now
    )
    .execute(executor)
    .await?;
    Ok(plaintext)
}

/// One live project runtime credential, as handed back by the mint endpoint.
#[derive(Debug, Clone)]
pub struct ProjectRuntimeToken {
    pub plaintext: String,
    pub expires_at: i64,
    /// How many previously live project tokens this rotation killed.
    pub rotated_out: u64,
}

/// Rotate the project's runtime credential (design §17 API TOKENS: "Each
/// rotatable" — only the mint half existed, so calling it twice left BOTH
/// tokens valid forever, F1). Every live *unbound* runtime token for the
/// project is revoked first, then one expiring token is minted.
///
/// Run-bound tokens are deliberately untouched: they belong to workers
/// running right now, and a human minting a curl token must not kill them.
/// What holds afterwards is per-project — at most one live project runtime
/// token, plus one per in-flight run.
pub async fn rotate_project_runtime(
    pool: &SqlitePool,
    project_id: &str,
    now: i64,
) -> anyhow::Result<ProjectRuntimeToken> {
    let rotated_out = revoke_project_runtime(pool, project_id, now).await?;
    let expires_at = now + PROJECT_RUNTIME_TTL_MS;
    let plaintext =
        insert_token(pool, TokenKind::Runtime, Some(project_id), None, Some(expires_at), now)
            .await?;
    Ok(ProjectRuntimeToken { plaintext, expires_at, rotated_out })
}

/// Revoke the project's live runtime credentials that are not backing a run —
/// the explicit revocation F1 found missing. Returns how many died.
pub async fn revoke_project_runtime(
    pool: &SqlitePool,
    project_id: &str,
    now: i64,
) -> anyhow::Result<u64> {
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ?
         WHERE kind = 'runtime' AND project_id = ? AND run_id IS NULL AND revoked_at IS NULL",
        now,
        project_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Revoke every credential whose expiry has passed. [`lookup_active`] already
/// refuses an expired token, so this changes no authorization decision — it
/// is hygiene: without it an aged-out token sits in the table as
/// `revoked_at IS NULL` forever and "zero live runtime tokens that are not
/// backing a running run" stops being checkable by looking (F1). The lease
/// sweeper runs it on its own schedule.
pub async fn revoke_expired(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ?
         WHERE revoked_at IS NULL AND expires_at IS NOT NULL AND expires_at <= ?",
        now,
        now
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// One live runtime credential and what binds it.
#[derive(Debug, Clone)]
pub struct LiveRuntimeToken {
    pub project_id: String,
    pub run_id: Option<String>,
    pub expires_at: Option<i64>,
}

/// Every runtime credential still live. The hygiene read behind F1: after a
/// full walk this holds nothing but the tokens of currently-running runs.
pub async fn live_runtime_tokens(pool: &SqlitePool) -> anyhow::Result<Vec<LiveRuntimeToken>> {
    let rows = sqlx::query!(
        "SELECT project_id, run_id, expires_at FROM token
         WHERE kind = 'runtime' AND revoked_at IS NULL ORDER BY id"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LiveRuntimeToken {
            project_id: r.project_id.expect("CHECK: runtime tokens carry a project"),
            run_id: r.run_id,
            expires_at: r.expires_at,
        })
        .collect())
}

/// Revoke every credential bound to a run. Called from the same transaction
/// that terminalizes the run, so a token cannot outlive the work it was
/// issued for (S2).
pub async fn revoke_for_run<'e, E>(executor: E, run_id: &str, now: i64) -> anyhow::Result<u64>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ? WHERE run_id = ? AND revoked_at IS NULL",
        now,
        run_id
    )
    .execute(executor)
    .await?;
    Ok(res.rows_affected())
}

/// Resolve a presented plaintext to an identity. Claim tokens are not
/// identities — they are consumed by [`consume_claim`] only. A token past its
/// expiry resolves to [`TokenLookup::Expired`] whether or not the sweeper has
/// got to it yet: the clock ends it, not the cleanup (F1).
pub async fn lookup_active(
    pool: &SqlitePool,
    plaintext: &str,
    now: i64,
) -> anyhow::Result<TokenLookup> {
    let token_hash = hash(plaintext);
    let row = sqlx::query!(
        "SELECT kind, project_id, run_id, expires_at FROM token
         WHERE token_hash = ? AND revoked_at IS NULL",
        token_hash
    )
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(TokenLookup::Unknown);
    };
    if r.expires_at.is_some_and(|e| e <= now) {
        return Ok(TokenLookup::Expired);
    }
    Ok(match r.kind.as_str() {
        "session" => TokenLookup::Active(Identity::Human),
        "runtime" => TokenLookup::Active(Identity::Runtime {
            project_id: r.project_id.expect("CHECK: runtime tokens carry a project"),
            run_id: r.run_id,
        }),
        _ => TokenLookup::Unknown,
    })
}

/// One-time claim consumption (INV-AUTH-5): atomic revoke-if-active, so a
/// second visit — or a race — gets `false`.
pub async fn consume_claim<'e, E>(executor: E, plaintext: &str, now: i64) -> anyhow::Result<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let token_hash = hash(plaintext);
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ?
         WHERE token_hash = ? AND kind = 'claim' AND revoked_at IS NULL",
        now,
        token_hash
    )
    .execute(executor)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn has_active_session(pool: &SqlitePool) -> anyhow::Result<bool> {
    let row = sqlx::query!(
        "SELECT COUNT(*) AS n FROM token WHERE kind = 'session' AND revoked_at IS NULL"
    )
    .fetch_one(pool)
    .await?;
    Ok(row.n > 0)
}

/// Revoke one token by its plaintext. Used when a credential is minted for a
/// spawn that then fails: an undelivered runtime token must not outlive the
/// dispatch that needed it (INV-AUTH-4; smoke walk 3, N1). Returns false if
/// it was already unknown or revoked.
pub async fn revoke(pool: &SqlitePool, plaintext: &str, now: i64) -> anyhow::Result<bool> {
    let token_hash = hash(plaintext);
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ? WHERE token_hash = ? AND revoked_at IS NULL",
        now,
        token_hash
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Revoke every active token of one kind (rotation; sign-everyone-out).
pub async fn revoke_all(pool: &SqlitePool, kind: TokenKind, now: i64) -> anyhow::Result<u64> {
    let kind_s = kind.as_str();
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ? WHERE kind = ? AND revoked_at IS NULL",
        now,
        kind_s
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
