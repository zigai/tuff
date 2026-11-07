# Install the executable binary to ~/.local/bin
install:
    TUFF_BUILD_VERSION="$(git describe --tags --dirty --always)" cargo install --locked --path crates/ruff --bin tuff --force --root "$HOME/.local"
