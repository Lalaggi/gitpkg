use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

pub fn get_commit_hash(path: &str) -> Option<String> {
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

pub fn get_remote_commit_hash(url: &str, branch: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("ls-remote");
    match branch {
        Some(b) => {
            cmd.arg(url).arg(format!("refs/heads/{}", b));
        }
        None => {
            cmd.arg(url).arg("HEAD");
        }
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().next().map(|s| s.to_string())
}

pub fn check_branch_exists(url: &str, branch: &str, verbose: bool) -> bool {
    let mut cmd = Command::new("git");
    cmd.args(["ls-remote", "--heads", "--tags", url, branch]);
    if verbose {
        let status = cmd.status();
        status.map(|s| s.success()).unwrap_or(false)
    } else {
        let output = cmd.output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                !stdout.trim().is_empty()
            }
            _ => false,
        }
    }
}

pub fn run_cmd(mut cmd: Command, verbose: bool) -> bool {
    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn run_git_clone_with_progress(
    url: &str,
    path: &str,
    verbose: bool,
    branch: Option<&str>,
    submodules: bool,
) -> bool {
    if verbose {
        let mut cmd = Command::new("git");
        cmd.arg("clone");
        if let Some(b) = branch {
            cmd.arg("-b").arg(b).arg("--single-branch");
        }
        cmd.arg(url).arg(path);
        if !run_cmd(cmd, true) {
            return false;
        }
        if submodules {
            let mut sub = Command::new("git");
            sub.arg("submodule").arg("update").arg("--init").arg("--recursive");
            sub.current_dir(path);
            return run_cmd(sub, true);
        }
        return true;
    }

    let mut clone_cmd = Command::new("git");
    clone_cmd.arg("clone");
    if let Some(b) = branch {
        clone_cmd.arg("-b").arg(b).arg("--single-branch");
    }
    clone_cmd.arg(url).arg(path).arg("--progress");

    let ok = (|| {
        let mut child = match clone_cmd
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

        loop {
            buf.clear();

            match reader.read_until(b'\r', &mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }

            let line = match String::from_utf8(buf.clone()) {
                Ok(s) => s,
                Err(_) => continue,
            };

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
    })();

    if ok && submodules {
        let mut sub = Command::new("git");
        sub.arg("submodule").arg("update").arg("--init").arg("--recursive");
        sub.current_dir(path);
        return run_cmd(sub, verbose);
    }

    ok
}
