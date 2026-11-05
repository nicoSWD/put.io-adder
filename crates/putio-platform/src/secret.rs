//! The put.io token in the operating system's secret store.
//!
//! Keychain (macOS), Credential Manager (Windows) or the Secret Service (Linux). There is
//! intentionally **no file fallback**: if no secret store is available the app tells the user instead of
//! writing the token somewhere readable.

use std::sync::Mutex;

use putio_api::Token;

const SERVICE: &str = "put.io adder";
const ACCOUNT: &str = "oauth-token";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No secret store could be opened (e.g. no Secret Service running on Linux).
    #[error("no secret store available: {0}")]
    Unavailable(String),
    /// The store refused or failed. The message never contains the secret.
    #[error("secret store error: {0}")]
    Backend(String),
}

pub trait SecretStore: Send + Sync {
    /// `Ok(None)` when no token is stored yet (or the stored value is not a valid token).
    fn load(&self) -> Result<Option<Token>, SecretError>;
    fn save(&self, token: &Token) -> Result<(), SecretError>;
    /// Deleting a token that is not there is not an error.
    fn delete(&self) -> Result<(), SecretError>;
}

/// The real thing. Create it once at start-up with [`KeyringStore::open`].
#[derive(Debug)]
pub struct KeyringStore {
    entry: keyring_core::Entry,
}

impl KeyringStore {
    /// Select the platform's credential store and open the entry for the token.
    pub fn open() -> Result<Self, SecretError> {
        static INIT: Mutex<bool> = Mutex::new(false);
        {
            let mut done = INIT
                .lock()
                .map_err(|_| SecretError::Unavailable("lock poisoned".into()))?;
            if !*done {
                install_default_store()?;
                *done = true;
            }
        }
        let entry = keyring_core::Entry::new(SERVICE, ACCOUNT)
            .map_err(|e| SecretError::Unavailable(e.to_string()))?;
        Ok(Self { entry })
    }
}

#[cfg(target_os = "macos")]
fn install_default_store() -> Result<(), SecretError> {
    let store = apple_native_keyring_store::keychain::Store::new()
        .map_err(|e| SecretError::Unavailable(e.to_string()))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_default_store() -> Result<(), SecretError> {
    let store = windows_native_keyring_store::Store::new()
        .map_err(|e| SecretError::Unavailable(e.to_string()))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_default_store() -> Result<(), SecretError> {
    let store = zbus_secret_service_keyring_store::Store::new()
        .map_err(|e| SecretError::Unavailable(e.to_string()))?;
    keyring_core::set_default_store(store);
    Ok(())
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<Token>, SecretError> {
        match self.entry.get_password() {
            // A value that is not a valid token (tampered, corrupted) counts as "no token".
            Ok(value) => Ok(Token::new(value).ok()),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError::Backend(error.to_string())),
        }
    }

    fn save(&self, token: &Token) -> Result<(), SecretError> {
        self.entry
            .set_password(token.expose())
            .map_err(|e| SecretError::Backend(e.to_string()))
    }

    fn delete(&self) -> Result<(), SecretError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::Backend(error.to_string())),
        }
    }
}

/// In-memory store for tests and for `--no-keychain` style development runs.
#[derive(Debug, Default)]
pub struct MemoryStore {
    token: Mutex<Option<Token>>,
}

impl MemoryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<Token>, SecretError> {
        self.token
            .lock()
            .map(|t| t.clone())
            .map_err(|_| SecretError::Backend("lock poisoned".into()))
    }

    fn save(&self, token: &Token) -> Result<(), SecretError> {
        *self
            .token
            .lock()
            .map_err(|_| SecretError::Backend("lock poisoned".into()))? = Some(token.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        *self
            .token
            .lock()
            .map_err(|_| SecretError::Backend("lock poisoned".into()))? = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::new();
        assert!(store.load().unwrap().is_none());
        store.save(&Token::new("abc").unwrap()).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "abc");
        store.delete().unwrap();
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// Touches the real OS secret store (on macOS this can show a Keychain prompt), so it only runs
    /// on request: `cargo test -p putio-platform -- --ignored keyring`.
    #[test]
    #[ignore = "uses the real OS secret store"]
    fn keyring_round_trip() {
        let store = KeyringStore::open().unwrap();
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
        store.save(&Token::new("test-token-123").unwrap()).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "test-token-123");
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
