use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::build::{
    build_cargo, build_cmake, build_electron, build_go, build_gradle, build_just,
    build_make, build_mason, build_meson, build_nodejs, build_ninja, build_python, build_rake,
    build_shell, find_installed_executable,
};
use crate::cli::{
    build_git_url_with, detect_package_manager, get_remote_url, install_system_package,
    is_installed, remove_system_package,
};
use crate::data::{
    create_data_symlinks, create_desktop_symlinks, install_data_files,
    refresh_desktop_database,
};
use crate::detect::{build_system_packages, detect_build_system};
use crate::error::GitpkgError;
use crate::git::{
    check_branch_exists, get_commit_hash, get_remote_commit_hash,
    run_git_clone_with_progress,
};
use crate::package::{
    find_matching_packages, find_package_by_key, home_dir, home_dir_or_err,
    install_root, parse_pkg, parse_pkg_with_supplier, prompt_package_selection, read_package_list,
    remove_from_package_list, temp_path, write_info as write_info_file,
};
use crate::util::{dir_size_bytes, format_mb};

pub fn install(
    package: &str,
    verbose: bool,
    supplier: Option<&str>,
    branch: Option<&str>,
    config: &crate::build::BuildConfig,
    submodules: bool,
    ssh: bool,
    system_wide: bool,
) -> Result<(), GitpkgError> {
    let (user, repo) = parse_pkg(package);
    let url = build_git_url_with(&user, &repo, supplier, ssh, None);
    let supplier_domain = supplier.unwrap_or("github.com");

    let path = temp_path(&user, &repo)?;
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path)?;
    }

    println!("Cloning {} from {} into {}", package, supplier_domain, path);
    if !run_git_clone_with_progress(&url, &path, verbose, branch, submodules) {
        let _ = fs::remove_dir_all(&path);
        return Err(GitpkgError::CloneFailed);
    }
    println!("Successfully cloned {}!", package);

    let bs = match detect_build_system(&path) {
        Some(s) => s,
        None => {
            let _ = fs::remove_dir_all(&path);
            println!("Could not detect build system");
            return Ok(());
        }
    };
    let pm = match detect_package_manager() {
        Some(p) => p,
        None => {
            let _ = fs::remove_dir_all(&path);
            println!("No package manager detected");
            return Ok(());
        }
    };

    let mut installed_deps: Vec<String> = Vec::new();

    let compiler = if bs == "python" {
        if is_installed("python3") || is_installed("python") {
            None
        } else {
            println!("Python not found, attempting to install via {}...", pm);
            if !install_system_package(pm, "python3") {
                let _ = fs::remove_dir_all(&path);
                eprintln!("Failed installing python");
                return Ok(());
            }
            installed_deps.push("python3".to_string());
            None
        }
    } else {
        Some(match build_system_packages(bs, pm) {
            Some(c) => c,
            None => {
                let _ = fs::remove_dir_all(&path);
                println!("No compiler mapping for {} on {}", bs, pm);
                return Ok(());
            }
        })
    };

    if let Some(comp) = compiler {
        let check_bin = if bs == "pnpm" || bs == "yarn" || bs == "electron" { "node" } else { bs };
        if !is_installed(check_bin) {
            println!("Installing {} for {} via {}...", comp, bs, pm);
            if !install_system_package(pm, comp) {
                let _ = fs::remove_dir_all(&path);
                eprintln!("Failed installing {}", comp);
                return Ok(());
            }
            installed_deps.push(comp.to_string());
        }
    }

    build(
        &user,
        &repo,
        verbose,
        Some(supplier_domain),
        branch,
        config,
        ssh,
        system_wide,
        &installed_deps,
        None,
    )?;
    Ok(())
}

pub fn build(
    user: &str,
    repo: &str,
    verbose: bool,
    supplier: Option<&str>,
    branch: Option<&str>,
    config: &crate::build::BuildConfig,
    _ssh: bool,
    system_wide: bool,
    installed_deps: &[String],
    remote_url: Option<&str>,
) -> Result<bool, GitpkgError> {
    let temp = temp_path(user, repo)?;
    let commit = get_commit_hash(&temp).unwrap_or_else(|| "unknown".to_string());
    let supplier_domain = supplier.unwrap_or("github.com");
    let install_path = install_root(user, repo, &commit, supplier_domain)?;
    let pkg_key = crate::package::get_package_key(user, repo, supplier_domain);
    fs::create_dir_all(&install_path)?;

    let bs = match detect_build_system(&temp) {
        Some(s) => s,
        None => {
            println!("No build system detected");
            return Ok(false);
        }
    };
    println!("Building {} with {}", repo, bs);

    let status = match bs {
        "cargo" => build_cargo(&temp, &install_path, verbose)?,
        "make" => build_make(&temp, &install_path, repo, verbose, config)?,
        "cmake" => build_cmake(&temp, &install_path, repo, verbose, config)?,
        "meson" => build_meson(&temp, &install_path, repo, verbose)?,
        "python" => build_python(&temp, &install_path, repo, verbose)?,
        "mason" => build_mason(&temp, &install_path, repo, verbose)?,
        "ninja" => build_ninja(&temp, &install_path, repo, verbose)?,
        "go" => build_go(&temp, &install_path, repo, verbose)?,
        "npm" | "pnpm" | "yarn" => build_nodejs(&temp, &install_path, repo, verbose, bs)?,
        "electron" => build_electron(&temp, &install_path, repo, verbose)?,
        "gradle" => build_gradle(&temp, &install_path, repo, verbose, config.java_home.as_deref())?,
        "sh" => build_shell(&temp, &install_path, repo, verbose)?,
        "just" => build_just(&temp, &install_path, repo, verbose, config)?,
        "rake" => build_rake(&temp, &install_path, repo, verbose)?,
        _ => {
            println!("Unsupported build system: {}", bs);
            return Ok(false);
        }
    };

    let status = match status {
        Some(s) => s,
        None => {
            println!("Build failed for {}", repo);
            return Ok(false);
        }
    };

    if status.success() {
        println!("Installed to {}", install_path);

        let src_dir = if bs == "electron" {
            Path::new(&install_path).join("electron")
        } else {
            Path::new(&temp).to_path_buf()
        };
        let data_files = install_data_files(&src_dir, Path::new(&install_path), repo);
        if !data_files.is_empty() {
            println!("Installed {} data file(s)", data_files.len());
        }

        let data_symlinks = create_data_symlinks(Path::new(&install_path), repo, false);

        if repo == "gitpkg" && (user == "Lalaggi" || user == "el1lovescomputers") {
            if let Some(home) = home_dir() {
                let completion_src = Path::new(&temp).join("gitpkg-completion.sh");
                if completion_src.exists() {
                    let dest_dir = home.join(".local/share/gitpkg");
                    let dest = dest_dir.join("gitpkg-completion.sh");
                    let _ = fs::create_dir_all(&dest_dir);
                    match fs::copy(&completion_src, &dest) {
                        Ok(_) => {
                            println!("Installed updated completion script to {}", dest.display());
                            let bashrc = home.join(".bashrc");
                            let source_line =
                                "source $HOME/.local/share/gitpkg/gitpkg-completion.sh";
                            if let Ok(content) = fs::read_to_string(&bashrc) {
                                if !content.contains(source_line) {
                                    let new_content = format!(
                                        "{}\n# gitpkg completion\n{}\n",
                                        content, source_line
                                    );
                                    let _ = fs::write(&bashrc, new_content);
                                }
                            }
                        }
                        Err(e) => eprintln!("Failed to install completion script: {}", e),
                    }
                }
            }
        }

        if !["npm", "pnpm", "yarn", "electron"].contains(&bs) {
            let _ = fs::remove_dir_all(&temp);
        }

        // Always tie the symlink to the actual detected binary, with the repo
        // name only as a last-resort fallback. This fixes installs where the
        // Cargo/package binary name differs from the repo name.
        let exe_path = match find_installed_executable(Path::new(&install_path), repo) {
            Some(p) => p,
            None => Path::new(&install_path).join("bin").join(repo),
        };
        let exe_name = exe_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(repo)
            .to_string();

        // Decide where the symlink goes. Default to the user dir
        // (~/.local/bin, no sudo). Only use /usr/bin when the user explicitly
        // passed --system AND sudo auth succeeds.
        let symlink_dir: Option<std::path::PathBuf> = if system_wide {
            if crate::cli::superuser_auth() {
                Some(Path::new("/usr/bin").to_path_buf())
            } else {
                eprintln!(
                    "Warning: --system requested but {} authentication failed.",
                    crate::cli::superuser().program()
                );
                eprintln!("Falling back to ~/.local/bin (no superuser).");
                home_dir().map(|h| h.join(".local/bin"))
            }
        } else {
            home_dir().map(|h| h.join(".local/bin"))
        };

        let (symlink_created, symlink_path) = match symlink_dir {
            Some(dir) => {
                let is_system = dir.starts_with("/usr");
                fs::create_dir_all(&dir)?;
                let target = dir.join(&exe_name);

                // Refuse to clobber an existing non-symlink.
                if target.exists() && !target.is_symlink() {
                    eprintln!(
                        "Refusing to overwrite existing non-symlink at {}. Remove it manually.",
                        target.display()
                    );
                    (false, target)
                } else {
                    let _ = fs::remove_file(&target);
                    let ok = if is_system {
                        let target_s = target.to_str().unwrap_or("");
                        let exe_s = exe_path.to_str().unwrap_or("");
                        let _ = crate::cli::run_as(&["rm", "-f", target_s]);
                        crate::cli::run_as(&["ln", "-s", exe_s, target_s])
                            .map(|s| s.success())
                            .unwrap_or(false)
                    } else {
                        std::os::unix::fs::symlink(&exe_path, &target).is_ok()
                    };

                    if ok {
                        println!("Created symlink: {} -> {}", target.display(), exe_path.display());
                        if !is_system {
                            println!("Note: Make sure ~/.local/bin is in your PATH");
                            println!("Add this to your ~/.bashrc or ~/.zshrc:");
                            println!("  export PATH=\"$HOME/.local/bin:$PATH\"");
                        }
                        (true, target)
                    } else {
                        eprintln!("Failed to create symlink at {}", target.display());
                        (false, target)
                    }
                }
            }
            None => {
                eprintln!("Error: HOME is not set; cannot create symlink.");
                (false, Path::new("").to_path_buf())
            }
        };

        if symlink_created && system_wide {
            println!("Copying icons to system location...");
            let _ = create_data_symlinks(Path::new(&install_path), repo, true);
        }

        let needs_wrapper = !data_files.is_empty();
        let wrapper_path = if needs_wrapper {
            let wp = Path::new(&install_path)
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

            fs::write(&wp, wrapper_content)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&wp)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&wp, perms)?;
            Some(wp)
        } else {
            None
        };

        // If a wrapper is needed, point the symlink at the wrapper instead.
        let final_exe_path = if let Some(ref wp) = wrapper_path {
            if symlink_created {
                let is_system = symlink_path.starts_with("/usr");
                let _ = fs::remove_file(&symlink_path);
                let ok = if is_system {
                    let link_s = symlink_path.to_str().unwrap_or("");
                    let wp_s = wp.to_str().unwrap_or("");
                    let _ = crate::cli::run_as(&["rm", "-f", link_s]);
                    crate::cli::run_as(&["ln", "-s", wp_s, link_s])
                        .map(|s| s.success())
                        .unwrap_or(false)
                } else {
                    std::os::unix::fs::symlink(wp, &symlink_path).is_ok()
                };
                if !ok {
                    eprintln!(
                        "Warning: failed to repoint symlink {} to wrapper.",
                        symlink_path.display()
                    );
                }
            }
            wp.clone()
        } else {
            exe_path.clone()
        };

        if !symlink_created {
            println!(
                "You can run the executable directly at: {}",
                final_exe_path.display()
            );
        }

        let final_symlink_path = symlink_path.to_string_lossy().into_owned();

        let mut desktop_path = None;
        let desktop_file_src = Path::new(&install_path)
            .join("share")
            .join("applications")
            .join(format!("{}.desktop", repo));
        if desktop_file_src.exists() {
            if let Some(home) = home_dir() {
                let desktop_file_dst = home
                    .join(".local/share/applications")
                    .join(format!("gitpkg.{}.{}.desktop", user, repo));
                fs::create_dir_all(
                    desktop_file_dst
                        .parent()
                        .ok_or_else(|| GitpkgError::Parse("Invalid desktop file path".into()))?,
                )?;
                let content = fs::read_to_string(&desktop_file_src)?;
                let new_content = content
                    .lines()
                    .map(|l| {
                        if l.starts_with("Exec=") {
                            format!("Exec={}", final_exe_path.to_str().unwrap_or(""))
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                fs::write(&desktop_file_dst, new_content)?;
                desktop_path = Some(desktop_file_dst.to_string_lossy().into_owned());
            }
        }

        let desktop_symlinks = create_desktop_symlinks(Path::new(&install_path), &pkg_key);

        // Capture the actual remote URL (preserves SSH) for future upgrades.
        let stored_remote = remote_url
            .map(|s| s.to_string())
            .or_else(|| get_remote_url(&temp));

        write_info_file(
            user,
            repo,
            &commit,
            bs,
            detect_package_manager().unwrap_or("unknown"),
            &install_path,
            &final_symlink_path,
            desktop_path.as_deref(),
            supplier_domain,
            !data_files.is_empty(),
            &data_symlinks,
            &desktop_symlinks,
            branch,
            config.make_target.as_deref(),
            config.build_flags.as_deref(),
            config.submodules,
            system_wide,
            installed_deps,
            stored_remote.as_deref(),
        )?;

        refresh_desktop_database();

        println!("Metadata written to info.gitpkg");
        Ok(true)
    } else {
        println!("Build failed for {}", repo);
        Ok(false)
    }
}

pub fn remove(package: &str, remove_deps: bool) -> Result<(), GitpkgError> {
    let (user, repo) = parse_pkg(package);

    let matches = find_matching_packages(&user, &repo);

    if matches.is_empty() {
        return Err(GitpkgError::PackageNotFound(format!("{}/{}", user, repo)));
    }

    let (pkg_key, supplier, info_path) = if matches.len() > 1 {
        match prompt_package_selection(&matches) {
            Some(idx) => matches[idx].clone(),
            None => {
                return Err(GitpkgError::Cancelled);
            }
        }
    } else {
        matches[0].clone()
    };

    println!("Removing {} from {}...", pkg_key, supplier);

    let info_content = fs::read_to_string(&info_path).map_err(|e| {
        GitpkgError::Parse(format!("Failed to read info file: {}", e))
    })?;

    let info: toml::Value = toml::from_str(&info_content).map_err(|e| {
        GitpkgError::Parse(format!("Failed to parse info file: {}", e))
    })?;

    if let Some(symlink_path) = info.get("symlink_path").and_then(|v| v.as_str()) {
        if Path::new(symlink_path).exists() {
            let removed = if fs::remove_file(symlink_path).is_ok() {
                true
            } else {
                // Root-owned symlink (e.g. /usr/bin): try the superuser
                // provider explicitly.
                crate::cli::run_as(&["rm", "-f", symlink_path])
                    .map(|s| s.success())
                    .unwrap_or(false)
            };
            if removed {
                println!("Removed symlink: {}", symlink_path);
            } else {
                eprintln!(
                    "WARNING: could not remove symlink {}. Remove it manually.",
                    symlink_path
                );
            }
        }
    }

    if remove_deps {
        if let Some(deps) = info.get("system_deps").and_then(|v| v.as_array()) {
            let pm = info
                .get("package_manager")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            for dep in deps {
                if let Some(dep_name) = dep.as_str() {
                    println!("Removing system dependency: {}", dep_name);
                    if !remove_system_package(pm, dep_name) {
                        eprintln!("WARNING: failed to remove system dependency {}", dep_name);
                    }
                }
            }
        }
    } else if let Some(deps) = info.get("system_deps").and_then(|v| v.as_array()) {
        let names: Vec<&str> = deps.iter().filter_map(|d| d.as_str()).collect();
        if !names.is_empty() {
            println!(
                "Left {} system dependencies installed: {}",
                names.len(),
                names.join(", ")
            );
            println!("Re-run with --remove-deps to remove them.");
        }
    }

    if let Some(desktop_path) = info.get("desktop_file").and_then(|v| v.as_str()) {
        if Path::new(desktop_path).exists() {
            match fs::remove_file(desktop_path) {
                Ok(_) => println!("Removed desktop file: {}", desktop_path),
                Err(e) => eprintln!("Failed to remove desktop file {}: {}", desktop_path, e),
            }
        }
    }

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

    let package_dir = home_dir_or_err()?
        .join(".local/share/gitpkg")
        .join(&pkg_key)
        .to_string_lossy()
        .into_owned();

    if Path::new(&package_dir).exists() {
        match fs::remove_dir_all(&package_dir) {
            Ok(_) => println!("Removed installation directory: {}", package_dir),
            Err(e) => eprintln!(
                "Failed to remove installation directory {}: {}",
                package_dir, e
            ),
        }
    }

    let temp = temp_path(&user, &repo)?;
    if Path::new(&temp).exists() {
        let _ = fs::remove_dir_all(&temp);
    }

    remove_from_package_list(&pkg_key)?;

    println!("Successfully removed {}", pkg_key);
    Ok(())
}

pub fn goto(package: &str, spawn_shell: bool) -> Result<(), GitpkgError> {
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    let exact_match = find_package_by_key(package);

    let (pkg_key, _supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        let matches = find_matching_packages(&user, &repo);
        if matches.is_empty() {
            return Err(GitpkgError::PackageNotFound(package.to_string()));
        }

        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    return Err(GitpkgError::Cancelled);
                }
            }
        } else {
            0
        };

        matches[selected].clone()
    };

    let info_content = fs::read_to_string(&info_path).map_err(|e| {
        GitpkgError::Parse(format!("Failed to read info file: {}", e))
    })?;

    let info: toml::Value = toml::from_str(&info_content).map_err(|e| {
        GitpkgError::Parse(format!("Failed to parse info file: {}", e))
    })?;

    let install_path = match info.get("install_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            return Err(GitpkgError::Parse(format!(
                "No install_path found for {}",
                pkg_key
            )));
        }
    };

    if spawn_shell {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        println!("Spawning shell {} at {}", shell, install_path);
        let status = Command::new(shell).current_dir(install_path).status()?;
        if !status.success() {
            eprintln!("Shell exited with non-zero status");
        }
    } else {
        println!("{}", install_path);
    }
    Ok(())
}

pub fn clean(package: &str) -> Result<(), GitpkgError> {
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    let exact_match = find_package_by_key(package);

    let (pkg_key, supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        let matches = find_matching_packages(&user, &repo);

        if matches.is_empty() {
            println!("Package {} is not installed, nothing to clean", package);
            return Ok(());
        }

        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    return Err(GitpkgError::Cancelled);
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

    let temp = temp_path(&user, &repo)?;
    if Path::new(&temp).exists() {
        match fs::remove_dir_all(&temp) {
            Ok(_) => println!("Removed temp directory: {}", temp),
            Err(e) => eprintln!("Failed to remove temp directory: {}", e),
        }
    }

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
        return Ok(());
    }

    let current_commit = current_commit.unwrap();
    println!("Current version: {}", current_commit);

    let package_dir = home_dir_or_err()?
        .join(".local/share/gitpkg")
        .join(pkg_key)
        .to_string_lossy()
        .into_owned();

    if let Ok(entries) = fs::read_dir(&package_dir) {
        let mut removed_count = 0;
        let mut freed_bytes: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name != current_commit && dir_name != "info.gitpkg" {
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
    Ok(())
}

pub fn clean_all() -> Result<(), GitpkgError> {
    println!("Cleaning all temp files and old versions...");

    let gitpkg_dir = home_dir_or_err()?
        .join(".local/share/gitpkg")
        .to_string_lossy()
        .into_owned();

    let temp_dir = Path::new(&gitpkg_dir).join("temp");
    if temp_dir.exists() {
        match fs::remove_dir_all(&temp_dir) {
            Ok(_) => {
                println!("Removed all temp files");
                let _ = fs::create_dir_all(&temp_dir);
            }
            Err(e) => eprintln!("Failed to remove temp directory: {}", e),
        }
    }

    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    println!("Cleaning old versions for {} package(s)...", packages.len());
    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        println!("\n--- Cleaning {} ---", package);
        clean(&package)?;
    }

    println!("\nCleanup complete!");
    Ok(())
}

pub fn list() -> Result<(), GitpkgError> {
    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    println!("Installed packages:");
    println!("{:-<60}", "");

    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        if let Some(info_path) = packages.get(&package) {
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
    Ok(())
}

pub fn versions(package: &str) -> Result<(), GitpkgError> {
    let (user, repo, supplier_hint) = parse_pkg_with_supplier(package);

    let exact_match = find_package_by_key(package);

    let (pkg_key, supplier, info_path) = if let Some(m) = exact_match {
        m
    } else {
        let matches = find_matching_packages(&user, &repo);
        if matches.is_empty() {
            println!("Package {} is not installed", package);
            return Ok(());
        }
        let selected = if let Some(ref sup) = supplier_hint {
            matches.iter().position(|(_, s, _)| s == sup).unwrap_or(0)
        } else if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => idx,
                None => {
                    return Err(GitpkgError::Cancelled);
                }
            }
        } else {
            0
        };
        matches[selected].clone()
    };

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

    let package_dir = home_dir_or_err()?
        .join(".local/share/gitpkg")
        .join(&pkg_key);

    println!("Versions for {} (supplier: {})", pkg_key, supplier);

    if let Ok(entries) = fs::read_dir(&package_dir) {
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

                let install_dt = fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|st| chrono::DateTime::<chrono::Utc>::from(st));

                rows.push((name, size, is_current, install_dt));
            }
        }

        if rows.is_empty() {
            println!("  (No versions found)");
            return Ok(());
        }

        rows.sort_by(|a, b| match (&a.3, &b.3) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });

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
    Ok(())
}

pub fn upgrade(package: &str, verbose: bool, supplier: Option<&str>) -> Result<(), GitpkgError> {
    let (user, repo, stored_supplier, info_path) = if package.contains('_') && package.contains('/')
    {
        match find_package_by_key(package) {
            Some((_pkg_key, sup, path)) => {
                let (u, r) = parse_pkg(package);
                (u, r, sup, path)
            }
            None => {
                return Err(GitpkgError::PackageNotFound(package.to_string()));
            }
        }
    } else {
        let (user, repo) = parse_pkg(package);

        let matches = find_matching_packages(&user, &repo);

        if !matches.is_empty() {
            let (_pkg_key, stored_supplier, info_path) = if matches.len() > 1 && supplier.is_none() {
                match prompt_package_selection(&matches) {
                    Some(idx) => matches[idx].clone(),
                    None => {
                        return Err(GitpkgError::Cancelled);
                    }
                }
            } else if matches.len() > 1 && supplier.is_some() {
                let supplier_str = supplier.unwrap();
                match matches.iter().find(|(_, s, _)| s == supplier_str) {
                    Some(m) => m.clone(),
                    None => {
                        return Err(GitpkgError::PackageNotFound(format!(
                            "{}/{} from {}",
                            user, repo, supplier_str
                        )));
                    }
                }
            } else {
                matches[0].clone()
            };
            (user, repo, stored_supplier, info_path)
        } else if supplier.is_none() && user == "Lalaggi" && repo == "gitpkg" {
            // Fallback: check if this is a GitHub package that was previously on Codeberg
            // (e.g. user ran upgrade with old binary that didn't have migration code)
            let old_matches = find_matching_packages("el1lovescomputers", "gitpkg");
            if old_matches.is_empty() {
                return Err(GitpkgError::PackageNotFound(format!("{}/{}", user, repo)));
            }
            println!("Found package under old Codeberg account, migrating...");
            let (_pkg_key, stored_supplier, info_path) = old_matches[0].clone();
            ("el1lovescomputers".to_string(), "gitpkg".to_string(), stored_supplier, info_path)
        } else {
            return Err(GitpkgError::PackageNotFound(format!("{}/{}", user, repo)));
        }
    };

    let info_content = fs::read_to_string(&info_path).map_err(|e| {
        GitpkgError::Parse(format!("Failed to read info file: {}", e))
    })?;

    let info: toml::Value = toml::from_str(&info_content).map_err(|e| {
        GitpkgError::Parse(format!("Failed to parse info file: {}", e))
    })?;

    let current_commit = match info.get("latest_commit").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return Err(GitpkgError::Parse(
                "No commit hash found in info file".into(),
            ));
        }
    };

    let stored_branch = info.get("branch").and_then(|v| v.as_str()).map(|s| s.to_string());

    let build_config = crate::build::BuildConfig::from_info(&info);

    let mut supplier_to_use = supplier.unwrap_or(&stored_supplier);

    let mut stored_remote = info
        .get("remote_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let stored_system_wide = info
        .get("system_wide")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Migrate gitpkg itself from Codeberg to GitHub
    let mut migrated = false;
    if user == "el1lovescomputers" && repo == "gitpkg" && supplier.is_none() {
        if stored_supplier == "codeberg.org" {
            println!("Migrating gitpkg source from Codeberg (el1lovescomputers) to GitHub (Lalaggi)...");
            supplier_to_use = "github.com";
            stored_remote = None;
            migrated = true;
        }
    }

    println!(
        "Checking for updates to {} from {}...",
        crate::package::get_package_key(&user, &repo, &stored_supplier),
        supplier_to_use
    );
    if let Some(ref b) = stored_branch {
        println!("Tracking branch: {}", b);
    }
    println!("Current commit: {}", current_commit);

    let url = build_git_url_with(
        &user,
        &repo,
        Some(supplier_to_use),
        false,
        stored_remote.as_deref(),
    );

    let latest_commit = match get_remote_commit_hash(&url, stored_branch.as_deref()) {
        Some(c) => c,
        None => {
            return Err(GitpkgError::Git(
                "Failed to get latest commit hash from remote".into(),
            ));
        }
    };

    println!("Latest commit:  {}", latest_commit);

    if current_commit == latest_commit {
        println!(
            "{} is already up to date!",
            crate::package::get_package_key(&user, &repo, &stored_supplier)
        );
        return Ok(());
    }

    println!("Update available! Cloning and building new version...");

    let path = temp_path(&user, &repo)?;
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path)?;
    }

    if !run_git_clone_with_progress(
        &url,
        &path,
        verbose,
        stored_branch.as_deref(),
        build_config.submodules,
    ) {
        return Err(GitpkgError::CloneFailed);
    }

    let stored_deps: Vec<String> = info
        .get("system_deps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let built = build(
        &user,
        &repo,
        verbose,
        Some(supplier_to_use),
        stored_branch.as_deref(),
        &build_config,
        false,
        stored_system_wide,
        &stored_deps,
        stored_remote.as_deref(),
    )?;

    if !built {
        return Err(GitpkgError::BuildFailed(crate::package::get_package_key(
            &user,
            &repo,
            &stored_supplier,
        )));
    }

    // Clean up old Codeberg package list entry after migration
    if migrated {
        crate::package::remove_old_supplier_entry(&user, &repo, &stored_supplier);
    }

    println!(
        "Successfully upgraded {} from {} to {}",
        crate::package::get_package_key(&user, &repo, &stored_supplier),
        &current_commit[..8.min(current_commit.len())],
        &latest_commit[..8.min(latest_commit.len())]
    );
    Ok(())
}

pub fn upgrade_all(verbose: bool) -> Result<(), GitpkgError> {
    let packages = read_package_list();

    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    println!("Found {} installed package(s)", packages.len());

    let mut keys: Vec<_> = packages.keys().cloned().collect();
    keys.sort();
    for package in keys {
        println!("\n--- Upgrading {} ---", package);
        upgrade(&package, verbose, None)?;
    }

    println!("\nAll packages checked for updates!");
    Ok(())
}

/// Migrate a single package from one supplier to another.
pub fn migrate(
    package: &str,
    destination_supplier: &str,
    new_username: Option<&str>,
    _verbose: bool,
    cfg: &crate::config::Config,
) -> Result<(), GitpkgError> {
    let (user, repo, stored_supplier, info_path) = if package.contains('_') && package.contains('/')
    {
        match find_package_by_key(package) {
            Some((_pkg_key, sup, path)) => {
                let (u, r) = parse_pkg(package);
                (u, r, sup, path)
            }
            None => {
                return Err(GitpkgError::PackageNotFound(package.to_string()));
            }
        }
    } else {
        let (user, repo) = parse_pkg(package);
        let matches = find_matching_packages(&user, &repo);
        if matches.is_empty() {
            return Err(GitpkgError::PackageNotFound(format!("{}/{}", user, repo)));
        }
        let (_pkg_key, stored_supplier, info_path) = if matches.len() > 1 {
            match prompt_package_selection(&matches) {
                Some(idx) => matches[idx].clone(),
                None => return Err(GitpkgError::Cancelled),
            }
        } else {
            matches[0].clone()
        };
        (user, repo, stored_supplier, info_path)
    };

    if stored_supplier == destination_supplier {
        println!(
            "{} is already from {}, nothing to migrate",
            crate::package::get_package_key(&user, &repo, &stored_supplier),
            destination_supplier
        );
        return Ok(());
    }

    // Resolve new username: CLI flag > config > prompt
    let resolved_new_username = if let Some(name) = new_username {
        name.to_string()
    } else if let Some(name) = cfg.forge_usernames.get(destination_supplier) {
        name.clone()
    } else if let Some(name) = cfg.forge_usernames.get(&stored_supplier) {
        name.clone()
    } else {
        print!(
            "Enter your username on {}: ",
            destination_supplier
        );
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            return Err(GitpkgError::Parse("Username cannot be empty".into()));
        }
        trimmed
    };

    // Read current info file
    let info_content = fs::read_to_string(&info_path).map_err(|e| {
        GitpkgError::Parse(format!("Failed to read info file: {}", e))
    })?;
    let mut info: toml::Value = toml::from_str(&info_content).map_err(|e| {
        GitpkgError::Parse(format!("Failed to parse info file: {}", e))
    })?;

    // Rewrite remote URL
    let old_remote = info
        .get("remote_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_remote = rewrite_remote_url(old_remote, destination_supplier, &resolved_new_username, &repo);

    // Update info fields
    let old_key = crate::package::get_package_key(&user, &repo, &stored_supplier);
    let new_pkg_key = crate::package::get_package_key(&resolved_new_username, &repo, destination_supplier);
    let old_install_path = info.get("install_path").and_then(|v| v.as_str()).map(|s| s.to_string());
    let old_symlink_path = info.get("symlink_path").and_then(|v| v.as_str()).map(|s| s.to_string());

    if let Some(t) = info.as_table_mut() {
        t.insert("supplier".to_string(), toml::Value::String(destination_supplier.to_string()));
        t.insert("user".to_string(), toml::Value::String(resolved_new_username.clone()));
        t.insert(
            "remote_url".to_string(),
            toml::Value::String(new_remote),
        );

        // Update install_path with new package key
        if let Some(old_install) = old_install_path {
            let new_install = old_install.replace(&old_key, &new_pkg_key);
            t.insert("install_path".to_string(), toml::Value::String(new_install));
        }

        // Update symlink_path with new install_path
        if let Some(old_symlink) = old_symlink_path {
            let new_symlink = old_symlink.replace(&old_key, &new_pkg_key);
            t.insert("symlink_path".to_string(), toml::Value::String(new_symlink));
        }
    }

    // Write updated info file
    let new_toml = toml::to_string_pretty(&info).map_err(|e| {
        GitpkgError::Parse(format!("Failed to serialize info: {}", e))
    })?;
    fs::write(&info_path, &new_toml)?;

    // Update package list: add new entry, remove old
    let new_pkg_key = crate::package::get_package_key(&resolved_new_username, &repo, destination_supplier);
    crate::package::add_to_package_list(&resolved_new_username, &repo, &info_path, destination_supplier)?;
    crate::package::remove_old_supplier_entry(&user, &repo, &stored_supplier);

    let old_key = crate::package::get_package_key(&user, &repo, &stored_supplier);
    println!("Migrated {} -> {}", old_key, new_pkg_key);
    println!(
        "Source: {} -> {}",
        stored_supplier, destination_supplier
    );

    Ok(())
}

/// Rewrite a remote URL to point to a different supplier/username.
fn rewrite_remote_url(old_url: &str, new_supplier: &str, new_user: &str, repo: &str) -> String {
    if old_url.starts_with("git@") {
        format!("git@{}:{}/{}.git", new_supplier, new_user, repo)
    } else {
        format!("https://{}/{}/{}.git", new_supplier, new_user, repo)
    }
}

/// Migrate all installed packages from one supplier to another.
/// Only migrates packages where the username matches the configured
/// forge_usernames for that supplier (i.e. only YOUR packages, not others').
pub fn migrate_all(
    destination_supplier: &str,
    new_username: Option<&str>,
    verbose: bool,
    cfg: &crate::config::Config,
) -> Result<(), GitpkgError> {
    let packages = read_package_list();
    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    // Find packages from non-destination suppliers where username matches config
    let mut to_migrate = Vec::new();
    for (pkg_key, info_path) in &packages {
        if let Ok(content) = fs::read_to_string(info_path) {
            if let Ok(info) = toml::from_str::<toml::Value>(&content) {
                let supplier = info
                    .get("supplier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("github.com");
                let user = info
                    .get("user")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if supplier == destination_supplier {
                    continue;
                }

                // Only migrate if username matches the configured username for this supplier
                if let Some(expected_user) = cfg.forge_usernames.get(supplier) {
                    if user == expected_user.as_str() {
                        to_migrate.push(pkg_key.clone());
                    }
                }
            }
        }
    }

    if to_migrate.is_empty() {
        println!(
            "No packages found to migrate to {} (configure [forge_usernames] in config.toml)",
            destination_supplier
        );
        return Ok(());
    }

    println!(
        "Found {} package(s) to migrate to {}",
        to_migrate.len(),
        destination_supplier
    );

    for pkg_key in &to_migrate {
        println!("\n--- Migrating {} ---", pkg_key);
        migrate(pkg_key, destination_supplier, new_username, verbose, cfg)?;
    }

    println!(
        "\nMigrated {} package(s) to {}",
        to_migrate.len(),
        destination_supplier
    );
    Ok(())
}

pub fn change_branch(
    package: &str,
    new_branch: &str,
    verbose: bool,
    cli_supplier: Option<&str>,
) -> Result<(), GitpkgError> {
    let (user, repo, stored_supplier, info_path) = if package.contains('_') && package.contains('/')
    {
        match find_package_by_key(package) {
            Some((_pkg_key, sup, path)) => {
                let (u, r) = parse_pkg(package);
                (u, r, sup, path)
            }
            None => {
                return Err(GitpkgError::PackageNotFound(package.to_string()));
            }
        }
    } else {
        let (u, r) = parse_pkg(package);
        let matches = find_matching_packages(&u, &r);
        if matches.is_empty() {
            return Err(GitpkgError::PackageNotFound(package.to_string()));
        }
        let (_pkg_key, sup, path) = if matches.len() > 1 && cli_supplier.is_none() {
            match prompt_package_selection(&matches) {
                Some(idx) => matches[idx].clone(),
                None => {
                    return Err(GitpkgError::Cancelled);
                }
            }
        } else if matches.len() > 1 && cli_supplier.is_some() {
            let sup_str = cli_supplier.unwrap();
            match matches.iter().find(|(_, s, _)| s == sup_str) {
                Some(m) => m.clone(),
                None => {
                    return Err(GitpkgError::PackageNotFound(format!(
                        "{}/{} from {}",
                        u, r, sup_str
                    )));
                }
            }
        } else {
            matches[0].clone()
        };
        (u, r, sup, path)
    };

    let supplier_to_use = cli_supplier.unwrap_or(&stored_supplier);
    let pkg_key = crate::package::get_package_key(&user, &repo, supplier_to_use);

    let stored_remote = fs::read_to_string(&info_path)
        .ok()
        .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
        .and_then(|info| info.get("remote_url").and_then(|v| v.as_str()).map(|s| s.to_string()));

    let url = build_git_url_with(
        &user,
        &repo,
        Some(supplier_to_use),
        false,
        stored_remote.as_deref(),
    );
    println!("Checking if '{}' exists on remote...", new_branch);
    if !check_branch_exists(&url, new_branch, verbose) {
        return Err(GitpkgError::Git(format!(
            "Branch or tag '{}' does not exist on remote",
            new_branch
        )));
    }
    println!("Ref '{}' exists!", new_branch);

    let (build_config, stored_system_wide, stored_deps) = fs::read_to_string(&info_path)
        .ok()
        .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
        .map(|info| {
            let cfg = crate::build::BuildConfig::from_info(&info);
            let sw = info
                .get("system_wide")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let deps: Vec<String> = info
                .get("system_deps")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|d| d.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            (cfg, sw, deps)
        })
        .unwrap_or_default();

    if let Ok(content) = fs::read_to_string(&info_path) {
        if let Ok(info) = toml::from_str::<toml::Value>(&content) {
            if let Some(sp) = info.get("symlink_path").and_then(|v| v.as_str()) {
                let p = Path::new(sp);
                if p.exists() {
                    let _ = fs::remove_file(p);
                    let _ = crate::cli::run_as(&["rm", "-f", sp]);
                }
            }
            if let Some(dp) = info.get("desktop_file").and_then(|v| v.as_str()) {
                let p = Path::new(dp);
                if p.exists() {
                    let _ = fs::remove_file(p);
                }
            }
            if let Some(arr) = info.get("data_symlinks").and_then(|v| v.as_array()) {
                for entry in arr {
                    if let Some(s) = entry.as_str() {
                        let p = Path::new(s);
                        if p.exists() {
                            let _ = fs::remove_file(p);
                        }
                    }
                }
            }
            if let Some(arr) = info.get("desktop_symlinks").and_then(|v| v.as_array()) {
                for entry in arr {
                    if let Some(s) = entry.as_str() {
                        let p = Path::new(s);
                        if p.exists() {
                            let _ = fs::remove_file(p);
                        }
                    }
                }
            }
        }
    }

    let path = temp_path(&user, &repo)?;
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path)?;
    }
    println!("Cloning '{}'...", new_branch);
    if !run_git_clone_with_progress(&url, &path, verbose, Some(new_branch), build_config.submodules) {
        return Err(GitpkgError::CloneFailed);
    }

    let built = build(
        &user,
        &repo,
        verbose,
        Some(supplier_to_use),
        Some(new_branch),
        &build_config,
        false,
        stored_system_wide,
        &stored_deps,
        stored_remote.as_deref(),
    )?;

    if !built {
        return Err(GitpkgError::BuildFailed(format!(
            "{} (branch '{}')",
            pkg_key, new_branch
        )));
    }

    println!("Successfully switched {} to branch '{}'", pkg_key, new_branch);
    Ok(())
}
