//! Clichés chiffrés, un rédacteur, versions immuables. Aucun accès SQL distant.
use crate::{
    auth, backup, crypto, db,
    sftp_transport::{Connection, Remote},
    Core, Reply, Shared,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::State;
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub directory: String,
    pub fingerprint: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub phrase: String,
}
impl Drop for Config {
    fn drop(&mut self) {
        self.password.zeroize();
        self.phrase.zeroize();
    }
}
impl Config {
    pub fn validate(&self, authenticated: bool) -> Result<()> {
        if self.host.is_empty()
            || self.host.len() > 253
            || self.host.contains(['/', '\\', '@', '\0', ' ', '\n'])
            || self.port == 0
        {
            bail!("Adresse ou port SFTP invalide");
        }
        if authenticated
            && (self.username.is_empty()
                || self.password.is_empty()
                || !self.fingerprint.starts_with("SHA256:"))
        {
            bail!("Renseignez le compte SFTP et confirmez l’empreinte SSH du serveur");
        }
        if self.username.len() > 255
            || self.password.len() > 4096
            || self.directory.len() > 1024
            || !self.directory.starts_with('/')
            || self.directory.contains(['\\', '\0'])
            || self.directory.split('/').any(|x| x == "..")
        {
            bail!("Utilisez un dossier SFTP absolu, sans ..");
        }
        if authenticated {
            crypto::validate_password(&self.phrase)?;
        }
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Revision {
    number: u64,
    digest: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Local {
    base: Option<Revision>,
    lease: Option<String>,
    owner: String,
    last_sync: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Owner {
    token: String,
    label: String,
    since: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Commit {
    format: u32,
    number: u64,
    parent: Option<Revision>,
    archive: String,
    sha256: String,
    size: u64,
    version: String,
    schema: i64,
    created_at: String,
}
#[derive(Serialize, Deserialize)]
struct Signed {
    payload: String,
    mac: String,
}
#[derive(Serialize)]
pub struct Status {
    pub enabled: bool,
    pub writable: bool,
    pub pending: bool,
    pub reserved: bool,
    pub revision: Option<u64>,
    pub last_sync: Option<String>,
    pub message: String,
}
pub struct Gate {
    root: PathBuf,
    pub enabled: AtomicBool,
    pub writable: AtomicBool,
    pub closing: AtomicBool,
}
impl Gate {
    pub fn new(root: &Path) -> Self {
        let enabled = root.join("sharing/config.dpapi").is_file();
        Self {
            root: root.into(),
            enabled: AtomicBool::new(enabled),
            writable: AtomicBool::new(!enabled),
            closing: AtomicBool::new(false),
        }
    }
    fn pending(&self) -> bool {
        self.root.join("sharing/pending").exists()
    }
    pub fn can_write(&self) -> bool {
        !self.enabled.load(Ordering::SeqCst) || self.writable.load(Ordering::SeqCst)
    }
}
pub fn require_write(c: &Core) -> Result<()> {
    if !c.sharing.can_write() {
        bail!("Espace partagé en consultation. Réservez les modifications dans Partage SFTP avant de travailler.");
    }
    if c.sharing.enabled.load(Ordering::SeqCst) {
        db::atomic_write(&c.root.join("sharing/pending"), b"pending-v1")?;
    }
    Ok(())
}
pub fn require_released(c: &Core) -> Result<()> {
    if c.sharing.enabled.load(Ordering::SeqCst)
        && (c.sharing.pending() || local(&c.root)?.lease.is_some())
    {
        bail!("Envoyez et libérez votre travail partagé avant d’installer la mise à jour.");
    }
    Ok(())
}
pub fn require_local(c: &Core) -> Result<()> {
    if c.sharing.enabled.load(Ordering::SeqCst) {
        bail!("Cette opération remplace toute la base. Revenez d’abord au mode local, après avoir envoyé le travail partagé.");
    }
    Ok(())
}
fn local(root: &Path) -> Result<Local> {
    let path = root.join("sharing/state.dpapi");
    if !path.exists() {
        return Ok(Local::default());
    }
    Ok(serde_json::from_slice(&crypto::unprotect(
        &std::fs::read(path)?,
    )?)?)
}
fn save_local(root: &Path, s: &Local) -> Result<()> {
    db::atomic_write(
        &root.join("sharing/state.dpapi"),
        &crypto::protect(&serde_json::to_vec(s)?)?,
    )
}
fn config(root: &Path) -> Result<Config> {
    Ok(serde_json::from_slice(&crypto::unprotect(
        &std::fs::read(root.join("sharing/config.dpapi"))?,
    )?)?)
}
fn supplied(root: &Path, mut input: Config) -> Result<Config> {
    if let Ok(old) = config(root) {
        // Ne réutilise les secrets qu’avec exactement le même hébergement et compte.
        if old.host == input.host
            && old.port == input.port
            && old.username == input.username
            && old.directory == input.directory
        {
            if input.password.is_empty() {
                input.password = old.password.clone();
            }
            if input.phrase.is_empty() {
                input.phrase = old.phrase.clone();
            }
        }
    }
    Ok(input)
}
fn status(c: &Core, text: &str) -> Result<Status> {
    let s = local(&c.root)?;
    Ok(Status {
        enabled: c.sharing.enabled.load(Ordering::SeqCst),
        writable: c.sharing.can_write(),
        pending: c.sharing.pending(),
        reserved: s.lease.is_some(),
        revision: s.base.map(|b| b.number),
        last_sync: s.last_sync,
        message: text.into(),
    })
}
fn mac_key(c: &Config) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    crypto::derive_backup_key(&c.phrase, b"LibreERP-SFTP-v1")
}
fn signed<T: Serialize>(value: &T, key: &[u8]) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    let mut h = Hmac::<Sha256>::new_from_slice(key)?;
    h.update(&bytes);
    Ok(serde_json::to_vec(&Signed {
        payload: B64.encode(bytes),
        mac: hex::encode(h.finalize().into_bytes()),
    })?)
}
fn verified<T: serde::de::DeserializeOwned>(bytes: &[u8], key: &[u8]) -> Result<T> {
    let s: Signed = serde_json::from_slice(bytes)?;
    let bytes = B64.decode(s.payload)?;
    let mut h = Hmac::<Sha256>::new_from_slice(key)?;
    h.update(&bytes);
    h.verify_slice(&hex::decode(s.mac)?)
        .context("Phrase de partage incorrecte ou manifeste distant altéré")?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn revision_name(n: u64) -> String {
    format!("versions/{n:020}.json")
}
fn head(r: &dyn Remote, key: &[u8]) -> Result<Option<(Revision, Commit)>> {
    let mut numbers = vec![];
    for name in r.list("versions")? {
        if name.len() == 25
            && name.ends_with(".json")
            && name.as_bytes()[..20].iter().all(u8::is_ascii_digit)
        {
            numbers.push(name[..20].parse::<u64>()?);
        }
    }
    let Some(n) = numbers.into_iter().max() else {
        return Ok(None);
    };
    if n == 0 {
        bail!("Numéro de version partagé invalide");
    }
    let bytes = r
        .read(&revision_name(n))?
        .context("Manifeste partagé absent")?;
    let c: Commit = verified(&bytes, key)?;
    if c.format != 1
        || c.number != n
        || c.schema > db::SCHEMA_VERSION
        || semver::Version::parse(&c.version)? > semver::Version::parse(env!("CARGO_PKG_VERSION"))?
    {
        bail!("Cette version partagée nécessite une mise à jour de Libre ERP");
    }
    uuid::Uuid::parse_str(
        c.archive
            .strip_suffix(".zip")
            .context("Nom d’archive invalide")?,
    )?;
    if c.size > crate::sftp_transport::MAX_ARCHIVE
        || c.sha256.len() != 64
        || !c.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("Manifeste partagé invalide");
    }
    Ok(Some((
        Revision {
            number: n,
            digest: hex::encode(Sha256::digest(&bytes)),
        },
        c,
    )))
}
fn same_base(base: &Option<Revision>, remote: &Option<(Revision, Commit)>) -> Result<()> {
    if base.as_ref() != remote.as_ref().map(|x| &x.0) {
        bail!("La version du serveur diffère. Les modifications locales sont conservées ; aucun écrasement n’a été effectué. Synchronisez avant de modifier.");
    }
    Ok(())
}
fn verify_ancestor(
    r: &dyn Remote,
    base: &Option<Revision>,
    remote: &Revision,
    key: &[u8],
) -> Result<()> {
    if let Some(base) = base {
        if remote.number < base.number {
            bail!("Le serveur présente une ancienne version. Synchronisation refusée.");
        }
        let bytes = r
            .read(&revision_name(base.number))?
            .context("L’historique partagé a disparu")?;
        let _: Commit = verified(&bytes, key)?;
        if hex::encode(Sha256::digest(&bytes)) != base.digest {
            bail!("L’historique partagé a été remplacé. Données locales conservées.");
        }
    }
    Ok(())
}
fn acquire(r: &dyn Remote, lease: &str, label: &str) -> Result<()> {
    if r.mkdir("reservation")? {
        r.put(
            "reservation/owner.json",
            &serde_json::to_vec(&Owner {
                token: lease.into(),
                label: label.into(),
                since: db::now(),
            })?,
        )?;
    }
    owns(r, lease)
}
fn owns(r: &dyn Remote, lease: &str) -> Result<()> {
    let bytes = r.read("reservation/owner.json")?.context(
        "Réservation absente ou interrompue. Intervention de l’administrateur nécessaire.",
    )?;
    let o: Owner = serde_json::from_slice(&bytes)?;
    if o.token != lease {
        bail!(
            "Modifications réservées par {} depuis {}. Vous pouvez consulter les données.",
            o.label,
            o.since
        );
    }
    Ok(())
}
fn release(r: &dyn Remote, lease: &str) -> Result<()> {
    owns(r, lease)?;
    r.remove("reservation/owner.json")?;
    r.rmdir("reservation")?;
    Ok(())
}
fn digest(path: &Path) -> Result<(u64, String)> {
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok((size, hex::encode(h.finalize())))
}
async fn network<T: Send + 'static>(
    cfg: Config,
    f: impl FnOnce(Connection, Config) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = Connection::connect(&cfg)?;
        f(r, cfg)
    })
    .await?
}
fn clear_pending(c: &Core) -> Result<()> {
    let p = c.root.join("sharing/pending");
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}
async fn publish(c: &mut Core) -> Result<()> {
    crate::not_downloading(c)?;
    c.sharing.writable.store(false, Ordering::SeqCst);
    let cfg = config(&c.root)?;
    let mut s = local(&c.root)?;
    let lease = s.lease.clone().context("Aucune réservation locale")?;
    let base = s.base.clone();
    let l = lease.clone();
    let (already, key, owned) = network(cfg.clone(), move |r, cfg| {
        let key = mac_key(&cfg)?;
        let owned = owns(&r, &l).is_ok();
        let h = head(&r, &key)?;
        Ok((h, key, owned))
    })
    .await?;
    if !c.sharing.pending() && !owned {
        s.lease = None;
        save_local(&c.root, &s)?;
        return Ok(());
    }
    if !owned {
        bail!("La réservation de ce poste a été perdue. Travail local conservé : exportez une sauvegarde avant toute récupération.");
    }
    // Reprise après envoi réussi mais coupure avant la sauvegarde de l’état local.
    let journal = c.root.join("sharing/upload.json");
    if journal.exists() {
        let pending: Revision = serde_json::from_slice(&std::fs::read(&journal)?)?;
        if already.as_ref().is_some_and(|h| h.0 == pending) {
            s.base = Some(pending);
            s.last_sync = Some(db::now());
            save_local(&c.root, &s)?;
            clear_pending(c)?;
        }
    }
    let result = async {
        same_base(&s.base, &already)?;
        if c.sharing.pending() {
            let destination = c.root.join("sharing/outgoing");
            let path =
                PathBuf::from(backup::create(c, None, &cfg.phrase, &destination, "manual").await?);
            let (size, hash) = digest(&path)?;
            let archive = format!("{}.zip", db::id());
            let number = base.as_ref().map_or(1, |x| x.number.saturating_add(1));
            let commit = Commit {
                format: 1,
                number,
                parent: s.base.clone(),
                archive: archive.clone(),
                sha256: hash,
                size,
                version: env!("CARGO_PKG_VERSION").into(),
                schema: db::SCHEMA_VERSION,
                created_at: db::now(),
            };
            let bytes = signed(&commit, &key)?;
            let rev = Revision {
                number,
                digest: hex::encode(Sha256::digest(&bytes)),
            };
            db::atomic_write(&journal, &serde_json::to_vec(&rev)?)?;
            let expected = s.base.clone();
            let l = lease.clone();
            let file = path.clone();
            let check = c
                .root
                .join("sharing")
                .join(format!("verify-{}.zip", db::id()));
            let check2 = check.clone();
            let confirmed_digest = rev.digest.clone();
            let uploaded = network(cfg.clone(), move |r, _| {
                owns(&r, &l)?;
                same_base(&expected, &head(&r, &key)?)?;
                let name = format!("archives/{archive}");
                r.upload(&name, &file)?;
                r.download(&name, &check, size)?;
                if digest(&check)?.1 != commit.sha256 {
                    bail!("Archive envoyée incomplète : version non publiée");
                }
                owns(&r, &l)?;
                same_base(&expected, &head(&r, &key)?)?;
                let temporary = format!("versions/{}.partial", db::id());
                r.put(&temporary, &bytes)?;
                r.publish(&temporary, &revision_name(number))?;
                let read = r
                    .read(&revision_name(number))?
                    .context("Publication non confirmée")?;
                if hex::encode(Sha256::digest(read)) != confirmed_digest {
                    bail!("La confirmation du serveur ne correspond pas à l’envoi");
                }
                Ok(())
            })
            .await;
            let _ = std::fs::remove_file(check2);
            uploaded?;
            s.base = Some(rev);
            s.last_sync = Some(db::now());
            save_local(&c.root, &s)?;
            clear_pending(c)?;
            // L’archive locale et toutes les versions distantes restent disponibles.
        }
        let l = lease.clone();
        network(cfg, move |r, _| release(&r, &l)).await?;
        s.lease = None;
        save_local(&c.root, &s)?;
        if journal.exists() {
            std::fs::remove_file(journal)?;
        }
        Ok(())
    }
    .await;
    // Même en cas d’échec, aucune nouvelle modification avant revalidation de la réservation.
    result
}
async fn open(c: &mut Core) -> Result<Status> {
    crate::not_downloading(c)?;
    if !c.sharing.enabled.load(Ordering::SeqCst) {
        return status(c, "Travail local uniquement");
    }
    c.sharing.writable.store(false, Ordering::SeqCst);
    let cfg = config(&c.root)?;
    let mut s = local(&c.root)?;
    if c.sharing.pending() {
        return status(c,"Travail local en attente d’envoi. Connectez-vous pour reprendre la réservation et envoyer vos modifications.");
    }
    let base = s.base.clone();
    let incoming = c
        .root
        .join("sharing")
        .join(format!("incoming-{}.zip", db::id()));
    let target = incoming.clone();
    let remote = network(cfg.clone(), move |r, cfg| {
        let key = mac_key(&cfg)?;
        let h = head(&r, &key)?.context("Aucune version partagée sur ce serveur")?;
        verify_ancestor(&r, &base, &h.0, &key)?;
        if base.as_ref() != Some(&h.0) {
            r.download(&format!("archives/{}", h.1.archive), &target, h.1.size)?;
            if digest(&target)?.1 != h.1.sha256 {
                bail!("Archive distante altérée. Base locale conservée.");
            }
        }
        Ok(h)
    })
    .await?;
    if s.base.as_ref() != Some(&remote.0) {
        // Toute validation est effectuée avant de fermer/remplacer le pool courant.
        let rid = backup::validate_restore(&c.root, &incoming, &cfg.phrase).await?;
        c.pool.close().await;
        backup::request_restore(&c.root, &rid)?;
        backup::apply_pending(&c.root)?;
        c.key = db::prepare(&c.root)?;
        c.reopen().await?;
        db::migrate(&c.pool).await?;
        // Les téléchargements de mise à jour et planifications appartiennent au poste.
        sqlx::query("UPDATE update_state SET installed_version=?,phase='idle',candidate_version=NULL,signed_manifest_json=NULL,downloaded_path=NULL,artifact_sha256=NULL,manifest_verified_at=NULL,artifact_verified_at=NULL,previous_installer_path=NULL,pre_update_backup_id=NULL,last_error=NULL WHERE singleton=1").bind(env!("CARGO_PKG_VERSION")).execute(&c.pool).await?;
        sqlx::query("DELETE FROM settings WHERE key IN ('backup.schedule','backup.last_error')")
            .execute(&c.pool)
            .await?;
        crate::audit::verify(&c.pool, &c.key, &c.root).await?;
        s.base = Some(remote.0);
        s.last_sync = Some(db::now());
        s.lease = None;
        save_local(&c.root, &s)?;
    }
    let _ = std::fs::remove_file(incoming);
    status(
        c,
        "Dernière version récupérée. Connectez-vous pour consulter ou réserver les modifications.",
    )
}

#[tauri::command]
pub async fn shared_status(state: State<'_, Shared>) -> Reply<Status> {
    crate::response(status(&*state.lock().await, ""))
}
#[tauri::command]
pub async fn shared_settings(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    crate::response(async{
    let c=state.lock().await;auth::require(&c,&token,Some("backup.manage")).await?;
    Ok(match config(&c.root){Ok(v)=>serde_json::json!({"host":v.host,"port":v.port,"username":v.username,"directory":v.directory,"fingerprint":v.fingerprint}),Err(_)=>serde_json::json!({"host":"","port":22,"username":"","directory":"/libre-erp","fingerprint":""})})
}.await)
}
#[tauri::command]
pub async fn shared_probe(state: State<'_, Shared>, token: String, input: Config) -> Reply<String> {
    crate::response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            tauri::async_runtime::spawn_blocking(move || crate::sftp_transport::probe(&input))
                .await?
        }
        .await,
    )
}
#[tauri::command]
pub async fn shared_test(state: State<'_, Shared>, token: String, input: Config) -> Reply<String> {
    crate::response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            let cfg = supplied(&c.root, input)?;
            network(cfg, |r, _| {
                r.test()?;
                Ok(
                    "Connexion, lecture, écriture et protections contre l’écrasement vérifiées."
                        .into(),
                )
            })
            .await
        }
        .await,
    )
}
async fn update_connection(c: &Core, input: Config) -> Result<Status> {
    let old = config(&c.root)?;
    let mut cfg = supplied(&c.root, input)?;
    if cfg.phrase.is_empty() {
        cfg.phrase = old.phrase.clone();
    }
    let base = local(&c.root)?.base;
    let journal = c.root.join("sharing/upload.json");
    let expected = if journal.exists() {
        Some(serde_json::from_slice::<Revision>(&std::fs::read(
            journal,
        )?)?)
    } else {
        None
    };
    network(cfg.clone(), move |r, cfg| {
        r.test()?;
        let key = mac_key(&cfg)?;
        match (base, head(&r, &key)?) {
            (Some(base), Some((rev, _))) => verify_ancestor(&r, &Some(base), &rev, &key)?,
            (Some(_), None) => {
                bail!("Cet hébergement ne contient pas l’historique de votre espace partagé")
            }
            (None, Some((rev, _))) if expected.as_ref() != Some(&rev) => {
                bail!("Cet espace ne correspond pas à la création en attente sur ce PC")
            }
            _ => {}
        }
        Ok(())
    })
    .await?;
    db::atomic_write(
        &c.root.join("sharing/config.dpapi"),
        &crypto::protect(&serde_json::to_vec(&cfg)?)?,
    )?;
    status(
        c,
        "Connexion enregistrée. Le travail local et la réservation sont conservés.",
    )
}
#[tauri::command]
pub async fn shared_connection_save(
    state: State<'_, Shared>,
    token: String,
    input: Config,
) -> Reply<Status> {
    crate::response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            update_connection(&c, input).await
        }
        .await,
    )
}

#[tauri::command]
pub async fn shared_configure(
    state: State<'_, Shared>,
    token: String,
    input: Config,
    action: String,
) -> Reply<Status> {
    crate::response(
        async {
            if crate::portable::active(){bail!("Le partage SFTP nécessite la version installée de Libre ERP. La version portable conserve son fonctionnement local.");}
            let mut c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            if c.sharing.pending() || local(&c.root)?.lease.is_some() {
                bail!("Envoyez et libérez le travail actuel avant de changer d’espace partagé");
            }
            crate::not_downloading(&c)?;
            if action != "create" && action != "join" {
                bail!("Choisissez créer ou rejoindre");
            }
            let cfg = supplied(&c.root, input)?;
            cfg.validate(true)?;
            let creating = action == "create";
            network(cfg.clone(), move |r, cfg| {
                r.test()?;
                r.mkdir("archives")?;
                r.mkdir("versions")?;
                let h = head(&r, &mac_key(&cfg)?)?;
                if creating && h.is_some() {
                    bail!("Ce dossier contient déjà un espace partagé. Choisissez Rejoindre.");
                }
                if !creating && h.is_none() {
                    bail!("Aucune version partagée à rejoindre dans ce dossier");
                }
                Ok(())
            })
            .await?;
            std::fs::create_dir_all(c.root.join("sharing"))?;
            save_local(&c.root, &Local::default())?;
            db::atomic_write(
                &c.root.join("sharing/config.dpapi"),
                &crypto::protect(&serde_json::to_vec(&cfg)?)?,
            )?;
            c.sharing.enabled.store(true, Ordering::SeqCst);
            c.sharing.writable.store(false, Ordering::SeqCst);
            if creating {
                let lease = db::id();
                let mut s = local(&c.root)?;
                s.lease = Some(lease.clone());
                s.owner = "Création de l’espace partagé".into();
                save_local(&c.root, &s)?;
                network(cfg, move |r, _| {
                    acquire(&r, &lease, "Création de l’espace partagé")
                })
                .await?;
                db::atomic_write(&c.root.join("sharing/pending"), b"initial-v1")?;
                publish(&mut c).await?;
                status(
                    &c,
                    "Espace partagé créé. Les comptes et documents sont inclus.",
                )
            } else {
                open(&mut c).await
            }
        }
        .await,
    )
}
#[tauri::command]
pub async fn shared_open(state: State<'_, Shared>) -> Reply<Status> {
    crate::response(open(&mut *state.lock().await).await)
}
#[tauri::command]
pub async fn shared_edit(state: State<'_, Shared>, token: String) -> Reply<Status> {
    crate::response(async{
    let c=state.lock().await;let u=auth::require(&c,&token,None).await?;
    if !c.sharing.enabled.load(Ordering::SeqCst){return status(&c,"");}
    c.sharing.writable.store(false,Ordering::SeqCst);
    let user=auth::user(&c.pool,&u).await?;
    if user.roles.iter().all(|r|r=="commission"){return status(&c,"Consultation uniquement pour ce rôle");}
    let cfg=config(&c.root)?;let mut s=local(&c.root)?;let base=s.base.clone();
    let previous_lease=s.lease.clone();
    let lease=s.lease.clone().unwrap_or_else(db::id);s.lease=Some(lease.clone());s.owner=user.display_name.clone();
    // Persiste le jeton avant la réservation réseau pour reprendre après un arrêt brutal.
    save_local(&c.root,&s)?;
    let label=user.display_name;
    let result=network(cfg,move|r,cfg|{
        let key=mac_key(&cfg)?;same_base(&base,&head(&r,&key)?)?;
        if let Some(bytes)=r.read("reservation/owner.json")? {
            let o:Owner=serde_json::from_slice(&bytes)?;
            if o.token!=lease {return Ok(Some(format!("Modifications réservées par {} depuis {}. Consultation disponible.",o.label,o.since)));}
        }
        acquire(&r,&lease,&label)?;Ok(None)
    }).await;
    match result {Ok(Some(text))=>{s.lease=if c.sharing.pending(){previous_lease}else{None};save_local(&c.root,&s)?;c.sharing.writable.store(false,Ordering::SeqCst);status(&c,&text)},Ok(None)=>{c.sharing.writable.store(true,Ordering::SeqCst);status(&c,"Vous avez réservé les modifications. Pensez à envoyer votre travail avant de fermer.")},Err(e)=>{c.sharing.writable.store(false,Ordering::SeqCst);status(&c,&e.to_string())}}
}.await)
}
#[tauri::command]
pub async fn shared_publish(state: State<'_, Shared>, token: String) -> Reply<Status> {
    crate::response(
        async {
            let mut c = state.lock().await;
            auth::require(&c, &token, None).await?;
            publish(&mut c).await?;
            status(&c, "Travail envoyé et réservation libérée.")
        }
        .await,
    )
}
#[tauri::command]
pub async fn shared_disable(state: State<'_, Shared>, token: String) -> Reply<Status> {
    crate::response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            if c.sharing.pending() || local(&c.root)?.lease.is_some() {
                bail!("Envoyez et libérez d’abord le travail partagé");
            }
            let p = c.root.join("sharing/config.dpapi");
            if p.exists() {
                std::fs::remove_file(p)?;
            }
            c.sharing.enabled.store(false, Ordering::SeqCst);
            c.sharing.writable.store(true, Ordering::SeqCst);
            status(
                &c,
                "Mode local activé. La copie locale devient indépendante de l’espace partagé.",
            )
        }
        .await,
    )
}
#[tauri::command]
pub async fn shared_close(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    send: bool,
) -> Reply<()> {
    crate::response(
        async {
            let mut c = state.lock().await;
            // Fermer sans envoyer conserve le jeton et le travail ; aucun déverrouillage distant.
            if send && c.sharing.enabled.load(Ordering::SeqCst) && local(&c.root)?.lease.is_some() {
                publish(&mut c).await?;
            }
            c.sharing.closing.store(true, Ordering::SeqCst);
            drop(c);
            app.exit(0);
            Ok(())
        }
        .await,
    )
}

#[tauri::command]
pub async fn shared_reservation(
    state: State<'_, Shared>,
    token: String,
) -> Reply<serde_json::Value> {
    crate::response(async{
    let c=state.lock().await;auth::require(&c,&token,Some("backup.manage")).await?;
    network(config(&c.root)?,|r,_|{
        let bytes=r.read("reservation/owner.json")?;
        Ok(match bytes{Some(b)=>{let o:Owner=serde_json::from_slice(&b)?;serde_json::json!({"token":o.token,"label":o.label,"since":o.since})},None=>serde_json::json!({"token":"incomplete","label":"Réservation interrompue avant identification","since":"inconnue"})})
    }).await
}.await)
}
#[tauri::command]
pub async fn shared_release_abandoned(
    state: State<'_, Shared>,
    token: String,
    expected: String,
    confirmation: String,
) -> Reply<Status> {
    crate::response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            if confirmation != "LIBERER" {
                bail!("Saisissez LIBERER pour confirmer");
            }
            if c.sharing.pending() || c.sharing.writable.load(Ordering::SeqCst) {
                bail!("Ce poste contient du travail réservé. Envoyez-le avant cette opération.");
            }
            network(config(&c.root)?, move |r, _| {
                let actual = match r.read("reservation/owner.json")? {
                    Some(b) => serde_json::from_slice::<Owner>(&b)?.token,
                    None => "incomplete".into(),
                };
                if actual != expected {
                    bail!("La réservation a changé : relisez son état avant de continuer.");
                }
                // Archive le verrou au lieu de supprimer ses informations de reprise.
                r.publish(
                    "reservation",
                    &format!("reservation-abandonnee-{}", db::id()),
                )?;
                Ok(())
            })
            .await?;
            let mut s = local(&c.root)?;
            s.lease = None;
            save_local(&c.root, &s)?;
            status(
                &c,
                "Ancienne réservation archivée. Synchronisez avant de réserver à nouveau.",
            )
        }
        .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
    };
    #[derive(Default)]
    struct Memory {
        files: RefCell<BTreeMap<String, Vec<u8>>>,
        dirs: RefCell<BTreeSet<String>>,
    }
    impl Remote for Memory {
        fn read(&self, n: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.files.borrow().get(n).cloned())
        }
        fn list(&self, d: &str) -> Result<Vec<String>> {
            Ok(self
                .files
                .borrow()
                .keys()
                .filter_map(|n| n.strip_prefix(&format!("{d}/")).map(str::to_owned))
                .collect())
        }
        fn mkdir(&self, n: &str) -> Result<bool> {
            Ok(self.dirs.borrow_mut().insert(n.into()))
        }
        fn put(&self, n: &str, b: &[u8]) -> Result<()> {
            if self.files.borrow().contains_key(n) {
                bail!("exists")
            };
            self.files.borrow_mut().insert(n.into(), b.into());
            Ok(())
        }
        fn remove(&self, n: &str) -> Result<()> {
            self.files.borrow_mut().remove(n);
            Ok(())
        }
        fn rmdir(&self, n: &str) -> Result<()> {
            self.dirs.borrow_mut().remove(n);
            Ok(())
        }
        fn publish(&self, a: &str, b: &str) -> Result<()> {
            if self.files.borrow().contains_key(b) {
                bail!("exists")
            };
            let bytes = self.files.borrow_mut().remove(a).context("missing")?;
            self.files.borrow_mut().insert(b.into(), bytes);
            Ok(())
        }
        fn upload(&self, _: &str, _: &Path) -> Result<()> {
            unreachable!()
        }
        fn download(&self, _: &str, _: &Path, _: u64) -> Result<()> {
            unreachable!()
        }
    }
    fn commit(n: u64) -> Commit {
        Commit {
            format: 1,
            number: n,
            parent: None,
            archive: format!("{}.zip", db::id()),
            sha256: "a".repeat(64),
            size: 10,
            version: env!("CARGO_PKG_VERSION").into(),
            schema: db::SCHEMA_VERSION,
            created_at: db::now(),
        }
    }

    #[tokio::test]
    #[ignore = "Serveur SFTP de test éphémère : python tests/sftp-fixture-server.py"]
    async fn sftp_two_computers() {
        use sqlx::Row;
        let port = std::env::var("ERP_SFTP_FIXTURE_PORT")
            .expect("serveur de test absent")
            .parse()
            .unwrap();
        let cfg = Config {
            host: "127.0.0.1".into(),
            port,
            username: "fixture".into(),
            password: "fixture-secret".into(),
            directory: "/libre-erp".into(),
            fingerprint: std::env::var("ERP_SFTP_FIXTURE_FINGERPRINT").unwrap(),
            phrase: "Phrase commune fictive 2026!".into(),
        };
        let a = std::env::temp_dir().join(format!("libre-erp-sftp-pc-a-{}", db::id()));
        let b = std::env::temp_dir().join(format!("libre-erp-sftp-pc-b-{}", db::id()));
        let mut ca = crate::initialise(a.clone()).await.unwrap();
        let mut cb = crate::initialise(b.clone()).await.unwrap();
        let login = auth::bootstrap(
            &ca,
            "Commune fictive",
            "Alice Test",
            "alice",
            "Compte fictif 2026!",
        )
        .await
        .unwrap();
        let org: String = sqlx::query("SELECT organisation_id FROM users WHERE id=?")
            .bind(&login.user.id)
            .fetch_one(&ca.pool)
            .await
            .unwrap()
            .get(0);
        let file = crate::vault::put(
            &ca,
            &org,
            &login.user.id,
            "exemple.txt",
            "text/plain",
            b"Piece jointe fictive",
        )
        .await
        .unwrap();
        let mut wrong = cfg.clone();
        wrong.fingerprint = "SHA256:incorrect".into();
        assert!(Connection::connect(&wrong).is_err());
        network(cfg.clone(), |r, _| {
            r.test()?;
            r.mkdir("archives")?;
            r.mkdir("versions")?;
            Ok(())
        })
        .await
        .unwrap();
        let (one, two) = tokio::join!(
            network(cfg.clone(), |r, _| acquire(&r, "race-a", "Alice Test")),
            network(cfg.clone(), |r, _| acquire(&r, "race-b", "Bob Test"))
        );
        assert_ne!(
            one.is_ok(),
            two.is_ok(),
            "Un seul poste doit obtenir la réservation simultanée"
        );
        let winner = if one.is_ok() { "race-a" } else { "race-b" };
        network(cfg.clone(), move |r, _| release(&r, winner))
            .await
            .unwrap();
        for c in [&ca, &cb] {
            std::fs::create_dir_all(c.root.join("sharing")).unwrap();
            db::atomic_write(
                &c.root.join("sharing/config.dpapi"),
                &crypto::protect(&serde_json::to_vec(&cfg).unwrap()).unwrap(),
            )
            .unwrap();
            save_local(&c.root, &Local::default()).unwrap();
            c.sharing.enabled.store(true, Ordering::SeqCst);
            c.sharing.writable.store(false, Ordering::SeqCst);
        }
        let lease = db::id();
        let l = lease.clone();
        network(cfg.clone(), move |r, _| acquire(&r, &l, "Alice Test"))
            .await
            .unwrap();
        save_local(
            &ca.root,
            &Local {
                lease: Some(lease),
                owner: "Alice Test".into(),
                ..Default::default()
            },
        )
        .unwrap();
        db::atomic_write(&ca.root.join("sharing/pending"), b"initial").unwrap();
        publish(&mut ca).await.unwrap();
        assert!(!ca.sharing.pending());
        assert!(local(&a).unwrap().lease.is_none());
        open(&mut cb).await.unwrap();
        assert_eq!(local(&b).unwrap().base, local(&a).unwrap().base);
        assert_eq!(
            &*crate::vault::read(&cb, &file, &org).await.unwrap(),
            b"Piece jointe fictive"
        );
        let remote_user = auth::login(&cb, "alice", "Compte fictif 2026!", None)
            .await
            .unwrap();
        assert!(auth::create_user(
            &cb,
            &remote_user.token,
            "Interdit",
            "interdit",
            "Autre compte fictif!",
            "agent"
        )
        .await
        .is_err());
        let mut state = local(&a).unwrap();
        let lease = db::id();
        let l = lease.clone();
        network(cfg.clone(), move |r, _| acquire(&r, &l, "Alice Test"))
            .await
            .unwrap();
        state.lease = Some(lease);
        save_local(&a, &state).unwrap();
        ca.sharing.writable.store(true, Ordering::SeqCst);
        assert!(
            network(cfg.clone(), |r, _| acquire(&r, "poste-b", "Bob Test"))
                .await
                .is_err()
        );
        auth::create_user(
            &ca,
            &login.token,
            "Bob Test",
            "bob",
            "Compte Bob fictif!",
            "agent",
        )
        .await
        .unwrap();
        assert!(ca.sharing.pending());
        // Mot de passe hébergeur corrigé sans toucher aux modifications réservées.
        let before_lease=local(&a).unwrap().lease;
        let mut expired=cfg.clone();expired.password="ancien mot de passe fictif".into();
        db::atomic_write(&a.join("sharing/config.dpapi"),&crypto::protect(&serde_json::to_vec(&expired).unwrap()).unwrap()).unwrap();
        update_connection(&ca,cfg.clone()).await.unwrap();
        assert!(ca.sharing.pending());assert_eq!(before_lease,local(&a).unwrap().lease);
        publish(&mut ca).await.unwrap();
        assert_eq!(local(&a).unwrap().base.as_ref().unwrap().number, 2);
        open(&mut cb).await.unwrap();
        assert!(auth::login(&cb, "bob", "Compte Bob fictif!", None)
            .await
            .is_ok());
        assert_eq!(
            &*crate::vault::read(&cb, &file, &org).await.unwrap(),
            b"Piece jointe fictive"
        );
        // Données en attente : ouvrir ne remplace jamais le poste, même hors connexion.
        db::atomic_write(&b.join("sharing/pending"), b"unsent").unwrap();
        let before = local(&b).unwrap().base;
        assert!(open(&mut cb).await.unwrap().pending);
        assert_eq!(before, local(&b).unwrap().base);
        clear_pending(&cb).unwrap();
        // La phrase ne permet pas d’adopter silencieusement un autre historique.
        let mut wrong_phrase = cfg.clone();
        wrong_phrase.phrase = "Une autre phrase fictive!".into();
        assert!(network(wrong_phrase, |r, cfg| head(&r, &mac_key(&cfg)?))
            .await
            .is_err());
        let archives = network(cfg, |r, _| r.list("archives")).await.unwrap();
        assert_eq!(archives.len(), 2);
        ca.pool.close().await;
        cb.pool.close().await;
        for root in [a, b] {
            assert!(root.starts_with(std::env::temp_dir()));
            assert!(root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("libre-erp-sftp-pc-"));
            std::fs::remove_dir_all(root).unwrap();
        }
        println!("SFTP réel : deux postes, clé SSH, création, consultation, exclusion, envoi, restauration, comptes et pièce jointe : OK");
    }
    #[test]
    fn exclusive_reservation_and_resume() {
        let r = Memory::default();
        acquire(&r, "poste-a", "Alice").unwrap();
        assert!(acquire(&r, "poste-b", "Bob").is_err());
        acquire(&r, "poste-a", "Alice").unwrap();
        assert!(release(&r, "poste-b").is_err());
        release(&r, "poste-a").unwrap();
        acquire(&r, "poste-b", "Bob").unwrap();
    }
    #[test]
    fn interrupted_reservation_is_not_stolen() {
        let r = Memory::default();
        r.mkdir("reservation").unwrap();
        assert!(acquire(&r, "b", "Bob").is_err());
        assert!(r.read("reservation/owner.json").unwrap().is_none());
    }
    #[test]
    fn authenticated_commits_and_no_overwrite() {
        let r = Memory::default();
        let k = [8u8; 32];
        let bytes = signed(&commit(1), &k).unwrap();
        r.put("versions/tmp.partial", &bytes).unwrap();
        assert!(head(&r, &k).unwrap().is_none());
        r.publish("versions/tmp.partial", &revision_name(1))
            .unwrap();
        let (rev, _) = head(&r, &k).unwrap().unwrap();
        assert!(same_base(&Some(rev.clone()), &head(&r, &k).unwrap()).is_ok());
        assert!(same_base(&None, &head(&r, &k).unwrap()).is_err());
        assert!(head(&r, &[9u8; 32]).is_err());
        r.put("versions/second.partial", b"broken").unwrap();
        assert!(r
            .publish("versions/second.partial", &revision_name(1))
            .is_err());
        assert_eq!(head(&r, &k).unwrap().unwrap().0, rev);
    }
    #[test]
    fn rollback_and_replaced_history_rejected() {
        let r = Memory::default();
        let k = [1; 32];
        let bytes = signed(&commit(1), &k).unwrap();
        r.put(&revision_name(1), &bytes).unwrap();
        let rev = head(&r, &k).unwrap().unwrap().0;
        assert!(verify_ancestor(
            &r,
            &Some(Revision {
                number: 2,
                digest: "x".into()
            }),
            &rev,
            &k
        )
        .is_err());
        assert!(verify_ancestor(
            &r,
            &Some(Revision {
                number: 1,
                digest: "other".into()
            }),
            &rev,
            &k
        )
        .is_err());
    }
    #[test]
    fn invalid_remote_paths_rejected() {
        let c = Config {
            host: "example.org".into(),
            port: 22,
            username: "u".into(),
            password: "secret".into(),
            directory: "/libre-erp/../other".into(),
            fingerprint: "SHA256:abc".into(),
            phrase: "phrase de partage".into(),
        };
        assert!(c.validate(true).is_err());
    }
}
