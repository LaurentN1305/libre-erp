//! Preuve de publication commune à l'updater et à la récupération indépendante.
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
#[derive(Serialize, Deserialize)]
pub struct Envelope {
    pub payload: String,
    pub signature: String,
}
#[derive(Deserialize)]
pub struct Payload {
    pub channel: String,
    pub target: String,
    pub sequence: u64,
    pub expires_at: String,
    pub release: serde_json::Value,
}
pub fn authenticate(bytes: &[u8], key: &[u8; 32]) -> Result<Payload> {
    if bytes.len() > 1024 * 1024 {
        bail!("Manifeste trop volumineux")
    }
    let env: Envelope = serde_json::from_slice(bytes)?;
    let payload = B64.decode(env.payload)?;
    if payload.len() > 512 * 1024 {
        bail!("Contenu signé trop volumineux")
    }
    let sig = B64.decode(env.signature)?;
    VerifyingKey::from_bytes(key)?
        .verify_strict(&payload, &Signature::from_slice(&sig)?)
        .context("Signature Ed25519 du manifeste invalide")?;
    let p: Payload = serde_json::from_slice(&payload)?;
    if !["stable", "beta"].contains(&p.channel.as_str())
        || p.target != "windows-x86_64"
        || p.sequence == 0
    {
        bail!("Canal, plateforme ou séquence invalide")
    }
    semver::Version::parse(p.release["version"].as_str().context("Version absente")?)?;
    chrono::DateTime::parse_from_rfc3339(&p.expires_at)?;
    Ok(p)
}
pub fn expected_hash(p: &Payload) -> Result<String> {
    let s = p.release["sha256"]
        .as_str()
        .context("Empreinte SHA-256 absente du manifeste signé")?;
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Empreinte signée invalide")
    };
    Ok(s.to_lowercase())
}
pub fn verify_bytes(p: &Payload, bytes: &[u8]) -> Result<()> {
    if hex::encode(Sha256::digest(bytes)) != expected_hash(p)? {
        bail!("Le paquet ne correspond pas à l’empreinte signée")
    };
    Ok(())
}
pub fn verify_file(envelope: &[u8], path: &Path, key: &[u8; 32]) -> Result<String> {
    let p = authenticate(envelope, key)?;
    let expected = expected_hash(&p)?;
    // Un retour volontaire peut utiliser une preuve historique expirée ; l'updater impose l'expiration.
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() > 1024 * 1024 * 1024 {
        bail!("Installeur trop volumineux")
    }
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 1024 * 1024 * 1024 {
            bail!("Installeur trop volumineux")
        };
        hash.update(&buf[..n]);
    }
    if hex::encode(hash.finalize()) != expected {
        bail!("Installeur modifié ou preuve ne correspondant pas à ce fichier")
    }
    Ok(p.release["version"].as_str().unwrap().to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn archived_proof_checks_signer_and_exact_installer() {
        let dir = std::env::temp_dir().join(format!("erp-proof-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("setup.exe");
        std::fs::write(&file, b"paquet test").unwrap();
        let key = SigningKey::from_bytes(&[13; 32]);
        let payload=serde_json::to_vec(&serde_json::json!({"channel":"stable","target":"windows-x86_64","sequence":1,"expires_at":"2020-01-01T00:00:00Z","release":{"version":"0.11.0","sha256":hex::encode(Sha256::digest(b"paquet test"))}})).unwrap();
        let envelope = serde_json::to_vec(&Envelope {
            payload: B64.encode(&payload),
            signature: B64.encode(key.sign(&payload).to_bytes()),
        })
        .unwrap();
        assert_eq!(
            verify_file(&envelope, &file, &key.verifying_key().to_bytes()).unwrap(),
            "0.11.0"
        );
        assert!(verify_file(
            &envelope,
            &file,
            &SigningKey::from_bytes(&[14; 32]).verifying_key().to_bytes()
        )
        .is_err());
        std::fs::write(&file, b"paquet change").unwrap();
        assert!(verify_file(&envelope, &file, &key.verifying_key().to_bytes()).is_err());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
