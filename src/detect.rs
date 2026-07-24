use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn is_python_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if let Some(ext) = path.extension() {
        if ext == "py" {
            return true;
        }
    }
    let output = Command::new("file")
        .arg("--mime-type")
        .arg("-b")
        .arg(path)
        .output();
    if let Ok(o) = output {
        let mime = String::from_utf8_lossy(&o.stdout);
        return mime.contains("python");
    }
    false
}

pub fn detect_build_system(path: &str) -> Option<&'static str> {
    let base = Path::new(path);

    for (file, sys) in [
        ("Makefile", "make"),
        ("Justfile", "just"),
        ("justfile", "just"),
        ("CMakeLists.txt", "cmake"),
        ("meson.build", "meson"),
        ("mason.toml", "mason"),
        ("build.ninja", "ninja"),
        ("Cargo.toml", "cargo"),
        ("build.gradle", "gradle"),
        ("go.mod", "go"),
        ("pyproject.toml", "python"),
        ("setup.py", "python"),
        ("setup.cfg", "python"),
        ("Pipfile", "python"),
        ("poetry.lock", "python"),
        ("requirements.txt", "python"),
        ("Rakefile", "rake"),
    ] {
        if base.join(file).exists() {
            return Some(sys);
        }
    }

    if base.join("package.json").exists() {
        if detect_electron(path) {
            return Some("electron");
        }
        return Some(detect_js_package_manager(path));
    }

    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() && is_python_file(&entry_path) {
                let name = entry_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                if !name.starts_with(".") && name != "setup.py" {
                    return Some("python");
                }
            }
            if entry_path.is_dir() {
                let dir_name = entry_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                if dir_name.starts_with(".") || dir_name == "__pycache__" || dir_name == "venv" {
                    continue;
                }
                if let Ok(subentries) = fs::read_dir(&entry_path) {
                    for subentry in subentries.flatten() {
                        let sub_path = subentry.path();
                        if sub_path.is_file() && is_python_file(&sub_path) {
                            return Some("python");
                        }
                    }
                }
            }
        }
    }

    if let Ok(entries) = fs::read_dir(base) {
        let files: Vec<_> = entries.flatten().filter(|e| e.path().is_file()).collect();

        for entry in &files {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with(".") {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some("sh") {
                return Some("sh");
            }
        }

        for entry in &files {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with(".") || path.extension().is_some() {
                continue;
            }
            if let Ok(mut f) = std::fs::File::open(&path) {
                let mut buf = [0u8; 64];
                use std::io::Read;
                if f.read_exact(&mut buf).is_ok() {
                    let head = String::from_utf8_lossy(&buf);
                    if head.starts_with("#!") && (head.contains("/sh") || head.contains("/bash")
                        || head.contains("/zsh") || head.contains("/dash")
                        || head.contains("/ksh") || head.contains("/env bash")
                        || head.contains("/env sh"))
                    {
                        return Some("sh");
                    }
                }
            }
        }
    }

    None
}

pub fn detect_electron(path: &str) -> bool {
    let base = Path::new(path);
    let package_json = base.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let deps = json.get("dependencies").and_then(|v| v.as_object());
            let dev_deps = json.get("devDependencies").and_then(|v| v.as_object());
            if deps.and_then(|d| d.get("electron")).is_some()
                || dev_deps.and_then(|d| d.get("electron")).is_some()
            {
                return true;
            }
        }
    }
    false
}

pub fn detect_js_package_manager(path: &str) -> &'static str {
    let base = Path::new(path);

    let package_json = base.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(pm) = json.get("packageManager").and_then(|v| v.as_str()) {
                if pm.starts_with("pnpm") {
                    return "pnpm";
                } else if pm.starts_with("yarn") {
                    return "yarn";
                } else if pm.starts_with("npm") {
                    return "npm";
                }
            }
        }
    }

    if base.join("pnpm-lock.yaml").exists() {
        return "pnpm";
    }
    if base.join("yarn.lock").exists() {
        return "yarn";
    }

    "npm"
}

pub fn build_system_packages(build_system: &str, pm: &str) -> Option<&'static str> {
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

    for &sys in ["make", "cmake", "meson", "mason", "ninja"].iter() {
        map.insert(sys, make_map.clone());
    }

    let mut npm_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        npm_map.insert(p, "nodejs npm");
    }
    map.insert("npm", npm_map);

    let mut pnpm_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        pnpm_map.insert(p, "nodejs npm");
    }
    map.insert("pnpm", pnpm_map);

    let mut yarn_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        yarn_map.insert(p, "nodejs npm");
    }
    map.insert("yarn", yarn_map);

    let mut electron_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        electron_map.insert(p, "nodejs npm");
    }
    map.insert("electron", electron_map);

    let mut gradle_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        gradle_map.insert(p, "gradle");
    }
    map.insert("gradle", gradle_map);

    let mut python_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        python_map.insert(p, "python");
    }
    map.insert("python", python_map);

    let mut just_map = HashMap::new();
    just_map.insert("apt", "just");
    just_map.insert("dnf", "just");
    just_map.insert("yum", "just");
    just_map.insert("pacman", "just");
    just_map.insert("zypper", "just");
    just_map.insert("apk", "just");
    just_map.insert("nix-env", "just");
    map.insert("just", just_map);

    let mut rake_map = HashMap::new();
    for &p in ["apt", "dnf", "yum", "pacman", "zypper", "apk", "nix-env"].iter() {
        rake_map.insert(p, "ruby");
    }
    map.insert("rake", rake_map);

    map.get(build_system)?.get(pm).copied()
}
