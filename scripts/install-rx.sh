#!/bin/sh
# RX source installer: macOS, Linux, and Windows through WSL.
# The website pins RX_DEFAULT_REVISION to the published library commit.
set -eu
RX_DEFAULT_REVISION=main

rx_fail() { printf 'RX: %s\n' "$*" >&2; exit 1; }
rx_download() {
    if command -v curl >/dev/null 2>&1; then
        curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$1" --output "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --quiet --output-document="$2" "$1"
    else
        rx_fail 'Install curl or wget, then run this installer again.'
    fi
}
rx_recent_rust() {
    command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1 || return 1
    rustc --version 2>/dev/null | awk '{split($2,v,"."); if (v[1]>1 || (v[1]==1 && v[2]>=88)) ok=1} END {exit !ok}'
}
rx_main() {
    case "${1:-}" in
        --help|-h)
            printf '%s\n' 'Install RX from source (macOS, Linux, WSL).' \
                'Usage: sh install-rx.sh' \
                'RX_INSTALL_ROOT overrides the installation root; rx goes in its bin directory.' \
                'Default: CARGO_INSTALL_ROOT, CARGO_HOME, or ~/.cargo.' \
                'RX_REVISION selects a library Git revision (default: published revision).' \
                'Requires a C compiler and tar. Installs Rust if needed. No sudo is used.'
            return ;;
        '') ;;
        *) rx_fail "Unknown option: $1 (use --help)" ;;
    esac
    [ "$#" -eq 0 ] || rx_fail 'Unexpected arguments (use --help).'
    case "$(uname -s)" in
        Darwin)
            xcode-select -p >/dev/null 2>&1 || rx_fail 'Install Apple Command Line Tools with xcode-select --install, then run this installer again.' ;;
        Linux) ;;
        *) rx_fail 'Use macOS or Linux; on Windows, run this command inside WSL.' ;;
    esac
    command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1 ||
        rx_fail 'A C compiler/linker is required. Install your OS development tools (for example, build-essential on Debian/Ubuntu), then retry.'
    command -v tar >/dev/null 2>&1 || rx_fail 'Install tar, then retry.'
    [ -n "${HOME:-}" ] || rx_fail 'HOME is not set.'
    rx_cargo_home=${CARGO_HOME:-$HOME/.cargo}
    rx_root=${RX_INSTALL_ROOT:-${CARGO_INSTALL_ROOT:-$rx_cargo_home}}
    case "$rx_root" in /*) ;; *) rx_fail 'The installation root must be an absolute path.' ;; esac
    rx_revision=${RX_REVISION:-$RX_DEFAULT_REVISION}
    case "$rx_revision" in ''|*[!a-zA-Z0-9._-]*) rx_fail 'RX_REVISION must be a commit hash, branch, or tag without slashes.' ;; esac
    rx_work=$(mktemp -d "${TMPDIR:-/tmp}/rx-install.XXXXXXXX")
    trap 'rm -rf "$rx_work"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    PATH="$rx_cargo_home/bin:$PATH"
    export PATH
    rx_stable=0
    if ! rx_recent_rust; then
        if command -v rustup >/dev/null 2>&1; then
            printf '%s\n' 'Preparing a supported Rust toolchain…'
            rustup toolchain install stable --profile minimal
            rx_stable=1
        else
            printf '%s\n' 'Installing the Rust build toolchain…'
            rx_download https://sh.rustup.rs "$rx_work/rustup.sh"
            sh "$rx_work/rustup.sh" -y --profile minimal --default-toolchain stable
        fi
    fi
    printf 'Downloading Pill Packer (%s)…\n' "$rx_revision"
    rx_download "https://codeload.github.com/AuthenticityLabs/pill-packer/tar.gz/$rx_revision" "$rx_work/source.tar.gz"
    mkdir "$rx_work/source"
    tar -xzf "$rx_work/source.tar.gz" --strip-components=1 -C "$rx_work/source"
    [ -f "$rx_work/source/rust/Cargo.lock" ] || rx_fail 'The downloaded source has no Rust lockfile.'
    printf 'Building RX for %s / %s…\n' "$(uname -s)" "$(uname -m)"
    if [ "$rx_stable" -eq 1 ]; then
        set -- +stable
    else
        set --
    fi
    cargo "$@" install --path "$rx_work/source/rust" --locked --features cli --bin rx \
        --root "$rx_root" --target-dir "$rx_work/target" --force
    [ -x "$rx_root/bin/rx" ] || rx_fail 'The build did not install an executable.'
    "$rx_root/bin/rx" --help >/dev/null
    printf '\nInstalled RX: %s/bin/rx\n' "$rx_root"
    case ":$PATH:" in
        *":$rx_root/bin:"*) printf '%s\n' 'Open a new terminal and run: rx --help' ;;
        *) printf 'Add %s/bin to your shell PATH, then run: rx --help\n' "$rx_root" ;;
    esac
}
rx_main "$@"
