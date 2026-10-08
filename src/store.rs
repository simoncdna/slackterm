use anyhow::Result;

use crate::session::Session;

pub trait SessionStore {
    fn save(&self, session: &Session) -> Result<()>;
    fn load(&self) -> Result<Option<Session>>;
    /// Returns whether a session was stored.
    fn clear(&self) -> Result<bool>;
}

/// Keeps the whole session as one keychain item, so reading it costs at most
/// one keychain access prompt.
pub struct KeyringStore;

const SERVICE: &str = "slackterm";
const ACCOUNT: &str = "session";

impl SessionStore for KeyringStore {
    fn save(&self, session: &Session) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT)?;
        entry.set_password(&serde_json::to_string(session)?)?;
        Ok(())
    }

    fn load(&self) -> Result<Option<Session>> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT)?;
        match entry.get_password() {
            Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn clear(&self) -> Result<bool> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT)?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::cell::RefCell<Option<String>>);

#[cfg(test)]
impl SessionStore for MemoryStore {
    fn save(&self, session: &Session) -> Result<()> {
        *self.0.borrow_mut() = Some(serde_json::to_string(session)?);
        Ok(())
    }

    fn load(&self) -> Result<Option<Session>> {
        Ok(match &*self.0.borrow() {
            Some(json) => Some(serde_json::from_str(json)?),
            None => None,
        })
    }

    fn clear(&self) -> Result<bool> {
        Ok(self.0.borrow_mut().take().is_some())
    }
}
