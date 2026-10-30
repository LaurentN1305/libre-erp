use anyhow::{bail, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::path::Path;

fn canonical(
    sequence: i64,
    id: &str,
    time: &str,
    actor: Option<&str>,
    action: &str,
    payload: &str,
    prev: &[u8],
) -> Vec<u8> {
    serde_json::to_vec(&(
        sequence,
        id,
        time,
        actor,
        action,
        payload,
        hex::encode(prev),
    ))
    .expect("Tuple serializable")
}
pub async fn append(
    conn: &mut SqliteConnection,
    key: &[u8],
    actor: Option<&str>,
    action: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let last =
        sqlx::query("SELECT sequence,event_hash FROM audit_events ORDER BY sequence DESC LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;
    let (seq, prev) = last
        .map(|r| (r.get::<i64, _>(0) + 1, r.get::<Vec<u8>, _>(1)))
        .unwrap_or((1, vec![0; 32]));
    let id = crate::db::id();
    let time = crate::db::now();
    let payload = payload.to_string();
    let body = canonical(seq, &id, &time, actor, action, &payload, &prev);
    let digest = Sha256::digest(&body).to_vec();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)?;
    mac.update(&body);
    let tag = mac.finalize().into_bytes().to_vec();
    sqlx::query("INSERT INTO audit_events(sequence,id,occurred_at,actor_id,action,payload_json,previous_hash,event_hash,hmac) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(seq).bind(id).bind(time).bind(actor).bind(action).bind(payload).bind(prev).bind(digest).bind(tag).execute(conn).await?;
    Ok(())
}
pub async fn verify(pool: &SqlitePool, key: &[u8], root: &Path) -> Result<()> {
    let rows=sqlx::query("SELECT sequence,id,occurred_at,actor_id,action,payload_json,previous_hash,event_hash,hmac FROM audit_events ORDER BY sequence").fetch_all(pool).await?;
    let mut prev = vec![0u8; 32];
    let mut seq = 0i64;
    for r in rows {
        let n: i64 = r.get(0);
        let id: String = r.get(1);
        let time: String = r.get(2);
        let actor: Option<String> = r.get(3);
        let action: String = r.get(4);
        let payload: String = r.get(5);
        let previous: Vec<u8> = r.get(6);
        let hash: Vec<u8> = r.get(7);
        let tag: Vec<u8> = r.get(8);
        if n != seq + 1 || previous != prev {
            bail!("Chaîne d’audit interrompue.");
        }
        let body = canonical(n, &id, &time, actor.as_deref(), &action, &payload, &prev);
        if Sha256::digest(&body).as_slice() != hash {
            bail!("Empreinte d’audit invalide.");
        }
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)?;
        mac.update(&body);
        mac.verify_slice(&tag)
            .map_err(|_| anyhow::anyhow!("Signature d’audit invalide"))?;
        prev = hash;
        seq = n;
    }
    let anchor = root.join("audit.dpapi");
    if anchor.exists() {
        let bytes = std::fs::read(&anchor)?;
        let saved = if crate::portable::applies(root) {
            crate::crypto::unseal(key, &bytes, b"erp.portable.audit.v1")?
        } else {
            crate::crypto::unprotect(&bytes)?
        };
        let (old_seq, old_hash): (i64, String) = serde_json::from_slice(&saved)?;
        if old_seq > seq {
            bail!("Le journal d’audit a été tronqué.");
        }
        if old_seq > 0 {
            let hash: Option<Vec<u8>> =
                sqlx::query_scalar("SELECT event_hash FROM audit_events WHERE sequence=?")
                    .bind(old_seq)
                    .fetch_optional(pool)
                    .await?;
            if hash.map(hex::encode).as_deref() != Some(old_hash.as_str()) {
                bail!("Le point de contrôle d’audit ne correspond pas.");
            }
        }
    }
    checkpoint_value(pool, root, Some(key)).await
}
pub async fn checkpoint(pool: &SqlitePool, root: &Path) -> Result<()> {
    checkpoint_value(pool, root, None).await
}
async fn checkpoint_value(pool: &SqlitePool, root: &Path, key: Option<&[u8]>) -> Result<()> {
    let row =
        sqlx::query("SELECT sequence,event_hash FROM audit_events ORDER BY sequence DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    let value = row
        .map(|r| (r.get::<i64, _>(0), hex::encode(r.get::<Vec<u8>, _>(1))))
        .unwrap_or((0, hex::encode([0u8; 32])));
    crate::db::atomic_write(
        &root.join("audit.dpapi"),
        &if crate::portable::applies(root) {
            if let Some(k) = key {
                crate::crypto::seal(k, &serde_json::to_vec(&value)?, b"erp.portable.audit.v1")?
            } else {
                crate::portable::seal_anchor(&serde_json::to_vec(&value)?)?
            }
        } else {
            crate::crypto::protect(&serde_json::to_vec(&value)?)?
        },
    )
}
