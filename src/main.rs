use std::{
    collections::HashMap,
    env,
    fs::{self},
    path::Path,
    process::{Command, Stdio},
};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: gitpkg <install|remove|clean|list|upgrade> [args] [-v] [--supplier <domain>]"
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
            remove(&args[2]);
        }
        "clean" => {
            if args.len() >= 3 && &args[2] == "all" {
                clean_all();
            } else if args.len() >= 3 {
                clean(&args[2]);
            } else {
                clean_all();
            }
        }
        "list" => list(),
        "upgrade" => {
            if args.len() < 3 {
                eprintln!(
                    "Usage: gitpkg upgrade <user>/<repo> or gitpkg upgrade all [--supplier <domain>]"
                );
                return;
            }
            if &args[2] == "all" {
                upgrade_all(verbose);
            } else {
                upgrade(&args[2], verbose, supplier.as_deref());
            }
        }
        _ => eprintln!("Unknown command: {}", command),
    }
}

fn parse_pkg(arg: &str) -> (&str, &str) {
    let mut parts = arg.split('/');
    (parts.next().unwrap(), parts.next().unwrap())
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
    for (file, sys) in [
        ("Cargo.toml", "cargo"),
        ("Makefile", "make"),
        ("CMakeLists.txt", "cmake"),
        ("package.json", "npm"),
        ("build.gradle", "gradle"),
        ("meson.build", "meson"),
        ("mason.toml", "mason"),
        ("go.mod", "go"),
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

fn find_built_executable(build_dir: &Path, repo: &str) -> Option<String> {
    // First try to parse Makefile if it exists
    let makefile = build_dir.join("Makefile");
    let search_names = if makefile.exists() {
        find_executables_in_makefile(&makefile, repo)
    } else {
        vec![
            repo.to_string(),
            repo.to_lowercase(),
            "a.out".to_string(),
            "main".to_string(),
        ]
    };

    // Search for executables in the build directory and subdirectories
    let search_dirs = vec![
        build_dir.to_path_buf(),
        build_dir.join("bin"),
        build_dir.join("build"),
        build_dir.join("out"),
        build_dir.join("target"),
    ];

    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }

        // Check each potential name
        for exe_name in &search_names {
            let exe_path = dir.join(exe_name);
            if exe_path.exists() && exe_path.is_file() {
                // Check if it's executable
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(&exe_path) {
                        if metadata.permissions().mode() & 0o111 != 0 {
                            return Some(exe_path.to_string_lossy().to_string());
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    return Some(exe_path.to_string_lossy().to_string());
                }
            }
        }

        // Also search all files in the directory for executables
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if metadata.permissions().mode() & 0o111 != 0 {
                                // Found an executable file
                                let path = entry.path();
                                let filename = path.file_name().unwrap().to_string_lossy();
                                // Skip common non-executable files
                                if !filename.ends_with(".sh")
                                    && !filename.ends_with(".py")
                                    && !filename.ends_with(".pl")
                                    && !filename.starts_with(".")
                                    && !filename.contains("Makefile")
                                {
                                    return Some(path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
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

    let repo_name = if repo.ends_with(".git") {
        repo.to_string()
    } else {
        format!("{}.git", repo)
    };

    match supplier_domain {
        // SourceHut special case
        // Repos live at git.sr.ht and users are prefixed with ~
        "sr.ht" | "git.sr.ht" => {
            format!("https://git.sr.ht/~{}/{}", user, repo_name)
        }

        // Default: GitHub / GitLab / Codeberg style
        _ => {
            format!("https://{}/{}/{}", supplier_domain, user, repo_name)
        }
    }
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

fn get_supplier_from_url(url: &str) -> Option<String> {
    // Extract domain from URL like "https://gitlab.com/user/repo.git"
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
    let url = build_git_url(user, repo, supplier);
    let supplier_domain = supplier.unwrap_or("github.com");

    let path = temp_path(user, repo);
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path).unwrap();
    }

    println!("Cloning {} from {} into {}", package, supplier_domain, path);
    let mut clone_cmd = Command::new("git");
    clone_cmd.arg("clone").arg(&url).arg(&path);
    if !run_cmd(clone_cmd, verbose) {
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

    build(user, repo, verbose, Some(supplier_domain));
}

fn build(user: &str, repo: &str, verbose: bool, supplier: Option<&str>) {
    let temp = temp_path(user, repo);
    let commit = get_commit_hash(&temp).unwrap_or_else(|| "unknown".to_string());
    let supplier_domain = supplier.unwrap_or("github.com");
    let install_path = install_root(user, repo, &commit, supplier_domain);
    fs::create_dir_all(&install_path).unwrap();

    let bs = match detect_build_system(&temp) {
        Some(s) => s,
        None => {
            println!("No build system detected");
            return;
        }
    };
    println!("Building {} with {}", repo, bs);

    // Build command per system
    let status = match bs {
        "cargo" => {
            let mut cmd = Command::new("cargo");
            cmd.arg("install")
                .arg("--path")
                .arg(&temp)
                .arg("--root")
                .arg(&install_path)
                .arg("--force");
            if !verbose {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            cmd.status().unwrap()
        }
        "make" | "cmake" | "meson" | "mason" => {
            // Create bin directory
            let bin_dir = Path::new(&install_path).join("bin");
            fs::create_dir_all(&bin_dir).unwrap();

            // Run make
            let mut make_cmd = Command::new("make");
            make_cmd.current_dir(&temp);
            if !verbose {
                make_cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            let make_status = make_cmd.status().unwrap();

            if !make_status.success() {
                make_status
            } else {
                // Find the built executable
                match find_built_executable(Path::new(&temp), repo) {
                    Some(exe_path) => {
                        println!("Found executable: {}", exe_path);
                        let dest = bin_dir.join(repo);
                        fs::copy(&exe_path, &dest).unwrap();
                        // Make sure it's executable
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let mut perms = fs::metadata(&dest).unwrap().permissions();
                            perms.set_mode(0o755);
                            fs::set_permissions(&dest, perms).unwrap();
                        }
                        make_status
                    }
                    None => {
                        eprintln!("Could not find executable after build");
                        eprintln!("Searched in: {}", temp);
                        eprintln!("Try running with -v flag to see build output");
                        return;
                    }
                }
            }
        }
        "go" => {
            let bin_dir = Path::new(&install_path).join("bin");
            fs::create_dir_all(&bin_dir).unwrap();

            let mut cmd = Command::new("go");
            cmd.arg("build")
                .arg("-o")
                .arg(bin_dir.join(repo))
                .current_dir(&temp);
            if !verbose {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            cmd.status().unwrap()
        }
        "npm" => {
            // Run npm install and npm build if available
            let mut install_cmd = Command::new("npm");
            install_cmd.arg("install").current_dir(&temp);
            if !verbose {
                install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            let install_status = install_cmd.status().unwrap();

            if !install_status.success() {
                install_status
            } else {
                // Try npm run build or just mark as success
                let mut build_cmd = Command::new("npm");
                build_cmd.arg("run").arg("build").current_dir(&temp);
                if !verbose {
                    build_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                }
                // Don't fail if build script doesn't exist
                let _ = build_cmd.status();

                // For npm packages, we'll just symlink the whole directory
                println!("Note: npm packages installed in place at {}", temp);
                install_status
            }
        }
        "gradle" => {
            let mut cmd = Command::new("gradle");
            cmd.arg("build").current_dir(&temp);
            if !verbose {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            cmd.status().unwrap()
        }
        _ => {
            println!("Unsupported build system: {}", bs);
            return;
        }
    };

    if status.success() {
        println!("Installed to {}", install_path);

        // Remove old build files (only for non-npm)
        if bs != "npm" {
            let _ = fs::remove_dir_all(&temp);
        }

        // Handle symlink and .desktop
        let exe_path = Path::new(&install_path).join("bin").join(repo);

        // Authenticate sudo before creating symlink
        println!("Creating symlink to /usr/bin (requires sudo)...");
        let sudo_auth = Command::new("sudo").arg("-v").status();

        let symlink_path = Path::new("/usr/bin").join(repo);

        let symlink_created = if sudo_auth.is_ok() && sudo_auth.unwrap().success() {
            // Remove old symlink if exists
            let _ = Command::new("sudo")
                .arg("rm")
                .arg("-f")
                .arg(&symlink_path)
                .status();

            // Create new symlink with sudo
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
        } else {
            eprintln!("Failed to authenticate sudo");
            false
        };

        if !symlink_created {
            eprintln!("Trying ~/.local/bin instead...");

            // Fallback to ~/.local/bin
            let local_bin = Path::new(&env::var("HOME").unwrap()).join(".local/bin");
            fs::create_dir_all(&local_bin).unwrap();
            let local_symlink = local_bin.join(repo);
            let _ = fs::remove_file(&local_symlink);

            match std::os::unix::fs::symlink(&exe_path, &local_symlink) {
                Ok(_) => {
                    println!(
                        "Created symlink: {} -> {}",
                        local_symlink.display(),
                        exe_path.display()
                    );
                    println!("Note: Make sure ~/.local/bin is in your PATH");
                    println!("Add this to your ~/.bashrc or ~/.zshrc:");
                    println!("  export PATH=\"$HOME/.local/bin:$PATH\"");
                }
                Err(e) => {
                    eprintln!("Failed to create symlink: {}", e);
                    println!(
                        "You can run the executable directly at: {}",
                        exe_path.display()
                    );
                }
            }
        }

        let final_symlink_path = if symlink_created {
            symlink_path.to_str().unwrap().to_string()
        } else {
            Path::new(&env::var("HOME").unwrap())
                .join(".local/bin")
                .join(repo)
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
                        format!("Exec={}", exe_path.to_str().unwrap())
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&desktop_file_dst, new_content).unwrap();
            desktop_path = Some(desktop_file_dst.to_str().unwrap().to_string());
        }

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
        );

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
         supplier = \"{}\"\n",
        user,
        repo,
        commit,
        build_system,
        pm,
        Utc::now().to_rfc3339(),
        install_path,
        symlink_path,
        supplier
    );

    if let Some(dp) = desktop_path {
        toml_data.push_str(&format!("desktop_file = \"{}\"\n", dp));
    }

    fs::write(&info_file, toml_data).unwrap();

    // Add to global package list
    add_to_package_list(user, repo, info_file.to_str().unwrap(), supplier);
}

fn remove(package: &str) {
    let (user, repo) = parse_pkg(package);

    // Find all matching packages
    let matches = find_matching_packages(user, repo);

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
    let temp = temp_path(user, repo);
    if Path::new(&temp).exists() {
        let _ = fs::remove_dir_all(&temp);
    }

    // Remove from global package list
    remove_from_package_list(&pkg_key);

    println!("Successfully removed {}", pkg_key);
}

fn clean(package: &str) {
    let (user, repo) = parse_pkg(package);

    // Find all matching packages
    let matches = find_matching_packages(user, repo);

    if matches.is_empty() {
        println!(
            "Package {}/{} is not installed, nothing to clean",
            user, repo
        );
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

    println!(
        "Cleaning old versions and temp files for {} from {}...",
        pkg_key, supplier
    );

    // Clean temp directory for this package
    let temp = temp_path(user, repo);
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
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                // Skip if it's the current version or the info file
                if dir_name != current_commit && dir_name != "info.gitpkg" {
                    println!("Removing old version: {}", dir_name);
                    match fs::remove_dir_all(&path) {
                        Ok(_) => {
                            removed_count += 1;
                            println!("  Removed: {}", path.display());
                        }
                        Err(e) => eprintln!("  Failed to remove {}: {}", path.display(), e),
                    }
                }
            }
        }

        if removed_count > 0 {
            println!("Removed {} old version(s)", removed_count);
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

    for (package, _) in packages {
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

    for (package, info_path) in packages.iter() {
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

                println!("Package:    {}", package);
                println!(
                    "  Commit:   {} ({})",
                    &commit[..commit.len().min(8)],
                    commit
                );
                println!("  Build:    {}", build_sys);
                println!("  Supplier: {}", supplier);
                println!("  Installed: {}", timestamp);
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

    println!("{:-<60}", "");
    println!("Total: {} package(s)", packages.len());
}

fn upgrade(package: &str, verbose: bool, supplier: Option<&str>) {
    let (user, repo) = parse_pkg(package);

    // Find all matching packages
    let matches = find_matching_packages(user, repo);

    if matches.is_empty() {
        eprintln!(
            "Package {}/{} is not installed. Use 'install' instead.",
            user, repo
        );
        return;
    }

    let (pkg_key, stored_supplier, info_path) = if matches.len() > 1 && supplier.is_none() {
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
        pkg_key, supplier_to_use
    );
    println!("Current commit: {}", current_commit);

    // Clone to temp to get latest commit
    let url = build_git_url(user, repo, Some(supplier_to_use));
    let path = temp_path(user, repo);

    if Path::new(&path).exists() {
        fs::remove_dir_all(&path).unwrap();
    }

    let mut clone_cmd = Command::new("git");
    clone_cmd.arg("clone").arg(&url).arg(&path);
    if !run_cmd(clone_cmd, verbose) {
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
        println!("{} is already up to date!", pkg_key);
        let _ = fs::remove_dir_all(&path);
        return;
    }

    println!("Update available! Building new version...");

    // Build the new version (this will create a new install path with the new commit hash)
    build(user, repo, verbose, Some(supplier_to_use));

    println!(
        "Successfully upgraded {} from {} to {}",
        pkg_key,
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

    for (pkg_key, info_path) in packages.iter() {
        println!("\n--- Upgrading {} ---", pkg_key);

        // Read info file to get user/repo/supplier
        let info_content = match fs::read_to_string(info_path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("Failed to read info for {}, skipping", pkg_key);
                continue;
            }
        };

        let info: toml::Value = match toml::from_str(&info_content) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("Failed to parse info for {}, skipping", pkg_key);
                continue;
            }
        };

        let user = match info.get("user").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => {
                eprintln!("No user in info for {}, skipping", pkg_key);
                continue;
            }
        };

        let repo = match info.get("repo").and_then(|v| v.as_str()) {
            Some(r) => r,
            None => {
                eprintln!("No repo in info for {}, skipping", pkg_key);
                continue;
            }
        };

        let supplier = info
            .get("supplier")
            .and_then(|v| v.as_str())
            .unwrap_or("github.com");

        upgrade(&format!("{}/{}", user, repo), verbose, Some(supplier));
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
