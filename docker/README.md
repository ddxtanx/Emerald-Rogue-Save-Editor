# Sandbox image

Hermes runs the docker terminal backend with **no-new-privileges** and (with
`terminal.docker_run_as_host_user: true`) as a bare host uid. Two consequences that
decide the shape of this image:

* `sudo` refuses outright - `sudo: The "no new privileges" flag is set` - so **nothing can
  be installed at run time**. `pacman` is in the container and useless. Every tool has to
  be baked in at build time.
* The runtime uid often has **no passwd entry**: `whoami` fails, `$HOME` is empty, and
  things that cache under `~` break - cargo's registry, `git commit` ("identity not
  set", and it cannot write `~/.gitconfig`), pip, and the shader cache. The image answers
  that twice over: it pre-creates a user at the uid you run as (`SANDBOX_UID`/`SANDBOX_GID`
  build args), and it sets `HOME` plus `CARGO_HOME`, `GIT_CONFIG_GLOBAL`, `TMPDIR`,
  `XDG_*` to baked, `0777` absolute paths, so it still works when the uid drifts.

## Build

```sh
./docker/build-sandbox.sh            # uid/gid taken from your current shell
```

The build context is the repo root but `.dockerignore` keeps it tiny (`target/`,
`assets/`); the only files used are `Cargo.toml`/`Cargo.lock`, which pre-warm the
crates.io registry so a recreated container does not re-download the egui/eframe tree on
its first build.

## Point Hermes at it

`~/.hermes/config.yaml`:

```yaml
terminal:
  backend: docker
  docker_image: "er-sandbox:latest"
  docker_run_as_host_user: true
  container_cpu: 4
  container_memory: 8192          # a full texlive + cargo build wants more than the default
  container_disk: 51200
  docker_extra_args:
    - "--shm-size=1g"             # /tmp filled during ImageMagick pixel dumps once already
```

Keep whatever volume mapping you have now - copy it off the running container rather than
guessing:

```sh
docker inspect -f '{{json .Mounts}}' $(docker ps -qf name=hermes | head -1) | jq
```

Then restart Hermes (`/new` is not enough; the sandbox container is created by the
process).

## What is in it, and why

| Group | Packages | Why it is there |
|---|---|---|
| Build | `base-devel`, `rust`, `git`, `curl`, `ca-certificates`, `ripgrep`, `which`, `file`, `procps-ng` | cargo/rustc/gcc for the editor; `git` vanished once and blocked every commit for hours; `rust` (distro cargo) instead of rustup, because rustup's `~/.cargo` shims are what broke when the mount changed |
| GUI | `mesa`, `xorg-server-xvfb`, `xdotool`, `xorg-xwininfo`, `imagemagick` | the live-interface pass: run the window under `:99`, drive it, screenshot, crop, frame-diff |
| Docs | `texlive-basic/latex/latexrecommended/latexextra/fontsrecommended/fontsextra/pictures/xetex/luatex` | compiling the worksheet `.tex` sources in this workspace (~1.5-2 GB unpacked; drop `latexextra`/`fontsextra` for a lean image and accept some worksheets stop compiling) |
| PDF | `poppler`+`poppler-data`, `ghostscript`, `qpdf`, `mupdf`, `tesseract`+`tesseract-data-eng`, and a `/opt/pdfenv` venv with `pypdf`/`pymupdf`/`pdfplumber` | text extraction, page rasterising, form/object inspection, OCR on scans |
| Best-effort | `perl-image-exiftool`, `python-pypdf`, `python-pillow`, `ocrmypdf`, `unpaper`, `img2pdf`, `netpbm`, `gifsicle`, `optipng` | names not confirmed against archlinux.org; each installs independently and prints `optional package MISSING: x` in the build log instead of failing the image |

## Verification status - read this before trusting it

**Not built.** There is no `docker`/`podman` binary and no root in the sandbox I work in, so
this image has never been assembled; the package names were checked against
archlinux.org (Tex Live split names against the ArchWiki), and the unverified ones are in
the best-effort layer on purpose. The `docker run` smoke test at the end of
`build-sandbox.sh` prints versions for cargo/rustc/git/Xvfb/mesa/imagemagick/latexmk/
poppler/tesseract and imports the python PDF libs - run that first and paste me anything
that errors.

Once the image is live, the in-container checks that matter for this project:

```sh
cd /workspace/hermes/er-save-editor && cargo build && cargo test
Xvfb :99 -screen 0 1400x1000x24 &            # then the eframe app + import/xdotool loop
pdflatex -interaction=nonstopmode Exam-1-2-patched.tex
pdftotext -layout some-worksheet.pdf - | head
```
