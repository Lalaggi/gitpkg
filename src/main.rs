mod build;
mod cli;
mod commands;
mod config;
mod data;
mod detect;
mod error;
mod git;
mod package;
mod util;

use clap::{Parser, Subcommand};

use error::GitpkgError;

#[derive(Parser)]
#[command(
    name = "gitpkg",
    about = "Minimal git-based package manager",
    after_help = "Shortnames for --supplier/--provider/--host:\n  \
        gh, github          github.com\n  \
        gl, gitlab          gitlab.com\n  \
        cb, codeberg        codeberg.org\n  \
        glg, gnome, ...     gitlab.gnome.org\n\n\
        Defaults for --system, --ssh, --remove-deps, -v and --submodules\n\
        can be set in ~/.config/gitpkg/config.toml (see `gitpkg config --init`).\n\
        Explicit CLI flags always override the config file."
)]
struct Cli {
    /// Print build/clone output
    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,

    /// Install symlink to /usr/bin (needs superuser)
    #[arg(long = "system", global = true)]
    system: bool,

    /// Clone via git@<host>:<user>/<repo>.git
    #[arg(long = "ssh", global = true)]
    ssh: bool,

    /// Also remove system packages gitpkg installed
    #[arg(long = "remove-deps", global = true)]
    remove_deps: bool,

    /// Superuser provider: sudo, pkexec, doas, or auto
    #[arg(long = "superuser", global = true)]
    superuser: Option<String>,

    /// Git hosting supplier (or shortname: gh, gl, cb, etc.)
    #[arg(long = "supplier", alias = "provider", alias = "host", global = true)]
    supplier: Option<String>,

    /// JAVA_HOME used for gradle builds (e.g. /usr/lib/jvm/java-21-openjdk)
    #[arg(long = "java-home", global = true)]
    java_home: Option<String>,

    /// Clone a specific branch
    #[arg(long = "branch", global = true)]
    branch: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install a package
    Install {
        /// Repository in user/repo format
        repo: String,

        /// Build a specific make target (e.g. build-i686)
        #[arg(long = "target")]
        target: Option<String>,

        /// Extra args passed to make/cmake (e.g. "-j4")
        #[arg(long = "flags")]
        flags: Option<String>,

        /// Init and update git submodules after clone
        #[arg(long = "submodules")]
        submodules: bool,
    },

    /// Remove a package
    Remove {
        /// Repository in user/repo format
        repo: String,
    },

    /// Remove old versions or all
    Clean {
        /// Package name or "all"
        target: Option<String>,
    },

    /// List installed packages
    List,

    /// Upgrade package or all (defaults to all)
    Upgrade {
        /// Package name, "all", or "self"
        target: Option<String>,
    },

    /// Alias for upgrade
    Update {
        /// Package name, "all", or "self"
        target: Option<String>,
    },

    /// List installed versions for a package
    Versions {
        /// Repository in user/repo format
        repo: String,
    },

    /// Alias for versions
    Version {
        /// Repository in user/repo format
        repo: String,
    },

    /// Print path to installed package (or spawn shell with -s)
    Goto {
        /// Repository in user/repo format
        repo: String,

        /// Spawn a shell in the package directory
        #[arg(short = 's', long = "shell")]
        shell: bool,
    },

    /// Switch installed package to a different branch
    ChangeBranch {
        /// Repository in user/repo format
        repo: String,

        /// Branch name to switch to
        branch: String,
    },

    /// Write a default ~/.config/gitpkg/config.toml
    Config {
        /// Initialize config file
        #[arg(long = "init")]
        init: bool,
    },

    /// Migrate a package from one supplier to another
    Migrate {
        /// Package name, "all", or "self"
        target: Option<String>,

        /// Destination supplier (e.g. github, gh)
        #[arg(long = "to")]
        destination: String,

        /// New username on the destination supplier (overrides config)
        #[arg(long = "new-username")]
        new_username: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), GitpkgError> {
    let args: Vec<String> = std::env::args().collect();

    // `config --init` writes a default config template and exits.
    // Handle before full parsing so it works even without a valid config.
    if args.len() >= 2 && args[1] == "config" {
        if args.len() >= 3 && args[2] == "--init" {
            config::write_default()?;
        } else {
            println!("Usage: gitpkg config --init   (write a default ~/.config/gitpkg/config.toml)");
        }
        return Ok(());
    }

    let cli = Cli::parse();

    let cfg = config::Config::load()?;

    // CLI flags override config-file defaults.
    let verbose = cli.verbose || cfg.verbose;
    let system_wide = cli.system || cfg.system;
    let ssh = cli.ssh || cfg.ssh;
    let remove_deps = cli.remove_deps || cfg.remove_deps;
    let submodules_from_cli = cli
        .command
        .as_ref()
        .and_then(|c| match c {
            Command::Install { submodules, .. } => Some(*submodules),
            _ => None,
        })
        .unwrap_or(false);
    let submodules = submodules_from_cli || cfg.submodules;

    let provider = cli.superuser.unwrap_or_else(|| cfg.superuser.clone());
    cli::set_superuser(&provider);

    let supplier = cli.supplier.as_deref().map(cli::resolve_supplier_shortname);

    let java_home = cli.java_home.clone().or_else(|| cfg.java_home.clone());

    let branch = match &cli.branch {
        Some(b) if b.is_empty() => {
            return Err(GitpkgError::Parse(
                "--branch requires a non-empty branch name".into(),
            ));
        }
        Some(b) => Some(b.clone()),
        None => None,
    };

    let build_config = match &cli.command {
        Some(Command::Install {
            target, flags, ..
        }) => crate::build::BuildConfig {
            make_target: target.clone(),
            build_flags: flags.clone(),
            submodules,
            java_home: java_home.clone(),
        },
        _ => crate::build::BuildConfig {
            submodules,
            java_home: java_home.clone(),
            ..Default::default()
        },
    };

    match cli.command {
        Some(Command::Install { repo, .. }) => {
            commands::install(
                &repo,
                verbose,
                supplier.as_deref(),
                branch.as_deref(),
                &build_config,
                submodules,
                ssh,
                system_wide,
            )?;
        }
        Some(Command::Remove { repo }) => {
            let target = cli::resolve_self_alias(&repo);
            commands::remove(&target, remove_deps)?;
        }
        Some(Command::Goto { repo, shell }) => {
            let target = cli::resolve_self_alias(&repo);
            commands::goto(&target, shell)?;
        }
        Some(Command::Clean { target }) => match target.as_deref() {
            Some("all") | None => commands::clean_all()?,
            Some(t) => {
                let resolved = cli::resolve_self_alias(t);
                commands::clean(&resolved)?;
            }
        },
        Some(Command::Versions { repo }) => {
            let target = cli::resolve_self_alias(&repo);
            commands::versions(&target)?;
        }
        Some(Command::Version { repo }) => {
            eprintln!(
                "Warning: 'version' is an alias for 'versions'. It is recommended to use 'versions'."
            );
            let target = cli::resolve_self_alias(&repo);
            commands::versions(&target)?;
        }
        Some(Command::ChangeBranch { repo, branch }) => {
            let target = cli::resolve_self_alias(&repo);
            commands::change_branch(&target, &branch, verbose, supplier.as_deref())?;
        }
        Some(Command::List) => commands::list()?,
        Some(Command::Upgrade { target }) => match target.as_deref() {
            Some("all") | None => commands::upgrade_all(verbose)?,
            Some("self") => {
                commands::upgrade("Lalaggi/gitpkg", verbose, supplier.as_deref())?;
            }
            Some(t) => {
                let resolved = cli::resolve_self_alias(t);
                commands::upgrade(&resolved, verbose, supplier.as_deref())?;
            }
        },
        Some(Command::Update { target }) => match target.as_deref() {
            Some("all") | None => commands::upgrade_all(verbose)?,
            Some("self") => {
                commands::upgrade("Lalaggi/gitpkg", verbose, supplier.as_deref())?;
            }
            Some(t) => {
                let resolved = cli::resolve_self_alias(t);
                commands::upgrade(&resolved, verbose, supplier.as_deref())?;
            }
        },
        Some(Command::Config { .. }) => {
            // Already handled above before config load.
        }
        Some(Command::Migrate {
            target,
            destination,
            new_username,
        }) => {
            let dest = cli::resolve_supplier_shortname(&destination);
            match target.as_deref() {
                Some("all") | None => {
                    commands::migrate_all(&dest, new_username.as_deref(), verbose, &cfg)?;
                }
                Some("self") => {
                    commands::migrate(
                        "Lalaggi/gitpkg",
                        &dest,
                        new_username.as_deref(),
                        verbose,
                        &cfg,
                    )?;
                }
                Some(t) => {
                    let resolved = cli::resolve_self_alias(t);
                    commands::migrate(
                        &resolved,
                        &dest,
                        new_username.as_deref(),
                        verbose,
                        &cfg,
                    )?;
                }
            }
        }
        None => {
            // clap prints help and exits when no subcommand is given.
        }
    }
    Ok(())
}
