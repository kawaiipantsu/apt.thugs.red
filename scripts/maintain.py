#!/usr/bin/env python3
"""Local build/release maintenance. Network publication is always an explicit command."""
import datetime
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)


def command(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def capture(args):
    return subprocess.check_output(args, text=True).strip()


def version():
    value = (ROOT / "VERSION").read_text().strip()
    assert re.fullmatch(r"\d+\.\d+\.\d+", value), "VERSION must be SemVer"
    return value


def version_check():
    v = version()
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    assert cargo["workspace"]["package"]["version"] == v, "Cargo version drift"
    assert json.loads((ROOT / "docs/openapi.json").read_text())["info"]["version"] == v, "OpenAPI version drift"
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    for package in lock["package"]:
        if package["name"] in {"xxc-aptd", "xxc-apt-cli", "xxc-aptd-core", "xxc-aptd-web", "xtask"}:
            assert package["version"] == v, "Cargo.lock version drift"
    line = (ROOT / "debian/changelog").read_text().splitlines()[0]
    assert re.fullmatch(r"xxc-aptd \(" + re.escape(v) + r"-\d+\) .*", line), "Debian version drift"
    for path in (ROOT / "docs/man").glob("*.scd"):
        assert f"XXC-APTD {v}" in path.read_text().splitlines()[0], f"man page version drift: {path.name}"
    print(f"version-check: {v}; application, lockfile, Debian revision and man pages agree")


def choose_bump(messages):
    priority = 0
    for message in messages:
        subject = message.splitlines()[0]
        match = re.match(r"(feat|fix|docs|test|build|ci|chore|refactor|perf|style|revert)(\([^)]*\))?(!)?: .+", subject)
        if not match:
            raise ValueError("Ambiguous commit history; choose bump-major, bump-minor or bump-patch explicitly")
        candidate = 3 if match[3] or re.search(r"(?m)^BREAKING[ -]CHANGE:", message) else (2 if match[1] == "feat" else 1)
        priority = max(priority, candidate)
    if priority == 0:
        raise ValueError("No release commits; choose an explicit bump")
    return {1: "patch", 2: "minor", 3: "major"}[priority]


def bump(kind):
    old = version()
    if kind == "auto":
        try:
            tag = capture(["git", "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"])
        except subprocess.CalledProcessError:
            raise SystemExit("No release tag; select bump-major, bump-minor or bump-patch explicitly")
        messages = capture(["git", "log", "--format=%B%x00", f"{tag}..HEAD"]).split("\0")
        kind = choose_bump([x.strip() for x in messages if x.strip()])
    major, minor, patch = map(int, old.split("."))
    new = {"major": f"{major+1}.0.0", "minor": f"{major}.{minor+1}.0", "patch": f"{major}.{minor}.{patch+1}"}[kind]
    notes = ROOT / "CHANGELOG.md"
    released_notes = promote_changelog(notes.read_text(), new)
    (ROOT / "VERSION").write_text(new + "\n")
    cargo = ROOT / "Cargo.toml"
    cargo.write_text(cargo.read_text().replace(f'version = "{old}"', f'version = "{new}"', 1))
    for path in list((ROOT / "docs/man").glob("*.scd")) + [ROOT / "config/aptd.conf.example"]:
        path.write_text(path.read_text().replace(f"XXC-APTD {old}", f"XXC-APTD {new}"))
    # Only current-version descriptions change; evidence, examples and screenshot
    # captions retain the version they actually describe.
    for path in [ROOT / "README.md", ROOT / "docs/ARCHITECTURE.md", ROOT / "docs/ROADMAP.md"]:
        path.write_text(path.read_text().replace(old, new))
    api_path = ROOT / "docs/openapi.json"
    api = json.loads(api_path.read_text())
    api["info"]["version"] = new
    api_path.write_text(json.dumps(api, indent=2) + "\n")
    changelog(new, 1, f"Release {new}.")
    notes.write_text(released_notes)
    command(["cargo", "check", "--workspace"], stdout=subprocess.DEVNULL)
    version_check()


def promote_changelog(text, new):
    section = re.search(r"(?ms)^## Unreleased\n(.*?)(?=^## |\Z)", text)
    assert section and section[1].strip(), "Write concrete Unreleased notes before bumping"
    assert not re.search(r"(?m)^## " + re.escape(new) + r"(?:\s|$)", text), "Version already in changelog"
    stamp = datetime.date.today().isoformat()
    return text[:section.start()] + f"## Unreleased\n\n## {new} — {stamp}\n\n{section[1].strip()}\n\n" + text[section.end():]


def release_notes(text, v):
    section = re.search(r"(?ms)^## " + re.escape(v) + r"(?: — [^\n]+)?\n(.*?)(?=^## |\Z)", text)
    assert section and section[1].strip(), "Release version has no changelog notes"
    return section[1].strip() + "\n"


def release_artifacts():
    """Select only this application's exact current Debian revision."""
    deb_version = re.fullmatch(r"xxc-aptd \(([^)]+)\) .*", (ROOT / "debian/changelog").read_text().splitlines()[0])[1]
    artifacts = sorted((ROOT / "dist").glob(f"xxc-aptd_{deb_version}_*.deb"))
    assert artifacts, "Run make deb for the current version/revision first"
    for artifact in artifacts:
        assert not artifact.is_symlink(), "Release artifacts must be regular files"
        for field, expected in [("Package", "xxc-aptd"), ("Version", deb_version)]:
            assert capture(["dpkg-deb", "-f", str(artifact), field]) == expected, "Debian artifact metadata mismatch"
    return artifacts


def prepare_release():
    version_check()
    assert not capture(["git", "status", "--porcelain"]), "Publication requires a clean worktree"
    sha = capture(["git", "rev-parse", "HEAD"])
    assert capture(["git", "rev-parse", f"v{version()}^{{commit}}"]) == sha, "Build and publish from the release tag"
    artifacts = release_artifacts()
    for artifact in artifacts:
        with tempfile.TemporaryDirectory(prefix="xxc-release-check-") as tmp:
            command(["dpkg-deb", "-x", str(artifact), tmp])
            for name in ["usr/sbin/xxc-aptd", "usr/bin/xxc-apt-cli"]:
                reported = capture([str(Path(tmp) / name), "--version"])
                assert f"{version()} ({sha[:12]})" in reported, "Rebuild the package from the clean release commit"
    output = ROOT / "dist" / f"v{version()}"
    output.mkdir(exist_ok=True)
    checksums = output / "SHA256SUMS"
    lines = []
    for artifact in artifacts:
        with artifact.open("rb") as file:
            lines.append(f"{hashlib.file_digest(file, 'sha256').hexdigest()}  {artifact.name}\n")
    checksums.write_text("".join(lines))
    notes = output / "release-notes.md"
    notes.write_text(release_notes((ROOT / "CHANGELOG.md").read_text(), version()))
    print(f"Release artifacts verified for v{version()} ({sha[:12]})")
    return artifacts, checksums, notes


def changelog(v, revision, text):
    path = ROOT / "debian/changelog"
    previous = path.read_text() if path.exists() else ""
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%a, %d %b %Y %H:%M:%S +0000")
    entry = f"xxc-aptd ({v}-{revision}) unstable; urgency=medium\n\n  * {text}\n\n -- XXC-APTD project <maintainer@example.invalid>  {stamp}\n\n" + previous
    path.write_text(entry.rstrip() + "\n")


def docs_check():
    required = ["ARCHITECTURE", "CONFIGURATION", "REPOSITORY", "API", "SECURITY", "OPERATIONS", "INSTALLATION", "REVERSE-PROXY", "SIGNING", "KEY-ROTATION", "DEVELOPMENT", "DEBIAN-PACKAGING", "UI", "ROADMAP", "RELEASES", "BACKUP-RESTORE"]
    for name in required:
        assert (ROOT / "docs" / f"{name}.md").is_file(), f"Missing {name}"
    for path in [ROOT / "README.md", *list((ROOT / "docs").glob("*.md"))]:
        for link in re.findall(r"\]\(([^)]+)\)", path.read_text()):
            if "://" in link or link.startswith("#"):
                continue
            assert (path.parent / link.split("#")[0]).exists(), f"Broken local link in {path.name}: {link}"
    example = tomllib.loads((ROOT / "config/aptd.conf.example").read_text())
    manual = (ROOT / "docs/man/aptd.conf.5.scd").read_text()
    for fields in example.values():
        for key in fields:
            assert key in manual, f"Configuration man page omits {key}"
    api = json.loads((ROOT / "docs/openapi.json").read_text())
    source = "\n".join(p.read_text() for p in (ROOT / "crates/xxc-aptd-web/src").glob("*.rs"))
    routes = set(re.findall(r'\.route\(\s*"(/api/v1/[^"]+)"', source))
    assert routes == set(api["paths"]), "OpenAPI route inventory differs from implementation"
    assert "--about" in (ROOT / "docs/man/xxc-aptd.8.scd").read_text()
    print("docs-check: canonical docs, links, config fields and OpenAPI route inventory pass")


def wiki_build():
    manifest = json.loads((ROOT / "docs/wiki/manifest.json").read_text())
    output = ROOT / ".build/wiki"
    output.mkdir(parents=True, exist_ok=True)
    mapping = {source: target for target, source in manifest.items()}
    for target, source in manifest.items():
        path = ROOT / "docs" / source
        assert path.is_file(), f"Wiki source missing: {source}"
        text = path.read_text()
        def link(match):
            name, url = match.groups()
            if url in mapping:
                return f"[{name}]({mapping[url].removesuffix('.md')})"
            if "://" not in url and not url.startswith("#"):
                from posixpath import normpath
                url = "https://github.com/kawaiipantsu/apt.thugs.red/blob/main/" + normpath("docs/" + url)
            return f"[{name}]({url})"
        text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, text)
        (output / target).write_text("<!-- Generated from canonical project documentation; do not edit here. -->\n\n" + text)
    (output / "Home.md").write_text("# XXC-APTD\n\n" + "\n".join(f"- [{x[:-3]}]({x[:-3]})" for x in manifest) + "\n")
    return output


def wiki_sync():
    remote = os.environ.get("WIKI_REMOTE")
    if not remote:
        raise SystemExit("Set WIKI_REMOTE, e.g. make wiki-sync WIKI_REMOTE=https://github.com/kawaiipantsu/apt.thugs.red.wiki.git")
    tree = wiki_build()
    with tempfile.TemporaryDirectory(prefix="xxc-wiki-") as tmp:
        command(["git", "clone", "--quiet", remote, tmp])
        for file in tree.glob("*.md"):
            shutil.copyfile(file, Path(tmp) / file.name)
        command(["git", "-C", tmp, "add", "--", *[p.name for p in tree.glob("*.md")]])
        if subprocess.run(["git", "-C", tmp, "diff", "--cached", "--quiet"]).returncode:
            command(["git", "-C", tmp, "commit", "-m", "docs(wiki): synchronize canonical project documentation"], stdout=subprocess.DEVNULL)
            command(["git", "-C", tmp, "push"], stdout=subprocess.DEVNULL)


def release(kind):
    assert not capture(["git", "status", "--porcelain"]), "Release requires a clean worktree"
    for field in ["user.name", "user.email"]:
        assert capture(["git", "config", "--global", "--get", field]), "Configure global Git identity"
    bump(kind)
    command(["make", "ci"])
    command(["git", "add", "-u"])
    command(["git", "commit", "-m", f"chore(release): v{version()}"], stdout=subprocess.DEVNULL)
    command(["git", "tag", "-a", f"v{version()}", "-m", f"XXC-APTD {version()}"])
    # CI ran before the commit. Rebuild so distributed binaries identify the
    # clean tagged source, rather than the previous commit and a dirty worktree.
    command(["make", "lintian"])
    prepare_release()
    print("Release commit and tag created. Review before pushing.")


def main(task):
    if task == "version-check":
        version_check()
    elif task == "docs-check":
        docs_check()
    elif task.startswith("bump-"):
        bump(task.removeprefix("bump-"))
    elif task == "deb-revision":
        first = (ROOT / "debian/changelog").read_text().splitlines()[0]
        revision = int(re.search(r"-(\d+)\)", first)[1]) + 1
        changelog(version(), revision, "Packaging maintenance.")
    elif task in {"wiki-build", "wiki-check"}:
        wiki_build()
        print("wiki: generated canonical documentation into .build/wiki")
    elif task == "wiki-sync":
        wiki_sync()
    elif task == "man":
        output = ROOT / ".build/man"
        output.mkdir(parents=True, exist_ok=True)
        for source in (ROOT / "docs/man").glob("*.scd"):
            data = subprocess.check_output(["scdoc"], input=source.read_bytes())
            (output / source.name.removesuffix(".scd")).write_bytes(data)
            (output / (source.name.removesuffix(".scd") + ".gz")).write_bytes(gzip.compress(data, mtime=0))
    elif task == "collect-deb":
        (ROOT / "dist").mkdir(exist_ok=True)
        for file in ROOT.parent.glob(f"xxc-aptd_{version()}-*.deb"):
            shutil.copyfile(file, ROOT / "dist" / file.name)
    elif task.startswith("release-"):
        release(task.removeprefix("release-"))
    elif task == "prepare-release":
        prepare_release()
    elif task == "publish-release":
        category = os.environ.get("DISCUSSION_CATEGORY")
        artifacts, checksums, notes = prepare_release()
        args = ["gh", "release", "create", f"v{version()}", "--verify-tag", "--title", f"XXC-APTD {version()}", "--notes-file", str(notes)]
        if category:
            args.extend(["--discussion-category", category])
        command([*args, *map(str, artifacts), str(checksums)])
    else:
        raise SystemExit("Unknown maintenance task")


if __name__ == "__main__":
    try:
        main(sys.argv[1])
    except (AssertionError, ValueError) as error:
        raise SystemExit(str(error))
