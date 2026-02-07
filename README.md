# gitpkg

`gitpkg` is a minimal Git-based package manager for GitHub (default), Gitlab, and other Git suppliers
gitpkg requires `git` installed as a system package
It allows you to clone repos, build them (if a supported language), and automatically symlink executables into `/usr/bin/` for easy access.

> ⚠️ Note: **sr.ht is not supported.** Only GitHub, Gitlab, and Codeberg are officially supported.

---

# Supported Languages

**Rust** - Requires `cargo` and `rustc`

**C and C++** - Requires `gcc`, `build-essentials`, and a Makefile in the project root

**Golang** - Requires `go` (recommended), `gc` or `gccgo`

**Java and Kotlin** (work in progress) - Requires Gradle/Meson (depending on project setup)

**npm packages** (work in progress) - Requires npm


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
curl -fsSL "https://codeberg.org/el1lovescomputers/gitpkg/raw/branch/main/install.sh" | sh
```
### Manual:
```bash
cd ~/.cache/
git clone codeberg.org/el1lovescomputers/gitpkg.git
cd gitpkg
cargo build --release
./target/release/gitpkg install el1lovescomputers/gitpkg --supplier "codeberg.org"

```

---

# Usage

### Install

Install (github): `gitpkg install <user>/<repo>`

Install (Other. e.g. codeberg) `gitpkg install <user>/<repo> --supplier "codeberg.org"`


### List

List: `gitpkg list`

### Upgrade
Upgrade all: `gitpkg upgrade` or `gitpkg upgrade all`

Upgrade specifc: `gitpkg upgrade <user>/<repo>`


### Remove

Remove: `gitpkg remove <user>/<repo>`


### Clean

Clean old versions - All: `gitpkg clean all`

Clean old versions - Specific: `gitpkg clean <user>/<repo>`


### In case of conflicts e.g. github: el1lovescomputers/gitpkg and codeberg: el1lovescomputers/gitpkg

> Non github repos will automatically be installed as `<supplier>_<user>/<repo>`. e.g. codeberg_el1lovescomputers/gitpkg

Use `--supplier`, or `<supplier>_<user>/<repo>` to specify the correct version.

Without conflict, gitpkg will assume `<user>/<repo> = <supplier>_<user>/<repo>` when it can



# License
This Project uses the GNU GENERAL PUBLIC LICENSE - Version 3