//! Outil indépendant du WebView et du démarrage de la base active.
use crate::{backup, db};
use anyhow::{bail, Context, Result};
use std::{
    io::{self, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;
fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}
pub fn run() -> Result<()> {
    println!("LIBRE ERP — RÉCUPÉRATION DES DONNÉES\nFermez d’abord l’application principale.\n");
    if std::env::current_exe()?
        .parent()
        .is_some_and(|p| p.join("portable.json").exists())
    {
        crate::portable::unlock_console()?;
    }
    let root = crate::portable::root().unwrap_or(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("AppData local introuvable")?)
            .join("fr.securiteerp.desktop"),
    );
    std::fs::create_dir_all(root.join("recovery"))?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("process.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("L’application utilise encore les données. Fermez-la avant de continuer.")?;
    if root.join("recovery/pending-restore.json").exists() {
        backup::apply_pending(&root)?;
        println!("Restauration interrompue terminée. Vous pouvez rouvrir l’application.");
        return Ok(());
    }
    println!("Une restauration remplace les données actives par celles de l’archive. Les fichiers actuels seront conservés dans le dossier de récupération.\n");
    let archive =
        PathBuf::from(prompt("Chemin complet de la sauvegarde ZIP : ")?.trim_matches('"'));
    let password = Zeroizing::new(rpassword::prompt_password("Phrase de récupération : ")?);
    let runtime = tokio::runtime::Runtime::new()?;
    let rid = runtime.block_on(backup::validate_restore(&root, &archive, &password))?;
    println!("Archive et base chiffrée vérifiées.");
    if prompt("Saisissez RESTAURER pour confirmer : ")? != "RESTAURER" {
        bail!("Restauration annulée.");
    }
    backup::request_restore(&root, &rid)?;
    backup::apply_pending(&root)?;
    db::atomic_write(
        &root.join("recovery/last-recovery.txt"),
        format!("Restauration manuelle {}", db::now()).as_bytes(),
    )?;
    println!("Restauration terminée. Rouvrez l’application et connectez-vous avec un compte contenu dans la sauvegarde.");
    let _ = prompt("Appuyez sur Entrée pour fermer.");
    Ok(())
}
