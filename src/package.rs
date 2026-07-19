use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Resolve the user's home directory without panicking.
/// Returns `None` (instead of crashing) when HOME is unset, so callers can
/// report a clear error rather than a cryptic `.unwrap()` panic under e.g.
/// `sudo -E` with HOME stripped.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
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

pub fn list_file_path() -> String {
    match home_dir() {
        Some(h) => h.join(".local/share/gitpkg/list.gitpkg").to_string_lossy().into_owned(),
        None => {
            eprintln!("Error: HOME environment variable is not set; cannot locate gitpkg state.");
            std::process::exit(1);
        }
    }
}

pub fn read_package_list() -> HashMap<String, String> {
    let list_path = list_file_path();
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

pub fn write_package_list(packages: &HashMap<String, String>) {
    let list_path = list_file_path();

    if let Some(parent) = Path::new(&list_path).parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let mut content = String::new();
    let mut sorted_packages: Vec<_> = packages.iter().collect();
    sorted_packages.sort_by_key(|(k, _)| k.as_str());

    for (pkg, path) in sorted_packages {
        content.push_str(&format!("{} = {}\n", pkg, path));
    }

    fs::write(&list_path, content).unwrap();
}

pub fn add_to_package_list(user: &str, repo: &str, info_path: &str, supplier: &str) {
    let mut packages = read_package_list();
    let key = get_package_key(user, repo, supplier);
    packages.insert(key, info_path.to_string());
    write_package_list(&packages);
}

pub fn remove_from_package_list(package_key: &str) {
    let mut packages = read_package_list();
    packages.remove(package_key);
    write_package_list(&packages);
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
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;

    if let Ok(choice) = input.trim().parse::<usize>() {
        if choice >= 1 && choice <= matches.len() {
            return Some(choice - 1);
        }
    }

    None
}

pub fn temp_path(user: &str, repo: &str) -> String {
    let hash = format!("{:x}", md5::compute(format!("{}{}", user, repo)));
    match home_dir() {
        Some(h) => h
            .join(".local/share/gitpkg/temp")
            .join(hash)
            .to_string_lossy()
            .into_owned(),
        None => {
            eprintln!("Error: HOME environment variable is not set; cannot create temp dir.");
            std::process::exit(1);
        }
    }
}

pub fn install_root(user: &str, repo: &str, commit: &str, supplier: &str) -> String {
    let pkg_key = get_package_key(user, repo, supplier);
    match home_dir() {
        Some(h) => h
            .join(".local/share/gitpkg")
            .join(pkg_key)
            .join(commit)
            .to_string_lossy()
            .into_owned(),
        None => {
            eprintln!("Error: HOME environment variable is not set; cannot determine install root.");
            std::process::exit(1);
        }
    }
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
) {
    use chrono::Utc;

    let pkg_key = get_package_key(user, repo, supplier);
    let info_dir = match home_dir() {
        Some(h) => h
            .join(".local/share/gitpkg")
            .join(pkg_key)
            .to_string_lossy()
            .into_owned(),
        None => {
            eprintln!("Error: HOME environment variable is not set; cannot write info file.");
            std::process::exit(1);
        }
    };
    fs::create_dir_all(&info_dir).unwrap();
    let info_file = Path::new(&info_dir).join("info.gitpkg");

    let mut toml_data = format!(
        "user = \"{}\"\n\
         repo = \"{}\"\n\
         latest_commit = \"{}\"\n\
         build_system = \"{}\"\n\
         package_manager = \"{}\"\n\
         timestamp = \"{}\"\n\
         install_path = \"{}\"\n\
         symlink_path = \"{}\"\n\
         supplier = \"{}\"\n\
         has_data_files = {}\n\
         system_wide = {}\n",
        user,
        repo,
        commit,
        build_system,
        pm,
        Utc::now().to_rfc3339(),
        install_path,
        symlink_path,
        supplier,
        has_data_files,
        system_wide
    );

    if !installed_deps.is_empty() {
        toml_data.push_str("system_deps = [\n");
        for d in installed_deps {
            toml_data.push_str(&format!("  \"{}\",\n", d));
        }
        toml_data.push_str("]\n");
    }

    if let Some(r) = remote_url {
        if !r.is_empty() {
            toml_data.push_str(&format!(
                "remote_url = \"{}\"\n",
                r.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
    }

    if let Some(b) = branch {
        toml_data.push_str(&format!("branch = \"{}\"\n", b));
    }

    if let Some(t) = make_target {
        toml_data.push_str(&format!("make_target = \"{}\"\n", t));
    }

    if let Some(f) = build_flags {
        toml_data.push_str(&format!("build_flags = \"{}\"\n", f.replace('\\', "\\\\").replace('"', "\\\"")));
    }

    if submodules {
        toml_data.push_str("submodules = true\n");
    }

    if !data_symlinks.is_empty() {
        toml_data.push_str("data_symlinks = [\n");
        for p in data_symlinks {
            toml_data.push_str(&format!("  \"{}\",\n", p.display()));
        }
        toml_data.push_str("]\n");
    }

    if !desktop_symlinks.is_empty() {
        toml_data.push_str("desktop_symlinks = [\n");
        for p in desktop_symlinks {
            toml_data.push_str(&format!("  \"{}\",\n", p.display()));
        }
        toml_data.push_str("]\n");
    }

    if let Some(dp) = desktop_path {
        toml_data.push_str(&format!("desktop_file = \"{}\"\n", dp));
    }

    fs::write(&info_file, toml_data).unwrap();

    add_to_package_list(user, repo, info_file.to_str().unwrap(), supplier);
}
