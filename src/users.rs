use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, MutexGuard};

/// The passwords the one user of this server may be let in with, shared by
/// every connection.
///
/// Redis has as many users as it is given; this server has the one Redis starts
/// with, and all a connection can do is prove it knows a password of it.
///
/// What is kept is not the password but a hash of it, so that a client asking
/// after the user, or anyone reading a dump of the server, learns nothing it
/// could log in with.
#[derive(Clone, Default)]
pub struct Users(Arc<Mutex<Vec<String>>>);

impl Users {
    /// Takes on a password the user may be let in with.
    ///
    /// A password already known is not taken on twice: it is the same password,
    /// however often it is given.
    pub fn add_password(&self, password: &[u8]) {
        let hashed = hashed(password);
        let mut passwords = self.passwords();

        if !passwords.contains(&hashed) {
            passwords.push(hashed);
        }
    }

    /// The passwords the user may be let in with, hashed, in the order they
    /// were taken on.
    pub fn hashed_passwords(&self) -> Vec<String> {
        self.passwords().clone()
    }

    /// Whether the user will let anyone in, having no password of its own to
    /// check against.
    pub fn wants_no_password(&self) -> bool {
        self.passwords().is_empty()
    }

    fn passwords(&self) -> MutexGuard<'_, Vec<String>> {
        // A panic elsewhere poisons the lock but leaves the passwords intact,
        // so recover rather than shutting everybody out.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A password as it is kept: its SHA-256 hash, written in lower-case hex.
fn hashed(password: &[u8]) -> String {
    Sha256::digest(password)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_a_password_the_way_redis_hashes_one() {
        // Lower-case hex, and the hash of the password rather than of anything
        // around it.
        assert_eq!(
            hashed(b"mypassword"),
            "89e01536ac207279409d4de1e5253e01f4a1769e696db0d6062ca9b8f56767c8"
        );
        assert_eq!(
            hashed(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn wants_no_password_until_it_is_given_one() {
        let users = Users::default();

        assert!(users.wants_no_password());
        assert!(users.hashed_passwords().is_empty());

        users.add_password(b"mypassword");

        assert!(!users.wants_no_password());
    }

    #[test]
    fn keeps_the_password_hashed_rather_than_as_it_was_given() {
        let users = Users::default();

        users.add_password(b"mypassword");

        assert_eq!(
            users.hashed_passwords(),
            ["89e01536ac207279409d4de1e5253e01f4a1769e696db0d6062ca9b8f56767c8"]
        );
    }

    #[test]
    fn keeps_every_password_it_is_given_in_the_order_it_was_given() {
        let users = Users::default();

        users.add_password(b"first");
        users.add_password(b"second");

        assert_eq!(
            users.hashed_passwords(),
            [hashed(b"first"), hashed(b"second")]
        );
    }

    #[test]
    fn keeps_one_password_once_however_often_it_is_given() {
        let users = Users::default();

        users.add_password(b"mypassword");
        users.add_password(b"mypassword");

        assert_eq!(users.hashed_passwords().len(), 1);
    }

    #[test]
    fn takes_a_password_that_is_not_text() {
        let users = Users::default();

        // A password is a run of bytes, as a value is, and may hold anything.
        users.add_password(b"\xff\x00\r\n");

        assert_eq!(users.hashed_passwords(), [hashed(b"\xff\x00\r\n")]);
    }

    #[test]
    fn is_one_set_of_passwords_however_many_hands_hold_it() {
        let users = Users::default();
        let shared = users.clone();

        shared.add_password(b"mypassword");

        assert!(!users.wants_no_password());
    }
}
