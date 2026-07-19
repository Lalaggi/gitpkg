use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::{is_installed, run_as};
use crate::package::home_dir;

pub fn install_data_files(
    source_dir: &Path,
    install_path: &Path,
    repo: &str,
) -> Vec<(PathBuf, PathBuf)> {
    let mut installed = Vec::new();

    let data_dirs = [
        source_dir.to_path_buf(),
        source_dir.join("data"),
        source_dir.join("resources"),
        source_dir.join("share"),
        source_dir.join(repo),
    ];

    for data_dir in &data_dirs {
        if !data_dir.exists() {
            continue;
        }

        for entry in fs::read_dir(data_dir).ok().into_iter().flatten() {
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
                match fs::copy(&path, &dest) {
                    Ok(_) => {
                        println!("Installed resource: {}", dest.display());
                        installed.push((path.clone(), dest));
                    }
                    Err(e) => eprintln!("Failed to copy resource {}: {}", filename, e),
                }
            }

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

            if path.is_dir() && filename == "icons" {
                let dest_dir = install_path.join("share/icons");
                match copy_dir_all(&path, &dest_dir) {
                    Ok(_) => println!("Installed icons to: {}", dest_dir.display()),
                    Err(e) => eprintln!("Failed to copy icons: {}", e),
                }
            }
        }
    }

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

pub fn create_data_symlinks(install_path: &Path, repo: &str, system_wide: bool) -> Vec<PathBuf> {
    use std::os::unix::fs as unix_fs;

    let mut created = Vec::new();

    let home = match home_dir() {
        Some(h) => h,
        None => return created,
    };

    let app_share_dir = install_path.join("share").join(repo);
    if app_share_dir.exists() {
        let local_share_app = Path::new(&home).join(".local/share").join(repo);

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

    fn copy_icon_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
        if let Ok(entries) = fs::read_dir(src) {
            for entry in entries.flatten() {
                let sub_src = entry.path();
                let sub_name = sub_src.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let sub_dest = dest.join(sub_name);

                if sub_src.is_dir() {
                    fs::create_dir_all(&sub_dest)?;
                    copy_icon_dir(&sub_src, &sub_dest)?;
                } else if sub_src.is_file() {
                    fs::copy(&sub_src, &sub_dest)?;
                }
            }
        }
        Ok(())
    }

    let app_icons_dir = install_path.join("share/icons");
    if app_icons_dir.exists() {
        let user_icons_dir = Path::new(&home).join(".local/share/icons/hicolor");
        let is_system = system_wide;

        if let Err(e) = fs::create_dir_all(&user_icons_dir) {
            eprintln!("Failed to create user icons directory: {}", e);
        } else {
            if let Ok(entries) = fs::read_dir(&app_icons_dir) {
                for entry in entries.flatten() {
                    let src = entry.path();
                    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");

                    if name == "meson.build" || name == "CMakeLists.txt" {
                        continue;
                    }

                    if src.is_dir() {
                        if let Ok(subentries) = fs::read_dir(&src) {
                            for subentry in subentries.flatten() {
                                let sub_src = subentry.path();
                                let sub_name =
                                    sub_src.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                let dest = user_icons_dir.join(sub_name);

                                if sub_src.is_dir() {
                                    if let Err(e) = copy_icon_dir(&sub_src, &dest) {
                                        eprintln!(
                                            "Failed to copy icon dir {}: {}",
                                            dest.display(),
                                            e
                                        );
                                    } else {
                                        if is_system {
                                            println!(
                                                "Installed system icon dir: {}",
                                                dest.display()
                                            );
                                        } else {
                                            println!("Installed icon dir: {}", dest.display());
                                        }
                                        created.push(dest);
                                    }
                                } else if sub_src.is_file() {
                                    if let Err(e) = fs::copy(&sub_src, &dest) {
                                        eprintln!("Failed to copy icon {}: {}", dest.display(), e);
                                    } else {
                                        if is_system {
                                            println!("Installed system icon: {}", dest.display());
                                        } else {
                                            println!("Installed icon: {}", dest.display());
                                        }
                                        created.push(dest);
                                    }
                                }
                            }
                        }
                    } else if src.is_file() {
                        let dest = user_icons_dir.join(name);
                        if let Err(e) = fs::copy(&src, &dest) {
                            eprintln!("Failed to copy icon {}: {}", dest.display(), e);
                        } else {
                            if is_system {
                                println!("Installed system icon: {}", dest.display());
                            } else {
                                println!("Installed icon: {}", dest.display());
                            }
                            created.push(dest);
                        }
                    }
                }
            }

            if is_installed("gtk-update-icon-cache") {
                let _ = Command::new("gtk-update-icon-cache")
                    .arg("-f")
                    .arg("-t")
                    .arg(&user_icons_dir)
                    .status();
                if is_system {
                    println!("Updated system icon cache for hicolor");
                } else {
                    println!("Updated icon cache for hicolor");
                }
            }
        }

        if system_wide {
            let system_icons_dir = Path::new("/usr/share/icons/hicolor");

            if let Ok(entries) = fs::read_dir(&app_icons_dir) {
                for entry in entries.flatten() {
                    let src = entry.path();
                    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");

                    if name == "meson.build" || name == "CMakeLists.txt" {
                        continue;
                    }

                    if src.is_dir() {
                        let dest_subdir = system_icons_dir.join(name);
                        let dest_subdir_s = dest_subdir.to_str().unwrap_or("");
                        if !run_as(&["mkdir", "-p", dest_subdir_s])
                            .map(|s| s.success())
                            .unwrap_or(false)
                        {
                            eprintln!(
                                "Failed to create system icon subdir: {}",
                                dest_subdir.display()
                            );
                            continue;
                        }
                        if let Ok(subentries) = fs::read_dir(&src) {
                            for subentry in subentries.flatten() {
                                let sub_src = subentry.path();
                                let sub_name =
                                    sub_src.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                let sub_dest = dest_subdir.join(sub_name);
                                let sub_src_s = sub_src.to_str().unwrap_or("");
                                let sub_dest_s = sub_dest.to_str().unwrap_or("");
                                if run_as(&["cp", "-r", sub_src_s, sub_dest_s])
                                    .map(|s| s.success())
                                    .unwrap_or(false)
                                {
                                    println!("Installed system icon: {}", sub_dest.display());
                                } else {
                                    eprintln!(
                                        "Failed to copy system icon: {}",
                                        sub_dest.display()
                                    );
                                }
                            }
                        }
                    } else if src.is_file() {
                        let dest = system_icons_dir.join(name);
                        let src_s = src.to_str().unwrap_or("");
                        let dest_s = dest.to_str().unwrap_or("");
                        if run_as(&["cp", src_s, dest_s])
                            .map(|s| s.success())
                            .unwrap_or(false)
                        {
                            println!("Installed system icon: {}", dest.display());
                        }
                    }
                }
            }

            if is_installed("gtk-update-icon-cache") {
                let cache_dir = system_icons_dir.to_str().unwrap_or("");
                let _ = run_as(&[
                    "gtk-update-icon-cache",
                    "-f",
                    "--ignore-theme-index",
                    cache_dir,
                ]);
                println!("Updated system icon cache at /usr/share/icons/hicolor");
            }
        }
    }

    created
}

pub fn create_desktop_symlinks(install_path: &Path, pkg_key: &str) -> Vec<PathBuf> {
    use std::os::unix::fs as unix_fs;

    let mut created = Vec::new();

    let home = match home_dir() {
        Some(h) => h,
        None => return created,
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

pub fn refresh_desktop_database() {
    if !is_installed("update-desktop-database") {
        return;
    }

    let home = match home_dir() {
        Some(h) => h,
        None => return,
    };

    let gitpkg_apps = Path::new(&home)
        .join(".local/share/applications")
        .join("gitpkg");
    if gitpkg_apps.exists() {
        if let Ok(entries) = fs::read_dir(&gitpkg_apps) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_symlink() && path.read_link().map_or(true, |t| !t.exists()) {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    let apps_dir = Path::new(&home).join(".local/share/applications");
    let _ = Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();
}

pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
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
