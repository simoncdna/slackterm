use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::session::Session;

pub trait SessionStore {
    fn save(&self, session: &Session) -> Result<Saved>;
    fn load(&self) -> Result<Option<Session>>;
    /// Returns whether a session was stored.
    fn clear(&self) -> Result<bool>;
}

/// Where a session was saved.
#[derive(Debug, PartialEq)]
pub enum Saved {
    Keychain,
    /// In a file only the user can read, because the keychain refused it.
    File {
        path: PathBuf,
        keychain_error: String,
    },
}

/// Keeps the session in the system keychain (Keychain on macOS, Credential
/// Manager on Windows, Secret Service on Linux), or in a file only the user
/// can read where there is none: SSH sessions, WSL, Linux desktops without
/// gnome-keyring.
pub struct SystemStore<K = Keyring> {
    keychain: K,
    file: PathBuf,
}

impl<K: Keychain> SystemStore<K> {
    /// Whether the system keychain can be reached.
    pub fn check_keychain(&self) -> Result<()> {
        self.keychain.get().map(|_| ())
    }

    /// Where the session goes when there is no keychain.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl SystemStore {
    pub fn new() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "slackterm")
            .context("could not find the data directory")?;
        Ok(Self {
            keychain: Keyring,
            file: dirs.data_local_dir().join("session.json"),
        })
    }
}

impl<K: Keychain> SessionStore for SystemStore<K> {
    fn save(&self, session: &Session) -> Result<Saved> {
        let json = serde_json::to_string(session)?;
        match self.keychain.set(&json) {
            Ok(()) => {
                // Saved while there was no keychain, and now out of date.
                remove_file(&self.file)?;
                Ok(Saved::Keychain)
            }
            Err(e) => {
                write_private(&self.file, &json)?;
                Ok(Saved::File {
                    path: self.file.clone(),
                    keychain_error: format!("{e:#}"),
                })
            }
        }
    }

    /// A keychain that cannot be reached holds nothing usable, so its errors
    /// only send the search on to the file.
    fn load(&self) -> Result<Option<Session>> {
        if let Ok(Some(json)) = self.keychain.get() {
            return Ok(Some(serde_json::from_str(&json)?));
        }
        match fs::read_to_string(&self.file) {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .with_context(|| format!("{} is invalid", self.file.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("could not read {}", self.file.display())),
        }
    }

    fn clear(&self) -> Result<bool> {
        let in_keychain = self.keychain.delete().unwrap_or(false);
        let in_file = remove_file(&self.file)?;
        Ok(in_keychain || in_file)
    }
}

/// The one keychain item that holds the session.
pub trait Keychain {
    fn set(&self, secret: &str) -> Result<()>;
    fn get(&self) -> Result<Option<String>>;
    /// Returns whether there was an item to delete.
    fn delete(&self) -> Result<bool>;
}

/// The system keychain. The whole session is one item, so reading it costs
/// at most one keychain access prompt.
pub struct Keyring;

const SERVICE: &str = "slackterm";
const ACCOUNT: &str = "session";

impl Keychain for Keyring {
    fn set(&self, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, ACCOUNT)?.set_password(secret)?;
        Ok(())
    }

    fn get(&self) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, ACCOUNT)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self) -> Result<bool> {
        match keyring::Entry::new(SERVICE, ACCOUNT)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

/// Writes `contents` to `path`, readable and writable by the user only.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("could not write {}", path.display()))?;
    // The mode above only applies when the file is created.
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .with_context(|| format!("could not restrict access to {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("could not write {}", path.display()))
}

/// Returns whether the file was there.
fn remove_file(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("could not delete {}", path.display())),
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::cell::RefCell<Option<String>>);

#[cfg(test)]
impl SessionStore for MemoryStore {
    fn save(&self, session: &Session) -> Result<Saved> {
        *self.0.borrow_mut() = Some(serde_json::to_string(session)?);
        Ok(Saved::Keychain)
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use anyhow::bail;

    use super::*;
    use crate::session::Workspace;

    /// Like a Linux server reached over SSH.
    struct NoKeychain;

    impl Keychain for NoKeychain {
        fn set(&self, _: &str) -> Result<()> {
            bail!("no secret service")
        }
        fn get(&self) -> Result<Option<String>> {
            bail!("no secret service")
        }
        fn delete(&self) -> Result<bool> {
            bail!("no secret service")
        }
    }

    #[derive(Default)]
    struct MemoryKeychain(RefCell<Option<String>>);

    impl Keychain for MemoryKeychain {
        fn set(&self, secret: &str) -> Result<()> {
            *self.0.borrow_mut() = Some(secret.to_string());
            Ok(())
        }
        fn get(&self) -> Result<Option<String>> {
            Ok(self.0.borrow().clone())
        }
        fn delete(&self) -> Result<bool> {
            Ok(self.0.borrow_mut().take().is_some())
        }
    }

    fn session(team_name: &str) -> Session {
        Session {
            cookie_d: "xoxd-a".into(),
            workspaces: vec![Workspace {
                team_id: "T1".into(),
                team_name: team_name.into(),
                url: "https://acme.slack.com/".into(),
                user_id: "U1".into(),
                user_name: "alex".into(),
                token: "xoxc-1".into(),
            }],
        }
    }

    fn team_name(store: &impl SessionStore) -> Option<String> {
        let session = store.load().unwrap()?;
        Some(session.workspaces[0].team_name.clone())
    }

    #[test]
    fn falls_back_to_a_private_file_without_a_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data").join("session.json");
        let store = SystemStore {
            keychain: NoKeychain,
            file: file.clone(),
        };
        assert_eq!(team_name(&store), None);

        let saved = store.save(&session("Acme")).unwrap();

        assert_eq!(
            saved,
            Saved::File {
                path: file.clone(),
                keychain_error: "no secret service".into()
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(team_name(&store).as_deref(), Some("Acme"));
        assert!(store.clear().unwrap());
        assert!(!file.exists());
        assert!(!store.clear().unwrap());
    }

    #[test]
    fn prefers_the_keychain_once_there_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        let without = SystemStore {
            keychain: NoKeychain,
            file: file.clone(),
        };
        without.save(&session("Old")).unwrap();
        let store = SystemStore {
            keychain: MemoryKeychain::default(),
            file: file.clone(),
        };
        assert_eq!(team_name(&store).as_deref(), Some("Old"));

        assert_eq!(store.save(&session("New")).unwrap(), Saved::Keychain);

        assert!(!file.exists(), "the outdated file is deleted");
        assert_eq!(team_name(&store).as_deref(), Some("New"));
        assert!(store.clear().unwrap());
        assert_eq!(team_name(&store), None);
    }
}
