# gitpkg

`gitpkg` is a minimal Git-based package manager for GitHub (default), Gitlab, and other Git suppliers
gitpkg requires `git` installed as a system package
It allows you to clone repos, build them (if a supported language), and automatically symlink executables into `/usr/bin/` for easy access.

> ⚠️ Note: **sr.ht is not supported.** Only GitHub, Gitlab, and Codeberg are officially supported.

---

# Supported Languages

**Anything with a Makefile** - Requires `make` - *should* work.

**CMake** - Requires `cmake`

**Meson** - Requires `ninja`

**Rust** - Requires `cargo` and `rustc`

**C and C++** - Requires `gcc`, `build-essentials`,

**Golang** - Requires `go` (recommended), `gc` or `gccgo`

**Java and Kotlin** (work in progress) - Requires Gradle/Meson (depending on project setup)

**Node.js packages** - Requires `nodejs` - Supports npm, pnpm, and yarn (detected via lock files or `packageManager` field in package.json)

**Python (mostly functional)** - Supports projects with pyproject.toml; creates a venv and installs the package


---

# Features

Automatically builds with multiple language support.
Installs to `~/.local/share/gitpkg/(supplier_)user/repo/<latest commit hash>/`
Symlinks executables to `/usr/bin/` or `~/.local/bin/`
Copies included .desktop files into `~/.local/share/applications/`


---

# Installation

### Automatic:
```bash
curl -fsSL "https://raw.githubusercontent.com/Lalaggi/gitpkg/main/install.sh" | sh
```
### Manual:
```bash
cd ~/.cache/
git clone https://github.com/Lalaggi/gitpkg.git
cd gitpkg
cargo build --release
./target/release/gitpkg install Lalaggi/gitpkg
```

---

# Migrating from Codeberg

If you previously installed gitpkg or other packages from Codeberg, you can migrate them to GitHub:

```bash
# Migrate gitpkg itself (happens automatically on upgrade)
gitpkg upgrade self

# Migrate all Codeberg packages to GitHub at once
gitpkg migrate --all --to github

# Migrate a single package
gitpkg migrate <user>/<repo> --to github
```

The migrate command updates the package source, remote URL, and package list entries. Old Codeberg directories are left in place and can be cleaned with `gitpkg clean`.

---

# Usage

```
gitpkg <command> [args] [flags]
```

### Global Flags

| Flag | Description |
|------|-------------|
| `-v` | Verbose output |
| `--supplier <domain>` | Specify git supplier (also `--provider` or `--host`) |
| `--branch <branch>` | Clone/switch to a specific branch |
| `--target <make-target>` | Build a specific make target (e.g. `build-i686`) |
| `--flags "<build args>"` | Extra args passed to make/cmake (e.g. `"-j4"`) |
| `--submodules` | Init and update git submodules after clone |
| `--ssh` | Clone via `git@<host>:<user>/<repo>.git` |
| `--system` | Install symlink to `/usr/bin` (needs superuser) |
| `--superuser <p>` | Superuser provider: `sudo`, `pkexec`, `doas`, or `auto` |

### Supplier Shortnames

| Shortname | Domain |
|-----------|--------|
| `gh`, `github` | `github.com` |
| `gl`, `gitlab` | `gitlab.com` |
| `cb`, `codeberg` | `codeberg.org` |
| `glg`, `gnome`, `gnome-gitlab`, ... | `gitlab.gnome.org` |

---

### Install

```
gitpkg install <user>/<repo> [flags]
```

Install a package. Clones the repo, detects the build system, builds it, and symlinks executables to `~/.local/bin/` (or `/usr/bin/` with `--system`).

**Flags specific to install:**
- `--branch <branch>` — clone a specific branch
- `--target <make-target>` — build a specific make target
- `--flags "<build args>"` — extra args passed to make/cmake
- `--submodules` — init+update git submodules after clone
- `--ssh` — clone via SSH
- `--system` — install symlink to `/usr/bin` (needs superuser)
- `--superuser <p>` — choose superuser provider

**Examples:**
```bash
gitpkg install user/repo                        # from GitHub (default)
gitpkg install user/repo --supplier codeberg     # from Codeberg
gitpkg install user/repo --branch stable         # specific branch
gitpkg install user/repo --ssh                   # clone via SSH
gitpkg install user/repo --system                # symlink to /usr/bin
```

---

### Remove

```
gitpkg remove <user>/<repo> [--remove-deps]
```

Remove an installed package, its symlink, and associated data files.

| Flag | Description |
|------|-------------|
| `--remove-deps` | Also remove system packages that gitpkg installed as dependencies |

---

### Clean

```
gitpkg clean <user>/<repo>
gitpkg clean all
```

Remove old versions and temp files. `all` cleans every installed package.

---

### List

```
gitpkg list
```

List all installed packages with commit, build system, supplier, and size info.

---

### Upgrade

```
gitpkg upgrade [<user>/<repo>|all]
```

Upgrade a specific package or all installed packages. Defaults to `all` if no argument is given. Checks the remote for new commits and rebuilds if updates are available.

---

### Update

```
gitpkg update [<user>/<repo>|all]
```

Alias for `upgrade`.

---

### Versions

```
gitpkg versions <user>/<repo>
```

List all installed versions (commits) for a package, showing size and install date. The current version is marked with `*`.

---

### Version

```
gitpkg version <user>/<repo>
```

Alias for `versions`.

---

### Goto

```
gitpkg goto <user>/<repo> [--shell|-s]
```

Print the path to the installed package. With `--shell` or `-s`, spawns a shell in that directory instead.

---

### Change-branch

```
gitpkg change-branch <user>/<repo> <branch-name>
```

Switch an installed package to a different branch. Verifies the branch exists on the remote, clones it, and rebuilds.

---

### Config

```
gitpkg config --init
```

Write a default config file to `~/.config/gitpkg/config.toml`. The config file can set defaults for `--system`, `--ssh`, `--remove-deps`, `-v`, `--submodules`, and `--superuser`. CLI flags always override config values.

---

### Migrate

```
gitpkg migrate <user>/<repo> --to <destination>
gitpkg migrate --all --to <destination>
```

Migrate a package (or all packages) from one supplier to another. Updates the remote URL, supplier, username, and package list entries. Use `--new-username` to override the config's forge username mapping.

**Examples:**
```bash
gitpkg migrate user/repo --to github              # migrate one package
gitpkg migrate user/repo --to github --new-username Lalaggi  # with explicit username
gitpkg migrate --all --to github                   # migrate all packages
```

---

### Help

```
gitpkg help
```

Show the built-in help text.

---

### Conflicts

In case of conflicts (e.g. `github: user/repo` and `codeberg: user/repo`), non-GitHub repos are automatically installed under `<supplier>_<user>/<repo>` (e.g. `codeberg_user/repo`).

Use `--supplier` or `<supplier>_<user>/<repo>` to specify the correct version.

Without conflict, gitpkg will assume `<user>/<repo> = <supplier>_<user>/<repo>` when it can.



# License
This Project uses the GNU GENERAL PUBLIC LICENSE - Version 3