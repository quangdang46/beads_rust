//! Credential encryption and credential file management (Issue #28).
//!
//! AES-256-GCM encryption for federation peer credentials, with a 32-byte
//! random key stored at `<beads_dir>/.beads-credential-key`.
//!
//! Also provides an INI-style credentials file parser (as used by Go beads)
//! for reading `~/.config/beads/credentials`.
//!
//! # Example
//!
//! ```rust,ignore
//! let dir = tempfile::TempDir::new()?;
//! let key = CredentialKey::load_or_create(dir.path())?;
//! let encrypted = key.encrypt_password("my-secret")?;
//! let decrypted = key.decrypt_password(&encrypted)?;
//! assert_eq!(decrypted, "my-secret");
//! ```

use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub use aes_gcm::aead::OsRng;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during credential operations.
#[derive(Error, Debug)]
pub enum CredentialError {
    /// I/O error reading or writing the key file or credentials file.
    #[error("credential I/O error: {0}")]
    Io(#[from] io::Error),
    /// Encryption or decryption failed.
    #[error("crypto operation failed: {0}")]
    CryptoOp(String),
    /// The encrypted payload is too short to contain a nonce.
    #[error("ciphertext too short (expected at least 12 bytes for nonce)")]
    CiphertextTooShort,
    /// The key file exists but has an unexpected size.
    #[error("credential key file has wrong length (expected 32 bytes, got {0})")]
    InvalidKeyLength(usize),
}

/// Alias for `Result` with [`CredentialError`].
pub type Result<T> = std::result::Result<T, CredentialError>;

// ---------------------------------------------------------------------------
// Credential key (Issue #28 - AES-256-GCM)
// ---------------------------------------------------------------------------

/// An AES-256-GCM credential encryption key.
///
/// The key is stored as a 32-byte random blob in the `.beads/` directory and
/// managed by [`load_or_create`][CredentialKey::load_or_create].
#[derive(Debug)]
pub struct CredentialKey {
    key_bytes: [u8; 32],
}

/// Filename for the credential key inside the beads directory.
const CREDENTIAL_KEY_FILE: &str = ".beads-credential-key";

impl CredentialKey {
    /// Load an existing key from `<beads_dir>/<CREDENTIAL_KEY_FILE>`, or
    /// generate and persist a new random 32-byte key if the file does not
    /// exist.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::Io`] if the key file cannot be read/written,
    /// or [`CredentialError::InvalidKeyLength`] if an existing file has the
    /// wrong size.
    pub fn load_or_create(beads_dir: &Path) -> Result<Self> {
        let key_path = beads_dir.join(CREDENTIAL_KEY_FILE);

        if key_path.exists() {
            let raw = fs::read(&key_path)?;
            Self::from_bytes(&raw)
        } else {
            let key = Self::generate()?;
            key.persist(&key_path)?;
            Ok(key)
        }
    }

    /// Encrypt a plaintext password with AES-256-GCM.
    ///
    /// Returns `None` (no-op) when `password` is empty.
    pub fn encrypt_password(&self, password: &str) -> Result<Option<Vec<u8>>> {
        if password.is_empty() {
            return Ok(None);
        }
        let cipher = self.make_cipher();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, password.as_bytes())
            .map_err(|e| CredentialError::CryptoOp(e.to_string()))?;
        // Prepend nonce to ciphertext for storage
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ciphertext);
        Ok(Some(out))
    }

    /// Decrypt an AES-256-GCM payload produced by [`encrypt_password`].
    ///
    /// Returns `Ok(String::new())` (no-op) when `encrypted` is empty.
    pub fn decrypt_password(&self, encrypted: &[u8]) -> Result<String> {
        if encrypted.is_empty() {
            return Ok(String::new());
        }
        let cipher = self.make_cipher();
        let nonce_size = 12; // AES-GCM standard nonce size
        if encrypted.len() < nonce_size {
            return Err(CredentialError::CiphertextTooShort);
        }
        let (nonce_bytes, ciphertext) = encrypted.split_at(nonce_size);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CredentialError::CryptoOp(e.to_string()))?;
        String::from_utf8(plaintext)
            .map_err(|e| CredentialError::CryptoOp(format!("UTF-8 error: {e}")))
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn generate() -> Result<Self> {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        let key_bytes = key.into();
        Ok(Self { key_bytes })
    }

    fn from_bytes(raw: &[u8]) -> Result<Self> {
        if raw.len() != 32 {
            return Err(CredentialError::InvalidKeyLength(raw.len()));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(raw);
        Ok(Self { key_bytes })
    }

    fn persist(&self, path: &Path) -> Result<()> {
        let mut file = fs::File::create(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&self.key_bytes)?;
        Ok(())
    }

    fn make_cipher(&self) -> Aes256Gcm {
        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        Aes256Gcm::new(key)
    }
}

// ---------------------------------------------------------------------------
// Credentials file (INI-style parser - Go `configfile/credentials.go` port)
// ---------------------------------------------------------------------------

/// Parse an INI-style credentials file and return the password for a
/// `[host:port]` section.
///
/// File format:
///
/// ```ini
/// [127.0.0.1:3307]
/// password=localDevPassword
///
/// [beads.company.com:3307]
/// password=teamServerPassword
/// ```
///
/// Inline `#` comments are stripped.  Returns `None` if the section is not
/// found or the file can't be read.
pub fn lookup_credentials_password(path: &Path, host: &str, port: u16) -> Option<String> {
    let section_key = format!("{host}:{port}");
    read_password_from_file(path, &section_key)
}

fn read_password_from_file(path: &Path, section_key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_section = false;

    for line in content.lines() {
        let line = line.trim();

        // Strip inline comments
        let line = match line.find('#') {
            Some(idx) => line[..idx].trim(),
            None => line,
        };
        if line.is_empty() {
            continue;
        }

        // Section header: [host:port]
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            if section == section_key {
                in_section = true;
            } else if in_section {
                // We've left our section without finding a password
                break;
            }
            continue;
        }

        // Key=value within our section
        if in_section {
            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim();
                let value = line[pos + 1..].trim();
                if key == "password" {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        TempDir::new().expect("create temp dir")
    }

    // -- Key management -----------------------------------------------------

    #[test]
    fn test_key_generate_and_load() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        // The key file should now exist
        let key_path = dir.path().join(CREDENTIAL_KEY_FILE);
        assert!(key_path.exists(), "key file should exist after load_or_create");

        // Loading again should read the same key
        let key2 = CredentialKey::load_or_create(dir.path()).unwrap();
        assert_eq!(key.key_bytes, key2.key_bytes, "key should be stable across loads");
    }

    #[test]
    fn test_key_load_invalid_size() {
        let dir = temp_dir();
        let key_path = dir.path().join(CREDENTIAL_KEY_FILE);
        fs::write(&key_path, b"too-short").unwrap();
        let err = CredentialKey::load_or_create(dir.path()).unwrap_err();
        assert!(matches!(err, CredentialError::InvalidKeyLength(9)));
    }

    // -- Encryption round-trip ----------------------------------------------

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        let password = "my-secret-password!@#";

        let encrypted = key
            .encrypt_password(password)
            .unwrap()
            .expect("should produce ciphertext");
        let decrypted = key.decrypt_password(&encrypted).unwrap();
        assert_eq!(decrypted, password);
    }

    #[test]
    fn test_encrypt_empty_password() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        let encrypted = key.encrypt_password("").unwrap();
        assert!(encrypted.is_none(), "empty password should produce None");

        let decrypted = key.decrypt_password(&[]).unwrap();
        assert_eq!(decrypted, "", "empty decrypt should produce empty string");
    }

    #[test]
    fn test_encrypt_different_nonces() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        let a = key.encrypt_password("same").unwrap().unwrap();
        let b = key.encrypt_password("same").unwrap().unwrap();
        assert_ne!(a, b, "same plaintext should produce different ciphertext (nonce)");
    }

    #[test]
    fn test_decrypt_tampered_fails() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        let mut encrypted = key.encrypt_password("secret").unwrap().unwrap();
        // Flip a byte in the ciphertext (after the nonce)
        encrypted[13] ^= 0xFF;
        let result = key.decrypt_password(&encrypted);
        assert!(result.is_err(), "tampered ciphertext should fail decryption");
    }

    #[test]
    fn test_decrypt_too_short() {
        let dir = temp_dir();
        let key = CredentialKey::load_or_create(dir.path()).unwrap();
        let result = key.decrypt_password(&[1, 2, 3, 4, 5]);
        assert!(matches!(result, Err(CredentialError::CiphertextTooShort)));
    }

    // -- Key file permissions (unix only) ----------------------------------

    #[test]
    #[cfg(unix)]
    fn test_key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        CredentialKey::load_or_create(dir.path()).unwrap();
        let key_path = dir.path().join(CREDENTIAL_KEY_FILE);
        let meta = fs::metadata(&key_path).unwrap();
        let mode = meta.permissions().mode();
        // Should be 0o600 (owner rw only)
        assert_eq!(
            mode & 0o777,
            0o600,
            "key file should have 0600 permissions, got {:#o}",
            mode & 0o777
        );
    }

    // -- Credentials file parser -------------------------------------------

    #[test]
    fn test_credentials_file_basic() {
        let dir = temp_dir();
        let cred_path = dir.path().join("credentials");
        let mut file = fs::File::create(&cred_path).unwrap();
        write!(
            file,
            "[127.0.0.1:3307]\npassword=localDevPassword\n\n[server.example.com:443]\npassword=teamSecret\n"
        )
        .unwrap();

        let pwd = lookup_credentials_password(&cred_path, "127.0.0.1", 3307);
        assert_eq!(pwd.as_deref(), Some("localDevPassword"));

        let pwd = lookup_credentials_password(&cred_path, "server.example.com", 443);
        assert_eq!(pwd.as_deref(), Some("teamSecret"));
    }

    #[test]
    fn test_credentials_file_with_comments() {
        let dir = temp_dir();
        let cred_path = dir.path().join("credentials");
        let mut file = fs::File::create(&cred_path).unwrap();
        write!(
            file,
            "# This is a comment\n[host:1234]\npassword=real # inline comment\n"
        )
        .unwrap();

        let pwd = lookup_credentials_password(&cred_path, "host", 1234);
        assert_eq!(pwd.as_deref(), Some("real"));
    }

    #[test]
    fn test_credentials_file_section_not_found() {
        let dir = temp_dir();
        let cred_path = dir.path().join("credentials");
        let mut file = fs::File::create(&cred_path).unwrap();
        write!(file, "[other:9999]\npassword=secret\n").unwrap();

        let pwd = lookup_credentials_password(&cred_path, "nonexistent", 1234);
        assert!(pwd.is_none(), "should return None for missing section");
    }

    #[test]
    fn test_credentials_file_missing() {
        let dir = temp_dir();
        let cred_path = dir.path().join("no-such-file");
        let pwd = lookup_credentials_password(&cred_path, "host", 1234);
        assert!(pwd.is_none(), "should return None for missing file");
    }
}
