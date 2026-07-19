use std::path::PathBuf;

use crate::package::home_dir;

/// User configuration loaded from `~/.config/gitpkg/config.toml`.
///
/// Every field mirrors a CLI flag and acts as its default. An explicit CLI
/// flag always overrides the config value.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub system: bool,
    pub ssh: bool,
    pub remove_deps: bool,
    pub verbose: bool,
    pub submodules: bool,
    /// Superuser provider: "sudo", "pkexec", "doas", or "auto".
    pub superuser: String,
}

impl Config {
    /// Load the config file, falling back to defaults if it is absent or
    /// unreadable. A malformed file prints a warning but does not abort.
    pub fn load() -> Config {
        let path = match config_path() {
            Some(p) => p,
            None => return Config::default(),
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Config::default(),
        };

        let value: toml::Value = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning: could not parse config {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return Config::default();
            }
        };

        let get_bool = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };

        Config {
            system: get_bool("system"),
            ssh: get_bool("ssh"),
            remove_deps: get_bool("remove_deps"),
            verbose: get_bool("verbose"),
            submodules: get_bool("submodules"),
            superuser: value
                .get("superuser")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
        }
    }
}

/// Path to the config file: `$XDG_CONFIG_HOME/gitpkg/config.toml`, or
/// `~/.config/gitpkg/config.toml` when XDG_CONFIG_HOME is unset.
#[allow(clippy::collapsible_if)]
pub fn config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("gitpkg").join("config.toml"));
        }
    }
    home_dir().map(|h| h.join(".config").join("gitpkg").join("config.toml"))
}

/// Write a default config file if one does not already exist. Used by the
/// `gitpkg config --init` helper so users have a template to edit.
pub fn write_default() -> std::io::Result<()> {
    let path = match config_path() {
        Some(p) => p,
        None => return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "HOME not set; cannot write config",
        )),
    };

    if path.exists() {
        println!("Config already exists at {}", path.display());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let template = r#"# gitpkg configuration
# Each setting is the default for the matching CLI flag.
# An explicit CLI flag always overrides these values.

# Install the symlink into /usr/bin instead of ~/.local/bin (needs sudo).
system = false

# Clone via git@<host>:<user>/<repo>.git instead of https://.
ssh = false

# `gitpkg remove` also uninstalls system packages gitpkg installed.
remove_deps = false

# Print build/clone output by default.
verbose = false

# Initialise git submodules after cloning.
submodules = false

# Superuser provider for privileged steps (system symlinks, system icons,
# system package installs/removals). One of: sudo, pkexec, doas, auto.
# "auto" picks the first available provider.
superuser = "auto"
"#;

    std::fs::write(&path, template)?;
    println!("Wrote default config to {}", path.display());
    Ok(())
}
