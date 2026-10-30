//! Mode transportable : aucun secret DPAPI, aucune clé dans les arguments ou l’environnement.
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use zeroize::Zeroizing;
struct ContextKey {
    root: PathBuf,
    password: Zeroizing<String>,
    key: Mutex<Zeroizing<Vec<u8>>>,
}
static PORTABLE: OnceLock<ContextKey> = OnceLock::new();
#[derive(Serialize, Deserialize)]
struct Envelope {
    format: u32,
    salt: String,
    sealed: String,
}
pub fn active() -> bool {
    PORTABLE.get().is_some()
}
pub fn root() -> Option<PathBuf> {
    PORTABLE.get().map(|c| c.root.clone())
}
pub fn applies(path: &Path) -> bool {
    PORTABLE.get().is_some_and(|c| path.starts_with(&c.root))
}
pub fn wrap(key: &[u8], password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let derived = crate::crypto::derive_backup_key(password, &salt)?;
    Ok(serde_json::to_vec(&Envelope {
        format: 1,
        salt: B64.encode(salt),
        sealed: B64.encode(crate::crypto::seal(
            &derived,
            key,
            b"erp.portable.master.v1",
        )?),
    })?)
}
pub fn unwrap(data: &[u8], password: &str) -> Result<Zeroizing<Vec<u8>>> {
    if data.len() > 8192 {
        bail!("Enveloppe portable trop volumineuse.")
    }
    let e: Envelope = serde_json::from_slice(data)?;
    if e.format != 1 {
        bail!("Format portable inconnu.")
    }
    let salt = B64.decode(e.salt)?;
    if salt.len() != 16 {
        bail!("Sel portable invalide.")
    }
    let k = crate::crypto::derive_backup_key(password, &salt)?;
    let key = crate::crypto::unseal(&k, &B64.decode(e.sealed)?, b"erp.portable.master.v1")?;
    if key.len() != 32 {
        bail!("Clé portable invalide.")
    }
    Ok(key)
}
pub fn prepare(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let c = PORTABLE.get().context("Mode portable non déverrouillé")?;
    let key = unwrap(&std::fs::read(path.join("master.portable"))?, &c.password)?;
    *c.key
        .lock()
        .map_err(|_| anyhow::anyhow!("Verrou portable indisponible"))? = key.clone();
    Ok(key)
}
pub fn stage_key(path: &Path, key: &[u8]) -> Result<()> {
    let c = PORTABLE.get().context("Mode portable non déverrouillé")?;
    crate::db::atomic_write(&path.join("master.portable"), &wrap(key, &c.password)?)
}
pub fn seal_anchor(data: &[u8]) -> Result<Vec<u8>> {
    let c = PORTABLE.get().context("Mode portable non déverrouillé")?;
    let k = c
        .key
        .lock()
        .map_err(|_| anyhow::anyhow!("Verrou portable indisponible"))?;
    crate::crypto::seal(&k, data, b"erp.portable.audit.v1")
}
pub fn unlock_console() -> Result<()> {
    let folder = std::env::current_exe()?
        .parent()
        .context("Dossier de l’application absent")?
        .to_owned();
    if !folder.join("portable.json").is_file() {
        bail!("Le fichier portable.json doit accompagner cette application.")
    }
    #[cfg(windows)]
    {
        let raw = folder.to_string_lossy();
        if raw.starts_with("\\\\") {
            bail!("Choisissez un disque local ou une clé USB, pas un partage réseau.")
        }
        let drive: Vec<u16> = format!("{}\\", &raw[..2])
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(drive.as_ptr()) } == 4 {
            bail!("Le mode portable nécessite un disque local ou une clé USB.")
        }
    }
    let root = folder.join("Donnees");
    std::fs::create_dir_all(&root)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("process.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("Ces données sont déjà ouvertes. Fermez l’autre fenêtre.")?;
    crate::backup::apply_pending(&root)?;
    println!(
        "LIBRE ERP — ESPACE PORTABLE\nDonnées : {}\nNe retirez pas la clé pendant l’utilisation.\n",
        root.display()
    );
    let path = root.join("master.portable");
    let password = Zeroizing::new(rpassword::prompt_password(if path.exists() {
        "Mot de passe de déverrouillage : "
    } else {
        "Créez un mot de passe de déverrouillage (12 caractères minimum) : "
    })?);
    crate::crypto::validate_password(&password)?;
    let key = if path.exists() {
        unwrap(&std::fs::read(&path)?, &password)?
    } else {
        if root.join("db/registre.db").exists() || root.join("master.dpapi").exists() {
            bail!("Données existantes non portables : utilisez la sauvegarde/restauration pour les transférer.")
        }
        let confirm = Zeroizing::new(rpassword::prompt_password("Confirmez le mot de passe : ")?);
        if *confirm != *password {
            bail!("Les mots de passe diffèrent.")
        }
        let key = crate::crypto::random_key();
        crate::db::atomic_write(&path, &wrap(&key, &password)?)?;
        key
    };
    PORTABLE
        .set(ContextKey {
            root,
            password,
            key: Mutex::new(key),
        })
        .map_err(|_| anyhow::anyhow!("Mode déjà initialisé"))?;
    drop(lock);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_envelope_is_independent_and_authenticated() {
        let k = crate::crypto::random_key();
        let p = "Phrase transportable 123!";
        let a = wrap(&k, p).unwrap();
        assert_eq!(&*unwrap(&a, p).unwrap(), &*k);
        assert!(unwrap(&a, "Mauvaise phrase 123!").is_err());
        let mut e: Envelope = serde_json::from_slice(&a).unwrap();
        let mut b = B64.decode(&e.sealed).unwrap();
        b[15] ^= 1;
        e.sealed = B64.encode(b);
        assert!(unwrap(&serde_json::to_vec(&e).unwrap(), p).is_err());
    }

    #[tokio::test]
    async fn installed_backup_restores_to_portable_and_moves_without_dpapi() {
        let installed_root =
            std::env::temp_dir().join(format!("erp-source-portable-{}", crate::db::id()));
        let mut installed = crate::initialise(installed_root.clone()).await.unwrap();
        crate::auth::bootstrap(
            &installed,
            "Commune portable",
            "Admin",
            "admin",
            "Phrase utilisateur test 123",
        )
        .await
        .unwrap();
        let archive = crate::backup::create(
            &mut installed,
            None,
            "Phrase sauvegarde test 123",
            &installed_root.join("backups"),
            "manual",
        )
        .await
        .unwrap();
        let root = std::env::temp_dir().join(format!("erp-portable-{}", crate::db::id()));
        std::fs::create_dir_all(&root).unwrap();
        let password = Zeroizing::new("Phrase USB test 123!".to_string());
        let key = crate::crypto::random_key();
        crate::db::atomic_write(
            &root.join("master.portable"),
            &wrap(&key, &password).unwrap(),
        )
        .unwrap();
        assert!(PORTABLE
            .set(ContextKey {
                root: root.clone(),
                password,
                key: Mutex::new(key)
            })
            .is_ok());
        crate::db::prepare(&root).unwrap();
        let rid = crate::backup::validate_restore(
            &root,
            Path::new(&archive),
            "Phrase sauvegarde test 123",
        )
        .await
        .unwrap();
        crate::backup::request_restore(&root, &rid).unwrap();
        crate::backup::apply_pending(&root).unwrap();
        let portable = crate::initialise(root.clone()).await.unwrap();
        crate::auth::login(&portable, "admin", "Phrase utilisateur test 123", None)
            .await
            .unwrap();
        assert!(!root.join("master.dpapi").exists());
        crate::audit::verify(&portable.pool, &portable.key, &root)
            .await
            .unwrap();
        portable.pool.close().await;
        drop(portable);
        let moved = root.join("autre-disque");
        std::fs::create_dir_all(moved.join("db")).unwrap();
        std::fs::create_dir_all(moved.join("files")).unwrap();
        for file in ["db/registre.db", "master.portable", "audit.dpapi"] {
            std::fs::copy(root.join(file), moved.join(file)).unwrap();
        }
        let reopened = crate::initialise(moved.clone()).await.unwrap();
        crate::auth::login(&reopened, "admin", "Phrase utilisateur test 123", None)
            .await
            .unwrap();
        crate::audit::verify(&reopened.pool, &reopened.key, &moved)
            .await
            .unwrap();
        assert!(!moved.join("master.dpapi").exists());
        reopened.pool.close().await;
        installed.pool.close().await;
        drop(reopened);
        drop(installed);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(installed_root).unwrap();
    }
}
