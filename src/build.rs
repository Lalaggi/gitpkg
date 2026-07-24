use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::is_installed;
use crate::detect::{detect_js_package_manager, is_python_file};
use crate::error::GitpkgError;
use crate::util::pascal_to_kebab_case;

/// Per-package build customization. `make_target` overrides the default make
/// goal (e.g. `build-i686`); `build_flags` are extra args appended to the
/// underlying build command (make or cmake).
#[derive(Clone, Debug, Default)]
pub struct BuildConfig {
    pub make_target: Option<String>,
    pub build_flags: Option<String>,
    pub submodules: bool,
}

impl BuildConfig {
    pub fn from_info(info: &toml::Value) -> BuildConfig {
        let make_target = info
            .get("make_target")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let build_flags = info
            .get("build_flags")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let submodules = info
            .get("submodules")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        BuildConfig {
            make_target,
            build_flags,
            submodules,
        }
    }

    fn flags_vec(&self) -> Vec<String> {
        self.build_flags
            .as_deref()
            .map(|s| shell_words::split(s).unwrap_or_default())
            .unwrap_or_default()
    }
}

pub fn find_executables_in_makefile(makefile_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(makefile_path) {
        for line in content.lines() {
            let line = line.trim();

            if let Some(colon_pos) = line.find(':') {
                let target = line[..colon_pos].trim();
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

    targets.push(repo.to_string());
    targets.push(repo.to_lowercase());
    targets.push("a.out".to_string());
    targets.push("main".to_string());

    targets
}

pub fn find_executables_in_meson(meson_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(meson_path) {
        for line in content.lines() {
            let line = line.trim();

            if line.contains("executable(") {
                if let Some(start) = line.find("executable(") {
                    let after_exec = &line[start + 11..];
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

    if targets.is_empty() {
        targets.push(repo.to_string());
        targets.push(repo.to_lowercase());
    }

    targets
}

pub fn find_executables_in_cmake(cmake_path: &Path, repo: &str) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(cmake_path) {
        for line in content.lines() {
            let line = line.trim();

            if line.contains("add_executable(") {
                if let Some(start) = line.find("add_executable(") {
                    let after_exec = &line[start + 15..];
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

    if targets.is_empty() {
        targets.push(repo.to_string());
        targets.push(repo.to_lowercase());
    }

    targets
}

pub fn find_executables_in_cargo(cargo_path: &Path) -> Vec<String> {
    let mut targets = Vec::new();

    if let Ok(content) = fs::read_to_string(cargo_path) {
        let mut in_package = false;
        let mut in_bin = false;
        let mut package_name = String::new();
        let mut has_explicit_bin = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("[package]") {
                in_package = true;
                in_bin = false;
                continue;
            }

            if trimmed.starts_with("[[bin]]") {
                in_bin = true;
                in_package = false;
                has_explicit_bin = true;
                continue;
            }

            if trimmed.starts_with('[') {
                in_package = false;
                in_bin = false;
            }

            if in_package && trimmed.starts_with("name = ") {
                if let Some(val) = trimmed.split('=').nth(1) {
                    package_name = val.trim().trim_matches('"').to_string();
                }
            }

            if in_bin && trimmed.starts_with("name = ") {
                if let Some(val) = trimmed.split('=').nth(1) {
                    let name = val.trim().trim_matches('"').to_string();
                    if !name.is_empty() {
                        targets.push(name);
                    }
                }
            }
        }

        if !has_explicit_bin && !package_name.is_empty() {
            targets.push(package_name);
        }
    }

    targets
}

pub fn find_all_executables_recursive(dir: &Path) -> Vec<String> {
    let mut executables = Vec::new();

    fn search_dir(dir: &Path, executables: &mut Vec<String>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_dir() {
                    let dir_name = path.file_name().unwrap().to_string_lossy();
                    if !dir_name.starts_with('.')
                        && dir_name != "node_modules"
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

pub fn prompt_executable_selection(executables: &[String]) -> Option<String> {
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
    io::stdout().flush().ok()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;

    if let Ok(choice) = input.trim().parse::<usize>() {
        if choice >= 1 && choice <= executables.len() {
            return Some(executables[choice - 1].clone());
        }
    }

    None
}

pub fn find_built_executable(
    build_dir: &Path,
    repo: &str,
    build_system: &str,
) -> Option<String> {
    find_built_executable_with_dirs(build_dir, repo, build_system, &[])
}

/// Like `find_built_executable`, but also searches the given extra directories
/// first. Used when a custom make target builds into a subdirectory.
pub fn find_built_executable_with_dirs(
    build_dir: &Path,
    repo: &str,
    build_system: &str,
    extra_dirs: &[PathBuf],
) -> Option<String> {
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

    let cargo_toml = build_dir.join("Cargo.toml");
    if cargo_toml.exists() {
        let cargo_names = find_executables_in_cargo(&cargo_toml);
        search_names.extend(cargo_names);
    }

    search_names.push(repo.to_string());
    search_names.push(repo.to_lowercase());
    search_names.push(pascal_to_kebab_case(repo));
    search_names.push("a.out".to_string());
    search_names.push("main".to_string());

    let mut search_dirs = vec![
        build_dir.to_path_buf(),
        build_dir.join("bin"),
        build_dir.join("build"),
        build_dir.join("out"),
        build_dir.join("target"),
        build_dir.join("target/release"),
        build_dir.join("target/debug"),
        build_dir.join("src"),
    ];
    for d in extra_dirs {
        search_dirs.insert(0, d.clone());
    }

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

    println!("Expected executable not found, searching entire build directory...");
    let all_executables = find_all_executables_recursive(build_dir);

    if all_executables.is_empty() {
        return None;
    }

    let repo_lower = repo.to_lowercase();
    let preferred = all_executables.iter().find(|p| {
        Path::new(p)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase() == repo_lower)
            .unwrap_or(false)
    });
    if let Some(p) = preferred {
        return Some(p.clone());
    }

    prompt_executable_selection(&all_executables)
}

pub fn find_installed_executable(install_path: &Path, repo: &str) -> Option<PathBuf> {
    let bin_dir = install_path.join("bin");
    if !bin_dir.exists() {
        return None;
    }

    let mut prefs = vec![repo.to_string(), repo.to_lowercase()];

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

    for name in prefs {
        let candidate = bin_dir.join(&name);
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }

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

/// Returns true if the project's cargo configuration replaces the crates.io
/// source with a local `directory` source (i.e. it expects a populated
/// `vendor/` directory). Such projects require `cargo vendor` to be run
/// before the build, otherwise `cargo build`/`install` fails offline.
fn cargo_uses_vendor_dir(temp: &str) -> bool {
    let cargo_home = std::env::var("CARGO_HOME")
        .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
    let candidates = [
        format!("{}/.cargo/config.toml", temp),
        format!("{}/.cargo/config", temp),
        format!("{}/config.toml", cargo_home),
        format!("{}/config", cargo_home),
    ];
    for cfg in candidates {
        if let Ok(content) = fs::read_to_string(&cfg) {
            // A vendored directory source looks like:
            //   [source.<name>]
            //   directory = "vendor"
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("directory") && trimmed.contains('=') {
                    let val = trimmed.split('=').nth(1).unwrap_or("").trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn build_cargo(temp: &str, install_path: &str, verbose: bool) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    // Projects that replace crates.io with a vendored directory source need
    // their `vendor/` populated before `cargo install` can resolve deps.
    if cargo_uses_vendor_dir(temp) {
        println!("Vendoring Rust dependencies (cargo vendor)...");
        let mut vendor_cmd = Command::new("cargo");
        vendor_cmd.arg("vendor").arg("vendor").current_dir(temp);
        if !verbose {
            vendor_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        match vendor_cmd.status() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("cargo vendor failed (status {}); build may fail", s);
            }
            Err(e) => {
                eprintln!("Failed to run cargo vendor: {}", e);
            }
        }
    }

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
    Ok(Some(cmd.status()?))
}

pub fn build_make(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
    config: &BuildConfig,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(temp);
    if let Some(target) = &config.make_target {
        make_cmd.arg(target);
    }
    for flag in config.flags_vec() {
        make_cmd.arg(flag);
    }
    if !verbose {
        make_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let make_status = make_cmd.status()?;

    if !make_status.success() {
        return Ok(Some(make_status));
    }

    let mut extra_dirs: Vec<PathBuf> = Vec::new();
    if let Some(target) = &config.make_target {
        let target_dir = Path::new(temp).join(target);
        if target_dir.is_dir() {
            extra_dirs.push(target_dir);
        }
    }

    match find_built_executable_with_dirs(Path::new(temp), repo, "make", &extra_dirs) {
        Some(exe_path) => {
            println!("Found executable: {}", exe_path);
            let exe_name = Path::new(&exe_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo);
            let dest = bin_dir.join(exe_name);
            fs::copy(&exe_path, &dest)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            Ok(Some(make_status))
        }
        None => {
            eprintln!("Could not find executable after build");
            eprintln!("Searched in: {}", temp);
            eprintln!("Try running with -v flag to see build output");
            Ok(None)
        }
    }
}

pub fn build_cmake(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
    config: &BuildConfig,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let build_dir = Path::new(temp).join("build");
    fs::create_dir_all(&build_dir)?;

    let mut cmake_cmd = Command::new("cmake");
    cmake_cmd
        .arg("..")
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install_path))
        .current_dir(&build_dir);
    for flag in config.flags_vec() {
        cmake_cmd.arg(flag);
    }
    if !verbose {
        cmake_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    if !cmake_cmd.status()?.success() {
        eprintln!("CMake configuration failed");
        return Ok(None);
    }

    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(&build_dir);
    for flag in config.flags_vec() {
        make_cmd.arg(flag);
    }
    if !verbose {
        make_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let make_status = make_cmd.status()?;

    if !make_status.success() {
        return Ok(Some(make_status));
    }

    let mut install_cmd = Command::new("make");
    install_cmd.arg("install").current_dir(&build_dir);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status()?;

    let bin_dir = Path::new(install_path).join("bin");
    let exe_path = bin_dir.join(repo);

    if exe_path.exists() {
        Ok(Some(install_status))
    } else {
        match find_built_executable(&build_dir, repo, "cmake") {
            Some(built_exe) => {
                fs::create_dir_all(&bin_dir)?;
                let exe_name = Path::new(&built_exe)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(repo);
                let dest = bin_dir.join(exe_name);
                fs::copy(&built_exe, &dest)?;
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest, perms)?;
                Ok(Some(install_status))
            }
            None => {
                eprintln!("Could not find executable after build");
                Ok(None)
            }
        }
    }
}

pub fn build_meson(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let build_dir = Path::new(temp).join("build");

    let mut setup_cmd = Command::new("meson");
    setup_cmd
        .arg("setup")
        .arg(&build_dir)
        .arg(format!("--prefix={}", install_path))
        .current_dir(temp);
    if !verbose {
        setup_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    if !setup_cmd.status()?.success() {
        eprintln!("Meson setup failed");
        return Ok(None);
    }

    let mut compile_cmd = Command::new("meson");
    compile_cmd.arg("compile").arg("-C").arg(&build_dir);
    if !verbose {
        compile_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let compile_status = compile_cmd.status()?;

    if !compile_status.success() {
        return Ok(Some(compile_status));
    }

    println!("Installing with meson (this handles data files)...");
    let mut install_cmd = Command::new("meson");
    install_cmd.arg("install").arg("-C").arg(&build_dir);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status()?;

    let bin_dir = Path::new(install_path).join("bin");
    let exe_path = bin_dir.join(repo);

    if !exe_path.exists() {
        if let Ok(entries) = fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.to_lowercase() == repo.to_lowercase()
                        || name.contains(&repo.to_lowercase())
                    {
                        let dest = bin_dir.join(repo);
                        let _ = std::os::unix::fs::symlink(&path, &dest);
                        break;
                    }
                }
            }
        }
    }

    Ok(Some(install_status))
}

pub fn build_mason(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    build_make(temp, install_path, repo, verbose, &BuildConfig::default())
}

pub fn build_ninja(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut ninja_cmd = Command::new("ninja");
    ninja_cmd.current_dir(temp);
    if !verbose {
        ninja_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let ninja_status = ninja_cmd.status()?;

    if !ninja_status.success() {
        return Ok(Some(ninja_status));
    }

    match find_built_executable(Path::new(temp), repo, "ninja") {
        Some(exe_path) => {
            println!("Found executable: {}", exe_path);
            let exe_name = Path::new(&exe_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo);
            let dest = bin_dir.join(exe_name);
            fs::copy(&exe_path, &dest)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            Ok(Some(ninja_status))
        }
        None => {
            eprintln!("Could not find executable after build");
            eprintln!("Searched in: {}", temp);
            eprintln!("Try running with -v flag to see build output");
            Ok(None)
        }
    }
}

pub fn build_go(temp: &str, install_path: &str, repo: &str, verbose: bool) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut cmd = Command::new("go");
    cmd.arg("build")
        .arg("-o")
        .arg(bin_dir.join(repo))
        .current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    Ok(Some(cmd.status()?))
}

pub fn build_just(temp: &str, install_path: &str, repo: &str, verbose: bool) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut cmd = Command::new("just");
    cmd.current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;

    if !status.success() {
        return Ok(Some(status));
    }

    match find_built_executable(Path::new(temp), repo, "just") {
        Some(exe_path) => {
            if verbose {
                println!("Found executable: {}", exe_path);
            }
            let exe_name = Path::new(&exe_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo);
            let dest = bin_dir.join(exe_name);
            fs::copy(&exe_path, &dest)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            Ok(Some(status))
        }
        None => {
            eprintln!("Could not find executable after build");
            eprintln!("Searched in: {}", temp);
            eprintln!("Try running with -v flag to see build output");
            Ok(None)
        }
    }
}

pub fn build_rake(temp: &str, install_path: &str, repo: &str, verbose: bool) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut cmd = Command::new("rake");
    cmd.current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;

    if !status.success() {
        return Ok(Some(status));
    }

    match find_built_executable(Path::new(temp), repo, "rake") {
        Some(exe_path) => {
            if verbose {
                println!("Found executable: {}", exe_path);
            }
            let exe_name = Path::new(&exe_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo);
            let dest = bin_dir.join(exe_name);
            fs::copy(&exe_path, &dest)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            Ok(Some(status))
        }
        None => {
            eprintln!("Could not find executable after build");
            eprintln!("Searched in: {}", temp);
            eprintln!("Try running with -v flag to see build output");
            Ok(None)
        }
    }
}

pub fn build_nodejs(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
    js_pm: &str,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut install_cmd = Command::new(js_pm);
    install_cmd.arg("install").current_dir(temp);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status()?;

    if !install_status.success() {
        return Ok(Some(install_status));
    }

    let package_json_path = Path::new(temp).join("package.json");
    let has_build_script = fs::read_to_string(&package_json_path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .map(|j| {
            j.get("scripts")
                .and_then(|s| s.get("build"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .is_some()
        })
        .unwrap_or(false);

    if has_build_script {
        let mut build_cmd = Command::new(js_pm);
        build_cmd.arg("run").arg("build").current_dir(temp);
        if !verbose {
            build_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let _ = build_cmd.status();
    }

    let package_json = Path::new(temp).join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
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
                        if src.extension().and_then(|e| e.to_str()) == Some("js") {
                            let wrapper =
                                format!("#!/usr/bin/env node\nrequire('{}');", src.display());
                            fs::write(&dest, wrapper)?;
                            let mut perms = fs::metadata(&dest)?.permissions();
                            perms.set_mode(0o755);
                            fs::set_permissions(&dest, perms)?;
                        } else {
                            fs::copy(&src, &dest)?;
                        }
                    }
                }
            }
        }
    }

    println!("Note: {} package installed at {}", js_pm, install_path);
    Ok(Some(install_status))
}

pub fn build_electron(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let js_pm = detect_js_package_manager(temp);

    let mut install_cmd = Command::new(js_pm);
    install_cmd.arg("install").current_dir(temp);
    if !verbose {
        install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let install_status = install_cmd.status()?;

    if !install_status.success() {
        return Ok(Some(install_status));
    }

    let package_json = Path::new(temp).join("package.json");
    let build_scripts = ["build:app", "build:web", "build:electron", "build"];
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let scripts = json.get("scripts").and_then(|v| v.as_object());
            for script in &build_scripts {
                if scripts
                    .and_then(|s| s.get(*script))
                    .and_then(|v| v.as_str())
                    .is_some()
                {
                    let mut build_cmd = Command::new(js_pm);
                    build_cmd.arg("run").arg(script).current_dir(temp);
                    if !verbose {
                        build_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                    }
                    let _ = build_cmd.status();
                }
            }
        }
    }

    let main_entry = fs::read_to_string(&package_json)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|j| j.get("main").and_then(|v| v.as_str()).map(|s| s.to_string()));

    let electron_dir = Path::new(install_path).join("electron");
    if electron_dir.exists() {
        fs::remove_dir_all(&electron_dir)?;
    }
    fs::create_dir_all(electron_dir.parent().unwrap_or(Path::new(install_path)))?;
    fs::rename(temp, &electron_dir)?;

    if let Some(main) = main_entry {
        let src = electron_dir.join(&main);
        let dest = bin_dir.join(repo);
        let wrapper = format!(
            "#!/bin/sh\n# Edit flags below if electron has display issues (e.g. NVIDIA on Wayland)\nexec electron {} \"$@\"\n",
            src.display()
        );
        fs::write(&dest, wrapper)?;
        let mut perms = fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms)?;
    }

    println!("Note: electron package installed at {}", install_path);
    Ok(Some(install_status))
}

pub fn build_gradle(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut cmd = if Path::new(temp).join("gradlew").exists() {
        let mut c = Command::new("sh");
        c.arg(Path::new(temp).join("gradlew"));
        c
    } else {
        Command::new("gradle")
    };
    cmd.arg("build").arg("--no-daemon").current_dir(temp);
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;

    if status.success() {
        let build_libs = Path::new(temp).join("build/libs");
        if let Ok(entries) = fs::read_dir(&build_libs) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jar") {
                    let dest_jar = bin_dir.join(format!("{}.jar", repo));
                    fs::copy(&path, &dest_jar)?;

                    let wrapper = bin_dir.join(repo);
                    let script = format!(
                        "#!/bin/bash\nexec java -jar \"{}\" \"$@\"",
                        dest_jar.display()
                    );
                    fs::write(&wrapper, script)?;
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&wrapper)?.permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&wrapper, perms)?;
                    break;
                }
            }
        }
    }

    Ok(Some(status))
}

pub fn build_shell(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    let bin_dir = Path::new(install_path).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let find_script = |temp: &str, repo: &str| -> Option<std::path::PathBuf> {
        let base = Path::new(temp);

        let exact = base.join(repo);
        if exact.is_file() {
            return Some(exact);
        }

        let sh = base.join(format!("{}.sh", repo));
        if sh.is_file() {
            return Some(sh);
        }

        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|e| e.to_str()) == Some("sh")
                {
                    return Some(path);
                }
            }
        }

        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || path.extension().is_some() {
                    continue;
                }
                if let Ok(mut f) = std::fs::File::open(&path) {
                    let mut buf = [0u8; 64];
                    use std::io::Read;
                    if f.read_exact(&mut buf).is_ok() {
                        let head = String::from_utf8_lossy(&buf);
                        if head.starts_with("#!") && (head.contains("/sh")
                            || head.contains("/bash") || head.contains("/zsh")
                            || head.contains("/dash") || head.contains("/ksh")
                            || head.contains("/env bash") || head.contains("/env sh"))
                        {
                            return Some(path);
                        }
                    }
                }
            }
        }

        None
    };

    let script = match find_script(temp, repo) {
        Some(s) => s,
        None => {
            eprintln!("Could not find shell script in {}", temp);
            return Ok(None);
        }
    };

    if verbose {
        println!("Found script: {}", script.display());
    }

    let dest = bin_dir.join(repo);
    match fs::copy(&script, &dest) {
        Ok(_) => {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            use std::os::unix::process::ExitStatusExt;
            Ok(Some(std::process::ExitStatus::from_raw(0)))
        }
        Err(e) => {
            eprintln!("Failed to copy script: {}", e);
            Ok(None)
        }
    }
}

pub fn build_python(
    temp: &str,
    install_path: &str,
    repo: &str,
    verbose: bool,
) -> Result<Option<std::process::ExitStatus>, GitpkgError> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    let python_cmd = if is_installed("python3") {
        "python3"
    } else if is_installed("python") {
        "python"
    } else {
        eprintln!("Python not found on PATH; cannot build python package");
        return Ok(None);
    };

    let temp_path = Path::new(temp);

    let has_pyproject = temp_path.join("pyproject.toml").exists()
        || temp_path.join("setup.py").exists()
        || temp_path.join("setup.cfg").exists()
        || temp_path.join("Pipfile").exists()
        || temp_path.join("poetry.lock").exists();
    let has_requirements = temp_path.join("requirements.txt").exists();

    let bin_dir = Path::new(install_path).join("bin");
    if let Err(e) = fs::create_dir_all(&bin_dir) {
        eprintln!("Failed to create bin directory {}: {}", bin_dir.display(), e);
        return Ok(None);
    }

    let venv_dir = Path::new(install_path).join("venv");

    if venv_dir.exists() {
        let _ = fs::remove_dir_all(&venv_dir);
    }

    if let Err(e) = fs::create_dir_all(&venv_dir) {
        eprintln!("Failed to create venv directory {}: {}", venv_dir.display(), e);
        return Ok(None);
    }

    println!("Creating virtualenv at {}", venv_dir.display());
    let mut venv_cmd = Command::new(python_cmd);
    venv_cmd.arg("-m").arg("venv").arg(&venv_dir);
    if !verbose {
        venv_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let venv_status = venv_cmd.status()?;
    if !venv_status.success() {
        eprintln!("Failed to create virtualenv. On Debian/Ubuntu, try: apt install python3-venv");
        let _ = fs::remove_dir_all(&venv_dir);
        return Ok(Some(venv_status));
    }

    let venv_python = venv_dir.join("bin").join("python");
    if !venv_python.exists() {
        eprintln!(
            "Virtualenv created but python binary not found at {}",
            venv_python.display()
        );
        let _ = fs::remove_dir_all(&venv_dir);
        return Ok(None);
    }

    let mut pip_upgrade = Command::new(&venv_python);
    pip_upgrade
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("--no-cache-dir")
        .arg("--upgrade")
        .arg("pip")
        .arg("setuptools")
        .arg("wheel");
    if !verbose {
        pip_upgrade.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let _ = pip_upgrade.status();

    let req_file = temp_path.join("requirements.txt");
    if req_file.exists() {
        println!("Installing requirements in venv...");
        let mut req_cmd = Command::new(&venv_python);
        req_cmd
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--no-cache-dir")
            .arg("-r")
            .arg(&req_file);
        if !verbose {
            req_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let req_status = req_cmd.status()?;
        if !req_status.success() {
            eprintln!("Failed to install requirements.txt, cleaning up venv...");
            let _ = fs::remove_dir_all(&venv_dir);
            return Ok(Some(req_status));
        }
    }

    let install_status = if has_pyproject {
        let mut install_cmd = Command::new(&venv_python);
        install_cmd
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--no-cache-dir")
            .arg(".")
            .current_dir(temp);
        if !verbose {
            install_cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let status = install_cmd.status()?;
        if !status.success() {
            eprintln!("pip install failed for python package, cleaning up venv...");
            let _ = fs::remove_dir_all(&venv_dir);
            return Ok(Some(status));
        }
        Some(status)
    } else {
        None
    };

    let venv_bin = venv_dir.join("bin");
    let venv_python_names = ["python", "python3", "pip", "pip3", "wheel", "easy_install"];
    if let Ok(entries) = fs::read_dir(&venv_bin) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if venv_python_names.contains(&name) || name.ends_with(".pyc") {
                        continue;
                    }

                    let wrapper = bin_dir.join(name);
                    let script = format!(
                        "#!/bin/bash\n# Autogenerated wrapper for python package {}\nexport GITPKG_PACKAGE_ROOT=\"{}\"\nexec \"{}\" \"$@\"\n",
                        repo,
                        install_path,
                        path.display()
                    );
                    if let Err(e) = fs::write(&wrapper, script) {
                        eprintln!("Failed to write wrapper {}: {}", wrapper.display(), e);
                        continue;
                    }
                    if let Ok(meta) = fs::metadata(&wrapper) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        if let Err(e) = fs::set_permissions(&wrapper, perms) {
                            eprintln!(
                                "Failed to set permissions on {}: {}",
                                wrapper.display(),
                                e
                            );
                        }
                    }
                    println!("Installed python console script wrapper: {}", name);
                }
            }
        }
    }

    if !has_pyproject {
        if let Some((script_name, script_path)) = find_main_python_script(temp_path, repo) {
            let lib_dir = Path::new(install_path).join("lib").join(repo);
            if let Err(e) = fs::create_dir_all(&lib_dir) {
                eprintln!(
                    "Failed to create lib directory {}: {}",
                    lib_dir.display(),
                    e
                );
                return Ok(install_status);
            }

            let dest_script = lib_dir.join(&script_name);
            if let Err(e) = fs::copy(&script_path, &dest_script) {
                eprintln!(
                    "Failed to copy script {} to {}: {}",
                    script_path.display(),
                    dest_script.display(),
                    e
                );
                return Ok(install_status);
            }

            if let Ok(meta) = fs::metadata(&dest_script) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&dest_script, perms);
            }

            if let Ok(content) = fs::read_to_string(&dest_script) {
                if !content.starts_with("#!") {
                    let new_content = format!("#!/usr/bin/env python3\n{}", content);
                    if let Err(e) = fs::write(&dest_script, new_content) {
                        eprintln!(
                            "Failed to add shebang to {}: {}",
                            dest_script.display(),
                            e
                        );
                    }
                    if let Ok(meta) = fs::metadata(&dest_script) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = fs::set_permissions(&dest_script, perms);
                    }
                }
            }

            let exe_name = script_name.strip_suffix(".py").unwrap_or(&script_name);
            let symlink_path = bin_dir.join(exe_name);
            let _ = fs::remove_file(&symlink_path);
            if let Err(e) = std::os::unix::fs::symlink(&dest_script, &symlink_path) {
                eprintln!("Failed to create symlink: {}", e);
            } else {
                println!("Created symlink: {} -> {}", exe_name, dest_script.display());
            }
        } else if !has_requirements {
            eprintln!("No python script found (expected main.py, app.py, or <repo>.py)");
        }
    }

    Ok(install_status.or(Some(std::process::ExitStatus::from_raw(0))))
}

fn find_main_python_script(temp: &Path, repo: &str) -> Option<(String, PathBuf)> {
    let repo_name_path = temp.join(repo);
    if repo_name_path.is_file() && is_python_file(&repo_name_path) {
        return Some((repo.to_string(), repo_name_path));
    }

    let candidates: Vec<String> = vec![
        "main.py".to_string(),
        "app.py".to_string(),
        "cli.py".to_string(),
        "run.py".to_string(),
        "start.py".to_string(),
        "script.py".to_string(),
        "entrypoint.py".to_string(),
    ];

    for candidate in &candidates {
        let path = temp.join(candidate);
        if path.exists() && path.is_file() && is_python_file(&path) {
            return Some((candidate.clone(), path));
        }
    }

    if let Ok(entries) = fs::read_dir(temp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_python_file(&path) {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("script")
                    .to_string();
                if name == "setup.py" || name.starts_with("test_") || name.starts_with("_") {
                    continue;
                }
                return Some((name, path));
            }
        }
    }

    if let Ok(entries) = fs::read_dir(temp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name.starts_with(".") || dir_name == "__pycache__" || dir_name == "venv" {
                    continue;
                }
                if let Ok(subentries) = fs::read_dir(&path) {
                    for subentry in subentries.flatten() {
                        let sub_path = subentry.path();
                        if sub_path.is_file() && is_python_file(&sub_path) {
                            let name = sub_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("script")
                                .to_string();
                            return Some((name, sub_path));
                        }
                    }
                }
            }
        }
    }

    None
}
