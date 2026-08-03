use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::GitpkgError;

/// Resolve the user's home directory without panicking.
/// Returns `None` (instead of crashing) when HOME is unset, so callers can
/// report a clear error rather than a cryptic `.unwrap()` panic under e.g.
/// `sudo -E` with HOME stripped.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub fn home_dir_or_err() -> Result<PathBuf, GitpkgError> {
    home_dir().ok_or(GitpkgError::HomeNotFound)
}

pub fn parse_pkg(arg: &str) -> (String, String) {
    if let Some(underscore_pos) = arg.find('_') {
        if let Some(slash_pos) = arg.find('/') {
            if underscore_pos < slash_pos {
                let user_part = &arg[underscore_pos + 1..slash_pos];
                let repo_part = &arg[slash_pos + 1..];
                return (user_part.to_string(), repo_part.to_string());
            }
        }
    }

    let mut parts = arg.split('/');
    let user = parts.next().unwrap_or("").to_string();
    let repo = parts.next().unwrap_or("").to_string();
    (user, repo)
}

pub fn parse_pkg_with_supplier(arg: &str) -> (String, String, Option<String>) {
    if let Some(underscore_pos) = arg.find('_') {
        if let Some(slash_pos) = arg.find('/') {
            if underscore_pos < slash_pos {
                let supplier_part = &arg[..underscore_pos];
                let user_part = &arg[underscore_pos + 1..slash_pos];
                let repo_part = &arg[slash_pos + 1..];

                let supplier_domain = if supplier_part.contains('.') {
                    supplier_part.to_string()
                } else {
                    format!("{}.com", supplier_part)
                };

                return (
                    user_part.to_string(),
                    repo_part.to_string(),
                    Some(supplier_domain),
                );
            }
        }
    }

    let (user, repo) = parse_pkg(arg);
    (user, repo, None)
}

pub fn get_package_key(user: &str, repo: &str, supplier: &str) -> String {
    use crate::cli::normalize_supplier;
    if supplier == "github.com" {
        format!("{}/{}", user, repo)
    } else {
        format!("{}_{}/{}", normalize_supplier(supplier), user, repo)
    }
}

pub fn list_file_path() -> Result<String, GitpkgError> {
    let h = home_dir_or_err()?;
    Ok(h.join(".local/share/gitpkg/list.gitpkg").to_string_lossy().into_owned())
}

pub fn read_package_list() -> HashMap<String, String> {
    let list_path = match list_file_path() {
        Ok(p) => p,
        Err(_) => return HashMap::new(),
    };
    let mut packages = HashMap::new();

    if let Ok(content) = fs::read_to_string(&list_path) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() == 2 {
                packages.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }

    packages
}

pub fn write_package_list(packages: &HashMap<String, String>) -> Result<(), GitpkgError> {
    let list_path = list_file_path()?;

    if let Some(parent) = Path::new(&list_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = String::new();
    let mut sorted_packages: Vec<_> = packages.iter().collect();
    sorted_packages.sort_by_key(|(k, _)| k.as_str());

    for (pkg, path) in sorted_packages {
        content.push_str(&format!("{} = {}\n", pkg, path));
    }

    fs::write(&list_path, content)?;
    Ok(())
}

pub fn add_to_package_list(
    user: &str,
    repo: &str,
    info_path: &str,
    supplier: &str,
) -> Result<(), GitpkgError> {
    let mut packages = read_package_list();
    let key = get_package_key(user, repo, supplier);
    packages.insert(key, info_path.to_string());
    write_package_list(&packages)
}

pub fn remove_from_package_list(package_key: &str) -> Result<(), GitpkgError> {
    let mut packages = read_package_list();
    packages.remove(package_key);
    write_package_list(&packages)
}

/// Remove the old Codeberg package list entry for a package being migrated.
/// Called after migration to clean up the orphaned Codeberg key.
pub fn remove_old_supplier_entry(user: &str, repo: &str, old_supplier: &str) {
    let old_key = get_package_key(user, repo, old_supplier);
    let _ = remove_from_package_list(&old_key);
}

pub fn find_matching_packages(user: &str, repo: &str) -> Vec<(String, String, String)> {
    let packages = read_package_list();
    let mut matches = Vec::new();

    for (pkg_key, info_path) in packages {
        let parts: Vec<&str> = pkg_key.split('/').collect();
        if parts.len() == 2 {
            let pkg_repo = parts[1];
            let pkg_user_part = parts[0];

            let pkg_user = if pkg_user_part.contains('_') {
                pkg_user_part.split('_').last().unwrap_or("")
            } else {
                pkg_user_part
            };

            if pkg_user == user && pkg_repo == repo {
                if let Ok(info) = read_info_file(&info_path) {
                    let supplier = info.supplier;
                    matches.push((pkg_key, supplier, info_path));
                }
            }
        }
    }

    matches
}

pub fn find_package_by_key(pkg_key: &str) -> Option<(String, String, String)> {
    let packages = read_package_list();

    if let Some(info_path) = packages.get(pkg_key) {
        if let Ok(info) = read_info_file(info_path) {
            return Some((pkg_key.to_string(), info.supplier, info_path.clone()));
        }
    }

    let (user, repo) = parse_pkg(pkg_key);
    let matches = find_matching_packages(&user, &repo);

    if matches.len() == 1 {
        return Some(matches[0].clone());
    }

    None
}

pub fn prompt_package_selection(matches: &[(String, String, String)]) -> Option<usize> {
    println!("Multiple packages found:");
    for (i, (pkg_key, supplier, _)) in matches.iter().enumerate() {
        println!("[{}] {}: {}", i + 1, supplier, pkg_key);
    }

    print!("Select package (1-{}): ", matches.len());
    io::stdout().flush().ok()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;

    if let Ok(choice) = input.trim().parse::<usize>() {
        if choice >= 1 && choice <= matches.len() {
            return Some(choice - 1);
        }
    }

    None
}

/// Resolve a package argument to its key, supplier, and info file path.
///
/// Handles exact key matches, supplier-qualified names, and interactive
/// selection when multiple packages match.
pub fn resolve_package(package: &str) -> Result<(String, String, String), GitpkgError> {
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    let exact_match = find_package_by_key(package);

    if let Some(m) = exact_match {
        return Ok(m);
    }

    let matches = find_matching_packages(&user, &repo);
    if matches.is_empty() {
        return Err(GitpkgError::PackageNotFound(package.to_string()));
    }

    let selected = if let Some(ref sup) = supplier_hint {
        matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
    } else if matches.len() > 1 {
        match prompt_package_selection(&matches) {
            Some(idx) => idx,
            None => return Err(GitpkgError::Cancelled),
        }
    } else {
        0
    };

    Ok(matches[selected].clone())
}

pub fn temp_path(user: &str, repo: &str) -> Result<String, GitpkgError> {
    let hash = format!("{:x}", md5::compute(format!("{}{}", user, repo)));
    let h = home_dir_or_err()?;
    Ok(h.join(".local/share/gitpkg/temp")
        .join(hash)
        .to_string_lossy()
        .into_owned())
}

pub fn install_root(
    user: &str,
    repo: &str,
    commit: &str,
    supplier: &str,
) -> Result<String, GitpkgError> {
    let pkg_key = get_package_key(user, repo, supplier);
    let h = home_dir_or_err()?;
    Ok(h.join(".local/share/gitpkg")
        .join(pkg_key)
        .join(commit)
        .to_string_lossy()
        .into_owned())
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageInfo {
    pub user: String,
    pub repo: String,
    pub latest_commit: String,
    pub build_system: String,
    pub package_manager: String,
    pub timestamp: String,
    pub install_path: String,
    pub symlink_path: String,
    pub supplier: String,
    #[serde(default)]
    pub has_data_files: bool,
    #[serde(default)]
    pub system_wide: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_deps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_flags: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub submodules: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_symlinks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop_symlinks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_file: Option<String>,
}

pub fn read_info_file(info_path: &str) -> Result<PackageInfo, GitpkgError> {
    let content = fs::read_to_string(info_path).map_err(|e| {
        GitpkgError::Parse(format!("Failed to read info file: {}", e))
    })?;
    toml::from_str(&content).map_err(|e| {
        GitpkgError::Parse(format!("Failed to parse info file: {}", e))
    })
}

pub fn write_info(
    user: &str,
    repo: &str,
    commit: &str,
    build_system: &str,
    pm: &str,
    install_path: &str,
    symlink_path: &str,
    desktop_path: Option<&str>,
    supplier: &str,
    has_data_files: bool,
    data_symlinks: &[PathBuf],
    desktop_symlinks: &[PathBuf],
    branch: Option<&str>,
    make_target: Option<&str>,
    build_flags: Option<&str>,
    submodules: bool,
    system_wide: bool,
    installed_deps: &[String],
    remote_url: Option<&str>,
) -> Result<(), GitpkgError> {
    use chrono::Utc;

    let pkg_key = get_package_key(user, repo, supplier);
    let info_dir = home_dir_or_err()?
        .join(".local/share/gitpkg")
        .join(pkg_key)
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(&info_dir)?;
    let info_file = Path::new(&info_dir).join("info.gitpkg");

    let info = PackageInfo {
        user: user.to_string(),
        repo: repo.to_string(),
        latest_commit: commit.to_string(),
        build_system: build_system.to_string(),
        package_manager: pm.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        install_path: install_path.to_string(),
        symlink_path: symlink_path.to_string(),
        supplier: supplier.to_string(),
        has_data_files,
        system_wide,
        system_deps: installed_deps.to_vec(),
        remote_url: remote_url
            .filter(|r| !r.is_empty())
            .map(|r| r.to_string()),
        branch: branch.map(|b| b.to_string()),
        make_target: make_target.map(|t| t.to_string()),
        build_flags: build_flags.map(|f| f.to_string()),
        submodules,
        data_symlinks: data_symlinks
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        desktop_symlinks: desktop_symlinks
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        desktop_file: desktop_path.map(|d| d.to_string()),
    };

    let toml_data = toml::to_string_pretty(&info)?;
    fs::write(&info_file, toml_data)?;

    add_to_package_list(user, repo, info_file.to_str().unwrap_or(""), supplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pkg_simple() {
        let (user, repo) = parse_pkg("alice/myrepo");
        assert_eq!(user, "alice");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn test_parse_pkg_supplier_prefix() {
        let (user, repo) = parse_pkg("codeberg_alice/myrepo");
        assert_eq!(user, "alice");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn test_parse_pkg_with_supplier_gh() {
        // "gh" shortname is not resolved by parse_pkg_with_supplier;
        // it's only resolved at the CLI level via resolve_supplier_shortname.
        let (user, repo, supplier) = parse_pkg_with_supplier("gh/alice/myrepo");
        assert_eq!(user, "gh");
        assert_eq!(repo, "alice");
        assert_eq!(supplier, None);
    }

    #[test]
    fn test_parse_pkg_with_supplier_codeberg() {
        let (user, repo, supplier) = parse_pkg_with_supplier("codeberg.org_alice/myrepo");
        assert_eq!(user, "alice");
        assert_eq!(repo, "myrepo");
        assert_eq!(supplier, Some("codeberg.org".to_string()));
    }

    #[test]
    fn test_parse_pkg_with_supplier_no_supplier() {
        let (user, repo, supplier) = parse_pkg_with_supplier("alice/myrepo");
        assert_eq!(user, "alice");
        assert_eq!(repo, "myrepo");
        assert_eq!(supplier, None);
    }

    #[test]
    fn test_get_package_key_github() {
        let key = get_package_key("alice", "myrepo", "github.com");
        assert_eq!(key, "alice/myrepo");
    }

    #[test]
    fn test_get_package_key_codeberg() {
        let key = get_package_key("alice", "myrepo", "codeberg.org");
        assert_eq!(key, "codeberg_alice/myrepo");
    }

    #[test]
    fn test_package_info_deserialize() {
        let toml_str = r#"
user = "alice"
repo = "myrepo"
latest_commit = "abc123"
build_system = "cargo"
package_manager = "unknown"
timestamp = "2025-01-01T00:00:00Z"
install_path = "/home/alice/.local/share/gitpkg/alice/myrepo/abc123"
symlink_path = "/home/alice/.local/bin/myrepo"
supplier = "github.com"
has_data_files = false
system_wide = false
"#;
        let info: PackageInfo = toml::from_str(toml_str).unwrap();
        assert_eq!(info.user, "alice");
        assert_eq!(info.repo, "myrepo");
        assert_eq!(info.latest_commit, "abc123");
        assert_eq!(info.build_system, "cargo");
        assert_eq!(info.supplier, "github.com");
        assert!(!info.has_data_files);
        assert!(!info.system_wide);
        assert!(info.system_deps.is_empty());
        assert!(info.remote_url.is_none());
    }

    #[test]
    fn test_package_info_deserialize_optional_fields() {
        let toml_str = r#"
user = "alice"
repo = "myrepo"
latest_commit = "abc123"
build_system = "make"
package_manager = "apt"
timestamp = "2025-01-01T00:00:00Z"
install_path = "/tmp/install"
symlink_path = "/tmp/bin/myrepo"
supplier = "github.com"
branch = "develop"
make_target = "build-release"
build_flags = "-j4"
submodules = true
system_deps = ["gcc", "make"]
remote_url = "git@github.com:alice/myrepo.git"
"#;
        let info: PackageInfo = toml::from_str(toml_str).unwrap();
        assert_eq!(info.branch.as_deref(), Some("develop"));
        assert_eq!(info.make_target.as_deref(), Some("build-release"));
        assert_eq!(info.build_flags.as_deref(), Some("-j4"));
        assert!(info.submodules);
        assert_eq!(info.system_deps, vec!["gcc", "make"]);
        assert_eq!(info.remote_url.as_deref(), Some("git@github.com:alice/myrepo.git"));
    }

    #[test]
    fn test_package_info_roundtrip() {
        let original = PackageInfo {
            user: "alice".to_string(),
            repo: "myrepo".to_string(),
            latest_commit: "abc123".to_string(),
            build_system: "cargo".to_string(),
            package_manager: "unknown".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            install_path: "/tmp/install".to_string(),
            symlink_path: "/tmp/bin/myrepo".to_string(),
            supplier: "github.com".to_string(),
            has_data_files: false,
            system_wide: false,
            system_deps: vec![],
            remote_url: None,
            branch: None,
            make_target: None,
            build_flags: None,
            submodules: false,
            data_symlinks: vec![],
            desktop_symlinks: vec![],
            desktop_file: None,
        };

        let serialized = toml::to_string_pretty(&original).unwrap();
        let deserialized: PackageInfo = toml::from_str(&serialized).unwrap();
        assert_eq!(original.user, deserialized.user);
        assert_eq!(original.repo, deserialized.repo);
        assert_eq!(original.latest_commit, deserialized.latest_commit);
        assert_eq!(original.build_system, deserialized.build_system);
        assert_eq!(original.supplier, deserialized.supplier);
    }
}
