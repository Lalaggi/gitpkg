# gitpkg

`gitpkg` is a minimal Git-based package manager for GitHub, Gitlab, and other 
It allows you to clone repos, build them (if they are Rust/Cargo projects), and automatically symlink executables into `/usr/bin/` for easy access.

> ⚠️ Note: **sr.ht is not supported.** Only GitHub, Gitlab, and Codeberg are officially supported.

---

## Features

Automatically builds with multiple language support.
Installs to `~/.local/share/gitpkg/(supplier_)user/repo/<latest commit hash>/`
Symlinks executables to `/usr/bin/` or `~/.local/bin/`
Copies included .desktop files into `~/.local/share/applications/`


---

## Installation

```bash
#automatic:

curl -fsSL "https://codeberg.org/el1lovescomputers/gitpkg/raw/branch/main/install.sh" | sh

#or manual:

cd ~/.cache/
git clone codeberg.org/el1lovescomputers/gitpkg.git
cd gitpkg
cargo build --relase
./target/release/gitpkg install el1lovescomputers/gitpkg --supplier "codeberg.org"

```