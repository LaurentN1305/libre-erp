//! Pièces jointes : stockage opaque et chiffré, utilisable par M2/M6.
use crate::{crypto, db, Core};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use sqlx::Row;

#[allow(dead_code)]
pub async fn put(
    core: &Core,
    organisation: &str,
    actor: &str,
    name: &str,
    mime: &str,
    plaintext: &[u8],
) -> Result<String> {
    if plaintext.len() > 256 * 1024 * 1024 {
        bail!("La pièce jointe dépasse 256 Mio.");
    }
    if name.is_empty() || name.len() > 255 || name.contains(['\\', '/', '\0']) {
        bail!("Nom de fichier invalide.");
    }
    let id = db::id();
    let storage = format!("{id}.blob");
    let key = crypto::random_key();
    let sealed = crypto::seal(&key, plaintext, id.as_bytes())?;
    let envelope = crypto::seal(&core.key, &key, format!("file:{id}").as_bytes())?;
    db::atomic_write(&core.root.join("files").join(&storage), &sealed)?;
    let result=sqlx::query("INSERT INTO files(id,organisation_id,storage_key,original_name,mime_type,plaintext_sha256,byte_size,encryption_version,nonce,key_envelope,created_at,created_by) VALUES(?,?,?,?,?,?,?,1,?,?,?,?)")
        .bind(&id).bind(organisation).bind(&storage).bind(name).bind(mime).bind(hex::encode(Sha256::digest(plaintext))).bind(plaintext.len() as i64).bind(&sealed[..12]).bind(envelope).bind(db::now()).bind(actor).execute(&core.pool).await;
    if let Err(e) = result {
        std::fs::remove_file(core.root.join("files").join(storage))?;
        return Err(e.into());
    }
    Ok(id)
}
#[allow(dead_code)]
pub async fn read(
    core: &Core,
    id: &str,
    organisation: &str,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let row=sqlx::query("SELECT storage_key,key_envelope,plaintext_sha256 FROM files WHERE id=? AND organisation_id=?").bind(id).bind(organisation).fetch_one(&core.pool).await?;
    let storage: String = row.get(0);
    // Ne jamais interpréter une clé de stockage provenant de la DB comme un chemin libre.
    if storage != format!("{id}.blob") || uuid::Uuid::parse_str(id).is_err() {
        bail!("Référence de stockage invalide.");
    }
    let key = crypto::unseal(
        &core.key,
        &row.get::<Vec<u8>, _>(1),
        format!("file:{id}").as_bytes(),
    )?;
    let bytes = crypto::unseal(
        &key,
        &std::fs::read(core.root.join("files").join(storage))?,
        id.as_bytes(),
    )?;
    if hex::encode(Sha256::digest(&*bytes)) != row.get::<String, _>(2) {
        bail!("Pièce jointe altérée.");
    }
    Ok(bytes)
}
