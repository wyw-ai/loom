use std::collections::HashSet;

use parking_lot::RwLock;
use sha2::{Digest, Sha256};

/// Optional shared-password gate for one loom-server process.
///
/// The cleartext startup password is reduced to a digest immediately and is
/// never persisted by the server. Authentication is connection-local: a new
/// WebSocket/local transport must authenticate once before using business RPCs.
pub struct ServerAuth {
    password_digest: Option<[u8; 32]>,
    authenticated_connections: RwLock<HashSet<String>>,
}

impl ServerAuth {
    pub fn disabled() -> Self {
        Self {
            password_digest: None,
            authenticated_connections: RwLock::new(HashSet::new()),
        }
    }

    pub fn with_password(password: &str) -> Result<Self, &'static str> {
        if password.is_empty() {
            return Err("server password cannot be empty");
        }
        Ok(Self {
            password_digest: Some(password_digest(password)),
            authenticated_connections: RwLock::new(HashSet::new()),
        })
    }

    pub fn required(&self) -> bool {
        self.password_digest.is_some()
    }

    pub fn is_authenticated(&self, connection_id: &str) -> bool {
        !self.required()
            || self
                .authenticated_connections
                .read()
                .contains(connection_id)
    }

    pub fn authenticate(&self, connection_id: &str, password: &str) -> bool {
        let Some(expected) = self.password_digest else {
            return true;
        };
        let supplied = password_digest(password);
        if !constant_time_eq(&expected, &supplied) {
            return false;
        }
        self.authenticated_connections
            .write()
            .insert(connection_id.to_string());
        true
    }

    pub fn forget(&self, connection_id: &str) {
        self.authenticated_connections.write().remove(connection_id);
    }
}

fn password_digest(password: &str) -> [u8; 32] {
    Sha256::digest(password.as_bytes()).into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_auth_accepts_every_connection_without_login() {
        let auth = ServerAuth::disabled();
        assert!(!auth.required());
        assert!(auth.is_authenticated("conn_a"));
    }

    #[test]
    fn password_auth_is_connection_local_and_forgettable() {
        let auth = ServerAuth::with_password("correct horse battery staple").expect("auth");
        assert!(auth.required());
        assert!(!auth.is_authenticated("conn_a"));
        assert!(!auth.authenticate("conn_a", "wrong"));
        assert!(!auth.is_authenticated("conn_a"));
        assert!(auth.authenticate("conn_a", "correct horse battery staple"));
        assert!(auth.is_authenticated("conn_a"));
        assert!(!auth.is_authenticated("conn_b"));
        auth.forget("conn_a");
        assert!(!auth.is_authenticated("conn_a"));
    }

    #[test]
    fn empty_password_is_rejected() {
        assert!(ServerAuth::with_password("").is_err());
    }
}
