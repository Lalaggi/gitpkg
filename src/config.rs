use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::GitpkgError;
use crate::package::home_dir;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ConfigFile {
    system: bool,
    ssh: bool,
    remove_deps: bool,
    verbose: bool,
    submodules: bool,
    #[serde(default = "default_superuser")]
    superuser: String,
    java_home: Option<String>,
    forge_usernames: HashMap<String, String>,
}

fn default_superuser() -> String {
    "auto".to_string()
}

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
    /// JAVA_HOME used for gradle builds. Defaults to the ambient JDK when unset.
    pub java_home: Option<String>,
    /// Username mappings per supplier domain, used by `gitpkg migrate`.
    /// e.g. { "codeberg.org": "el1lovescomputers", "github.com": "Lalaggi" }
    pub forge_usernames: HashMap<String, String>,
}

impl Config {
    /// Load the config file, falling back to defaults if it is absent or
    /// unreadable. A malformed file prints a warning but does not abort.
    pub fn load() -> Result<Config, GitpkgError> {
        let path = match config_path() {
            Some(p) => p,
            None => return Ok(Config::default()),
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Ok(Config::default()),
        };

        let file: ConfigFile = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning: could not parse config {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return Ok(Config::default());
            }
        };

        Ok(Config {
            system: file.system,
            ssh: file.ssh,
            remove_deps: file.remove_deps,
            verbose: file.verbose,
            submodules: file.submodules,
            superuser: file.superuser,
            java_home: file.java_home.filter(|s| !s.is_empty()),
            forge_usernames: file.forge_usernames,
        })
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

# JAVA_HOME used for gradle builds. Uncomment to override the ambient JDK
# (e.g. some projects fail on very new JDKs and need an LTS one).
# java_home = "/usr/lib/jvm/java-21-openjdk"

# Forge username mappings used by `gitpkg migrate`.
# Maps supplier domain to your username on that supplier.
[forge_usernames]
"codeberg.org" = "el1lovescomputers"
"github.com" = "Lalaggi"
"#;

    std::fs::write(&path, template)?;
    println!("Wrote default config to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_deserialize_defaults() {
        let toml_str = "";
        let file: ConfigFile = toml::from_str(toml_str).unwrap();
        assert!(!file.system);
        assert!(!file.ssh);
        assert!(!file.remove_deps);
        assert!(!file.verbose);
        assert!(!file.submodules);
        assert_eq!(file.superuser, "auto");
        assert!(file.java_home.is_none());
        assert!(file.forge_usernames.is_empty());
    }

    #[test]
    fn test_config_file_deserialize_full() {
        let toml_str = r#"
system = true
ssh = true
remove_deps = true
verbose = true
submodules = true
superuser = "doas"
java_home = "/usr/lib/jvm/java-21"

[forge_usernames]
"codeberg.org" = "alice"
"github.com" = "bob"
"#;
        let file: ConfigFile = toml::from_str(toml_str).unwrap();
        assert!(file.system);
        assert!(file.ssh);
        assert!(file.remove_deps);
        assert!(file.verbose);
        assert!(file.submodules);
        assert_eq!(file.superuser, "doas");
        assert_eq!(file.java_home.as_deref(), Some("/usr/lib/jvm/java-21"));
        assert_eq!(file.forge_usernames.get("codeberg.org").unwrap(), "alice");
        assert_eq!(file.forge_usernames.get("github.com").unwrap(), "bob");
    }

    #[test]
    fn test_config_file_partial() {
        let toml_str = r#"
system = true
superuser = "pkexec"
"#;
        let file: ConfigFile = toml::from_str(toml_str).unwrap();
        assert!(file.system);
        assert!(!file.ssh);
        assert_eq!(file.superuser, "pkexec");
    }
}
