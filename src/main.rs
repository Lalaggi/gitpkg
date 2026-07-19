mod build;
mod cli;
mod commands;
mod config;
mod data;
mod detect;
mod git;
mod package;
mod util;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: gitpkg <install|remove|clean|list|upgrade|update|versions|version|goto|change-branch|help> [args] [-v] [--supplier|--provider|--host <domain>] [--branch <branch>]"
        );
        std::process::exit(1);
    }

    // `config --init` writes a default config template and exits.
    if args.len() >= 2 && args[1] == "config" {
        if args.len() >= 3 && args[2] == "--init" {
            if let Err(e) = config::write_default() {
                eprintln!("Failed to write config: {}", e);
                std::process::exit(1);
            }
        } else {
            println!("Usage: gitpkg config --init   (write a default ~/.config/gitpkg/config.toml)");
        }
        return;
    }

    let cfg = config::Config::load();

    // CLI flags override config-file defaults.
    let verbose = args.contains(&"-v".to_string()) || cfg.verbose;

    let system_wide = args.contains(&"--system".to_string()) || cfg.system;
    let ssh = args.contains(&"--ssh".to_string()) || cfg.ssh;
    let remove_deps = args.contains(&"--remove-deps".to_string()) || cfg.remove_deps;

    // Resolve the superuser provider from a CLI flag (if present) or the
    // config file, then store it process-wide for privileged operations.
    let superuser_arg = args
        .iter()
        .position(|a| a == "--superuser")
        .and_then(|pos| args.get(pos + 1).cloned());
    let provider = superuser_arg.unwrap_or_else(|| cfg.superuser.clone());
    cli::set_superuser(&provider);

    let mut positional: Vec<String> = Vec::new();
    let mut skip_next = false;
    let mut flags: Option<String> = None;
    let mut target: Option<String> = None;
    for (_i, arg) in args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "-v"
            || arg == "--supplier"
            || arg == "--provider"
            || arg == "--host"
            || arg == "--branch"
            || arg == "--shell"
            || arg == "-s"
            || arg == "--flags"
            || arg == "--target"
            || arg == "--submodules"
            || arg == "--system"
            || arg == "--ssh"
            || arg == "--remove-deps"
            || arg == "--superuser"
        {
            if *arg == "--supplier"
                || *arg == "--provider"
                || *arg == "--host"
                || *arg == "--branch"
                || *arg == "--flags"
                || *arg == "--target"
                || *arg == "--superuser"
            {
                skip_next = true;
            }
            continue;
        }
        positional.push(arg.clone());
    }

    let mut submodules = cfg.submodules;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--flags" => {
                if i + 1 < args.len() {
                    flags = Some(args[i + 1].clone());
                }
            }
            "--target" => {
                if i + 1 < args.len() {
                    target = Some(args[i + 1].clone());
                }
            }
            "--submodules" => {
                submodules = true;
            }
            _ => {}
        }
        i += 1;
    }

    let build_config = crate::build::BuildConfig {
        make_target: target,
        build_flags: flags,
        submodules,
    };
    if positional.is_empty() {
        eprintln!(
            "Usage: gitpkg <install|remove|clean|list|upgrade|update|versions|version|goto|change-branch|help> [args] [-v] [--supplier|--provider|--host <domain>] [--branch <branch>]"
        );
        std::process::exit(1);
    }
    let command = &positional[0];

    let supplier = if let Some(pos) =
        args.iter().position(|arg| arg == "--supplier" || arg == "--provider" || arg == "--host")
    {
        if pos + 1 < args.len() {
            Some(cli::resolve_supplier_shortname(&args[pos + 1]))
        } else {
            eprintln!("Error: --supplier flag requires a domain argument");
            eprintln!("Example: --supplier gitlab.com");
            std::process::exit(1);
        }
    } else {
        None
    };

    let branch = if let Some(pos) = args.iter().position(|arg| arg == "--branch") {
        if pos + 1 < args.len() {
            let b = args[pos + 1].clone();
            if b.is_empty() {
                eprintln!("Error: --branch requires a non-empty branch name");
                std::process::exit(1);
            }
            Some(b)
        } else {
            eprintln!("Error: --branch flag requires a branch name argument");
            eprintln!("Example: --branch stable");
            std::process::exit(1);
        }
    } else {
        None
    };

    match command.as_str() {
        "install" => {
            if positional.len() < 2 {
                eprintln!("Usage: gitpkg install <user>/<repo> [--supplier <domain>] [--branch <branch>] [--target <make-target>] [--flags \"<extra build args>\"] [--submodules]");
                return;
            }
            commands::install(
                &positional[1],
                verbose,
                supplier.as_deref(),
                branch.as_deref(),
                &build_config,
                submodules,
                ssh,
                system_wide,
            );
        }
        "remove" => {
            if positional.len() < 2 {
                eprintln!("Usage: gitpkg remove <user>/<repo>");
                return;
            }
            let target = cli::resolve_self_alias(&positional[1]);
            commands::remove(&target, remove_deps);
        }
        "goto" => {
            if positional.len() < 2 {
                eprintln!("Usage: gitpkg goto <user>/<repo> [--shell|-s]");
                return;
            }
            let spawn_shell =
                args.contains(&"--shell".to_string()) || args.contains(&"-s".to_string());
            let target = cli::resolve_self_alias(&positional[1]);
            commands::goto(&target, spawn_shell);
        }
        "clean" => {
            if positional.len() >= 2 && &positional[1] == "all" {
                commands::clean_all();
            } else if positional.len() >= 2 {
                let target = cli::resolve_self_alias(&positional[1]);
                commands::clean(&target);
            } else {
                commands::clean_all();
            }
        }
        "versions" => {
            if positional.len() < 2 {
                eprintln!("Usage: gitpkg versions <user>/<repo>");
                return;
            }
            let target = cli::resolve_self_alias(&positional[1]);
            commands::versions(&target);
        }
        "version" => {
            eprintln!("Warning: 'version' is an alias for 'versions'. It is recommended to use 'versions'.");
            if positional.len() < 2 {
                eprintln!("Usage: gitpkg version <user>/<repo>");
                return;
            }
            let target = cli::resolve_self_alias(&positional[1]);
            commands::versions(&target);
        }
        "change-branch" => {
            if positional.len() < 3 {
                eprintln!("Usage: gitpkg change-branch <user>/<repo> <branch-name>");
                return;
            }
            let target = cli::resolve_self_alias(&positional[1]);
            commands::change_branch(&target, &positional[2], verbose, supplier.as_deref());
        }
        "list" => commands::list(),
        "upgrade" => {
            if positional.len() < 2 || &positional[1] == "all" {
                commands::upgrade_all(verbose);
            } else {
                let target = if positional[1] == "self" {
                    "el1lovescomputers/gitpkg".to_string()
                } else {
                    positional[1].clone()
                };
                commands::upgrade(&target, verbose, supplier.as_deref());
            }
        }
        "help" | "-h" | "--help" => {
            println!("gitpkg — minimal git-based package manager");
            println!();
            println!("Usage: gitpkg <command> [args] [-v] [--supplier|--provider|--host <domain>] [--branch <branch>] [--target <make-target>] [--flags \"<build args>\"]");
            println!();
            println!("Shortnames for --supplier/--provider/--host:");
            println!("  gh, github          github.com");
            println!("  gl, gitlab          gitlab.com");
            println!("  cb, codeberg        codeberg.org");
            println!("  glg, gnome, ...     gitlab.gnome.org");
            println!();
            println!("Commands:");
            println!("  install <user>/<repo>       Install a package (--branch to clone specific branch)");
            println!("                                --target <t>  build a specific make target (e.g. build-i686)");
            println!("                                --flags \"<a>\" extra args passed to make/cmake (e.g. \"-j4\")");
            println!("                                --submodules  init+update git submodules after clone");
            println!("                                --ssh         clone via git@<host>:<user>/<repo>.git");
            println!("                                --system      install symlink to /usr/bin (needs superuser)");
            println!("                                --superuser <p>  sudo|pkexec|doas|auto (default: config/auto)");
            println!("  remove <user>/<repo>        Remove a package");
            println!("                                --remove-deps  also remove system packages gitpkg installed");
            println!("  clean <user>/<repo>|all     Remove old versions or all");
            println!("  list                        List installed packages");
            println!("  upgrade [<pkg>|all]         Upgrade package or all (defaults to all)");
            println!("  update [<pkg>|all]          Alias for upgrade");
            println!("  change-branch <pkg> <br>    Switch installed package to a different branch");
            println!("  versions <user>/<repo>      List installed versions for a package");
            println!("  version <user>/<repo>       Alias for versions");
            println!("  goto <user>/<repo>          Print path to installed package (or spawn shell with -s)");
            println!("  config --init              Write a default ~/.config/gitpkg/config.toml");
            println!("  help                        Show this help");
            println!();
            println!("Defaults for --system, --ssh, --remove-deps, -v and --submodules");
            println!("can be set in ~/.config/gitpkg/config.toml (see `gitpkg config --init`).");
            println!("Explicit CLI flags always override the config file.");
            return;
        }
        "update" => {
            if positional.len() < 2 || &positional[1] == "all" {
                commands::upgrade_all(verbose);
            } else {
                let target = if positional[1] == "self" {
                    "el1lovescomputers/gitpkg".to_string()
                } else {
                    positional[1].clone()
                };
                commands::upgrade(&target, verbose, supplier.as_deref());
            }
        }
        _ => eprintln!("Unknown command: {}", command),
    }
}
