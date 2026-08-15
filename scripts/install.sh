#!/usr/bin/env sh
set -eu

REPOSITORY="${LOOM_REPO:-hi-youichi/loom}"
VERSION="${LOOM_VERSION:-latest}"
INSTALL_DIR="${LOOM_INSTALL_DIR:-$HOME/.local/bin}"

usage() {
    cat <<'EOF'
Install Loom from GitHub Releases.

Usage:
  ./install.sh [--version VERSION] [--install-dir DIR] [--repo OWNER/REPO]

Environment variables:
  LOOM_VERSION       Release tag without the leading v (default: latest)
  LOOM_INSTALL_DIR   Installation directory (default: ~/.local/bin)
  LOOM_REPO          GitHub repository (default: hi-youichi/loom)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || { echo "missing value for --version" >&2; exit 2; }
            VERSION="$2"
            shift 2
            ;;
        --install-dir)
            [ "$#" -ge 2 ] || { echo "missing value for --install-dir" >&2; exit 2; }
            INSTALL_DIR="$2"
            shift 2
            ;;
        --repo)
            [ "$#" -ge 2 ] || { echo "missing value for --repo" >&2; exit 2; }
            REPOSITORY="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS/$ARCH" in
    Linux/x86_64|Linux/amd64) TARGET="x86_64-unknown-linux-gnu" ;;
    Darwin/x86_64|Darwin/amd64) TARGET="x86_64-apple-darwin" ;;
    Darwin/arm64|Darwin/aarch64) TARGET="aarch64-apple-darwin" ;;
    *)
        echo "unsupported platform: $OS/$ARCH" >&2
        exit 1
        ;;
esac

command -v curl >/dev/null 2>&1 || {
    echo "curl is required to install Loom" >&2
    exit 1
}
command -v tar >/dev/null 2>&1 || {
    echo "tar is required to install Loom" >&2
    exit 1
}

if [ "$VERSION" = "latest" ]; then
    RELEASE_URL="https://github.com/$REPOSITORY/releases/latest/download"
else
    VERSION="${VERSION#v}"
    RELEASE_URL="https://github.com/$REPOSITORY/releases/download/v$VERSION"
fi

ARCHIVE="loom-${VERSION}-${TARGET}.tar.gz"
if [ "$VERSION" = "latest" ]; then
    ARCHIVE="loom-latest-${TARGET}.tar.gz"
    # GitHub's latest-download URL uses the actual release version in the
    # filename, so resolve the tag before downloading the archive.
    TAG="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPOSITORY/releases/latest" | sed -n 's#.*/tag/##p')"
    [ -n "$TAG" ] || { echo "could not determine the latest Loom release" >&2; exit 1; }
    VERSION="${TAG#v}"
    RELEASE_URL="https://github.com/$REPOSITORY/releases/download/$TAG"
    ARCHIVE="loom-${VERSION}-${TARGET}.tar.gz"
fi

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t loom-install)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT TERM

ARCHIVE_PATH="$TMP_DIR/$ARCHIVE"
echo "Downloading Loom $VERSION for $TARGET..."
curl -fL --retry 3 --proto '=https' --tlsv1.2 \
    "$RELEASE_URL/$ARCHIVE" -o "$ARCHIVE_PATH"

mkdir -p "$INSTALL_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"
[ -f "$TMP_DIR/loom" ] || { echo "release archive does not contain loom" >&2; exit 1; }
chmod 755 "$TMP_DIR/loom"
mv "$TMP_DIR/loom" "$INSTALL_DIR/loom"

echo "Loom installed to $INSTALL_DIR/loom"
case ":${PATH:-}:" in
    *:"$INSTALL_DIR":*) ;;
    *) echo "Add $INSTALL_DIR to PATH to run: loom" ;;
esac
