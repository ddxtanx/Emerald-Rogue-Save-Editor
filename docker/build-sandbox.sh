#!/usr/bin/env bash
# Build the Hermes sandbox image for this repo, matching the uid the agent will run as.
#
#   ./docker/build-sandbox.sh              # uid/gid from your current shell
#   SANDBOX_UID=1000 SANDBOX_GID=1000 ./docker/build-sandbox.sh
#
# Then in ~/.hermes/config.yaml:
#   terminal:
#     backend: docker
#     docker_image: "er-sandbox:latest"
# (docker/README.md explains the rest, including what to copy from the running
# container's mounts so the workspace mapping does not change.)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UID_WANT="${SANDBOX_UID:-$(id -u)}"
GID_WANT="${SANDBOX_GID:-$(id -g)}"
TAG="${SANDBOX_TAG:-er-sandbox:latest}"

cd "$REPO"
echo "== building $TAG as uid=$UID_WANT gid=$GID_WANT =="
docker build \
    -f docker/Dockerfile.sandbox \
    --build-arg SANDBOX_UID="$UID_WANT" \
    --build-arg SANDBOX_GID="$GID_WANT" \
    -t "$TAG" \
    .

echo
echo "== smoke test inside the image =="
docker run --rm --entrypoint bash "$TAG" -c '
set -e
echo -n "cargo: "; cargo --version
echo -n "rustc: "; rustc --version
echo -n "git:   "; git --version
echo -n "python:"; python3 --version
echo -n "Xvfb:  "; command -v Xvfb
echo -n "gl:    "; ls /usr/lib/dri 2>/dev/null | head -2 || echo "no dri dir"
echo -n "magick:"; magick --version | head -1
echo -n "xdotool:"; xdotool --version 2>/dev/null | head -1 || true
echo -n "latex: "; (latexmk --version 2>/dev/null || pdflatex --version) | head -1
echo -n "pdf:   "; pdftotext -v 2>&1 | head -1
echo -n "ocr:   "; tesseract --version 2>&1 | head -1
python3 -c "import pypdf, fitz, pdfplumber; print('python pdf libs: pypdf', pypdf.__version__)"
echo -n "cargo home writable: "; test -w "$CARGO_HOME" && echo yes || echo NO
'
echo
echo "Done. Point terminal.docker_image at $TAG and restart Hermes."
