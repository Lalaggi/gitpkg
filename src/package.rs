use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

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
                if let Ok(content) = fs::read_to_string(&info_path) {
                    if let Ok(info) = toml::from_str::<toml::Value>(&content) {
                        let supplier = info
                            .get("supplier")
                            .and_then(|v| v.as_str())
                            .unwrap_or("github.com")
                            .to_string();
                        matches.push((pkg_key, supplier, info_path));
                    }
                }
            }
        }
    }

    matches
}

pub fn find_package_by_key(pkg_key: &str) -> Option<(String, String, String)> {
    let packages = read_package_list();

    if let Some(info_path) = packages.get(pkg_key) {
        if let Ok(content) = fs::read_to_string(info_path) {
            if let Ok(info) = toml::from_str::<toml::Value>(&content) {
                let supplier = info
                    .get("supplier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("github.com")
                    .to_string();
                return Some((pkg_key.to_string(), supplier, info_path.clone()));
            }
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

#[derive(Serialize)]
struct PackageInfo {
    user: String,
    repo: String,
    latest_commit: String,
    build_system: String,
    package_manager: String,
    timestamp: String,
    install_path: String,
    symlink_path: String,
    supplier: String,
    has_data_files: bool,
    system_wide: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system_deps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    make_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_flags: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    submodules: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    data_symlinks: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    desktop_symlinks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    desktop_file: Option<String>,
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
