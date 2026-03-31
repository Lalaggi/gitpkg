use std::{
    collections::HashMap,
    env,
    fs::{self},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: gitpkg <install|remove|clean|list|upgrade|update|versions|version|goto|help> [args] [-v] [--supplier <domain>]"
        );
        std::process::exit(1);
    }

    let verbose = args.contains(&"-v".to_string());
    let command = &args[1];

    // Parse --supplier flag
    let supplier = if let Some(pos) = args.iter().position(|arg| arg == "--supplier") {
        if pos + 1 < args.len() {
            Some(args[pos + 1].clone())
        } else {
            eprintln!("Error: --supplier flag requires a domain argument");
            eprintln!("Example: --supplier gitlab.com");
            std::process::exit(1);
        }
    } else {
        None
    };

    match command.as_str() {
        "install" => {
            if args.len() < 3 {
                eprintln!("Usage: gitpkg install <user>/<repo> [--supplier <domain>]");
                return;
            }
            install(&args[2], verbose, supplier.as_deref());
        }
        "remove" => {
            if args.len() < 3 {
                eprintln!("Usage: gitpkg remove <user>/<repo>");
                return;
            }
            let target = resolve_self_alias(&args[2]);
            remove(&target);
        }
        "goto" => {
            if args.len() < 3 {
                eprintln!("Usage: gitpkg goto <user>/<repo> [--shell|-s]");
                return;
            }
            let spawn_shell =
                args.contains(&"--shell".to_string()) || args.contains(&"-s".to_string());
            let target = resolve_self_alias(&args[2]);
            goto(&target, spawn_shell);
        }
        "clean" => {
            if args.len() >= 3 && &args[2] == "all" {
                clean_all();
            } else if args.len() >= 3 {
                let target = resolve_self_alias(&args[2]);
                clean(&target);
            } else {
                clean_all();
            }
        }
        "versions" => {
            if args.len() < 3 {
                eprintln!("Usage: gitpkg versions <user>/<repo>");
                return;
            }
            let target = resolve_self_alias(&args[2]);
            versions(&target);
        }
        "version" => {
            // Alias for versions
            eprintln!("Warning: 'version' is an alias for 'versions'. It is recommended to use 'versions'.");
            if args.len() < 3 {
                eprintln!("Usage: gitpkg version <user>/<repo>");
                return;
            }
            let target = resolve_self_alias(&args[2]);
            versions(&target);
        }
        "list" => list(),
        "upgrade" => {
            // Default to upgrading all when no target is provided
            if args.len() < 3 || &args[2] == "all" {
                upgrade_all(verbose);
            } else {
                let target = if args[2] == "self" {
                    "el1lovescomputers/gitpkg".to_string()
                } else {
                    args[2].clone()
                };
                upgrade(&target, verbose, supplier.as_deref());
            }
        }
        "help" | "-h" | "--help" => {
            println!("gitpkg — minimal git-based package manager");
            println!();
            println!("Usage: gitpkg <command> [args] [-v] [--supplier <domain>]");
            println!();
            println!("Commands:");
            println!("  install <user>/<repo>       Install a package");
            println!("  remove <user>/<repo>        Remove a package");
            println!("  clean <user>/<repo>|all     Remove old versions or all");
            println!("  list                        List installed packages");
            println!("  upgrade [<pkg>|all]         Upgrade package or all (defaults to all)");
            println!("  update [<pkg>|all]          Alias for upgrade (warns)");
            println!("  versions <user>/<repo>      List installed versions for a package");
            println!("  version <user>/<repo>       Alias for versions (warns)");
            println!("  goto <user>/<repo>          Print path to installed package (or spawn shell with -s)");
            println!("  help                        Show this help");
            return;
        }
        "update" => {
            // Alias for upgrade - show a warning recommending 'upgrade'
            eprintln!("Warning: 'update' and 'upgrade' do the same thing. It is recommended to use 'upgrade'.");
            if args.len() < 3 || &args[2] == "all" {
                upgrade_all(verbose);
            } else {
                let target = if args[2] == "self" {
                    "el1lovescomputers/gitpkg".to_string()
                } else {
                    args[2].clone()
                };
                upgrade(&target, verbose, supplier.as_deref());
            }
        }
        _ => eprintln!("Unknown command: {}", command),
    }
}

/// Resolve "self" alias to the author's repo key.
fn resolve_self_alias(arg: &str) -> String {
    if arg == "self" {
        "el1lovescomputers/gitpkg".to_string()
    } else {
        arg.to_string()
    }
}

/// Parse package argument. Handles both simple "user/repo" and supplier-prefixed "supplier_user/repo"
fn parse_pkg(arg: &str) -> (String, String) {
    // Check if this is a supplier-prefixed package key (e.g., "codeberg_el1lovescomputers/gitpkg")
    if let Some(underscore_pos) = arg.find('_') {
        if let Some(slash_pos) = arg.find('/') {
            if underscore_pos < slash_pos {
                // This looks like "supplier_user/repo" format
                // Extract just the user part after the supplier prefix
                let user_part = &arg[underscore_pos + 1..slash_pos];
                let repo_part = &arg[slash_pos + 1..];
                return (user_part.to_string(), repo_part.to_string());
            }
        }
    }

    // Standard "user/repo" format
    let mut parts = arg.split('/');
    let user = parts.next().unwrap_or("").to_string();
    let repo = parts.next().unwrap_or("").to_string();
    (user, repo)
}

/// Parse package with potential supplier info for clean command
fn parse_pkg_with_supplier(arg: &str) -> (String, String, Option<String>) {
    // Check if this is a supplier-prefixed package key
    if let Some(underscore_pos) = arg.find('_') {
        if let Some(slash_pos) = arg.find('/') {
            if underscore_pos < slash_pos {
                // Extract supplier and user
                let supplier_part = &arg[..underscore_pos];
                let user_part = &arg[underscore_pos + 1..slash_pos];
                let repo_part = &arg[slash_pos + 1..];

                // Reconstruct supplier domain
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

fn temp_path(user: &str, repo: &str) -> String {
    let hash = format!("{:x}", md5::compute(format!("{}{}", user, repo)));
    format!(
        "{}/.local/share/gitpkg/temp/{}",
        env::var("HOME").unwrap(),
        hash
    )
}

fn install_root(user: &str, repo: &str, commit: &str, supplier: &str) -> String {
    let pkg_key = get_package_key(user, repo, supplier);
    format!(
        "{}/.local/share/gitpkg/{}/{}",
        env::var("HOME").unwrap(),
        pkg_key,
        commit
    )
}

fn is_installed(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn detect_build_system(path: &str) -> Option<&'static str> {
    // Priority order: Universal build systems first, then language-specific
    for (file, sys) in [
        // Universal build systems (higher priority)
        ("Makefile", "make"),
        ("CMakeLists.txt", "cmake"),
        ("meson.build", "meson"),
        ("mason.toml", "mason"),
        // Language-specific build systems (lower priority)
        ("Cargo.toml", "cargo"),
        ("package.json", "npm"),
        ("build.gradle", "gradle"),
        ("go.mod", "go"),
        // Experimental: Python projects (pyproject.toml) checked later
        ("pyproject.toml", "python"),
    ] {
        if Path::new(path).join(file).exists() {
            return Some(sys);
        }
    }
    None
}

fn detect_package_manager() -> Option<&'static str> {
    ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"]
        .iter()
        .find(|&&pm| is_installed(pm))
        .copied()
}

fn build_system_packages(build_system: &str, pm: &str) -> Option<&'static str> {
    let mut map: HashMap<&str, HashMap<&str, &str>> = HashMap::new();

    let mut cargo_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        cargo_map.insert(p, "rustc");
    }
    map.insert("cargo", cargo_map);

    let mut go_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        go_map.insert(p, if p == "pacman" { "go" } else { "golang" });
    }
    map.insert("go", go_map);

    let mut make_map = HashMap::new();
    make_map.insert("apt", "build-essential");
    make_map.insert("dnf", "gcc gcc-c++ make");
    make_map.insert("yum", "gcc gcc-c++ make");
    make_map.insert("pacman", "base-devel");
    make_map.insert("zypper", "gcc gcc-c++ make");
    make_map.insert("apk", "build-base");
    make_map.insert("nix-env", "gcc");

    for &sys in ["make", "cmake", "meson", "mason"].iter() {
        map.insert(sys, make_map.clone());
    }

    let mut npm_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        npm_map.insert(p, "nodejs npm");
    }
    map.insert("npm", npm_map);

    let mut gradle_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        gradle_map.insert(p, "gradle");
    }
    map.insert("gradle", gradle_map);

    map.get(build_system)?.get(pm).copied()
}

fn get_commit_hash(path: &str) -> Option<String> {
    let o = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(path)
        .output()
        .ok()?;
    if o.status.success() {
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        None
    }
}

fn find_executables_in_makefile(makefile_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(makefile_path) {
        for line in content.lines() {
            let line = line.trim();

            // Look for target definitions (lines ending with :)
            if let Some(colon_pos) = line.find(':') {
                let target = line[..colon_pos].trim();
                // Skip special targets and targets with wildcards
                if !target.is_empty()
                    && !target.starts_with('.')
                    && !target.contains('%')
                    && !target.contains('$')
                    && target != "all"
                    && target != "clean"
                    && target != "install"
                    && target != "test"
                {
                    targets.push(target.to_string());
                }
            }

            // Look for gcc/g++/clang output with -o flag
            if line.contains("gcc")
                || line.contains("g++")
                || line.contains("clang")
                || line.contains("cc")
            {
                if let Some(o_pos) = line.find("-o") {
                    let after_o = &line[o_pos + 2..].trim();
                    if let Some(first_word) = after_o.split_whitespace().next() {
                        targets.push(first_word.to_string());
                    }
                }
            }
        }
    }

    // Always include the repo name and common variations
    targets.push(repo.to_string());
    targets.push(repo.to_lowercase());
    targets.push("a.out".to_string());
    targets.push("main".to_string());

    targets
}

fn find_executables_in_meson(meson_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(meson_path) {
        for line in content.lines() {
            let line = line.trim();

            // Look for executable() calls: executable('name', ...)
            if line.contains("executable(") {
                if let Some(start) = line.find("executable(") {
                    let after_exec = &line[start + 11..];
                    // Find the first quoted string
                    if let Some(quote_start) = after_exec.find('\'') {
                        let after_quote = &after_exec[quote_start + 1..];
                        if let Some(quote_end) = after_quote.find('\'') {
                            targets.push(after_quote[..quote_end].to_string());
                        }
                    } else if let Some(quote_start) = after_exec.find('"') {
                        let after_quote = &after_exec[quote_start + 1..];
                        if let Some(quote_end) = after_quote.find('"') {
                            targets.push(after_quote[..quote_end].to_string());
                        }
                    }
                }
            }
        }
    }

    // Fallback to common names
    if targets.is_empty() {
        targets.push(repo.to_string());
        targets.push(repo.to_lowercase());
    }

    targets
}

fn find_executables_in_cmake(cmake_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(cmake_path) {
        for line in content.lines() {
            let line = line.trim();

            // Look for add_executable() calls: add_executable(name ...)
            if line.contains("add_executable(") {
                if let Some(start) = line.find("add_executable(") {
                    let after_exec = &line[start + 15..];
                    // Get the first word (executable name)
                    if let Some(first_word) = after_exec.split_whitespace().next() {
                        let name = first_word
                            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                        if !name.is_empty() {
                            targets.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Fallback to common names
    if targets.is_empty() {
        targets.push(repo.to_string());
        targets.push(repo.to_lowercase());
    }

    targets
}

fn find_all_executables_recursive(dir: &Path) -> Vec<String> {
    let mut executables = Vec::new();

    fn search_dir(dir: &Path, executables: &mut Vec<String>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_dir() {
                    // Skip hidden directories and common build artifact dirs
                    let dir_name = path.file_name().unwrap().to_string_lossy();
                    if !dir_name.starts_with('.')
                        && dir_name != "node_modules"
                        && dir_name != "target"
                    {
                        search_dir(&path, executables);
                    }
                } else if path.is_file() {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(&path) {
                        if metadata.permissions().mode() & 0o111 != 0 {
                            let filename = path.file_name().unwrap().to_string_lossy();
                            if !filename.ends_with(".sh")
                                && !filename.ends_with(".py")
                                && !filename.ends_with(".pl")
                                && !filename.ends_with(".rb")
                                && !filename.ends_with(".js")
                                && !filename.starts_with(".")
                                && !filename.contains("Makefile")
                                && !filename.contains("CMake")
                            {
                                executables.push(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    search_dir(dir, &mut executables);
    executables
}

fn prompt_executable_selection(executables: &[String]) -> Option<String> {
    if executables.is_empty() {
        return None;
    }

    if executables.len() == 1 {
        return Some(executables[0].clone());
    }

    println!("\nMultiple executables found:");
    for (i, exe) in executables.iter().enumerate() {
        println!("[{}] {}", i + 1, exe);
    }

    print!("Select the main executable (1-{}): ", executables.len());
    use std::io::{self, Write};
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;

    if let Ok(choice) = input.trim().parse::<usize>() {
        if choice >= 1 && choice <= executables.len() {
            return Some(executables[choice - 1].clone());
        }
    }

    None
}

/// Find data files that need to be installed (gresources, schemas, icons, etc.)
#[allow(dead_code)]
fn find_data_files(source_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();

    // GTK/GLib resource files
    for entry in walkdir::WalkDir::new(source_dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Determine destination based on file type
        let dest_dir = if filename.ends_with(".gresource") {
            "share/{}"
        } else if filename.ends_with(".gschema.xml") {
            "share/glib-2.0/schemas"
        } else if filename == "glib-2.0" && path.is_dir() {
            continue; // Handle separately
        } else if extension == "desktop" {
            "share/applications"
        } else if filename.contains(".icon") || extension == "png" || extension == "svg" {
            if path.to_string_lossy().contains("icon") {
                "share/icons/hicolor"
            } else {
                continue;
            }
        } else if extension == "service" {
            "share/dbus-1/services"
        } else if filename.ends_with(".metainfo.xml") || filename.ends_with(".appdata.xml") {
            "share/metainfo"
        } else {
            continue;
        };

        // Get relative path from source
        let _relative = path.strip_prefix(source_dir).unwrap_or(path);
        files.push((path.to_path_buf(), dest_dir.to_string()));
    }

    files
}

/// Copy data files to installation directory
fn install_data_files(
    source_dir: &Path,
    install_path: &Path,
    repo: &str,
) -> Vec<(PathBuf, PathBuf)> {
    let mut installed = Vec::new();

    // GTK4/GLib specific: look for data directories
    let data_dirs = [
        source_dir.join("data"),
        source_dir.join("resources"),
        source_dir.join("share"),
        source_dir.join(repo), // Some apps put resources in a subdir named after repo
    ];

    for data_dir in &data_dirs {
        if !data_dir.exists() {
            continue;
        }

        // Copy gresource files
        for entry in fs::read_dir(data_dir).ok().into_iter().flatten() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Handle .gresource files
            if filename.ends_with(".gresource") {
                let dest_dir = install_path.join("share").join(repo);
                fs::create_dir_all(&dest_dir).unwrap();
                let dest = dest_dir.join(filename);
                match fs::copy(&path, &dest) {
                    Ok(_) => {
                        println!("Installed resource: {}", dest.display());
                        installed.push((path.clone(), dest));
                    }
                    Err(e) => eprintln!("Failed to copy resource {}: {}", filename, e),
                }
            }

            // Handle schema files
            if filename.ends_with(".gschema.xml") {
                let dest_dir = install_path.join("share/glib-2.0/schemas");
                fs::create_dir_all(&dest_dir).unwrap();
                let dest = dest_dir.join(filename);
                match fs::copy(&path, &dest) {
                    Ok(_) => {
                        println!("Installed schema: {}", dest.display());
                        installed.push((path.clone(), dest));
                    }
                    Err(e) => eprintln!("Failed to copy schema {}: {}", filename, e),
                }
            }

            // Handle desktop files
            if filename.ends_with(".desktop") {
                let dest_dir = install_path.join("share/applications");
                fs::create_dir_all(&dest_dir).unwrap();
                let dest = dest_dir.join(filename);
                match fs::copy(&path, &dest) {
                    Ok(_) => {
                        println!("Installed desktop file: {}", dest.display());
                        installed.push((path.clone(), dest));
                    }
                    Err(e) => eprintln!("Failed to copy desktop file {}: {}", filename, e),
                }
            }

            // Handle icon directories
            if path.is_dir() && filename == "icons" {
                let dest_dir = install_path.join("share/icons");
                match copy_dir_all(&path, &dest_dir) {
                    Ok(_) => println!("Installed icons to: {}", dest_dir.display()),
                    Err(e) => eprintln!("Failed to copy icons: {}", e),
                }
            }
        }
    }

    // Also check for data in build directory if it exists
    let build_data = source_dir.join("build").join("data");
    if build_data.exists() {
        for entry in fs::read_dir(&build_data).ok().into_iter().flatten() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if filename.ends_with(".gresource") {
                let dest_dir = install_path.join("share").join(repo);
                fs::create_dir_all(&dest_dir).unwrap();
                let dest = dest_dir.join(filename);
                if let Ok(_) = fs::copy(&path, &dest) {
                    println!("Installed built resource: {}", dest.display());
                    installed.push((path, dest));
                }
            }
        }
    }

    installed
}

/// Create compatibility symlinks for data files in standard user data locations
/// to help GTK/GLib applications that expect resources in ~/.local/share/<app>
fn create_data_symlinks(install_path: &Path, repo: &str) -> Vec<PathBuf> {
    use std::os::unix::fs as unix_fs;

    let mut created = Vec::new();

    // Symlink ~/.local/share/<repo> -> <install_path>/share/<repo>
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return created,
    };

    let app_share_dir = install_path.join("share").join(repo);
    if app_share_dir.exists() {
        let local_share_app = Path::new(&home).join(".local/share").join(repo);

        // Only create if it doesn't already exist to avoid clobbering real data
        if !local_share_app.exists() {
            if let Err(e) = fs::create_dir_all(
                local_share_app
                    .parent()
                    .unwrap_or(&Path::new(&home).join(".local/share")),
            ) {
                eprintln!(
                    "Failed to prepare directory for data symlink {}: {}",
                    local_share_app.display(),
                    e
                );
            } else if let Err(e) = unix_fs::symlink(&app_share_dir, &local_share_app) {
                eprintln!(
                    "Failed to create data symlink {} -> {}: {}",
                    local_share_app.display(),
                    app_share_dir.display(),
                    e
                );
            } else {
                println!(
                    "Created data symlink: {} -> {}",
                    local_share_app.display(),
                    app_share_dir.display()
                );
                created.push(local_share_app);
            }
        }
    }

    created
}

fn create_desktop_symlinks(install_path: &Path, pkg_key: &str) -> Vec<PathBuf> {
    use std::os::unix::fs as unix_fs;

    let mut created = Vec::new();

    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return created,
    };

    let src_dir = install_path.join("share").join("applications");
    if !src_dir.exists() {
        return created;
    }

    let dest_dir = Path::new(&home)
        .join(".local/share/applications")
        .join("gitpkg");

    if let Err(e) = fs::create_dir_all(&dest_dir) {
        eprintln!(
            "Failed to create gitpkg applications directory {}: {}",
            dest_dir.display(),
            e
        );
        return created;
    }

    let safe_pkg = pkg_key.replace('/', "_");

    if let Ok(entries) = fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e != "desktop")
                .unwrap_or(true)
            {
                continue;
            }

            let base = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("app.desktop");
            let dest_path = dest_dir.join(format!("gitpkg.{}.{}", safe_pkg, base));

            // Avoid overwriting existing files we don't own
            if dest_path.exists() {
                continue;
            }

            if let Err(e) = unix_fs::symlink(&path, &dest_path) {
                eprintln!(
                    "Failed to create desktop symlink {} -> {}: {}",
                    dest_path.display(),
                    path.display(),
                    e
                );
            } else {
                println!(
                    "Created desktop symlink: {} -> {}",
                    dest_path.display(),
                    path.display()
                );
                created.push(dest_path);
            }
        }
    }

    created
}

/// Refresh the desktop database for user applications, if supported.
fn refresh_desktop_database() {
    if !is_installed("update-desktop-database") {
        return;
    }

    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };

    let apps_dir = Path::new(&home).join(".local/share/applications");
    let _ = Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();
}

/// Recursively copy directory
fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.as_ref().join(entry.file_name());

        if path.is_dir() {
            copy_dir_all(&path, &dest)?;
        } else {
            fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

fn find_built_executable(build_dir: &Path, repo: &str, build_system: &str) -> Option<String> {
    // Try to get expected names from build files
    let mut search_names = Vec::new();

    match build_system {
        "make" => {
            let makefile = build_dir.join("Makefile");
            if makefile.exists() {
                search_names = find_executables_in_makefile(&makefile, repo);
            }
        }
        "meson" => {
            let meson_file = build_dir.parent().and_then(|p| {
                let mf = p.join("meson.build");
                if mf.exists() {
                    Some(mf)
                } else {
                    None
                }
            });
            if let Some(mf) = meson_file {
                search_names = find_executables_in_meson(&mf, repo);
            }
        }
        "cmake" => {
            let cmake_file = build_dir.parent().and_then(|p| {
                let cf = p.join("CMakeLists.txt");
                if cf.exists() {
                    Some(cf)
                } else {
                    None
                }
            });
            if let Some(cf) = cmake_file {
                search_names = find_executables_in_cmake(&cf, repo);
            }
        }
        _ => {}
    }

    // Fallback names if build file parsing didn't find anything
    if search_names.is_empty() {
        search_names = vec![
            repo.to_string(),
            repo.to_lowercase(),
            "a.out".to_string(),
            "main".to_string(),
        ];
    }

    // Search for executables in common build output directories
    let search_dirs = vec![
        build_dir.to_path_buf(),
        build_dir.join("bin"),
        build_dir.join("build"),
        build_dir.join("out"),
        build_dir.join("target"),
        build_dir.join("src"), // Common for meson projects
    ];

    // First, try to find executables with expected names
    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }

        for exe_name in &search_names {
            let exe_path = dir.join(exe_name);
            if exe_path.exists() && exe_path.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = fs::metadata(&exe_path) {
                    if metadata.permissions().mode() & 0o111 != 0 {
                        return Some(exe_path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    // If not found, search recursively for ALL executables
    println!("Expected executable not found, searching entire build directory...");
    let all_executables = find_all_executables_recursive(build_dir);

    if all_executables.is_empty() {
        return None;
    }

    // Prompt user to select if multiple found
    prompt_executable_selection(&all_executables)
}

/// Find the primary installed executable under <install_path>/bin.
fn find_installed_executable(install_path: &Path, repo: &str) -> Option<PathBuf> {
    let bin_dir = install_path.join("bin");
    if !bin_dir.exists() {
        return None;
    }

    // Preferred names to match
    let mut prefs = vec![repo.to_string(), repo.to_lowercase()];

    // Gather other filenames present
    if let Ok(entries) = fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&p) {
                    if meta.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    prefs.push(name.to_string());
                }
            }
        }
    }

    // Try preferences in order
    for name in prefs {
        let candidate = bin_dir.join(&name);
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }

    // Fallback: first executable file found
    if let Ok(entries) = fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&p) {
                    if meta.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                return Some(p);
            }
        }
    }

    None
}

fn list_file_path() -> String {
    format!(
        "{}/.local/share/gitpkg/list.gitpkg",
        env::var("HOME").unwrap()
    )
}

fn build_git_url(user: &str, repo: &str, supplier: Option<&str>) -> String {
    let supplier_domain = supplier.unwrap_or("github.com");

    // Handle different URL formats for different suppliers
    let repo_name = if repo.ends_with(".git") {
        repo.to_string()
    } else {
        format!("{}.git", repo)
    };

    format!("https://{}/{}/{}", supplier_domain, user, repo_name)
}

fn normalize_supplier(supplier: &str) -> String {
    // Remove .com, .org, .net, etc. from supplier domain
    supplier
        .trim_end_matches(".com")
        .trim_end_matches(".org")
        .trim_end_matches(".net")
        .trim_end_matches(".io")
        .trim_end_matches(".dev")
        .to_string()
}

fn get_package_key(user: &str, repo: &str, supplier: &str) -> String {
    if supplier == "github.com" {
        // GitHub packages use simple user/repo format for backward compatibility
        format!("{}/{}", user, repo)
    } else {
        // Other suppliers use normalized_supplier_user/repo format
        format!("{}_{}/{}", normalize_supplier(supplier), user, repo)
    }
}

fn find_matching_packages(user: &str, repo: &str) -> Vec<(String, String, String)> {
    // Returns Vec of (package_key, supplier, info_path)
    let packages = read_package_list();
    let mut matches = Vec::new();

    for (pkg_key, info_path) in packages {
        // Check if this package matches user/repo
        // Could be "user/repo" (github) or "supplier_user/repo" (others)
        let parts: Vec<&str> = pkg_key.split('/').collect();
        if parts.len() == 2 {
            let pkg_repo = parts[1];
            let pkg_user_part = parts[0];

            // Extract user from "supplier_user" or just "user"
            let pkg_user = if pkg_user_part.contains('_') {
                pkg_user_part.split('_').last().unwrap_or("")
            } else {
                pkg_user_part
            };

            if pkg_user == user && pkg_repo == repo {
                // Read supplier from info file
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

/// Find package by exact key (handles supplier-prefixed keys)
fn find_package_by_key(pkg_key: &str) -> Option<(String, String, String)> {
    let packages = read_package_list();

    // Direct match first
    if let Some(info_path) = packages.get(pkg_key) {
        // Try to read supplier from info file
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

    // Try parsing as user/repo and find matches
    let (user, repo) = parse_pkg(pkg_key);
    let matches = find_matching_packages(&user, &repo);

    if matches.len() == 1 {
        return Some(matches[0].clone());
    }

    None
}

fn prompt_package_selection(matches: &[(String, String, String)]) -> Option<usize> {
    println!("Multiple packages found:");
    for (i, (pkg_key, supplier, _)) in matches.iter().enumerate() {
        println!("[{}] {}: {}", i + 1, supplier, pkg_key);
    }

    print!("Select package (1-{}): ", matches.len());
    use std::io::{self, Write};
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

#[allow(dead_code)]
fn get_supplier_from_url(url: &str) -> Option<String> {
    // Extract domain from URL like "https://gitlab.com/user/repo.git "
    if let Some(start) = url.find("://") {
        let after_protocol = &url[start + 3..];
        if let Some(end) = after_protocol.find('/') {
            return Some(after_protocol[..end].to_string());
        }
    }
    None
}
fn read_package_list() -> HashMap<String, String> {
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

/// Return total size in bytes of a directory (recursive). Ignores errors per-file.
fn dir_size_bytes(path: &Path) -> u64 {
    let mut total: u64 = 0;
    if path.is_file() {
        if let Ok(meta) = fs::metadata(path) {
            return meta.len();
        }
        return 0;
    }

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Ok(meta) = fs::metadata(&p) {
                    total += meta.len();
                }
            } else if p.is_dir() {
                total += dir_size_bytes(&p);
            }
        }
    }
    total
}

fn format_mb(bytes: u64) -> String {
    let mb = (bytes as f64) / 1024.0 / 1024.0;
    format!("{:.2} MB", mb)
}

fn add_to_package_list(user: &str, repo: &str, info_path: &str, supplier: &str) {
    let mut packages = read_package_list();
    let key = get_package_key(user, repo, supplier);
    packages.insert(key, info_path.to_string());
    write_package_list(&packages);
}

fn remove_from_package_list(package_key: &str) {
    let mut packages = read_package_list();
    packages.remove(package_key);
    write_package_list(&packages);
}

fn write_package_list(packages: &HashMap<String, String>) {
    let list_path = list_file_path();

    // Ensure directory exists
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

fn install(package: &str, verbose: bool, supplier: Option<&str>) {
    let (user, repo) = parse_pkg(package);
    let url = build_git_url(&user, &repo, supplier);
    let supplier_domain = supplier.unwrap_or("github.com");

    let path = temp_path(&user, &repo);
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path).unwrap();
    }

    println!("Cloning {} from {} into {}", package, supplier_domain, path);
    if !run_git_clone_with_progress(&url, &path, verbose) {
        eprintln!("Git clone failed");
        return;
    }
    println!("Successfully cloned {}!", package);

    // Detect build system and package manager
    let bs = match detect_build_system(&path) {
        Some(s) => s,
        None => {
            println!("Could not detect build system");
            return;
        }
    };
    let pm = match detect_package_manager() {
        Some(p) => p,
        None => {
            println!("No package manager detected");
            return;
        }
    };
    let compiler = match build_system_packages(bs, pm) {
        Some(c) => c,
        None => {
            println!("No compiler mapping for {} on {}", bs, pm);
            return;
        }
    };

    // Install compiler if missing
    if !is_installed(bs) {
        println!("Installing {} for {} via {}...", compiler, bs, pm);
        let status = match pm {
            "apt" => Command::new("sudo")
                .arg("apt")
                .arg("install")
                .arg("-y")
                .arg(compiler)
                .status(),
            "dnf" => Command::new("sudo")
                .arg("dnf")
                .arg("install")
                .arg("-y")
                .arg(compiler)
                .status(),
            "yum" => Command::new("sudo")
                .arg("yum")
                .arg("install")
                .arg("-y")
                .arg(compiler)
                .status(),
            "pacman" => Command::new("sudo")
                .arg("pacman")
                .arg("-Sy")
                .arg("--noconfirm")
                .arg(compiler)
                .status(),
            "zypper" => Command::new("sudo")
                .arg("zypper")
                .arg("install")
                .arg("-y")
                .arg(compiler)
                .status(),
            "apk" => Command::new("sudo")
                .arg("apk")
                .arg("add")
                .arg(compiler)
                .status(),
            "nix-env" => Command::new("nix-env").arg("-iA").arg(compiler).status(),
            _ => {
                eprintln!("Unsupported package manager");
                return;
            }
        };
        if let Ok(s) = status {
            if !s.success() {
                eprintln!("Failed installing {}", compiler);
                return;
            }
        }
    }

    build(&user, &repo, verbose, Some(supplier_domain));
}

fn build_cargo(temp: &str, install_path: &str, verbose: bool) -> std::process::ExitStatus {
    let mut cmd = Command::new("cargo");
    cmd.arg("install")
        .arg("--path")
        .arg(temp)
        .arg("--root")
        .arg(install_path)
        .arg("--force");
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd.status().unwrap()
}

fn build_make(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Option<std::process::ExitStatus> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(temp);
    if !verbose {
        make_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let make_status = make_cmd.status().unwrap();

    if !make_status.success() {
        return Some(make_status);
    }

    match find_built_executable(Path::new(temp), repo, "make") {
        Some(exe_path) => {
            println!("Found executable: {}", exe_path);
            // Preserve the original executable name instead of renaming to the repo
            let exe_name = Path::new(&exe_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo);
            let dest = bin_dir.join(exe_name);
            fs::copy(&exe_path, &dest).unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms).unwrap();
            Some(make_status)
        }
        None => {
            eprintln!("Could not find executable after build");
            eprintln!("Searched in: {}", temp);
            eprintln!("Try running with -v flag to see build output");
            None
        }
    }
}

fn build_cmake(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Option<std::process::ExitStatus> {
    let build_dir = Path::new(temp).join("build");
    fs::create_dir_all(&build_dir).unwrap();

    // Configure with install prefix
    let mut cmake_cmd = Command::new("cmake");
    cmake_cmd
        .arg("..")
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install_path))
        .current_dir(&build_dir);
    if !verbose {
        cmake_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    if !cmake_cmd.status().unwrap().success() {
        eprintln!("CMake configuration failed");
        return None;
    }

    // Build
    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(&build_dir);
    if !verbose {
        make_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let make_status = make_cmd.status().unwrap();

    if !make_status.success() {
        return Some(make_status);
    }

    // Install (this handles data files properly)
    let mut install_cmd = Command::new("make");
    install_cmd.arg("install").current_dir(&build_dir);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status().unwrap();

    // Find the executable in the install prefix
    let bin_dir = Path::new(install_path).join("bin");
    let exe_path = bin_dir.join(repo);

    if exe_path.exists() {
        Some(install_status)
    } else {
        // Fallback: manually find and copy executable
        match find_built_executable(&build_dir, repo, "cmake") {
            Some(built_exe) => {
                fs::create_dir_all(&bin_dir).unwrap();
                // Preserve the original executable name instead of renaming to the repo
                let exe_name = Path::new(&built_exe)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(repo);
                let dest = bin_dir.join(exe_name);
                fs::copy(&built_exe, &dest).unwrap();
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest, perms).unwrap();
                Some(install_status)
            }
            None => {
                eprintln!("Could not find executable after build");
                None
            }
        }
    }
}

fn build_meson(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Option<std::process::ExitStatus> {
    let build_dir = Path::new(temp).join("build");

    // Setup with prefix
    let mut setup_cmd = Command::new("meson");
    setup_cmd
        .arg("setup")
        .arg(&build_dir)
        .arg(format!("--prefix={}", install_path))
        .current_dir(temp);
    if !verbose {
        setup_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    if !setup_cmd.status().unwrap().success() {
        eprintln!("Meson setup failed");
        return None;
    }

    // Compile
    let mut compile_cmd = Command::new("meson");
    compile_cmd.arg("compile").arg("-C").arg(&build_dir);
    if !verbose {
        compile_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let compile_status = compile_cmd.status().unwrap();

    if !compile_status.success() {
        return Some(compile_status);
    }

    // Install - this is crucial for GTK apps with resources
    println!("Installing with meson (this handles data files)...");
    let mut install_cmd = Command::new("meson");
    install_cmd.arg("install").arg("-C").arg(&build_dir);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status().unwrap();

    // Check if executable exists in install prefix
    let bin_dir = Path::new(install_path).join("bin");
    let exe_path = bin_dir.join(repo);

    if !exe_path.exists() {
        // Meson might install with different name, search for it
        if let Ok(entries) = fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    // Check if it's close to repo name
                    if name.to_lowercase() == repo.to_lowercase()
                        || name.contains(&repo.to_lowercase())
                    {
                        // Create symlink with expected name
                        let dest = bin_dir.join(repo);
                        let _ = std::os::unix::fs::symlink(&path, &dest);
                        break;
                    }
                }
            }
        }
    }

    Some(install_status)
}

fn build_mason(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Option<std::process::ExitStatus> {
    build_make(temp, install_path, repo, verbose)
}

fn build_go(temp: &str, install_path: &str, repo: &str, verbose: bool) -> std::process::ExitStatus {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let mut cmd = Command::new("go");
    cmd.arg("build")
        .arg("-o")
        .arg(bin_dir.join(repo))
        .current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd.status().unwrap()
}

fn build_npm(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> std::process::ExitStatus {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let mut install_cmd = Command::new("npm");
    install_cmd.arg("install").current_dir(temp);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status().unwrap();

    if !install_status.success() {
        return install_status;
    }

    let mut build_cmd = Command::new("npm");
    build_cmd.arg("run").arg("build").current_dir(temp);
    if !verbose {
        build_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let _ = build_cmd.status();

    // Try to find the main entry point or built files
    let package_json = Path::new(temp).join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Check for bin field
            if let Some(bin) = json.get("bin") {
                let bin_path = if bin.is_string() {
                    bin.as_str().map(|s| Path::new(temp).join(s))
                } else if let Some(obj) = bin.as_object() {
                    obj.get(repo)
                        .and_then(|v| v.as_str())
                        .map(|s| Path::new(temp).join(s))
                } else {
                    None
                };

                if let Some(src) = bin_path {
                    if src.exists() {
                        let dest = bin_dir.join(repo);
                        // Create wrapper script if it's a JS file
                        if src.extension().and_then(|e| e.to_str()) == Some("js") {
                            let wrapper =
                                format!("#!/usr/bin/env node\nrequire('{}');", src.display());
                            fs::write(&dest, wrapper).unwrap();
                            use std::os::unix::fs::PermissionsExt;
                            let mut perms = fs::metadata(&dest).unwrap().permissions();
                            perms.set_mode(0o755);
                            fs::set_permissions(&dest, perms).unwrap();
                        } else {
                            fs::copy(&src, &dest).unwrap();
                        }
                    }
                }
            }
        }
    }

    println!("Note: npm package installed at {}", install_path);
    install_status
}

fn build_gradle(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> std::process::ExitStatus {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let mut cmd = Command::new("gradle");
    cmd.arg("build").current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status().unwrap();

    // Find built JAR and create wrapper script
    if status.success() {
        let build_libs = Path::new(temp).join("build/libs");
        if let Ok(entries) = fs::read_dir(&build_libs) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jar") {
                    let dest_jar = bin_dir.join(format!("{}.jar", repo));
                    fs::copy(&path, &dest_jar).unwrap();

                    // Create wrapper script
                    let wrapper = bin_dir.join(repo);
                    let script = format!(
                        "#!/bin/bash\nexec java -jar \"{}\" \"$@\"",
                        dest_jar.display()
                    );
                    fs::write(&wrapper, script).unwrap();
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&wrapper).unwrap().permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&wrapper, perms).unwrap();
                    break;
                }
            }
        }
    }

    status
}

fn build_python(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Option<std::process::ExitStatus> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    let python_cmd = if is_installed("python3") {
        "python3"
    } else if is_installed("python") {
        "python"
    } else {
        eprintln!("Python not found on PATH; cannot build python package");
        return None;
    };

    let temp_path = Path::new(temp);

    // Check for pyproject.toml or setup.py - use pip install . approach
    let has_pyproject = temp_path.join("pyproject.toml").exists();
    let has_setup_py = temp_path.join("setup.py").exists();

    // Install requirements.txt with --break-system-packages if it exists
    let req_file = temp_path.join("requirements.txt");
    if req_file.exists() {
        println!("Installing requirements with --break-system-packages...");
        let mut req_cmd = Command::new(python_cmd);
        req_cmd
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--break-system-packages")
            .arg("-r")
            .arg(&req_file);
        if !verbose {
            req_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let req_status = req_cmd.status().ok()?;
        if !req_status.success() {
            eprintln!("Failed to install requirements.txt");
            return Some(req_status);
        }
    }

    // Find the main Python script
    let main_script = find_main_python_script(temp_path, repo);

    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir).ok()?;

    if let Some((script_name, script_path)) = main_script {
        // Put the script in its own folder: lib/<repo>/
        let lib_dir = Path::new(install_path).join("lib").join(repo);
        fs::create_dir_all(&lib_dir).ok()?;

        let dest_script = lib_dir.join(&script_name);
        fs::copy(&script_path, &dest_script).ok()?;

        // Ensure script has execute permission
        let mut perms = fs::metadata(&dest_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest_script, perms).ok();

        // Ensure script has a shebang
        if let Ok(content) = fs::read_to_string(&dest_script) {
            if !content.starts_with("#!") {
                let new_content = format!("#!/usr/bin/env {}\n{}", python_cmd, content);
                fs::write(&dest_script, new_content).ok();
                // Re-apply execute permission after write
                let mut perms = fs::metadata(&dest_script).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest_script, perms).ok();
            }
        }

        // Create symlink in bin/ pointing to the script
        let symlink_path = bin_dir.join(&script_name);
        let _ = fs::remove_file(&symlink_path);
        if let Err(e) = std::os::unix::fs::symlink(&dest_script, &symlink_path) {
            eprintln!("Failed to create symlink: {}", e);
        } else {
            println!(
                "Created symlink: {} -> {}",
                symlink_path.display(),
                dest_script.display()
            );
        }

        println!(
            "Installed python script: {} -> {}",
            symlink_path.display(),
            dest_script.display()
        );

        return Some(std::process::ExitStatus::from_raw(0));
    } else if has_pyproject || has_setup_py {
        // Fall back to pip install . for proper packages
        println!("Installing python package with pip...");
        let mut install_cmd = Command::new(python_cmd);
        install_cmd
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--break-system-packages")
            .arg(".");
        if !verbose {
            install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let install_status = install_cmd.current_dir(temp).status().ok()?;
        if !install_status.success() {
            eprintln!("pip install failed for python package");
            return Some(install_status);
        }

        // Find and symlink console scripts from site-packages
        let site_packages = Command::new(python_cmd)
            .arg("-c")
            .arg("import site; print(site.getsitepackages()[0])")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        if let Some(sp) = site_packages {
            let bin_in_sp = Path::new(&sp).join("..").join("bin");
            if let Ok(entries) = fs::read_dir(&bin_in_sp) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            // Skip python and pip
                            if name == "python"
                                || name.starts_with("python")
                                || name == "pip"
                                || name.starts_with("pip")
                            {
                                continue;
                            }
                            let symlink_path = bin_dir.join(name);
                            let _ = fs::remove_file(&symlink_path);
                            let _ = std::os::unix::fs::symlink(&path, &symlink_path);
                            println!("Linked console script: {}", name);
                        }
                    }
                }
            }
        }

        Some(install_status)
    } else {
        eprintln!("No python script found (expected main.py, app.py, or <repo>.py)");
        None
    }
}

fn find_main_python_script(temp: &Path, repo: &str) -> Option<(String, PathBuf)> {
    // Check for common main script names
    let candidates: Vec<String> = vec![
        "main.py".to_string(),
        "app.py".to_string(),
        "cli.py".to_string(),
        "run.py".to_string(),
        "start.py".to_string(),
        format!("{}.py", repo),
        "script.py".to_string(),
        "entrypoint.py".to_string(),
    ];

    for candidate in &candidates {
        let path = temp.join(candidate);
        if path.exists() && path.is_file() {
            return Some((candidate.clone(), path));
        }
    }

    // Search for .py files in temp (excluding __pycache__, venv, etc.)
    if let Ok(entries) = fs::read_dir(temp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext == "py" {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        // Skip common non-entry-point files
                        if !name.starts_with("test_")
                            && !name.starts_with("_")
                            && name != "setup.py"
                            && name != "__init__.py"
                        {
                            return Some((name.to_string(), path));
                        }
                    }
                }
            }
        }
    }

    None
}

fn build(user: &str, repo: &str, verbose: bool, supplier: Option<&str>) {
    let temp = temp_path(user, repo);
    let commit = get_commit_hash(&temp).unwrap_or_else(|| "unknown".to_string());
    let supplier_domain = supplier.unwrap_or("github.com");
    let install_path = install_root(user, repo, &commit, supplier_domain);
    let pkg_key = get_package_key(user, repo, supplier_domain);
    fs::create_dir_all(&install_path).unwrap();

    let bs = match detect_build_system(&temp) {
        Some(s) => s,
        None => {
            println!("No build system detected");
            return;
        }
    };
    println!("Building {} with {}", repo, bs);

    let status = match bs {
        "cargo" => Some(build_cargo(&temp, &install_path, verbose)),
        "make" => build_make(&temp, &install_path, repo, verbose),
        "cmake" => build_cmake(&temp, &install_path, repo, verbose),
        "meson" => build_meson(&temp, &install_path, repo, verbose),
        "python" => build_python(&temp, &install_path, repo, verbose),
        "mason" => build_mason(&temp, &install_path, repo, verbose),
        "go" => Some(build_go(&temp, &install_path, repo, verbose)),
        "npm" => Some(build_npm(&temp, &install_path, repo, verbose)),
        "gradle" => Some(build_gradle(&temp, &install_path, repo, verbose)),
        _ => {
            println!("Unsupported build system: {}", bs);
            return;
        }
    };

    let status = match status {
        Some(s) => s,
        None => {
            println!("Build failed for {}", repo);
            return;
        }
    };

    if status.success() {
        println!("Installed to {}", install_path);

        // Install data files (resources, schemas, etc.) for GTK/GLib apps
        let data_files = install_data_files(Path::new(&temp), Path::new(&install_path), repo);
        if !data_files.is_empty() {
            println!("Installed {} data file(s)", data_files.len());
        }

        // Create compatibility symlinks for data directories in ~/.local/share
        let data_symlinks = if !data_files.is_empty() {
            create_data_symlinks(Path::new(&install_path), repo)
        } else {
            Vec::new()
        };

        if bs != "npm" {
            let _ = fs::remove_dir_all(&temp);
        }

        // Determine actual installed executable (prefer real filename over repo)
        let exe_path = match find_installed_executable(Path::new(&install_path), repo) {
            Some(p) => p,
            None => Path::new(&install_path).join("bin").join(repo),
        };
        let exe_name = exe_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(repo)
            .to_string();

        println!("Creating symlink to /usr/bin (requires sudo)...");
        let sudo_auth = Command::new("sudo").arg("-v").status();

        let symlink_path = Path::new("/usr/bin").join(&exe_name);

        // Attempt to create system symlink safely (don't overwrite non-symlink files)
        let symlink_created = if sudo_auth.is_ok() && sudo_auth.unwrap().success() {
            // If path exists check if it's a symlink. If it's a regular file, do not overwrite.
            match std::fs::symlink_metadata(&symlink_path) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        // remove existing symlink and create new one
                        let _ = Command::new("sudo")
                            .arg("rm")
                            .arg("-f")
                            .arg(&symlink_path)
                            .status();
                        let status = Command::new("sudo")
                            .arg("ln")
                            .arg("-s")
                            .arg(&exe_path)
                            .arg(&symlink_path)
                            .status();
                        match status {
                            Ok(s) if s.success() => {
                                println!(
                                    "Replaced symlink: {} -> {}",
                                    symlink_path.display(),
                                    exe_path.display()
                                );
                                true
                            }
                            _ => {
                                eprintln!("Failed to update symlink in /usr/bin");
                                false
                            }
                        }
                    } else {
                        eprintln!(
                            "Refusing to overwrite existing non-symlink at {}. Remove it manually or use another installer.",
                            symlink_path.display()
                        );
                        false
                    }
                }
                Err(_) => {
                    // Path does not exist; create symlink
                    let status = Command::new("sudo")
                        .arg("ln")
                        .arg("-s")
                        .arg(&exe_path)
                        .arg(&symlink_path)
                        .status();
                    match status {
                        Ok(s) if s.success() => {
                            println!(
                                "Created symlink: {} -> {}",
                                symlink_path.display(),
                                exe_path.display()
                            );
                            true
                        }
                        _ => {
                            eprintln!("Failed to create symlink in /usr/bin");
                            false
                        }
                    }
                }
            }
        } else {
            eprintln!("Failed to authenticate sudo");
            false
        };

        // Create wrapper script that sets environment variables for resources
        let needs_wrapper = !data_files.is_empty();
        let final_exe_path = if needs_wrapper {
            // Create wrapper script
            let wrapper_path = Path::new(&install_path)
                .join("bin")
                .join(format!("{}-wrapper", exe_name));
            let gresource_path = Path::new(&install_path).join("share").join(repo);
            let schema_dir = Path::new(&install_path)
                .join("share")
                .join("glib-2.0")
                .join("schemas");

            let wrapper_content = format!(
                r#"#!/bin/bash
# Wrapper for {} - sets up resource paths
export GITPKG_PACKAGE_ROOT="{}"
export GRESOURCE_PATH="{}:$GRESOURCE_PATH"
export XDG_DATA_DIRS="{}:$XDG_DATA_DIRS"
export GSETTINGS_SCHEMA_DIR="{}:$GSETTINGS_SCHEMA_DIR"
exec {} "$@"
"#,
                repo,
                install_path,
                gresource_path.display(),
                Path::new(&install_path).join("share").display(),
                schema_dir.display(),
                exe_path.display()
            );

            fs::write(&wrapper_path, wrapper_content).unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&wrapper_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&wrapper_path, perms).unwrap();

            // Update symlink to point to wrapper
            if symlink_created {
                let _ = Command::new("sudo")
                    .arg("rm")
                    .arg("-f")
                    .arg(&symlink_path)
                    .status();
                let _ = Command::new("sudo")
                    .arg("ln")
                    .arg("-s")
                    .arg(&wrapper_path)
                    .arg(&symlink_path)
                    .status();
            }

            wrapper_path
        } else {
            exe_path.clone()
        };

        if !symlink_created {
            eprintln!("Trying ~/.local/bin instead...");

            let local_bin = Path::new(&env::var("HOME").unwrap()).join(".local/bin");
            fs::create_dir_all(&local_bin).unwrap();
            let local_symlink = local_bin.join(&exe_name);
            let _ = fs::remove_file(&local_symlink);

            let target = if needs_wrapper {
                &final_exe_path
            } else {
                &exe_path
            };

            match std::os::unix::fs::symlink(target, &local_symlink) {
                Ok(_) => {
                    println!(
                        "Created symlink: {} -> {}",
                        local_symlink.display(),
                        target.display()
                    );
                    println!("Note: Make sure ~/.local/bin is in your PATH");
                    println!("Add this to your ~/.bashrc or ~/.zshrc:");
                    println!("  export PATH=\"$HOME/.local/bin:$PATH\"");
                }
                Err(e) => {
                    eprintln!("Failed to create symlink: {}", e);
                    println!(
                        "You can run the executable directly at: {}",
                        target.display()
                    );
                }
            }
        }

        let final_symlink_path = if symlink_created {
            symlink_path.to_str().unwrap().to_string()
        } else {
            Path::new(&env::var("HOME").unwrap())
                .join(".local/bin")
                .join(&exe_name)
                .to_str()
                .unwrap()
                .to_string()
        };

        let mut desktop_path = None;
        let desktop_file_src = Path::new(&install_path)
            .join("share")
            .join("applications")
            .join(format!("{}.desktop", repo));
        if desktop_file_src.exists() {
            let desktop_file_dst = Path::new(&env::var("HOME").unwrap())
                .join(".local/share/applications")
                .join(format!("gitpkg.{}.{}.desktop", user, repo));
            fs::create_dir_all(desktop_file_dst.parent().unwrap()).unwrap();
            let content = fs::read_to_string(&desktop_file_src).unwrap();
            let new_content = content
                .lines()
                .map(|l| {
                    if l.starts_with("Exec=") {
                        format!("Exec={}", final_exe_path.to_str().unwrap())
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&desktop_file_dst, new_content).unwrap();
            desktop_path = Some(desktop_file_dst.to_str().unwrap().to_string());
        }

        // Symlink any desktop files from the package into ~/.local/share/applications/gitpkg
        let desktop_symlinks = create_desktop_symlinks(Path::new(&install_path), &pkg_key);

        write_info(
            user,
            repo,
            &commit,
            bs,
            detect_package_manager().unwrap_or("unknown"),
            &install_path,
            &final_symlink_path,
            desktop_path.as_deref(),
            supplier_domain,
            !data_files.is_empty(), // Track if package has data files
            &data_symlinks,
            &desktop_symlinks,
        );

        // Update the desktop database so new launchers are visible
        refresh_desktop_database();

        println!("Metadata written to info.gitpkg");
    } else {
        println!("Build failed for {}", repo);
    }
}

fn write_info(
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
) {
    use chrono::Utc;

    let pkg_key = get_package_key(user, repo, supplier);
    let info_dir = format!(
        "{}/.local/share/gitpkg/{}",
        env::var("HOME").unwrap(),
        pkg_key
    );
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
         has_data_files = {}\n",
        user,
        repo,
        commit,
        build_system,
        pm,
        Utc::now().to_rfc3339(),
        install_path,
        symlink_path,
        supplier,
        has_data_files
    );

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

    // Add to global package list
    add_to_package_list(user, repo, info_file.to_str().unwrap(), supplier);
    // Note: we do not write per-version timestamps; versions are sorted by directory modification time.
}

fn remove(package: &str) {
    let (user, repo) = parse_pkg(package);

    // Find all matching packages
    let matches = find_matching_packages(&user, &repo);

    if matches.is_empty() {
        eprintln!("Package {}/{} is not installed", user, repo);
        return;
    }

    let (pkg_key, supplier, info_path) = if matches.len() > 1 {
        // Multiple packages found, prompt user
        match prompt_package_selection(&matches) {
            Some(idx) => matches[idx].clone(),
            None => {
                eprintln!("Invalid selection");
                return;
            }
        }
    } else {
        matches[0].clone()
    };

    println!("Removing {} from {}...", pkg_key, supplier);

    // Read info file to get installation details
    let info_content = match fs::read_to_string(&info_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read info file: {}", e);
            return;
        }
    };

    let info: toml::Value = match toml::from_str(&info_content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse info file: {}", e);
            return;
        }
    };

    // Remove symlink
    if let Some(symlink_path) = info.get("symlink_path").and_then(|v| v.as_str()) {
        if Path::new(symlink_path).exists() {
            // Try without sudo first
            if fs::remove_file(symlink_path).is_err() {
                // Try with sudo
                let _ = Command::new("sudo")
                    .arg("rm")
                    .arg("-f")
                    .arg(symlink_path)
                    .status();
            }
            println!("Removed symlink: {}", symlink_path);
        }
    }

    // Remove desktop file
    if let Some(desktop_path) = info.get("desktop_file").and_then(|v| v.as_str()) {
        if Path::new(desktop_path).exists() {
            match fs::remove_file(desktop_path) {
                Ok(_) => println!("Removed desktop file: {}", desktop_path),
                Err(e) => eprintln!("Failed to remove desktop file {}: {}", desktop_path, e),
            }
        }
    }

    // Remove any data symlinks we created (e.g. in ~/.local/share)
    if let Some(data_symlinks) = info.get("data_symlinks").and_then(|v| v.as_array()) {
        for entry in data_symlinks {
            if let Some(path_str) = entry.as_str() {
                let p = Path::new(path_str);
                if p.exists() {
                    match fs::remove_file(p) {
                        Ok(_) => println!("Removed data symlink: {}", p.display()),
                        Err(e) => eprintln!("Failed to remove data symlink {}: {}", p.display(), e),
                    }
                }
            }
        }
    }

    // Remove any desktop symlinks we created under ~/.local/share/applications/gitpkg
    if let Some(desktop_symlinks) = info.get("desktop_symlinks").and_then(|v| v.as_array()) {
        for entry in desktop_symlinks {
            if let Some(path_str) = entry.as_str() {
                let p = Path::new(path_str);
                if p.exists() {
                    match fs::remove_file(p) {
                        Ok(_) => println!("Removed desktop symlink: {}", p.display()),
                        Err(e) => {
                            eprintln!("Failed to remove desktop symlink {}: {}", p.display(), e)
                        }
                    }
                }
            }
        }
    }

    // Remove installation directory (all versions)
    let package_dir = format!(
        "{}/.local/share/gitpkg/{}",
        env::var("HOME").unwrap(),
        pkg_key
    );

    if Path::new(&package_dir).exists() {
        match fs::remove_dir_all(&package_dir) {
            Ok(_) => println!("Removed installation directory: {}", package_dir),
            Err(e) => eprintln!(
                "Failed to remove installation directory {}: {}",
                package_dir, e
            ),
        }
    }

    // Clean up temp directory if it exists
    let temp = temp_path(&user, &repo);
    if Path::new(&temp).exists() {
        let _ = fs::remove_dir_all(&temp);
    }

    // Remove from global package list
    remove_from_package_list(&pkg_key);

    println!("Successfully removed {}", pkg_key);
}

fn goto(package: &str, spawn_shell: bool) {
    // Support supplier-prefixed keys and user/repo
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    // Try exact key first
    let exact_match = find_package_by_key(package);

    let (pkg_key, _supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        let matches = find_matching_packages(&user, &repo);
        if matches.is_empty() {
            eprintln!("Package {} is not installed", package);
            return;
        }

        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    eprintln!("Invalid selection");
                    return;
                }
            }
        } else {
            0
        };

        matches[selected].clone()
    };

    // Read info file
    let info_content = match fs::read_to_string(&info_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read info file: {}", e);
            return;
        }
    };

    let info: toml::Value = match toml::from_str(&info_content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse info file: {}", e);
            return;
        }
    };

    let install_path = match info.get("install_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            eprintln!("No install_path found for {}", pkg_key);
            return;
        }
    };

    if spawn_shell {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        println!("Spawning shell {} at {}", shell, install_path);
        let status = Command::new(shell).current_dir(install_path).status();
        if let Err(e) = status {
            eprintln!("Failed to spawn shell: {}", e);
        }
    } else {
        // Print path to stdout so callers can cd into it: cd "$(gitpkg goto user/repo)"
        println!("{}", install_path);
    }
}

fn clean(package: &str) {
    // Use the new parsing that handles supplier-prefixed keys
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    // First try exact key lookup (for supplier-prefixed packages like "codeberg_el1lovescomputers/gitpkg")
    let exact_match = find_package_by_key(package);

    let (pkg_key, supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        // Fall back to searching by user/repo
        let matches = find_matching_packages(&user, &repo);

        if matches.is_empty() {
            println!("Package {} is not installed, nothing to clean", package);
            return;
        }

        // If supplier was hinted from the package key, prefer that match
        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    eprintln!("Invalid selection");
                    return;
                }
            }
        } else {
            0
        };

        matches[selected].clone()
    };

    println!(
        "Cleaning old versions and temp files for {} from {}...",
        pkg_key, supplier
    );

    // Clean temp directory for this package
    let temp = temp_path(&user, &repo);
    if Path::new(&temp).exists() {
        match fs::remove_dir_all(&temp) {
            Ok(_) => println!("Removed temp directory: {}", temp),
            Err(e) => eprintln!("Failed to remove temp directory: {}", e),
        }
    }

    // Get current installed version from info.gitpkg
    let current_commit = if let Ok(content) = fs::read_to_string(&info_path) {
        if let Ok(info) = toml::from_str::<toml::Value>(&content) {
            info.get("latest_commit")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    if current_commit.is_none() {
        println!("Could not read current version, skipping old version cleanup");
        return;
    }

    let current_commit = current_commit.unwrap();
    println!("Current version: {}", current_commit);

    // Look for old versions in the package directory
    let package_dir = format!(
        "{}/.local/share/gitpkg/{}",
        env::var("HOME").unwrap(),
        pkg_key
    );

    if let Ok(entries) = fs::read_dir(&package_dir) {
        let mut removed_count = 0;
        let mut freed_bytes: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                // Skip if it's the current version or the info file
                if dir_name != current_commit && dir_name != "info.gitpkg" {
                    // compute size before removal
                    let size = dir_size_bytes(&path);
                    println!("Removing old version: {} ({})", dir_name, format_mb(size));
                    match fs::remove_dir_all(&path) {
                        Ok(_) => {
                            removed_count += 1;
                            freed_bytes += size;
                            println!("  Removed: {} (freed {})", path.display(), format_mb(size));
                        }
                        Err(e) => eprintln!("  Failed to remove {}: {}", path.display(), e),
                    }
                }
            }
        }

        if removed_count > 0 {
            println!(
                "Removed {} old version(s), freed {}",
                removed_count,
                format_mb(freed_bytes)
            );
        } else {
            println!("No old versions to clean");
        }
    }
}

fn clean_all() {
    println!("Cleaning all temp files and old versions...");

    let gitpkg_dir = format!("{}/.local/share/gitpkg", env::var("HOME").unwrap());

    // Clean all temp directories
    let temp_dir = Path::new(&gitpkg_dir).join("temp");
    if temp_dir.exists() {
        match fs::remove_dir_all(&temp_dir) {
            Ok(_) => {
                println!("Removed all temp files");
                // Recreate temp directory
                let _ = fs::create_dir_all(&temp_dir);
            }
            Err(e) => eprintln!("Failed to remove temp directory: {}", e),
        }
    }

    // Get list of all installed packages
    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return;
    }

    println!("Cleaning old versions for {} package(s)...", packages.len());
    // Iterate packages in alphabetical order for consistent output
    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        println!("\n--- Cleaning {} ---", package);
        clean(&package);
    }

    println!("\nCleanup complete!");
}

fn list() {
    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return;
    }

    println!("Installed packages:");
    println!("{:-<60}", "");

    // Iterate packages in alphabetical order for consistent output
    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        if let Some(info_path) = packages.get(&package) {
            // Read the info file to get details
            if let Ok(content) = fs::read_to_string(info_path) {
                if let Ok(info) = toml::from_str::<toml::Value>(&content) {
                    let commit = info
                        .get("latest_commit")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let build_sys = info
                        .get("build_system")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let timestamp = info
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let supplier = info
                        .get("supplier")
                        .and_then(|v| v.as_str())
                        .unwrap_or("github.com");
                    let has_data = info
                        .get("has_data_files")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    // Compute package size: sum of version directories under package dir
                    let mut size_bytes: u64 = 0;
                    if let Some(info_parent) = Path::new(info_path).parent() {
                        if let Ok(entries) = fs::read_dir(info_parent) {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                if name == "info.gitpkg" {
                                    continue;
                                }
                                if p.is_dir() {
                                    size_bytes += dir_size_bytes(&p);
                                }
                            }
                        }
                    }

                    println!("Package:    {}", package);
                    println!(
                        "  Commit:   {} ({})",
                        &commit[..commit.len().min(8)],
                        commit
                    );
                    println!("  Build:    {}", build_sys);
                    println!("  Supplier: {}", supplier);
                    println!("  Data:     {}", if has_data { "yes" } else { "no" });
                    println!("  Installed: {}", timestamp);
                    println!("  Size:      {}", format_mb(size_bytes));
                    println!();
                } else {
                    println!("Package:    {}", package);
                    println!("  (Failed to read info)");
                    println!();
                }
            } else {
                println!("Package:    {}", package);
                println!("  (Info file not found)");
                println!();
            }
        }
    }

    println!("{:-<60}", "");
    println!("Total: {} package(s)", packages.len());
}

fn versions(package: &str) {
    // Use the same package selection logic as clean/upgrade
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    let exact_match = find_package_by_key(package);

    let (pkg_key, supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        let matches = find_matching_packages(&user, &repo);
        if matches.is_empty() {
            println!("Package {} is not installed", package);
            return;
        }
        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    eprintln!("Invalid selection");
                    return;
                }
            }
        } else {
            0
        };
        matches[selected].clone()
    };

    // Read info to determine current version
    let current_commit = if let Ok(content) = fs::read_to_string(&info_path) {
        if let Ok(info) = toml::from_str::<toml::Value>(&content) {
            info.get("latest_commit")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let package_dir = Path::new(&env::var("HOME").unwrap())
        .join(".local/share/gitpkg")
        .join(&pkg_key);

    println!("Versions for {} (supplier: {})", pkg_key, supplier);

    if let Ok(entries) = fs::read_dir(&package_dir) {
        // rows: (name, size, is_current, Option<install_datetime>)
        let mut rows: Vec<(String, u64, bool, Option<chrono::DateTime<chrono::Utc>>)> = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "info.gitpkg" {
                continue;
            }
            if p.is_dir() {
                let size = dir_size_bytes(&p);
                let is_current = current_commit.as_deref() == Some(&name);

                // Use directory modification time as install datetime (may reflect install or filesystem mtime)
                let install_dt = fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|st| chrono::DateTime::<chrono::Utc>::from(st));

                rows.push((name, size, is_current, install_dt));
            }
        }

        if rows.is_empty() {
            println!("  (No versions found)");
            return;
        }

        // Sort by install datetime ascending (oldest first). Unknown timestamps go last.
        rows.sort_by(|a, b| match (&a.3, &b.3) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });

        // Ensure current version is always shown at the bottom
        if let Some(pos) = rows.iter().position(|r| r.2) {
            let cur = rows.remove(pos);
            rows.push(cur);
        }

        for (name, size, is_current, install_dt) in rows {
            let dt_str = install_dt
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_string());
            if is_current {
                println!("* {}  {}  {}  (current)", name, format_mb(size), dt_str);
            } else {
                println!("  {}  {}  {}", name, format_mb(size), dt_str);
            }
        }
    } else {
        println!("No versions found for {}", pkg_key);
    }
}

fn upgrade(package: &str, verbose: bool, supplier: Option<&str>) {
    // Check if this is a full package key (contains underscore and slash, e.g., "codeberg_el1lovescomputers/gitpkg")
    // or just a user/repo pair
    let (user, repo, stored_supplier, info_path) = if package.contains('_') && package.contains('/')
    {
        // This looks like a full key, try to find it directly
        match find_package_by_key(package) {
            Some((_pkg_key, sup, path)) => {
                // Parse the user/repo from the key for git operations
                let (u, r) = parse_pkg(package);
                (u, r, sup, path)
            }
            None => {
                eprintln!(
                    "Package {} is not installed. Use 'install' instead.",
                    package
                );
                return;
            }
        }
    } else {
        // Standard user/repo format
        let (user, repo) = parse_pkg(package);

        // Find all matching packages
        let matches = find_matching_packages(&user, &repo);

        if matches.is_empty() {
            eprintln!(
                "Package {}/{} is not installed. Use 'install' instead.",
                user, repo
            );
            return;
        }

        let (_pkg_key, stored_supplier, info_path) = if matches.len() > 1 && supplier.is_none() {
            // Multiple packages found and no supplier specified, prompt user
            match prompt_package_selection(&matches) {
                Some(idx) => matches[idx].clone(),
                None => {
                    eprintln!("Invalid selection");
                    return;
                }
            }
        } else if matches.len() > 1 && supplier.is_some() {
            // Multiple packages but supplier specified, find exact match
            let supplier_str = supplier.unwrap();
            match matches.iter().find(|(_, s, _)| s == supplier_str) {
                Some(m) => m.clone(),
                None => {
                    eprintln!(
                        "Package {}/{} from {} is not installed",
                        user, repo, supplier_str
                    );
                    return;
                }
            }
        } else {
            matches[0].clone()
        };

        (user, repo, stored_supplier, info_path)
    };

    // Read current info
    let info_content = match fs::read_to_string(&info_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read info file: {}", e);
            return;
        }
    };

    let info: toml::Value = match toml::from_str(&info_content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse info file: {}", e);
            return;
        }
    };

    let current_commit = match info.get("latest_commit").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            eprintln!("No commit hash found in info file");
            return;
        }
    };

    // Use provided supplier or stored supplier
    let supplier_to_use = supplier.unwrap_or(&stored_supplier);

    println!(
        "Checking for updates to {} from {}...",
        get_package_key(&user, &repo, &stored_supplier),
        supplier_to_use
    );
    println!("Current commit: {}", current_commit);

    // Clone to temp to get latest commit
    let url = build_git_url(&user, &repo, Some(supplier_to_use));
    let path = temp_path(&user, &repo);

    if Path::new(&path).exists() {
        fs::remove_dir_all(&path).unwrap();
    }

    if !run_git_clone_with_progress(&url, &path, verbose) {
        eprintln!("Git clone failed");
        return;
    }

    let latest_commit = match get_commit_hash(&path) {
        Some(c) => c,
        None => {
            eprintln!("Failed to get latest commit hash");
            let _ = fs::remove_dir_all(&path);
            return;
        }
    };

    println!("Latest commit:  {}", latest_commit);

    if current_commit == latest_commit {
        println!(
            "{} is already up to date!",
            get_package_key(&user, &repo, &stored_supplier)
        );
        let _ = fs::remove_dir_all(&path);
        return;
    }

    println!("Update available! Building new version...");

    // Build the new version (this will create a new install path with the new commit hash)
    build(&user, &repo, verbose, Some(supplier_to_use));

    println!(
        "Successfully upgraded {} from {} to {}",
        get_package_key(&user, &repo, &stored_supplier),
        &current_commit[..8],
        &latest_commit[..8]
    );
}

fn upgrade_all(verbose: bool) {
    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return;
    }

    println!("Found {} installed package(s)", packages.len());

    // Iterate packages in alphabetical order for consistent output
    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        println!("\n--- Upgrading {} ---", package);
        upgrade(&package, verbose, None); // None means use stored supplier
    }

    println!("\nAll packages checked for updates!");
}

// Helper: run a command with optional output
fn run_cmd(mut cmd: Command, verbose: bool) -> bool {
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

// Specialized helper for `git clone` that shows a simple progress bar in non-verbose mode.
fn run_git_clone_with_progress(url: &str, path: &str, verbose: bool) -> bool {
    if verbose {
        let mut cmd = Command::new("git");
        cmd.arg("clone").arg(url).arg(path);
        return run_cmd(cmd, true);
    }

    let mut child = match Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(path)
        .arg("--progress")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to start git clone: {}", e);
            return false;
        }
    };

    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            eprintln!("Failed to capture git stderr");
            return false;
        }
    };

    let mut reader = BufReader::new(stderr);
    let mut buf: Vec<u8> = Vec::new();
    let mut last_percent: u8 = 0;

    // Read progress output chunk by chunk, using '\r' which git uses for progress updates.
    loop {
        buf.clear();

        // Read until a carriage return or EOF.
        match reader.read_until(b'\r', &mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }

        let line = match String::from_utf8(buf.clone()) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // git writes progress to stderr in lines like:
        // "Receiving objects:  42% (123/456), 1.23 MiB | 1.23 MiB/s"
        if let Some(p_idx) = line.find('%') {
            let before = &line[..p_idx];
            if let Some(start) = before.rfind(' ') {
                let num_str = before[start..].trim();
                if let Ok(p) = num_str.parse::<u8>() {
                    if p != last_percent {
                        last_percent = p;
                        let bar_width = 40;
                        let filled = (p as usize * bar_width) / 100;
                        let empty = bar_width - filled;
                        let bar = format!("[{}{}]", "#".repeat(filled), " ".repeat(empty));
                        print!("\rCloning repository {} {}", bar, format!("{:3}%", p));
                        let _ = std::io::stdout().flush();
                    }
                }
            }
        }
    }

    // Finish the progress line with a newline.
    if last_percent > 0 {
        println!();
    }

    match child.wait() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("git clone failed to complete: {}", e);
            false
        }
    }
}
