//! Where the token lives: a file only the person can read, in the place
//! each system keeps such things. `EMX_TOKEN` and `EMX_BASE_URL` in the
//! environment win over the file, for scripts and servers.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

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

pub fn load() -> Result<Config, String> {
    let mut c = Config::default();
    if let Ok(raw) = fs::read_to_string(path()?) {
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
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let body = serde_json::json!({"token": c.token, "baseUrl": c.base_url});
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    #[cfg(unix)]
    {
        // The file may have existed with wider permissions.
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    writeln!(f, "{}", serde_json::to_string_pretty(&body).unwrap()).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn remove() -> Result<(), String> {
    let p = path()?;
    match fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}
