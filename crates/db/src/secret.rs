//! Encryption of integration secrets at rest (ADR 0011).
//!
//! A value is stored as `v1:<nonce>:<ciphertext>` (URL-safe base64, no
//! padding). The key is derived from the root secret with HKDF and differs
//! from the auth signing key. A value that cannot be decrypted is reported as
//! [`Unreadable`], never as a crash: the app starts and asks for the secret
//! again.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;

const VERSION: &str = "v1";
const NONCE_LEN: usize = 24;

/// The key that encrypts integration secrets.
#[derive(Clone)]
pub struct SecretKey(Key);

impl SecretKey {
    /// Derives the key from the root secret (`UNSEENARR_SECRET`).
    pub fn derive(root_secret: &str) -> Self {
        let mut key = [0u8; 32];
        Hkdf::<Sha256>::new(None, root_secret.as_bytes())
            .expand(b"unseen integration secrets v1", &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        Self(Key::from(key))
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// A plain-text secret. It has no `Serialize` and its `Debug` hides the
/// content, so it cannot leak through a response or a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The content. Call it only where the secret is used.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The stored value is not valid or the key does not match: the secret must
/// be entered again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("secret must be re-entered")
    }
}

impl std::error::Error for Unreadable {}

/// Encrypts `secret` with a random nonce.
pub fn encrypt(key: &SecretKey, secret: &Secret) -> String {
    let nonce: [u8; NONCE_LEN] = rand::random();
    let ciphertext = XChaCha20Poly1305::new(&key.0)
        .encrypt(&XNonce::from(nonce), secret.0.as_bytes())
        .expect("encrypting a short value cannot fail");
    format!(
        "{VERSION}:{}:{}",
        URL_SAFE_NO_PAD.encode(nonce),
        URL_SAFE_NO_PAD.encode(ciphertext)
    )
}

/// Decrypts a value written by [`encrypt`].
pub fn decrypt(key: &SecretKey, stored: &str) -> Result<Secret, Unreadable> {
    let mut parts = stored.split(':');
    let (Some(VERSION), Some(nonce), Some(ciphertext), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Unreadable);
    };
    let nonce: [u8; NONCE_LEN] = URL_SAFE_NO_PAD
        .decode(nonce)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Unreadable)?;
    let ciphertext = URL_SAFE_NO_PAD.decode(ciphertext).map_err(|_| Unreadable)?;
    let plain = XChaCha20Poly1305::new(&key.0)
        .decrypt(&XNonce::from(nonce), ciphertext.as_slice())
        .map_err(|_| Unreadable)?;
    String::from_utf8(plain).map(Secret).map_err(|_| Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = SecretKey::derive("root");
        let stored = encrypt(&key, &Secret::new("tmdb-key"));
        assert!(stored.starts_with("v1:"));
        assert!(!stored.contains("tmdb-key"));
        assert_eq!(decrypt(&key, &stored).unwrap().expose(), "tmdb-key");
    }

    #[test]
    fn same_value_gives_different_ciphertexts() {
        let key = SecretKey::derive("root");
        let secret = Secret::new("same");
        assert_ne!(encrypt(&key, &secret), encrypt(&key, &secret));
    }

    #[test]
    fn wrong_key_or_damaged_value_is_unreadable() {
        let stored = encrypt(&SecretKey::derive("root"), &Secret::new("x"));
        let other = SecretKey::derive("other");
        assert_eq!(decrypt(&other, &stored), Err(Unreadable));
        let key = SecretKey::derive("root");
        for bad in ["", "plain", "v2:a:b", "v1:a", "v1:a:b:c", "v1:!!:!!"] {
            assert_eq!(decrypt(&key, bad), Err(Unreadable), "{bad}");
        }
        let mut flipped = stored.clone();
        flipped.pop();
        flipped.push('A');
        if flipped != stored {
            assert_eq!(decrypt(&key, &flipped), Err(Unreadable));
        }
    }

    #[test]
    fn debug_hides_the_content() {
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "Secret(..)");
        assert_eq!(format!("{:?}", SecretKey::derive("root")), "SecretKey(..)");
    }
}
