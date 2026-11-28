//! Where the token is kept: the system's keychain (Credential Manager on
//! Windows, Keychain on macOS, the Secret Service on Linux), not a file.
//!
//! Earlier versions wrote `session.json` into the app's data folder. A
//! token found there is moved into the keychain, and the file removed
//! only after the keychain has it. Where there is no keychain to talk to
//! (a Linux session without a Secret Service, for one), the file is
//! still used, as before, so the app keeps working there.

use std::fs;
use std::path::Path;

/// The one secret the app keeps.
pub trait Vault {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, value: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// The system's keychain.
pub struct Keychain;

const SERVICE: &str = "ch.emxmail.desktop";
const USER: &str = "session";

impl Keychain {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, USER).map_err(|e| e.to_string())
    }
}

impl Vault for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        match Self::entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, value: &str) -> Result<(), String> {
        Self::entry()?.set_password(value).map_err(|e| e.to_string())
    }

    fn delete(&self) -> Result<(), String> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn encode(base_url: &str, token: &str) -> String {
    serde_json::json!({"baseUrl": base_url, "token": token}).to_string()
}

fn decode(raw: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let base = v.get("baseUrl")?.as_str()?.to_string();
    let token = v.get("token")?.as_str()?.to_string();
    Some((base, token))
}

fn write_file(path: &Path, body: &str) -> Result<(), String> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(body.as_bytes()).map_err(|e| e.to_string())
}

/// Keeps the token: in the keychain, and nowhere else when that works.
/// Only when the keychain refuses is it written to `file`, readable by
/// the person alone, and any older copy of the other kind is removed.
pub fn save(vault: &dyn Vault, file: &Path, base_url: &str, token: &str) -> Result<(), String> {
    let body = encode(base_url, token);
    match vault.set(&body) {
        Ok(()) => {
            let _ = fs::remove_file(file);
            Ok(())
        }
        Err(_) => write_file(file, &body),
    }
}

/// The kept token, if any. A token in the old file is moved to the
/// keychain first; the file is removed once the keychain has it, and
/// stays when the keychain cannot take it.
pub fn load(vault: &dyn Vault, file: &Path) -> Option<(String, String)> {
    if let Ok(Some(raw)) = vault.get() {
        if let Some(found) = decode(&raw) {
            // A file left beside a working keychain entry is stale.
            let _ = fs::remove_file(file);
            return Some(found);
        }
    }
    let raw = fs::read_to_string(file).ok()?;
    let found = decode(&raw)?;
    if vault.set(&raw).is_ok() && matches!(vault.get(), Ok(Some(ref back)) if *back == raw) {
        let _ = fs::remove_file(file);
    }
    Some(found)
}

/// Forgets the token wherever it is kept.
pub fn clear(vault: &dyn Vault, file: &Path) {
    let _ = vault.delete();
    let _ = fs::remove_file(file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A keychain in memory; `broken` makes it refuse, as a system
    /// without a Secret Service does.
    #[derive(Default)]
    struct Memory {
        value: RefCell<Option<String>>,
        broken: bool,
    }

    impl Vault for Memory {
        fn get(&self) -> Result<Option<String>, String> {
            if self.broken {
                return Err("no keychain".into());
            }
            Ok(self.value.borrow().clone())
        }
        fn set(&self, v: &str) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            *self.value.borrow_mut() = Some(v.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("emx-keychain-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("session.json")
    }

    #[test]
    fn an_old_token_file_moves_into_the_keychain() {
        let file = scratch("move");
        fs::write(&file, r#"{"baseUrl":"https://emxmail.ch","token":"emx_old"}"#).unwrap();
        let vault = Memory::default();
        let (base, token) = load(&vault, &file).unwrap();
        assert_eq!((base.as_str(), token.as_str()), ("https://emxmail.ch", "emx_old"));
        assert!(!file.exists(), "the file is gone once the keychain has the token");
        let kept = vault.get().unwrap().unwrap();
        assert_eq!(decode(&kept).unwrap().1, "emx_old");
        // The next start reads the keychain alone.
        assert_eq!(load(&vault, &file).unwrap().1, "emx_old");
    }

    #[test]
    fn the_file_stays_when_the_keychain_cannot_take_the_token() {
        let file = scratch("broken");
        fs::write(&file, r#"{"baseUrl":"https://emxmail.ch","token":"emx_old"}"#).unwrap();
        let vault = Memory {
            broken: true,
            ..Default::default()
        };
        assert_eq!(load(&vault, &file).unwrap().1, "emx_old");
        assert!(file.exists(), "nothing is lost when the move fails");
    }

    #[test]
    fn a_new_token_goes_to_the_keychain_and_not_to_disk() {
        let file = scratch("save");
        let vault = Memory::default();
        save(&vault, &file, "https://emxmail.ch", "emx_new").unwrap();
        assert!(!file.exists());
        assert_eq!(load(&vault, &file).unwrap().1, "emx_new");
    }

    #[test]
    fn without_a_keychain_a_new_token_is_kept_in_the_file() {
        let file = scratch("fallback");
        let vault = Memory {
            broken: true,
            ..Default::default()
        };
        save(&vault, &file, "https://emxmail.ch", "emx_new").unwrap();
        assert_eq!(load(&vault, &file).unwrap().1, "emx_new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn the_keychain_wins_over_a_stale_file_and_clear_forgets_both() {
        let file = scratch("clear");
        let vault = Memory::default();
        save(&vault, &file, "https://emxmail.ch", "emx_new").unwrap();
        fs::write(&file, r#"{"baseUrl":"https://emxmail.ch","token":"emx_stale"}"#).unwrap();
        assert_eq!(load(&vault, &file).unwrap().1, "emx_new");
        assert!(!file.exists());
        fs::write(&file, "{}").unwrap();
        clear(&vault, &file);
        assert!(load(&vault, &file).is_none());
        assert!(!file.exists());
    }
}
