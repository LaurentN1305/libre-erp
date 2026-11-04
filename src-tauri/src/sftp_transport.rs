//! Transport SFTP uniquement : aucun lancement de programme ni commande shell distante.
use crate::shared_workspace::Config;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use ssh2::{ErrorCode, OpenFlags, OpenType, RenameFlags, Session, Sftp};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};

pub const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
pub trait Remote {
    fn read(&self, name: &str) -> Result<Option<Vec<u8>>>;
    fn list(&self, directory: &str) -> Result<Vec<String>>;
    fn mkdir(&self, name: &str) -> Result<bool>;
    fn put(&self, name: &str, bytes: &[u8]) -> Result<()>;
    fn upload(&self, name: &str, source: &Path) -> Result<()>;
    fn download(&self, name: &str, target: &Path, expected: u64) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
    fn rmdir(&self, name: &str) -> Result<()>;
    fn publish(&self, temporary: &str, final_name: &str) -> Result<()>;
}
pub struct Connection {
    sftp: Sftp,
    root: PathBuf,
    _session: Session,
}
fn missing(e: &ssh2::Error) -> bool {
    e.code() == ErrorCode::SFTP(2)
}
fn session(c: &Config) -> Result<Session> {
    c.validate(false)?;
    let addresses = (c.host.as_str(), c.port)
        .to_socket_addrs()
        .context("Adresse du serveur introuvable")?;
    let mut stream = None;
    for addr in addresses.take(4) {
        if let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_secs(8)) {
            stream = Some(s);
            break;
        }
    }
    let stream = stream.context("Connexion SFTP impossible (serveur ou port inaccessible)")?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(20)))?;
    let mut s = Session::new()?;
    s.set_timeout(20000);
    s.set_tcp_stream(stream);
    s.handshake().context("La négociation SSH a échoué")?;
    Ok(s)
}
fn fingerprint(s: &Session) -> Result<String> {
    let (key, _) = s
        .host_key()
        .context("Le serveur ne présente pas de clé SSH")?;
    Ok(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(key))
    ))
}
pub fn probe(c: &Config) -> Result<String> {
    fingerprint(&session(c)?)
}
impl Connection {
    pub fn connect(c: &Config) -> Result<Self> {
        c.validate(true)?;
        let s = session(c)?;
        if fingerprint(&s)? != c.fingerprint {
            bail!("La clé SSH du serveur a changé. Connexion refusée : vérifiez auprès de votre hébergeur.");
        }
        s.userauth_password(&c.username, &c.password)
            .context("Identifiant ou mot de passe SFTP refusé")?;
        if !s.authenticated() {
            bail!("Authentification SFTP incomplète");
        }
        let sftp = s
            .sftp()
            .context("Cet hébergement ne fournit pas de service SFTP")?;
        let root = sftp
            .realpath(Path::new(&c.directory))
            .context("Le dossier distant doit déjà exister et être accessible")?;
        if !sftp.stat(&root)?.is_dir() {
            bail!("Le chemin distant n’est pas un dossier");
        }
        Ok(Self {
            sftp,
            root,
            _session: s,
        })
    }
    fn path(&self, name: &str) -> Result<PathBuf> {
        if name.is_empty()
            || name
                .split('/')
                .any(|x| x.is_empty() || x == "." || x == "..")
            || name.contains(['\\', ':', '\0'])
        {
            bail!("Chemin SFTP interne invalide");
        }
        Ok(self.root.join(name))
    }
    fn create(&self, name: &str) -> Result<ssh2::File> {
        Ok(self.sftp.open_mode(
            &self.path(name)?,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
            0o600,
            OpenType::File,
        )?)
    }
    pub fn test(&self) -> Result<()> {
        // Exclusivité + renommage sans écrasement testés sur des fichiers jetables.
        let a = format!("test-{}.tmp", crate::db::id());
        let b = format!("test-{}.tmp", crate::db::id());
        let result = (|| {
            self.put(&a, b"Libre ERP A")?;
            if self.put(&a, b"interdit").is_ok() {
                bail!("Le serveur ne garantit pas la création exclusive");
            }
            self.put(&b, b"Libre ERP B")?;
            if self.publish(&a, &b).is_ok() {
                bail!("Le serveur ne garantit pas le renommage sans écrasement");
            }
            if self.read(&b)?.as_deref() != Some(b"Libre ERP B") {
                bail!("Écriture distante non fiable");
            }
            self.remove(&b)?;
            self.publish(&a, &b)?;
            if self.read(&b)?.as_deref() != Some(b"Libre ERP A") {
                bail!("Vérification du transfert échouée");
            }
            Ok(())
        })();
        let _ = self.remove(&a);
        let _ = self.remove(&b);
        result
    }
}
impl Remote for Connection {
    fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let p = self.path(name)?;
        let stat = match self.sftp.lstat(&p) {
            Ok(v) => v,
            Err(e) if missing(&e) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !stat.is_file() || stat.size.unwrap_or(u64::MAX) > 65536 {
            bail!("Fichier de contrôle SFTP invalide ou trop volumineux");
        }
        let mut bytes = Vec::new();
        self.sftp.open(&p)?.take(65537).read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            bail!("Fichier de contrôle trop volumineux");
        }
        Ok(Some(bytes))
    }
    fn list(&self, directory: &str) -> Result<Vec<String>> {
        let entries = self.sftp.readdir(&self.path(directory)?)?;
        if entries.len() > 10000 {
            bail!("Plus de 10 000 versions : un archivage administrateur est nécessaire");
        }
        Ok(entries
            .into_iter()
            .filter_map(|(p, _)| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .collect())
    }
    fn mkdir(&self, name: &str) -> Result<bool> {
        let p = self.path(name)?;
        match self.sftp.mkdir(&p, 0o700) {
            Ok(()) => Ok(true),
            Err(e) => match self.sftp.lstat(&p) {
                Ok(s) if s.is_dir() => Ok(false),
                _ => Err(e.into()),
            },
        }
    }
    fn put(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut f = self.create(name)?;
        f.write_all(bytes)?;
        f.close()?;
        Ok(())
    }
    fn upload(&self, name: &str, source: &Path) -> Result<()> {
        let mut source = std::fs::File::open(source)?;
        if source.metadata()?.len() > MAX_ARCHIVE {
            bail!("Le partage est limité à 2 Gio par version");
        }
        let mut out = self.create(name)?;
        std::io::copy(&mut source, &mut out)?;
        out.close()?;
        Ok(())
    }
    fn download(&self, name: &str, target: &Path, expected: u64) -> Result<()> {
        if expected > MAX_ARCHIVE {
            bail!("Archive partagée supérieure à 2 Gio");
        }
        let p = self.path(name)?;
        let stat = self.sftp.lstat(&p)?;
        if !stat.is_file() || stat.size != Some(expected) {
            bail!("Taille de l’archive distante incorrecte");
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let count = std::io::copy(&mut self.sftp.open(&p)?.take(expected + 1), &mut out)?;
        out.sync_all()?;
        if count != expected {
            bail!("Téléchargement interrompu : données locales conservées");
        }
        Ok(())
    }
    fn remove(&self, name: &str) -> Result<()> {
        match self.sftp.unlink(&self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if missing(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    fn rmdir(&self, name: &str) -> Result<()> {
        self.sftp.rmdir(&self.path(name)?)?;
        Ok(())
    }
    fn publish(&self, temporary: &str, final_name: &str) -> Result<()> {
        self.sftp.rename(
            &self.path(temporary)?,
            &self.path(final_name)?,
            Some(RenameFlags::empty()),
        )?;
        Ok(())
    }
}
