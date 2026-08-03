#!/usr/bin/env bash
set -e

REPO="https://github.com/Lalaggi/gitpkg.git"
USER="Lalaggi"
PROJECT="gitpkg"
CACHE_DIR="$HOME/.cache/gitpkg/$PROJECT"

echo "=== gitpkg self-bootstrap installer ==="

# 1. Check cargo and rustc
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  echo "Cargo or rustc not found."
  read -p "Do you want to install Rust via rustup? [Y/n] " install_rust
  install_rust=${install_rust:-Y}
  if [[ "$install_rust" =~ ^[Yy]$ ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    export PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "Cannot continue without Rust. Exiting."
    exit 1
  fi
fi

# 2. Clone the repo
if [ -d "$CACHE_DIR" ]; then
  echo "Removing old cache directory..."
  rm -rf "$CACHE_DIR"
fi

echo "Cloning $PROJECT into $CACHE_DIR..."
mkdir -p "$(dirname "$CACHE_DIR")"
git clone "$REPO" "$CACHE_DIR"

# 3. Build the project
echo "Building $PROJECT..."
cd "$CACHE_DIR"
cargo build --release

# 4. Install into cache directory
INSTALL_DIR="$CACHE_DIR/install"
mkdir -p "$INSTALL_DIR/bin"
cp "target/release/$PROJECT" "$INSTALL_DIR/bin/"

# 5. Run gitpkg on itself
echo "Running gitpkg to install itself..."
"$INSTALL_DIR/bin/$PROJECT" install "$USER/$PROJECT"

# Install bash completion script to user data dir (if present in repo)
COMPLETION_SRC="$CACHE_DIR/gitpkg-completion.sh"
if [ -f "$COMPLETION_SRC" ]; then
  DEST_DIR="$HOME/.local/share/gitpkg"
  mkdir -p "$DEST_DIR"
  cp "$COMPLETION_SRC" "$DEST_DIR/gitpkg-completion.sh"
  chmod +x "$DEST_DIR/gitpkg-completion.sh"
  # Add source line to ~/.bashrc if not already present
  if ! grep -Fq "source \$HOME/.local/share/gitpkg/gitpkg-completion.sh" "$HOME/.bashrc" 2>/dev/null; then
    echo "source \$HOME/.local/share/gitpkg/gitpkg-completion.sh" >> "$HOME/.bashrc"
    echo "Appended completion source to ~/.bashrc"
  fi
fi

# 6. Cleanup
echo "Cleaning up cache directory..."
rm -rf "$CACHE_DIR"

echo "Self-bootstrap complete! $PROJECT installed as a gitpkg package."
echo "You may want to add ~/.local/bin to PATH if not already."
