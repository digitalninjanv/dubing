#!/usr/bin/env bash
# ==============================================================================
# AudioDub AI - One-Line Installer & Desktop Integrator
# Repository: https://github.com/digitalninjanv/dubing
# ==============================================================================
# Usage:
#   Install latest:
#     curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash
#
#   Install specific version:
#     curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash -s -- --version v0.1.0
#
#   Uninstall:
#     curl -fsSL https://raw.githubusercontent.com/digitalninjanv/dubing/main/install.sh | bash -s -- --uninstall
# ==============================================================================

set -euo pipefail

# Configuration
REPO="digitalninjanv/dubing"
APP_NAME="audiodub"
APP_DISPLAY_NAME="AudioDub AI"
APP_ID="io.github.digitalninjanv.AudioDub"

# Installation Paths (Rootless XDG standards)
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${HOME}/.local/share"
APPS_DIR="${DATA_DIR}/applications"
ICONS_DIR="${DATA_DIR}/icons/hicolor/scalable/apps"
METAINFO_DIR="${DATA_DIR}/metainfo"

# ANSI Color output
if [ -t 1 ]; then
    RED=$'\033[0;31m'
    GREEN=$'\033[0;32m'
    YELLOW=$'\033[1;33m'
    BLUE=$'\033[0;34m'
    BOLD=$'\033[1m'
    RESET=$'\033[0m'
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    BOLD=''
    RESET=''
fi

log_info() {
    printf "${BLUE}${BOLD}[INFO]${RESET} %s\n" "$1"
}

log_success() {
    printf "${GREEN}${BOLD}[SUCCESS]${RESET} %s\n" "$1"
}

log_warn() {
    printf "${YELLOW}${BOLD}[WARNING]${RESET} %s\n" "$1"
}

log_error() {
    printf "${RED}${BOLD}[ERROR]${RESET} %s\n" "$1" >&2
}

# Cleanup temporary files on exit
TEMP_DIR=""
cleanup() {
    if [ -n "${TEMP_DIR}" ] && [ -d "${TEMP_DIR}" ]; then
        rm -rf "${TEMP_DIR}"
    fi
}
trap cleanup EXIT

# Print Usage Help
print_help() {
    cat <<EOF
${BOLD}${APP_DISPLAY_NAME} Installer${RESET}

Usage:
  install.sh [OPTIONS]

Options:
  --version <tag>   Install a specific release version (e.g. v0.1.0)
  --uninstall       Remove AudioDub AI and all desktop integrations
  --help, -h        Show this help message

Examples:
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash -s -- --version v0.1.0
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash -s -- --uninstall
EOF
}

# Uninstall routine
uninstall_app() {
    log_info "Uninstalling ${APP_DISPLAY_NAME}..."

    local removed=0

    if [ -f "${BIN_DIR}/${APP_NAME}" ]; then
        rm -f "${BIN_DIR}/${APP_NAME}"
        log_info "Removed binary: ${BIN_DIR}/${APP_NAME}"
        removed=1
    fi

    if [ -f "${BIN_DIR}/${APP_NAME}-uninstall" ]; then
        rm -f "${BIN_DIR}/${APP_NAME}-uninstall"
        log_info "Removed uninstaller: ${BIN_DIR}/${APP_NAME}-uninstall"
    fi

    if [ -f "${APPS_DIR}/${APP_ID}.desktop" ]; then
        rm -f "${APPS_DIR}/${APP_ID}.desktop"
        log_info "Removed desktop entry: ${APPS_DIR}/${APP_ID}.desktop"
        removed=1
    fi

    if [ -f "${ICONS_DIR}/${APP_ID}.svg" ]; then
        rm -f "${ICONS_DIR}/${APP_ID}.svg"
        log_info "Removed icon: ${ICONS_DIR}/${APP_ID}.svg"
        removed=1
    fi

    if [ -f "${METAINFO_DIR}/${APP_ID}.metainfo.xml" ]; then
        rm -f "${METAINFO_DIR}/${APP_ID}.metainfo.xml"
        log_info "Removed metainfo: ${METAINFO_DIR}/${APP_ID}.metainfo.xml"
        removed=1
    fi

    # Refresh desktop databases
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "${APPS_DIR}" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "${DATA_DIR}/icons/hicolor" 2>/dev/null || true
    fi

    if [ "${removed}" -eq 1 ]; then
        log_success "${APP_DISPLAY_NAME} has been completely uninstalled."
    else
        log_info "No installation of ${APP_DISPLAY_NAME} was found."
    fi
    exit 0
}

# Check Operating System and Architecture
verify_platform() {
    local os
    local arch
    os="$(uname -s)"
    arch="$(uname -m)"

    if [ "${os}" != "Linux" ]; then
        log_error "AudioDub AI is designed for Linux. Detected OS: ${os}"
        exit 1
    fi

    if [ "${arch}" != "x86_64" ] && [ "${arch}" != "amd64" ]; then
        log_error "Pre-built binary is currently compiled for x86_64. Detected architecture: ${arch}"
        log_info "You can build from source using 'cargo build --release' on your architecture."
        exit 1
    fi
}

# Check dependencies and provide distro-specific advice
verify_dependencies() {
    log_info "Checking system requirements..."

    # Critical tools for script execution
    for tool in curl tar sha256sum; do
        if ! command -v "${tool}" >/dev/null 2>&1; then
            log_error "Required tool '${tool}' is not installed. Please install it first."
            exit 1
        fi
    done

    # Audio engine dependencies (FFmpeg)
    local missing_audio=0
    if ! command -v ffmpeg >/dev/null 2>&1; then
        missing_audio=1
    fi
    if ! command -v ffprobe >/dev/null 2>&1; then
        missing_audio=1
    fi

    if [ "${missing_audio}" -eq 1 ]; then
        log_warn "FFmpeg / ffprobe was not found on your system."
        log_warn "AudioDub AI requires FFmpeg to process and align audio."
        printf "\n  To install FFmpeg:\n"
        if command -v dnf >/dev/null 2>&1; then
            printf "    ${BOLD}sudo dnf install ffmpeg${RESET} (Fedora / RHEL)\n\n"
        elif command -v apt >/dev/null 2>&1; then
            printf "    ${BOLD}sudo apt update && sudo apt install ffmpeg${RESET} (Ubuntu / Debian)\n\n"
        elif command -v pacman >/dev/null 2>&1; then
            printf "    ${BOLD}sudo pacman -S ffmpeg${RESET} (Arch Linux)\n\n"
        elif command -v zypper >/dev/null 2>&1; then
            printf "    ${BOLD}sudo zypper install ffmpeg${RESET} (openSUSE)\n\n"
        else
            printf "    Please install 'ffmpeg' using your distribution package manager.\n\n"
        fi
    fi
}

# Parse Arguments
TARGET_VERSION=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            if [ -n "${2:-}" ]; then
                TARGET_VERSION="$2"
                shift 2
            else
                log_error "--version requires a tag argument (e.g. v0.1.0)"
                exit 1
            fi
            ;;
        --uninstall)
            uninstall_app
            ;;
        --help|-h)
            print_help
            exit 0
            ;;
        *)
            log_error "Unknown argument: $1"
            print_help
            exit 1
            ;;
    esac
done

# Run Pre-flight Checks
verify_platform
verify_dependencies

# Determine Version to Download
if [ -z "${TARGET_VERSION}" ]; then
    log_info "Detecting latest release of ${APP_DISPLAY_NAME}..."
    
    # 1. Try gh CLI if available and authenticated
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        TARGET_VERSION="$(gh release view --repo "${REPO}" --json tagName -q .tagName 2>/dev/null || true)"
    fi

    # 2. Try GitHub API
    if [ -z "${TARGET_VERSION}" ]; then
        AUTH_HEADER=()
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            AUTH_HEADER=(-H "Authorization: token ${GITHUB_TOKEN}")
        elif [ -n "${GH_TOKEN:-}" ]; then
            AUTH_HEADER=(-H "Authorization: token ${GH_TOKEN}")
        fi
        
        HTTP_RESP="$(curl -sSL "${AUTH_HEADER[@]}" -H "Accept: application/vnd.github.v3+json" "https://api.github.com/repos/${REPO}/releases/latest" || true)"
        TARGET_VERSION="$(echo "${HTTP_RESP}" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4 || true)"
    fi

    # 3. Default fallback
    if [ -z "${TARGET_VERSION}" ] || [ "${TARGET_VERSION}" = "releases" ] || [ "${TARGET_VERSION}" = "null" ]; then
        TARGET_VERSION="v0.3.0"
        log_info "Using release version: ${BOLD}${TARGET_VERSION}${RESET}"
    else
        log_info "Latest version detected: ${BOLD}${TARGET_VERSION}${RESET}"
    fi
fi

# Prepare Download URLs
BASE_URL="https://github.com/${REPO}/releases/download/${TARGET_VERSION}"
ARCHIVE_NAME="audiodub-${TARGET_VERSION}-linux-x86_64.tar.gz"
DOWNLOAD_URL="${BASE_URL}/${ARCHIVE_NAME}"
CHECKSUM_URL="${BASE_URL}/checksums.sha256"

# Create Isolated Temp Directory
TEMP_DIR="$(mktemp -d -t audiodub-install-XXXXXX)"
cd "${TEMP_DIR}"

DOWNLOAD_SUCCESS=0

# Attempt 1: Using GitHub CLI if available and authenticated
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    log_info "Downloading ${ARCHIVE_NAME} via GitHub CLI..."
    if gh release download "${TARGET_VERSION}" --repo "${REPO}" -p "${ARCHIVE_NAME}" --dir . >/dev/null 2>&1; then
        DOWNLOAD_SUCCESS=1
        gh release download "${TARGET_VERSION}" --repo "${REPO}" -p "checksums.sha256" --dir . >/dev/null 2>&1 || true
    fi
fi

# Attempt 2: Using curl with optional authentication header
if [ "${DOWNLOAD_SUCCESS}" -eq 0 ]; then
    log_info "Downloading ${ARCHIVE_NAME}..."
    AUTH_HEADER=()
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        AUTH_HEADER=(-H "Authorization: token ${GITHUB_TOKEN}")
    elif [ -n "${GH_TOKEN:-}" ]; then
        AUTH_HEADER=(-H "Authorization: token ${GH_TOKEN}")
    fi

    if curl -fSL "${AUTH_HEADER[@]}" --progress-bar "${DOWNLOAD_URL}" -o "${ARCHIVE_NAME}" 2>/dev/null; then
        DOWNLOAD_SUCCESS=1
        curl -fsSL "${AUTH_HEADER[@]}" "${CHECKSUM_URL}" -o "checksums.sha256" 2>/dev/null || true
    fi
fi

if [ "${DOWNLOAD_SUCCESS}" -eq 0 ]; then
    log_error "Failed to download ${ARCHIVE_NAME} (Release: ${TARGET_VERSION})."
    printf "\n  ${YELLOW}${BOLD}Tips:${RESET}\n"
    printf "  - If repository '${REPO}' is private, please authenticate first:\n"
    printf "      export GITHUB_TOKEN=\"<your_personal_access_token>\"\n"
    printf "      # or: gh auth login\n"
    printf "  - Or change the repository visibility to Public in GitHub:\n"
    printf "      Settings -> Danger Zone -> Change repository visibility -> Make public\n\n"
    exit 1
fi

# Checksum Verification
log_info "Verifying SHA256 checksum..."
if [ -f "checksums.sha256" ]; then
    if grep "${ARCHIVE_NAME}" checksums.sha256 > specific_checksum.sha256 2>/dev/null; then
        if sha256sum --check --status specific_checksum.sha256; then
            log_success "Checksum verified successfully!"
        else
            log_error "Checksum verification failed! Downloaded archive may be corrupted or compromised."
            exit 1
        fi
    else
        log_warn "Checksum for ${ARCHIVE_NAME} not found in checksums.sha256. Proceeding with caution."
    fi
else
    log_warn "Checksum file not available for this release. Proceeding with download verification."
fi

# Extract Tarball
log_info "Extracting package contents..."
tar -xzf "${ARCHIVE_NAME}"

# Create Destination Directories
mkdir -p "${BIN_DIR}"
mkdir -p "${APPS_DIR}"
mkdir -p "${ICONS_DIR}"
mkdir -p "${METAINFO_DIR}"

# Install Binary
log_info "Installing executable to ${BIN_DIR}/${APP_NAME}..."
FOUND_BIN="$(find . -type f -name "${APP_NAME}" | head -n 1)"
if [ -n "${FOUND_BIN}" ] && [ -f "${FOUND_BIN}" ]; then
    install -m 755 "${FOUND_BIN}" "${BIN_DIR}/${APP_NAME}"
else
    log_error "Binary '${APP_NAME}' not found inside archive."
    exit 1
fi

# Install Desktop Entry
log_info "Registering desktop application..."
FOUND_DESKTOP="$(find . -type f -name "${APP_ID}.desktop" | head -n 1)"
if [ -n "${FOUND_DESKTOP}" ] && [ -f "${FOUND_DESKTOP}" ]; then
    cp -f "${FOUND_DESKTOP}" "${APPS_DIR}/${APP_ID}.desktop"
fi

# Ensure Exec line in .desktop points to installed binary
if [ -f "${APPS_DIR}/${APP_ID}.desktop" ]; then
    sed -i "s|^Exec=.*|Exec=${BIN_DIR}/${APP_NAME}|" "${APPS_DIR}/${APP_ID}.desktop"
fi

# Install Icon
FOUND_ICON="$(find . -type f -name "${APP_ID}.svg" | head -n 1)"
if [ -n "${FOUND_ICON}" ] && [ -f "${FOUND_ICON}" ]; then
    cp -f "${FOUND_ICON}" "${ICONS_DIR}/${APP_ID}.svg"
fi

# Install Metainfo
FOUND_METAINFO="$(find . -type f -name "${APP_ID}.metainfo.xml" | head -n 1)"
if [ -n "${FOUND_METAINFO}" ] && [ -f "${FOUND_METAINFO}" ]; then
    cp -f "${FOUND_METAINFO}" "${METAINFO_DIR}/${APP_ID}.metainfo.xml"
fi

# Refresh Desktop Databases
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${APPS_DIR}" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "${DATA_DIR}/icons/hicolor" 2>/dev/null || true
fi

# Create Uninstaller Helper
cat <<EOF > "${BIN_DIR}/${APP_NAME}-uninstall"
#!/usr/bin/env bash
set -euo pipefail
echo "Removing ${APP_DISPLAY_NAME}..."
rm -f "${BIN_DIR}/${APP_NAME}"
rm -f "${BIN_DIR}/${APP_NAME}-uninstall"
rm -f "${APPS_DIR}/${APP_ID}.desktop"
rm -f "${ICONS_DIR}/${APP_ID}.svg"
rm -f "${METAINFO_DIR}/${APP_ID}.metainfo.xml"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${APPS_DIR}" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "${DATA_DIR}/icons/hicolor" 2>/dev/null || true
fi
echo "${APP_DISPLAY_NAME} has been completely removed."
EOF
chmod 755 "${BIN_DIR}/${APP_NAME}-uninstall"

# Verify PATH
PATH_NOTICE=""
case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *)
        PATH_NOTICE="Note: '${BIN_DIR}' is not yet in your PATH. You can add it with:
  export PATH=\"\$HOME/.local/bin:\$PATH\" (add to ~/.bashrc or ~/.zshrc)"
        ;;
esac

# Success Banner
printf "\n${GREEN}${BOLD}======================================================${RESET}\n"
printf "${GREEN}${BOLD}   ${APP_DISPLAY_NAME} (${TARGET_VERSION}) Installed Successfully!   ${RESET}\n"
printf "${GREEN}${BOLD}======================================================${RESET}\n\n"

printf "  ${BOLD}Desktop App:${RESET} Search for '${APP_DISPLAY_NAME}' in your Application Menu\n"
printf "  ${BOLD}Terminal CLI:${RESET} ${BIN_DIR}/${APP_NAME} --help\n"
printf "  ${BOLD}Uninstaller:${RESET}  ${BIN_DIR}/${APP_NAME}-uninstall\n"

if [ -n "${PATH_NOTICE}" ]; then
    printf "\n${YELLOW}${PATH_NOTICE}${RESET}\n\n"
fi
