//! Secret-store access on its own thread.
//!
//! Reading or writing the OS keychain can block for as long as the user takes to answer a system
//! dialog (macOS asks again whenever the app's code signature changes, and Linux may need to unlock
//! the keyring). Doing that on the UI thread would freeze the app before its window even appears,
//! so every secret-store call happens here, in order, and the UI hears back through a channel.

use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use putio_api::Token;
use putio_platform::secret::SecretStore;

enum Op {
    Save(Token),
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct Keychain {
    ops: Sender<Op>,
}

impl Keychain {
    /// Starts the thread, which first loads the saved token (delivered on the returned receiver) and
    /// then carries out [`Keychain::save`] and [`Keychain::delete`] requests in the order they were made.
    pub(crate) fn spawn(
        secrets: Arc<dyn SecretStore>,
        ctx: eframe::egui::Context,
    ) -> io::Result<(Self, Receiver<Option<Token>>)> {
        let (ops, queue) = mpsc::channel::<Op>();
        let (loaded_tx, loaded) = mpsc::channel();
        thread::Builder::new()
            .name("putio-keychain".into())
            .spawn(move || {
                let token = secrets.load().unwrap_or_else(|error| {
                    eprintln!("could not read the saved sign-in: {error}");
                    None
                });
                let _ = loaded_tx.send(token);
                ctx.request_repaint();
                while let Ok(op) = queue.recv() {
                    let result = match op {
                        Op::Save(token) => secrets.save(&token),
                        Op::Delete => secrets.delete(),
                    };
                    if let Err(error) = result {
                        eprintln!("secret store: {error}");
                    }
                }
            })?;
        Ok((Self { ops }, loaded))
    }

    pub(crate) fn save(&self, token: Token) {
        let _ = self.ops.send(Op::Save(token));
    }

    pub(crate) fn delete(&self) {
        let _ = self.ops.send(Op::Delete);
    }
}

#[cfg(test)]
mod tests {
    use putio_platform::secret::MemoryStore;

    use super::*;

    fn token(value: &str) -> Token {
        Token::new(value).unwrap()
    }

    #[test]
    fn delivers_the_saved_token_without_blocking_the_caller() {
        let store = Arc::new(MemoryStore::new());
        store.save(&token("saved")).unwrap();
        let (_keychain, loaded) = Keychain::spawn(store, eframe::egui::Context::default()).unwrap();
        assert_eq!(
            loaded
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap()
                .expose(),
            "saved"
        );
    }

    #[test]
    fn a_missing_token_is_reported_as_none() {
        let (_keychain, loaded) = Keychain::spawn(
            Arc::new(MemoryStore::new()),
            eframe::egui::Context::default(),
        )
        .unwrap();
        assert!(
            loaded
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn saves_and_deletes_are_applied_in_order() {
        let store = Arc::new(MemoryStore::new());
        let (keychain, loaded) = Keychain::spawn(
            Arc::clone(&store) as Arc<dyn SecretStore>,
            eframe::egui::Context::default(),
        )
        .unwrap();
        loaded
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        keychain.save(token("one"));
        keychain.save(token("two"));
        keychain.delete();
        keychain.save(token("three"));
        // The thread works through the queue in order; wait until the last write is visible.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while store
            .load()
            .unwrap()
            .map(|t| t.expose().to_owned())
            .as_deref()
            != Some("three")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the last save never arrived"
            );
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// A store that is slow to answer, like a keychain waiting for the user to click "Allow".
    struct SlowStore;

    impl SecretStore for SlowStore {
        fn load(&self) -> Result<Option<Token>, putio_platform::secret::SecretError> {
            thread::sleep(std::time::Duration::from_millis(300));
            Ok(None)
        }
        fn save(&self, _: &Token) -> Result<(), putio_platform::secret::SecretError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), putio_platform::secret::SecretError> {
            Ok(())
        }
    }

    #[test]
    fn a_slow_keychain_does_not_hold_up_spawn() {
        let started = std::time::Instant::now();
        let (_keychain, loaded) =
            Keychain::spawn(Arc::new(SlowStore), eframe::egui::Context::default()).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(150),
            "spawn must return at once"
        );
        assert!(loaded.try_recv().is_err(), "the answer is not there yet");
        assert!(
            loaded
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok()
        );
    }
}
