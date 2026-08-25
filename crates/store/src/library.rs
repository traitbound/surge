//! Library item repository (INV-DATA-2: immutable per version — insert and
//! read; publishing a new version is a new row). Trust transitions are the
//! only mutation (INV-AUTH-3).

use sqlx::SqlitePool;
use surge_domain::library::{LibraryItem, LibraryItemKind, TrustState};

fn parse_kind(s: &str) -> LibraryItemKind {
    match s {
        "hook" => LibraryItemKind::Hook,
        "subagent" => LibraryItemKind::Subagent,
        _ => LibraryItemKind::Skill,
    }
}

fn trust_columns(t: &TrustState) -> (&'static str, Option<String>, Option<i64>) {
    match t {
        TrustState::Local => ("local", None, None),
        TrustState::ImportedUntrusted => ("imported_untrusted", None, None),
        TrustState::ImportedReviewed { by, at } => ("imported_reviewed", Some(by.clone()), Some(*at)),
    }
}

fn parse_trust(s: &str, by: Option<String>, at: Option<i64>) -> TrustState {
    match s {
        "imported_untrusted" => TrustState::ImportedUntrusted,
        "imported_reviewed" => TrustState::ImportedReviewed {
            by: by.expect("CHECK enforces reviewer"),
            at: at.expect("CHECK enforces review time"),
        },
        _ => TrustState::Local,
    }
}

pub async fn insert(pool: &SqlitePool, item: &LibraryItem) -> anyhow::Result<()> {
    let kind = item.kind.as_str();
    let (trust, by, at) = trust_columns(&item.trust);
    sqlx::query!(
        "INSERT INTO library_item (id, kind, name, version, body, trust,
                                   trust_reviewed_by, trust_reviewed_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        item.id,
        kind,
        item.name,
        item.version,
        item.body,
        trust,
        by,
        at,
        item.created_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(
    pool: &SqlitePool,
    kind: LibraryItemKind,
    name: &str,
    version: i64,
) -> anyhow::Result<Option<LibraryItem>> {
    let kind_s = kind.as_str();
    let row = sqlx::query!(
        "SELECT id, kind, name, version, body, trust, trust_reviewed_by, trust_reviewed_at, created_at
         FROM library_item WHERE kind = ? AND name = ? AND version = ?",
        kind_s,
        name,
        version
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| LibraryItem {
        id: r.id,
        kind: parse_kind(&r.kind),
        name: r.name,
        version: r.version,
        body: r.body,
        trust: parse_trust(&r.trust, r.trust_reviewed_by, r.trust_reviewed_at),
        created_at: r.created_at,
    }))
}
