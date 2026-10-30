use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

pub fn random_key() -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(vec![0u8; 32]);
    OsRng.fill_bytes(&mut out);
    out
}
pub fn password_hash(value: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(value.as_bytes(), &salt)
        .map_err(|_| anyhow::anyhow!("Hachage impossible"))?
        .to_string())
}
pub fn password_matches(value: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .ok()
        .map(|p| {
            Argon2::default()
                .verify_password(value.as_bytes(), &p)
                .is_ok()
        })
        .unwrap_or(false)
}
pub fn validate_password(value: &str) -> Result<()> {
    if value.chars().count() < 12 || value.len() > 1024 {
        bail!("Choisissez un mot de passe entre 12 et 1 024 caractères.");
    }
    Ok(())
}
pub fn seal(key: &[u8], plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Clé invalide"))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: context,
            },
        )
        .map_err(|_| anyhow::anyhow!("Chiffrement impossible"))?;
    let mut out = nonce.to_vec();
    out.extend(ciphertext);
    Ok(out)
}
pub fn unseal(key: &[u8], data: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if data.len() < 28 {
        bail!("Données chiffrées tronquées.");
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Clé invalide"))?;
    Ok(Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&data[..12]),
                Payload {
                    msg: &data[12..],
                    aad: context,
                },
            )
            .map_err(|_| anyhow::anyhow!("Mot de passe incorrect ou archive altérée."))?,
    ))
}
pub fn derive_backup_key(password: &str, salt: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    validate_password(password)?;
    let mut key = Zeroizing::new(vec![0u8; 32]);
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| anyhow::anyhow!("Dérivation impossible"))?;
    Ok(key)
}

#[cfg(windows)]
fn dpapi(input: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into()?,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut dest = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI CurrentUser ; aucun secret transmis au WebView.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &source,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut dest,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut dest,
            )
        }
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error()).context("Protection Windows DPAPI impossible");
    }
    let bytes = Zeroizing::new(
        unsafe { std::slice::from_raw_parts(dest.pbData, dest.cbData as usize) }.to_vec(),
    );
    unsafe {
        std::ptr::write_bytes(dest.pbData, 0, dest.cbData as usize);
        LocalFree(dest.pbData as _);
    }
    Ok(bytes)
}
pub fn protect(input: &[u8]) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        Ok(dpapi(input, true)?.to_vec())
    }
    #[cfg(not(windows))]
    {
        let _ = input;
        bail!("DPAPI requiert Windows.")
    }
}
pub fn unprotect(input: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    #[cfg(windows)]
    {
        dpapi(input, false)
    }
    #[cfg(not(windows))]
    {
        let _ = input;
        bail!("DPAPI requiert Windows.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tampering_is_rejected() {
        let key = random_key();
        let mut sealed = seal(&key, b"secret", b"test").unwrap();
        assert_eq!(&*unseal(&key, &sealed, b"test").unwrap(), b"secret");
        assert!(unseal(&key, &sealed, b"wrong").is_err());
        sealed[12] ^= 1;
        assert!(unseal(&key, &sealed, b"test").is_err());
    }
    #[test]
    fn password_policy_and_hash() {
        assert!(validate_password("court").is_err());
        let h = password_hash("Phrase de test longue").unwrap();
        assert!(password_matches("Phrase de test longue", &h));
        assert!(!password_matches("autre", &h));
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let k = random_key();
        assert_eq!(&*unprotect(&protect(&k).unwrap()).unwrap(), &*k);
    }
}
