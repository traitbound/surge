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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Human,
    Runtime { project_id: String },
}

pub fn hash(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

fn generate(kind: TokenKind) -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{}{}", kind.prefix(), hex::encode(bytes))
}

/// Mint a token, store its hash, return the plaintext (the only time it exists).
pub async fn mint(
    pool: &SqlitePool,
    kind: TokenKind,
    project_id: Option<&str>,
    now: i64,
) -> anyhow::Result<String> {
    mint_for_run(pool, kind, project_id, None, now).await
}

/// Mint a credential bound to one run. The binding is what makes revocation
/// possible at all: the supervisor discards the plaintext at spawn, so a
/// token with no `run_id` can never be revoked by the lifecycle that created
/// it (smoke walk 4, S2).
pub async fn mint_for_run(
    pool: &SqlitePool,
    kind: TokenKind,
    project_id: Option<&str>,
    run_id: Option<&str>,
    now: i64,
) -> anyhow::Result<String> {
    debug_assert_eq!(kind == TokenKind::Runtime, project_id.is_some());
    let plaintext = generate(kind);
    let token_hash = hash(&plaintext);
    let kind_s = kind.as_str();
    sqlx::query!(
        "INSERT INTO token (kind, token_hash, project_id, run_id, created_at)
         VALUES (?, ?, ?, ?, ?)",
        kind_s,
        token_hash,
        project_id,
        run_id,
        now
    )
    .execute(pool)
    .await?;
    Ok(plaintext)
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
/// identities — they are consumed by [`consume_claim`] only.
pub async fn lookup_active(pool: &SqlitePool, plaintext: &str) -> anyhow::Result<Option<Identity>> {
    let token_hash = hash(plaintext);
    let row = sqlx::query!(
        "SELECT kind, project_id FROM token WHERE token_hash = ? AND revoked_at IS NULL",
        token_hash
    )
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some(r) if r.kind == "session" => Some(Identity::Human),
        Some(r) if r.kind == "runtime" => Some(Identity::Runtime {
            project_id: r.project_id.expect("CHECK: runtime tokens carry a project"),
        }),
        _ => None,
    })
}

/// One-time claim consumption (INV-AUTH-5): atomic revoke-if-active, so a
/// second visit — or a race — gets `false`.
pub async fn consume_claim(pool: &SqlitePool, plaintext: &str, now: i64) -> anyhow::Result<bool> {
    let token_hash = hash(plaintext);
    let res = sqlx::query!(
        "UPDATE token SET revoked_at = ?
         WHERE token_hash = ? AND kind = 'claim' AND revoked_at IS NULL",
        now,
        token_hash
    )
    .execute(pool)
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
