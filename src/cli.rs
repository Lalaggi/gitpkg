use std::process::Command;
use std::sync::OnceLock;

/// Superuser provider used for privileged operations (symlinks in /usr/bin,
/// system icon installs, and system-package installs/removals).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Superuser {
    Sudo,
    Pkexec,
    Doas,
}

impl Superuser {
    /// Parse a provider name. Accepts `sudo`, `pkexec`, `doas`, and
    /// `auto` (resolves to the first available provider).
    pub fn from_str(s: &str) -> Option<Superuser> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sudo" => Some(Superuser::Sudo),
            "pkexec" => Some(Superuser::Pkexec),
            "doas" => Some(Superuser::Doas),
            "auto" => Superuser::detect(),
            _ => None,
        }
    }

    /// Detect the first available provider: sudo, then pkexec, then doas.
    pub fn detect() -> Option<Superuser> {
        if is_installed("sudo") {
            Some(Superuser::Sudo)
        } else if is_installed("pkexec") {
            Some(Superuser::Pkexec)
        } else if is_installed("doas") {
            Some(Superuser::Doas)
        } else {
            None
        }
    }

    /// The program name for this provider.
    pub fn program(&self) -> &'static str {
        match self {
            Superuser::Sudo => "sudo",
            Superuser::Pkexec => "pkexec",
            Superuser::Doas => "doas",
        }
    }

    /// Verify the provider can elevate (used before system-wide installs).
    /// Returns false (and prints a hint) if elevation is unavailable.
    pub fn auth(&self) -> bool {
        let check = match self {
            // `sudo -v` primes credentials; fall back to `sudo true`.
            Superuser::Sudo => vec!["-v"],
            // `pkexec true` prompts via PolicyKit.
            Superuser::Pkexec => vec!["true"],
            // `doas -n true` checks cached auth non-interactively.
            Superuser::Doas => vec!["-n", "true"],
        };
        let status = Command::new(self.program()).args(&check).status();
        match status {
            Ok(s) => s.success(),
            Err(e) => {
                eprintln!("Failed to run {}: {}", self.program(), e);
                false
            }
        }
    }

    /// Build a `Command` that runs `args` under this provider.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(self.program());
        cmd.args(args);
        cmd
    }
}

static SUPERUSER: OnceLock<Superuser> = OnceLock::new();

/// Resolve and store the superuser provider. Called once at startup from
/// `main` using the CLI flag / config value. Falls back to auto-detect,
/// then to `sudo` (which will simply fail loudly if absent).
pub fn set_superuser(value: &str) {
    // Recognise "auto" and the three explicit providers. Anything else is
    // unrecognised: warn and fall back to auto-detect, then to sudo.
    let resolved = if value.trim().eq_ignore_ascii_case("auto") {
        Superuser::detect().unwrap_or(Superuser::Sudo)
    } else if let Some(s) = Superuser::from_str(value) {
        s
    } else {
        eprintln!(
            "Warning: unrecognised superuser provider '{}'. \
             Expected one of: sudo, pkexec, doas, auto.",
            value
        );
        Superuser::detect().unwrap_or(Superuser::Sudo)
    };
    let _ = SUPERUSER.set(resolved);
}

/// The resolved superuser provider for this process.
pub fn superuser() -> Superuser {
    *SUPERUSER.get().unwrap_or(&Superuser::Sudo)
}

/// Run `args` under the resolved superuser provider.
/// Returns `None` if the command could not be spawned.
pub fn run_as(args: &[&str]) -> Option<std::process::ExitStatus> {
    superuser().command(args).status().ok()
}

/// Check that the resolved provider can elevate privileges.
pub fn superuser_auth() -> bool {
    superuser().auth()
}


pub fn resolve_self_alias(arg: &str) -> String {
    if arg == "self" {
        "el1lovescomputers/gitpkg".to_string()
    } else {
        arg.to_string()
    }
}

pub fn resolve_supplier_shortname(arg: &str) -> String {
    match arg {
        "gh" | "github" => "github.com".to_string(),
        "gl" | "gitlab" => "gitlab.com".to_string(),
        "cb" | "codeberg" => "codeberg.org".to_string(),
        "glg" | "gnome" | "gnome.gitlab" | "gnome-gitlab" | "gitlab.gnome" | "gitlab-gnome" => {
            "gitlab.gnome.org".to_string()
        }
        _ => arg.to_string(),
    }
}

/// Build the clone URL for a repository.
///
/// Precedence (highest first):
/// 1. `remote_url` — a previously stored remote (e.g. an SSH URL the user
///    cloned with). Always reused so upgrades of a codeberg/SSH repo keep using
///    SSH instead of being forced back to HTTPS.
/// 2. `ssh` — build a `git@<supplier>:<user>/<repo>.git` URL.
/// 3. default — `https://<supplier>/<user>/<repo>.git`.
pub fn build_git_url_with(
    user: &str,
    repo: &str,
    supplier: Option<&str>,
    ssh: bool,
    remote_url: Option<&str>,
) -> String {
    if let Some(url) = remote_url {
        if !url.is_empty() {
            return url.to_string();
        }
    }

    if ssh {
        let supplier_domain = supplier.unwrap_or("github.com");
        let repo_name = if repo.ends_with(".git") {
            repo.to_string()
        } else {
            format!("{}.git", repo)
        };
        return format!("git@{}:{}/{}", supplier_domain, user, repo_name);
    }

    build_git_url_inner(user, repo, supplier, false)
}

fn build_git_url_inner(user: &str, repo: &str, supplier: Option<&str>, _unused: bool) -> String {
    let supplier_domain = supplier.unwrap_or("github.com");

    let repo_name = if repo.ends_with(".git") {
        repo.to_string()
    } else {
        format!("{}.git", repo)
    };

    format!("https://{}/{}/{}", supplier_domain, user, repo_name)
}

/// Capture the configured `origin` remote URL of an already-cloned repo, so it
/// can be persisted and reused on upgrades (preserves SSH remotes). Best effort:
/// returns `None` if git is unavailable or the remote can't be read.
pub fn get_remote_url(path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .current_dir(path)
        .output()
        .ok()?;
    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    } else {
        None
    }
}

pub fn normalize_supplier(supplier: &str) -> String {
    supplier
        .trim_end_matches(".com")
        .trim_end_matches(".org")
        .trim_end_matches(".net")
        .trim_end_matches(".io")
        .trim_end_matches(".dev")
        .to_string()
}

pub fn is_installed(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn detect_package_manager() -> Option<&'static str> {
    ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"]
        .iter()
        .find(|&&pm| is_installed(pm))
        .copied()
}

pub fn install_system_package(pm: &str, package_name: &str) -> bool {
    let pm_args: Vec<&str> = match pm {
        "apt" => vec!["apt", "install", "-y", package_name],
        "dnf" => vec!["dnf", "install", "-y", package_name],
        "yum" => vec!["yum", "install", "-y", package_name],
        "pacman" => vec!["pacman", "-Sy", "--noconfirm", package_name],
        "zypper" => vec!["zypper", "install", "-y", package_name],
        "apk" => vec!["apk", "add", package_name],
        "nix-env" => vec!["nix-env", "-iA", package_name],
        _ => {
            eprintln!("Unsupported package manager: {}", pm);
            return false;
        }
    };
    let status = superuser().command(&pm_args).status();
    match status {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("Failed to install {}: {}", package_name, e);
            false
        }
    }
}

/// Remove previously-installed system packages. Mirrors `install_system_package`.
/// Returns `true` if every removal succeeded (or the package was already gone).
pub fn remove_system_package(pm: &str, package_name: &str) -> bool {
    let pm_args: Vec<&str> = match pm {
        "apt" => vec!["apt", "remove", "-y", package_name],
        "dnf" => vec!["dnf", "remove", "-y", package_name],
        "yum" => vec!["yum", "remove", "-y", package_name],
        "pacman" => vec!["pacman", "-R", "--noconfirm", package_name],
        "zypper" => vec!["zypper", "remove", "-y", package_name],
        "apk" => vec!["apk", "del", package_name],
        "nix-env" => vec!["nix-env", "-e", package_name],
        _ => {
            eprintln!("Unsupported package manager: {}", pm);
            return false;
        }
    };
    let status = superuser().command(&pm_args).status();
    match status {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("Failed to remove {}: {}", package_name, e);
            false
        }
    }
}
