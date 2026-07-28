#!/usr/bin/env bash
#
# br (beads_rust) installer - Ultra-robust multi-platform installer with beautiful output
#
# One-liner install:
#   curl -fsSL "https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.sh?$(date +%s)" | bash
#
# Options:
#   --version vX.Y.Z   Install specific version (default: latest)
#   --dest DIR         Install to DIR (default: ~/.local/bin)
#   --system           Install to /usr/local/bin (requires sudo)
#   --easy-mode        Auto-update PATH in shell rc files
#   --verify           Run self-test after install
#   --artifact-url URL Use a custom release artifact URL
#   --checksum SHA     Provide expected SHA256 checksum
#   --checksum-url URL Provide a custom checksum URL
#   --insecure-skip-checksum
#                      Allow installation without checksum verification
#   --from-source      Build from source instead of downloading binary
#   --quiet            Suppress non-error output
#   --no-gum           Disable gum formatting even if available
#   --uninstall        Remove br and clean up
#   --help             Show this help
#
set -euo pipefail
umask 022
shopt -s lastpipe 2>/dev/null || true

# ============================================================================
# Curl|bash self-protection: re-download and re-exec from an on-disk copy
# (issue #250)
# ============================================================================
# When invoked via `curl … | bash`, bash reads the script progressively from
# its own stdin.  Bugs cascade from that:
#
#   1. Any later `read -r` (interactive prompts), heredoc, or command that
#      the script itself tries to consume from stdin will steal bytes bash
#      still needs to parse — producing confusing errors like
#         "line 32: syntax error near unexpected token '1334'"
#      that cite a line nowhere near the actual text they reference
#      (issue #250 bug 2).
#   2. If the TCP connection stalls or is truncated, bash may parse a
#      partial script and run half of it.
#   3. macOS Homebrew bash 5.3+ has tightened its piped-stdin parser, making
#      patterns that worked on older bash fail on current Apple Silicon
#      hardware.
#
# The fix is a two-step bootstrap: when we detect that this script is
# running from a pipe (no file path AND stdin is not a terminal), we
# download a fresh copy of install.sh to a temp file with curl/wget and
# re-exec bash against that file.  From that point on `$0` is a real path,
# `BASH_SOURCE[0]` is populated, interactive `read` can route to the
# controlling tty, and parsing errors disappear.
#
# Re-exec is guarded by BR_INSTALLER_SELF_REEXEC=1 to prevent infinite
# recursion if for some reason the on-disk copy still looks piped (e.g.
# `exec` with no tty on an exotic runtime).
if [[ -z "${BR_INSTALLER_SELF_REEXEC:-}" ]] \
    && [[ -z "${BASH_SOURCE[0]:-}" || ! -r "${BASH_SOURCE[0]:-}" ]]; then
    __br_self_owner="${OWNER:-quangdang46}"
    __br_self_repo="${REPO:-beads_rust}"
    __br_self_branch="${BR_INSTALLER_BRANCH:-main}"
    __br_self_url="${BR_INSTALLER_URL:-https://raw.githubusercontent.com/${__br_self_owner}/${__br_self_repo}/${__br_self_branch}/install.sh}"
    __br_self_tmp="$(mktemp -t br-installer.XXXXXX 2>/dev/null || mktemp 2>/dev/null || echo "/tmp/br-installer.$$.sh")"

    __br_self_fetched=0
    if command -v curl >/dev/null 2>&1; then
        # Cache-bust with a query param to sidestep stale CDN copies; the
        # server ignores unknown query strings on raw.githubusercontent.com.
        if curl -fsSL --retry 3 --max-time 60 \
            "${__br_self_url}?$(date +%s 2>/dev/null || echo self)" \
            -o "$__br_self_tmp" 2>/dev/null; then
            __br_self_fetched=1
        fi
    fi
    if [[ "$__br_self_fetched" -eq 0 ]] && command -v wget >/dev/null 2>&1; then
        if wget -qO "$__br_self_tmp" \
            "${__br_self_url}?$(date +%s 2>/dev/null || echo self)" 2>/dev/null; then
            __br_self_fetched=1
        fi
    fi

    if [[ "$__br_self_fetched" -eq 1 ]] && [[ -s "$__br_self_tmp" ]]; then
        chmod 0700 "$__br_self_tmp" 2>/dev/null || true
        export BR_INSTALLER_SELF_REEXEC=1
        # Route interactive input to the controlling tty if one is usable.
        # `[[ -r /dev/tty ]]` returns true in some CI harnesses where
        # opening /dev/tty actually fails with "No such device or address",
        # so probe by opening it in a subshell first.
        if ( : </dev/tty ) 2>/dev/null; then
            exec bash "$__br_self_tmp" "$@" </dev/tty
        else
            exec bash "$__br_self_tmp" "$@" </dev/null
        fi
    fi
    # Fall through: if the self-download failed we still try to run what
    # we have.  This preserves the old curl|bash behavior for environments
    # without curl/wget, at the cost of the known piped-stdin hazards.
    rm -f "$__br_self_tmp" 2>/dev/null || true
    unset __br_self_owner __br_self_repo __br_self_branch __br_self_url __br_self_tmp __br_self_fetched
fi

# ============================================================================
# Configuration
# ============================================================================
VERSION="${VERSION:-}"
OWNER="${OWNER:-quangdang46}"
REPO="${REPO:-beads_rust}"
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) BINARY_NAME="br.exe" ;;
    *) BINARY_NAME="br" ;;
esac
DEST_DEFAULT="$HOME/.local/bin"
DEST="${DEST:-$DEST_DEFAULT}"
EASY=0
QUIET=0
VERIFY=0
FROM_SOURCE=0
UNINSTALL=0
CHECKSUM="${CHECKSUM:-}"
CHECKSUM_URL="${CHECKSUM_URL:-}"
ARTIFACT_URL="${ARTIFACT_URL:-}"
INSECURE_SKIP_CHECKSUM=0
LOCK_FILE="/tmp/br-install.lock"
NO_GUM=0
# it's a one-time tool, not a steady-state surface). Pass
# suppresses every skill regardless of this flag.
MAX_RETRIES=3
DOWNLOAD_TIMEOUT=120
INSTALLER_VERSION="2.0.0"

# Colors for fallback output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
BOLD='\033[1m'
DIM='\033[2m'
ITALIC='\033[3m'
NC='\033[0m'

# Gum availability flag
GUM_AVAILABLE=false

# ============================================================================
# Gum auto-installation (from giil)
# ============================================================================
try_install_gum() {
    # Skip if in CI or non-interactive
    [[ -z "${CI:-}" ]] || return 1
    [[ -t 1 ]] || return 1

    printf >&2 '%s\n' "Note: installing 'gum' (charmbracelet/gum) for styled output. Pass --no-gum to skip."

    # Inline OS detection
    local os="unknown"
    case "$(uname -s)" in
        Darwin*) os="macos" ;;
        Linux*)  os="linux" ;;
    esac

    # Try to install gum quietly
    case "$os" in
        macos)
            if command -v brew &> /dev/null; then
                brew install gum &>/dev/null && return 0
            fi
            ;;
        linux)
            # Try common package managers
            if command -v apt-get &> /dev/null; then
                (
                    sudo mkdir -p /etc/apt/keyrings 2>/dev/null
                    curl -fsSL https://repo.charm.sh/apt/gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/charm.gpg 2>/dev/null
                    echo "deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *" | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null
                    sudo apt-get update -qq && sudo apt-get install -y -qq gum
                ) &>/dev/null && return 0
            elif command -v dnf &> /dev/null; then
                (
                    echo '[charm]
name=Charm
baseurl=https://repo.charm.sh/yum/
enabled=1
gpgcheck=1
gpgkey=https://repo.charm.sh/yum/gpg.key' | sudo tee /etc/yum.repos.d/charm.repo >/dev/null
                    sudo dnf install -y gum
                ) &>/dev/null && return 0
            elif command -v pacman &> /dev/null; then
                sudo pacman -S --noconfirm gum &>/dev/null && return 0
            fi

            # Fallback: download from GitHub releases
            local arch
            arch=$(uname -m)
            case "$arch" in
                x86_64) arch="amd64" ;;
                aarch64|arm64) arch="arm64" ;;
                *) return 1 ;;
            esac

            local tmp_dir
            tmp_dir=$(mktemp -d)
            local gum_version="0.14.5"
            local gum_url="https://github.com/charmbracelet/gum/releases/download/v${gum_version}/gum_${gum_version}_Linux_${arch}.tar.gz"

            (
                cd "$tmp_dir"
                curl -fsSL "$gum_url" -o gum.tar.gz
                tar -xzf gum.tar.gz
                if sudo mv gum /usr/local/bin/gum 2>/dev/null; then
                    :
                else
                    mkdir -p ~/.local/bin
                    mv gum ~/.local/bin/gum
                fi
            ) &>/dev/null && rm -rf "$tmp_dir" && return 0

            rm -rf "$tmp_dir"
            ;;
    esac

    return 1
}

check_gum() {
    # Respect NO_GUM flag
    if [[ "$NO_GUM" -eq 1 ]]; then
        GUM_AVAILABLE=false
        return 1
    fi

    if command -v gum &> /dev/null; then
        GUM_AVAILABLE=true
        return 0
    fi

    # Only try to install gum if interactive and not disabled
    if [[ -t 1 && -z "${CI:-}" ]]; then
        if try_install_gum; then
            if [[ -x "${HOME}/.local/bin/gum" && ":$PATH:" != *":${HOME}/.local/bin:"* ]]; then
                export PATH="${HOME}/.local/bin:${PATH}"
            fi
            if command -v gum &> /dev/null; then
                GUM_AVAILABLE=true
                return 0
            fi
        fi
    fi

    return 1
}

# ============================================================================
# Styled output functions (gum with ANSI fallback)
# ============================================================================

# Print styled banner
print_banner() {
    [ "$QUIET" -eq 1 ] && return 0

    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum style \
            --border double \
            --border-foreground 39 \
            --padding "0 2" \
            --margin "1 0" \
            --bold \
            "$(gum style --foreground 42 '🔗 br installer')" \
            "$(gum style --foreground 245 'Agent-first issue tracker (beads_rust)')"
    else
        echo ""
        echo -e "${BOLD}${BLUE}╔════════════════════════════════════════════════╗${NC}"
        echo -e "${BOLD}${BLUE}║${NC}  ${BOLD}${GREEN}🔗 br installer${NC}                               ${BOLD}${BLUE}║${NC}"
        echo -e "${BOLD}${BLUE}║${NC}  ${DIM}Agent-first issue tracker (beads_rust)${NC}        ${BOLD}${BLUE}║${NC}"
        echo -e "${BOLD}${BLUE}╚════════════════════════════════════════════════╝${NC}"
        echo ""
    fi
}

# Log functions
log_info() {
    [ "$QUIET" -eq 1 ] && return 0
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum log --level info "$1" >&2
    else
        echo -e "${GREEN}[br]${NC} $1" >&2
    fi
}

log_warn() {
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum log --level warn "$1" >&2
    else
        echo -e "${YELLOW}[br]${NC} $1" >&2
    fi
}

log_error() {
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum log --level error "$1" >&2
    else
        echo -e "${RED}[br]${NC} $1" >&2
    fi
}

log_step() {
    [ "$QUIET" -eq 1 ] && return 0
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum style --foreground 39 "→ $1" >&2
    else
        echo -e "${BLUE}→${NC} $1" >&2
    fi
}

log_success() {
    [ "$QUIET" -eq 1 ] && return 0
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum style --foreground 82 "✓ $1" >&2
    else
        echo -e "${GREEN}✓${NC} $1" >&2
    fi
}

log_debug() {
    [[ "${DEBUG:-0}" -eq 1 ]] || return 0
    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum log --level debug "$1" >&2
    else
        echo -e "${CYAN}[br:debug]${NC} $1" >&2
    fi
}

# Spinner wrapper for long operations
# Note: gum spin can only execute external binaries, not shell functions.
# We work around this by checking if the command is a function and using bash -c.
run_with_spinner() {
    local title="$1"
    shift
    if [[ "$GUM_AVAILABLE" == "true" && "$QUIET" -eq 0 ]]; then
        # Check if first argument is a shell function
        if declare -f "$1" >/dev/null 2>&1; then
            # Export the function and run via bash -c
            local func_name="$1"
            shift
            # Can't easily export functions to gum subshell, so fall back to no-spinner
            log_step "$title"
            "$func_name" "$@"
        else
            gum spin --spinner dot --title "$title" -- "$@"
        fi
    else
        log_step "$title"
        "$@"
    fi
}

# Die with error
die() {
    log_error "$@"
    exit 1
}

# ============================================================================
# Usage / Help (gum-styled)
# ============================================================================
usage() {
    check_gum || true

    if [[ "$GUM_AVAILABLE" == "true" ]]; then
        gum style \
            --border double \
            --border-foreground 39 \
            --padding "1 2" \
            --margin "1" \
            --bold \
            "$(gum style --foreground 42 '🔗 br installer v'${INSTALLER_VERSION})" \
            "$(gum style --foreground 245 'Agent-first issue tracker')"

        echo ""

        gum style --foreground 214 --bold "SYNOPSIS"
        echo "  curl -fsSL .../install.sh | bash"
        echo "  curl -fsSL .../install.sh | bash -s -- [OPTIONS]"
        echo ""

        gum style --foreground 214 --bold "OPTIONS"
        gum style --foreground 39 "  Installation"
        gum style --faint "    --version vX.Y.Z   Install specific version (default: latest)"
        gum style --faint "    --dest DIR         Install to DIR (default: ~/.local/bin)"
        gum style --faint "    --system           Install to /usr/local/bin (requires sudo)"
        gum style --faint "    --artifact-url URL Use a custom release artifact URL"
        gum style --faint "    --checksum SHA     Provide expected SHA256 checksum"
        gum style --faint "    --checksum-url URL Provide a custom checksum URL"
        gum style --faint "    --insecure-skip-checksum  Allow unverified binary install"
        gum style --faint "    --from-source      Build from source instead of binary"
        echo ""
        gum style --foreground 39 "  Behavior"
        gum style --faint "    --easy-mode        Auto-update PATH in shell rc files"
        gum style --faint "    --verify           Run self-test after install"
        gum style --faint "    --quiet            Suppress progress messages"
        gum style --faint "    --no-gum           Disable gum formatting"
        echo ""
        gum style --foreground 39 "  Maintenance"
        gum style --faint "    --uninstall        Remove br and clean up"
        gum style --faint "    --help             Show this help"
        echo ""

        gum style --foreground 214 --bold "ENVIRONMENT"
        gum style --faint "  HTTPS_PROXY        Use HTTPS proxy for downloads"
        gum style --faint "  HTTP_PROXY         Use HTTP proxy for downloads"
        gum style --faint "  BR_INSTALL_DIR     Override default install directory"
        gum style --faint "  VERSION            Override version to install"
        echo ""

        gum style --foreground 214 --bold "EXAMPLES"
        gum style --foreground 39 "  # Default install"
        echo "  curl -fsSL https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.sh | bash"
        echo ""
        gum style --foreground 39 "  # System install with auto PATH"
        echo "  curl -fsSL .../install.sh | sudo bash -s -- --system --easy-mode"
        echo ""
        gum style --foreground 39 "  # Force source build"
        echo "  curl -fsSL .../install.sh | bash -s -- --from-source"
        echo ""
        gum style --foreground 39 "  # Uninstall"
        echo "  curl -fsSL .../install.sh | bash -s -- --uninstall"
        echo ""

        gum style --foreground 214 --bold "PLATFORMS"
        echo "  $(gum style --foreground 82 '✓ Linux x86_64')"
        gum style --foreground 82 "  ✓ Linux ARM64"
        gum style --foreground 82 "  ✓ macOS Intel"
        gum style --foreground 82 "  ✓ macOS Apple Silicon"
        echo "  $(gum style --foreground 82 '✓ Windows x64') $(gum style --foreground 245 --faint '(via WSL or manual)')"
        echo ""

        gum style --foreground 245 --italic "Installer will auto-install gum for beautiful output if not present"

    else
        cat <<'EOF'
br installer - Install beads_rust (br) CLI tool

Usage:
  curl -fsSL https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.sh | bash
  curl -fsSL .../install.sh | bash -s -- [OPTIONS]

Options:
  --version vX.Y.Z   Install specific version (default: latest)
  --dest DIR         Install to DIR (default: ~/.local/bin)
  --system           Install to /usr/local/bin (requires sudo)
  --artifact-url URL Use a custom release artifact URL
  --checksum SHA     Provide expected SHA256 checksum
  --checksum-url URL Provide a custom checksum URL
  --insecure-skip-checksum
                      Allow installation without checksum verification
  --easy-mode        Auto-update PATH in shell rc files
  --verify           Run self-test after install
  --from-source      Build from source instead of downloading binary
  --quiet            Suppress non-error output
  --no-gum           Disable gum formatting even if available
  --uninstall        Remove br and clean up

Environment Variables:
  HTTPS_PROXY        Use HTTPS proxy for downloads
  HTTP_PROXY         Use HTTP proxy for downloads
  BR_INSTALL_DIR     Override default install directory
  VERSION            Override version to install

Platforms:
  ✓ Linux x86_64
  ✓ Linux ARM64
  ✓ macOS Intel
  ✓ macOS Apple Silicon
  ✓ Windows x64 (via WSL or manual)

Examples:
  # Default install
  curl -fsSL .../install.sh | bash

  # Custom prefix with easy mode
  curl -fsSL .../install.sh | bash -s -- --dest=/usr/local/bin --easy-mode

  # Force source build
  curl -fsSL .../install.sh | bash -s -- --from-source

  # Uninstall
  curl -fsSL .../install.sh | bash -s -- --uninstall
EOF
    fi
    exit 0
}

# ============================================================================
# Argument Parsing
# ============================================================================
while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2;;
        --version=*) VERSION="${1#*=}"; shift;;
        --dest) DEST="$2"; shift 2;;
        --dest=*) DEST="${1#*=}"; shift;;
        --system) DEST="/usr/local/bin"; shift;;
        --easy-mode) EASY=1; shift;;
        --verify) VERIFY=1; shift;;
        --artifact-url) ARTIFACT_URL="$2"; shift 2;;
        --checksum) CHECKSUM="$2"; shift 2;;
        --checksum-url) CHECKSUM_URL="$2"; shift 2;;
        --insecure-skip-checksum) INSECURE_SKIP_CHECKSUM=1; shift;;
        --from-source) FROM_SOURCE=1; shift;;
        --quiet|-q) QUIET=1; shift;;
        --no-gum) NO_GUM=1; shift;;
        --uninstall) UNINSTALL=1; shift;;
        -h|--help) usage;;
        *) shift;;
    esac
done

# Environment variable overrides
[ -n "${BR_INSTALL_DIR:-}" ] && DEST="$BR_INSTALL_DIR"

# Initialize gum early for beautiful output
check_gum || true

# ============================================================================
# Uninstall
# ============================================================================
do_uninstall() {
    print_banner
    log_step "Uninstalling br..."

    if [ -f "$DEST/$BINARY_NAME" ]; then
        rm -f "$DEST/$BINARY_NAME"
        log_success "Removed $DEST/$BINARY_NAME"
    else
        log_warn "Binary not found at $DEST/$BINARY_NAME"
    fi

    # Remove PATH modifications from shell rc files
    for rc in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile" "$HOME/.config/fish/config.fish"; do
        if [ -f "$rc" ] && grep -q "# br installer" "$rc" 2>/dev/null; then
            if [[ "$OSTYPE" == "darwin"* ]]; then
                sed -i '' '/# br installer/d' "$rc" 2>/dev/null || true
            else
                sed -i '/# br installer/d' "$rc" 2>/dev/null || true
            fi
            log_step "Cleaned $rc"
        fi
    done

    log_success "br uninstalled successfully"
    exit 0
}

[ "$UNINSTALL" -eq 1 ] && do_uninstall

# ============================================================================
# Platform Detection
# ============================================================================
detect_platform() {
    local os arch libc

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="macos" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *) die "Unsupported OS: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x64" ;;
        aarch64|arm64) arch="arm64" ;;
        *) die "Unsupported architecture: $(uname -m)" ;;
    esac

    # Distinguish glibc vs musl on Linux. Alpine and other musl-based distros
    # need the statically linked musl binary; the gnu artifact references
    # libgcc_s/_Unwind_* symbols that musl's libc-compat shim does not provide
    # (see #284).
    libc=""
    if [ "$os" = "linux" ]; then
        # Detection order, cheapest and most reliable first:
        #   1. /etc/alpine-release  — Alpine fast path (cheap stat).
        #   2. /proc/self/maps      — what *this running bash* is linked
        #      against. Bulletproof: it survives systems that have the
        #      musl cross-toolchain installed alongside glibc (which
        #      makes /lib/ld-musl-*.so* present even on glibc hosts), and
        #      side-steps the `set -o pipefail` interaction with `ldd`.
        #   3. `ldd --version` output sniff — last resort for exotic
        #      systems with no /proc (e.g. heavily restricted containers).
        #
        # Note on the ldd path: musl's `ldd` exits non-zero even when it
        # prints "musl libc" to stderr, so `if … | grep -q …` is never
        # taken under `pipefail`. We capture combined output first and
        # match with `case` to avoid the pipeline entirely.
        if [ -f /etc/alpine-release ]; then
            libc="musl"
        elif grep -q 'ld-musl' /proc/self/maps 2>/dev/null; then
            libc="musl"
        elif command -v ldd >/dev/null 2>&1; then
            ldd_output=$(ldd --version 2>&1 || true)
            case "$ldd_output" in
                *[Mm]usl*) libc="musl" ;;
            esac
        fi
        if [ "$libc" = "musl" ] && [ "$arch" != "x64" ] && [ "$arch" != "arm64" ]; then
            libc=""
        fi
    fi

    if [ -n "$libc" ]; then
        echo "${os}-${libc}-${arch}"
    else
        echo "${os}-${arch}"
    fi
}

# ============================================================================
# Version Resolution (with robust fallbacks)
# ============================================================================
resolve_version() {
    if [ -n "$VERSION" ]; then return 0; fi

    log_step "Resolving latest version..."
    local latest_url="https://api.github.com/repos/${OWNER}/${REPO}/releases/latest"
    local tag=""
    local attempts=0

    # Try GitHub API with retries
    while [ $attempts -lt $MAX_RETRIES ] && [ -z "$tag" ]; do
        attempts=$((attempts + 1))

        if command -v curl &>/dev/null; then
            tag=$(curl -fsSL \
                --connect-timeout 10 \
                --max-time 30 \
                -H "Accept: application/vnd.github.v3+json" \
                "$latest_url" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || echo "")
        elif command -v wget &>/dev/null; then
            tag=$(wget -qO- --timeout=30 "$latest_url" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || echo "")
        fi

        [ -z "$tag" ] && [ $attempts -lt $MAX_RETRIES ] && sleep 2
    done

    if [ -n "$tag" ] && [[ "$tag" =~ ^v[0-9] ]]; then
        VERSION="$tag"
        log_success "Latest version: $VERSION"
        return 0
    fi

    # Fallback: try redirect-based resolution
    log_step "Trying redirect-based version resolution..."
    local redirect_url="https://github.com/${OWNER}/${REPO}/releases/latest"
    if command -v curl &>/dev/null; then
        tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' "$redirect_url" 2>/dev/null | sed -E 's|.*/tag/||' || echo "")
    fi

    if [ -n "$tag" ] && [[ "$tag" =~ ^v[0-9] ]] && [[ "$tag" != *"/"* ]]; then
        VERSION="$tag"
        log_success "Latest version (via redirect): $VERSION"
        return 0
    fi

    log_warn "Could not resolve latest version; will try building from source"
    VERSION=""
}

release_download_tag() {
    local raw="$1"
    if [ -z "$raw" ]; then
        printf '%s\n' ""
    elif [[ "$raw" == v* ]]; then
        printf '%s\n' "$raw"
    else
        printf 'v%s\n' "$raw"
    fi
}

release_asset_version() {
    local raw="$1"
    printf '%s\n' "${raw#v}"
}

# ============================================================================
# Cross-platform locking using mkdir (atomic on all POSIX systems)
# ============================================================================
LOCK_DIR="${LOCK_FILE}.d"
LOCKED=0

acquire_lock() {
    if mkdir "$LOCK_DIR" 2>/dev/null; then
        LOCKED=1
        echo $$ > "$LOCK_DIR/pid"
        return 0
    fi

    # Check if existing lock is stale
    if [ -f "$LOCK_DIR/pid" ]; then
        local old_pid
        old_pid=$(cat "$LOCK_DIR/pid" 2>/dev/null || echo "")

        # Check if process is still running
        if [ -n "$old_pid" ] && ! kill -0 "$old_pid" 2>/dev/null; then
            log_warn "Removing stale lock (PID $old_pid not running)"
            rm -rf "$LOCK_DIR"
            if mkdir "$LOCK_DIR" 2>/dev/null; then
                LOCKED=1
                echo $$ > "$LOCK_DIR/pid"
                return 0
            fi
        fi

        # Check lock age (5 minute timeout)
        local lock_age=0
        if [[ "$OSTYPE" == "darwin"* ]]; then
            lock_age=$(( $(date +%s) - $(stat -f %m "$LOCK_DIR/pid" 2>/dev/null || echo 0) ))
        else
            lock_age=$(( $(date +%s) - $(stat -c %Y "$LOCK_DIR/pid" 2>/dev/null || echo 0) ))
        fi

        if [ "$lock_age" -gt 300 ]; then
            log_warn "Removing stale lock (age: ${lock_age}s)"
            rm -rf "$LOCK_DIR"
            if mkdir "$LOCK_DIR" 2>/dev/null; then
                LOCKED=1
                echo $$ > "$LOCK_DIR/pid"
                return 0
            fi
        fi
    fi

    if [ "$LOCKED" -eq 0 ]; then
        die "Another installation is running. If incorrect, run: rm -rf $LOCK_DIR"
    fi
}

# ============================================================================
# Cleanup
# ============================================================================
TMP=""
cleanup() {
    [ -n "$TMP" ] && rm -rf "$TMP"
    [ "$LOCKED" -eq 1 ] && rm -rf "$LOCK_DIR"
    return 0
}
trap cleanup EXIT

# ============================================================================
# PATH modification
# ============================================================================
maybe_add_path() {
    case ":$PATH:" in
        *:"$DEST":*) return 0;;
        *)
            if [ "$EASY" -eq 1 ]; then
                local updated=0
                for rc in "$HOME/.zshrc" "$HOME/.bashrc"; do
                    if [ -f "$rc" ] && [ -w "$rc" ]; then
                        if ! grep -qF "$DEST" "$rc" 2>/dev/null; then
                            echo "" >> "$rc"
                            echo "export PATH=\"$DEST:\$PATH\"  # br installer" >> "$rc"
                        fi
                        updated=1
                    fi
                done

                # Handle fish shell
                local fish_config="$HOME/.config/fish/config.fish"
                if [ -f "$fish_config" ] && [ -w "$fish_config" ]; then
                    if ! grep -qF "$DEST" "$fish_config" 2>/dev/null; then
                        echo "" >> "$fish_config"
                        echo "set -gx PATH $DEST \$PATH  # br installer" >> "$fish_config"
                    fi
                    updated=1
                fi

                if [ "$updated" -eq 1 ]; then
                    log_warn "PATH updated; restart shell or run: export PATH=\"$DEST:\$PATH\""
                else
                    log_warn "Add $DEST to PATH to use br"
                fi
            else
                log_warn "Add $DEST to PATH to use br"
            fi
        ;;
    esac
}

# ============================================================================
# Fix shell alias conflicts
# ============================================================================
fix_alias_conflicts() {
    # Check if 'br' is aliased to something else (common: bun run)
    for rc in "$HOME/.zshrc" "$HOME/.bashrc"; do
        if [ -f "$rc" ]; then
            # Add unalias after any potential alias definitions
            if ! grep -q "unalias br.*# br installer" "$rc" 2>/dev/null; then
                if grep -q "alias br=" "$rc" 2>/dev/null; then
                    echo "" >> "$rc"
                    echo "unalias br 2>/dev/null  # br installer - remove conflicting alias" >> "$rc"
                    log_step "Added unalias to $rc to prevent conflicts"
                fi
            fi
        fi
    done
}

