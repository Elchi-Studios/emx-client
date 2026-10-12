// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where the token lives: a file only the person can read, in the place
//! each system keeps such things. `EMX_TOKEN` and `EMX_BASE_URL` in the
//! environment win over the file, for scripts and servers.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub token: String,
    pub base_url: String,
}

pub fn path() -> Result<PathBuf, String> {
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let dir = dir.ok_or("no home directory to keep the token in; set EMX_TOKEN instead")?;
    Ok(dir.join("emx").join("config.json"))
}

/// The file's settings, under the environment's. Without a home folder
/// there is no file, which is fine when `EMX_TOKEN` is set, as under a
/// service manager or in a container.
pub fn load() -> Result<Config, String> {
    let mut c = Config::default();
    if let Some(raw) = path().ok().and_then(|p| fs::read_to_string(p).ok()) {
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("the config file is not JSON: {e}"))?;
        c.token = v.get("token").and_then(|t| t.as_str()).unwrap_or("").to_string();
        c.base_url = v
            .get("baseUrl")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
    }
    if let Ok(t) = std::env::var("EMX_TOKEN") {
        if !t.trim().is_empty() {
            c.token = t.trim().to_string();
        }
    }
    if let Ok(u) = std::env::var("EMX_BASE_URL") {
        if !u.trim().is_empty() {
            c.base_url = u.trim().to_string();
        }
    }
    if c.base_url.is_empty() {
        c.base_url = emx_sdk::DEFAULT_BASE_URL.to_string();
    }
    Ok(c)
}

pub fn save(c: &Config) -> Result<PathBuf, String> {
    let p = path()?;
    if let Some(dir) = p.parent() {
        create_private_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let body = serde_json::json!({"token": c.token, "baseUrl": c.base_url});
    let text = format!("{}\n", serde_json::to_string_pretty(&body).unwrap());
    write_private(&p, text.as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

/// The folder for the file, readable by the person alone when it is made.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

/// Writes `data` to `path` so that the file is either the old one or the
/// new one, never empty or half written, and readable by the person
/// alone: a file beside it is written and synced first, then put in its
/// place.
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        #[cfg(unix)]
        {
            // A file left from before may have wider permissions.
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn remove() -> Result<(), String> {
    let p = path()?;
    match fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_replaced_whole_and_private() {
        let dir = std::env::temp_dir().join(format!("emx-config-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        create_private_dir(&dir).unwrap();
        let p = dir.join("config.json");
        fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        }
        write_private(&p, b"new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "nothing beside the file is left");
        let _ = fs::remove_dir_all(&dir);
    }
}
