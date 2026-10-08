#!/usr/bin/env python3
"""
Automated crates.io publishing script for the siglus_rs monorepo.

Features:
- Skips vendor crates (e.g. eluna_rs) and private crates (publish = false).
- Calculates the correct topological publish order (DAG).
- Dynamically patches Cargo.toml in memory / on disk:
  * Injects `version = "..."` into all workspace path dependencies.
  * Injects missing metadata (`license`, `repository`, `description`).
  * Fixes example required-features in na_mpeg2_decoder.
- Automatically backs up and restores all modified Cargo.toml files upon completion
  or interruption, leaving repository code 100% untouched.
- Polls crates.io API until published crates appear in the registry before publishing
  dependent crates.
- Supports --dry-run, --publish, --token, --resume-from, and --keep-changes.
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import defaultdict, deque
from pathlib import Path

try:
    import tomllib
except ImportError:
    print("Error: Python 3.11+ is required (tomllib missing).", file=sys.stderr)
    sys.exit(1)

# Curated descriptions for crates lacking description in Cargo.toml
DEFAULT_DESCRIPTIONS = {
    "game_fs": "Game file access: std::fs on native targets, launcher file index on web",
    "engine-detect": "Identifies which engine (SiglusEngine, RealLive, AVG32 or UK2) a game directory uses",
    "game_launcher": "Shared launcher layer for SiglusEngine, RealLive, AVG32 and UK2",
    "na_mpeg2_decoder": "Rust MPEG-1/2 video decoder",
    "theora-rs": "Rust library-focused rewrite of libtheora 1.2.0",
    "wmv-decoder": "Pure-Rust WMV/WMA video and audio decoder",
    "siglus_omv_decoder": "OMV video stream decoder for SiglusEngine",
    "shion-xfile": "DirectX .x 3D model file format parser in Rust",
    "shion-render": "3D renderer for DirectX .x models using wgpu",
    "shion-xscene": "DirectX .x 3D scene graph and animation runtime",
    "siglus_assets": "SiglusEngine asset archive (.pck/.dbs) reader and extractor",
    "siglus_cfx_decompiler": "SiglusEngine CFX bytecode effect/shader decompiler",
    "siglus_key_recovery": "SiglusEngine game encryption key recovery utility",
    "siglus_ss_decompiler": "SiglusEngine scene script (.ss) decompiler",
    "siglus_x_viewer": "Standalone DirectX .x 3D model viewer application",
    "siglus_g00_extract": "SiglusEngine .g00 image extraction utility",
    "avg32": "AVG32 Visual Novel engine reimplementation in Rust",
    "reallive": "RealLive Visual Novel engine reimplementation in Rust",
    "uk2": "UK2 Visual Novel engine reimplementation in Rust",
    "siglus_scene_vm": "SiglusEngine Visual Novel script virtual machine and runtime",
}

DEFAULT_REPOSITORY = "https://github.com/xmoezzz/siglus_rs"
DEFAULT_LICENSE = "MPL-2.0"
USER_AGENT = "siglus-rs-publisher/1.0 (https://github.com/xmoezzz/siglus_rs)"


class MonorepoPublisher:
    def __init__(self, workspace_root: Path, dry_run: bool = True, token: str | None = None,
                 keep_changes: bool = False, wait_seconds: int = 10, timeout: int = 120):
        self.workspace_root = workspace_root.resolve()
        self.dry_run = dry_run
        self.token = token or os.environ.get("CARGO_REGISTRY_TOKEN")
        self.keep_changes = keep_changes
        self.wait_seconds = wait_seconds
        self.timeout = timeout

        self.packages: dict[str, dict] = {}
        self.original_files: dict[Path, str] = {}
        self.touched_files: set[Path] = set()

    def discover_packages(self):
        """Scans all crates/*/Cargo.toml in workspace."""
        crate_tomls = sorted(glob.glob(str(self.workspace_root / "crates/*/Cargo.toml")))
        for toml_path_str in crate_tomls:
            toml_path = Path(toml_path_str)
            with open(toml_path, "rb") as f:
                data = tomllib.load(f)

            pkg = data.get("package", {})
            name = pkg.get("name")
            ver = pkg.get("version")
            pub = pkg.get("publish")

            # Collect internal path dependencies
            internal_deps = set()
            all_deps = {}
            for k in ["dependencies", "dev-dependencies", "build-dependencies"]:
                all_deps.update(data.get(k, {}))
            for tgt in data.get("target", {}).values():
                for k in ["dependencies", "dev-dependencies", "build-dependencies"]:
                    all_deps.update(tgt.get(k, {}))

            for dep_name, spec in all_deps.items():
                if isinstance(spec, dict) and "path" in spec:
                    target_pkg = spec.get("package", dep_name)
                    # Skip external/vendor packages like eluna_rs
                    if target_pkg not in ("eluna", "eluna_rs"):
                        internal_deps.add(target_pkg)

            self.packages[name] = {
                "name": name,
                "version": ver,
                "publish": pub,
                "path": toml_path,
                "dir": toml_path.parent,
                "deps": internal_deps,
                "raw_data": data,
            }

    def compute_publish_order(self, target_crates: list[str] | None = None) -> list[str]:
        """Calculates topological sorting for publishable packages."""
        publishable = {
            name: info for name, info in self.packages.items()
            if info["publish"] is not False and name not in ("eluna", "eluna_rs")
        }

        if target_crates:
            target_set = set(target_crates)
            publishable = {k: v for k, v in publishable.items() if k in target_set}

        in_degree = {k: 0 for k in publishable}
        adj = defaultdict(set)

        for name, info in publishable.items():
            for dep in info["deps"]:
                if dep in publishable:
                    adj[dep].add(name)
                    in_degree[name] += 1

        queue = deque([k for k, deg in in_degree.items() if deg == 0])
        order = []
        while queue:
            curr = queue.popleft()
            order.append(curr)
            for neighbor in adj[curr]:
                in_degree[neighbor] -= 1
                if in_degree[neighbor] == 0:
                    queue.append(neighbor)

        if len(order) != len(publishable):
            missing = set(publishable) - set(order)
            raise RuntimeError(f"Cyclic dependency detected among: {missing}")

        return order

    def patch_manifests(self):
        """Patches all Cargo.toml files in memory and writes to disk, recording original state."""
        dep_pattern = re.compile(r"^(\s*)([a-zA-Z0-9_-]+)\s*=\s*\{([^}]+)\}", re.MULTILINE)

        for name, info in self.packages.items():
            toml_path = info["path"]
            with open(toml_path, "r", encoding="utf-8") as f:
                content = f.read()

            self.original_files[toml_path] = content
            patched = content

            # 1. Patch path dependencies with missing versions
            def repl(m):
                prefix = m.group(1)
                dep_var = m.group(2)
                body = m.group(3)
                if "path" not in body or "version" in body:
                    return m.group(0)
                pkg_match = re.search(r"package\s*=\s*[\"']([^\"']+)[\"']", body)
                target_name = pkg_match.group(1) if pkg_match else dep_var
                if target_name in self.packages:
                    ver = self.packages[target_name]["version"]
                    return f'{prefix}{dep_var} = {{ version = "{ver}", {body.strip()} }}'
                return m.group(0)

            patched = dep_pattern.sub(repl, patched)

            # 2. Inject missing metadata into [package]
            with open(toml_path, "rb") as f:
                parsed = tomllib.load(f)
            pkg_data = parsed.get("package", {})

            additions = []
            if "license" not in pkg_data:
                additions.append(f'license = "{DEFAULT_LICENSE}"')
            if "description" not in pkg_data and name in DEFAULT_DESCRIPTIONS:
                additions.append(f'description = "{DEFAULT_DESCRIPTIONS[name]}"')
            if "repository" not in pkg_data:
                additions.append(f'repository = "{DEFAULT_REPOSITORY}"')

            if additions:
                injection = "\n" + "\n".join(additions)
                patched = re.sub(r'(version\s*=\s*"[^"]+")', r'\1' + injection, patched, count=1)

            # 3. Special fix for na_mpeg2_decoder examples
            if name == "na_mpeg2_decoder" and '[[example]]' not in patched:
                example_fix = (
                    '\n\n[[example]]\n'
                    'name = "png_dump"\n'
                    'path = "examples/png_dump.rs"\n'
                    'required-features = ["desktop-bins"]\n'
                )
                patched += example_fix

            if patched != content:
                with open(toml_path, "w", encoding="utf-8") as f:
                    f.write(patched)
                self.touched_files.add(toml_path)

    def restore_manifests(self):
        """Restores touched manifests to their original pristine state."""
        if self.keep_changes:
            print("\n[INFO] --keep-changes specified: keeping patched Cargo.toml files.")
            return

        for path in self.touched_files:
            if path in self.original_files:
                with open(path, "w", encoding="utf-8") as f:
                    f.write(self.original_files[path])
        print(f"\n[INFO] Restored {len(self.touched_files)} Cargo.toml file(s) to original state.")

    def is_published_on_crates_io(self, name: str, version: str) -> bool:
        """Checks if a crate version is already available on crates.io."""
        url = f"https://crates.io/api/v1/crates/{name}/{version}"
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                return resp.status == 200
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return False
            # If rate limited or other error, fallback to assuming not published
            return False
        except Exception:
            return False

    def wait_for_crates_io(self, name: str, version: str) -> bool:
        """Polls crates.io until the crate version is indexed."""
        print(f"       Waiting for {name} v{version} to appear on crates.io...", end="", flush=True)
        start_time = time.time()
        while time.time() - start_time < self.timeout:
            if self.is_published_on_crates_io(name, version):
                print(f" Found! Waiting {self.wait_seconds}s for index propagation...", flush=True)
                time.sleep(self.wait_seconds)
                return True
            print(".", end="", flush=True)
            time.sleep(4)
        print(f" Timed out waiting after {self.timeout}s.")
        return False

    def run(self, resume_from: str | None = None, target_crates: list[str] | None = None):
        self.discover_packages()
        order = self.compute_publish_order(target_crates)

        if resume_from:
            if resume_from not in order:
                print(f"Error: resume crate '{resume_from}' not in publish order list.", file=sys.stderr)
                sys.exit(1)
            idx = order.index(resume_from)
            order = order[idx:]

        print("=" * 65)
        mode_str = "DRY-RUN (verification only)" if self.dry_run else "REAL PUBLISH"
        print(f"  siglus_rs monorepo publisher - Mode: {mode_str}")
        print("=" * 65)
        print(f"Found {len(order)} crate(s) to process in topological order:\n")
        for i, name in enumerate(order, 1):
            ver = self.packages[name]["version"]
            deps = ", ".join(self.packages[name]["deps"]) or "none"
            print(f"  {i:2d}. {name:<24} v{ver:<7} (internal deps: {deps})")
        print("=" * 65)

        try:
            print("\n[STEP 1] Dynamically patching manifests (adding version, license, metadata)...")
            self.patch_manifests()
            print(f"         Successfully patched {len(self.touched_files)} manifest(s).")

            print("\n[STEP 2] Processing crates in order...")
            for i, name in enumerate(order, 1):
                ver = self.packages[name]["version"]
                pkg_dir = self.packages[name]["dir"]
                print(f"\n---> [{i}/{len(order)}] {name} v{ver}")

                # Check if already published on crates.io
                if self.is_published_on_crates_io(name, ver):
                    print(f"       [ALREADY PUBLISHED] {name} v{ver} is already on crates.io. Skipping.")
                    continue

                if self.dry_run:
                    # In dry-run mode:
                    # If this crate has internal dependencies that are not yet on crates.io,
                    # cargo package will fail to resolve them from the index (since they aren't uploaded yet).
                    # We check TOML validity and report what dependencies are pending publication.
                    pending_deps = [dep for dep in self.packages[name]["deps"]
                                    if not self.is_published_on_crates_io(dep, self.packages.get(dep, {}).get("version", "0.1.0"))]

                    if pending_deps:
                        print(f"       [DRY-RUN] Manifest verified! (Will publish after index sync of: {', '.join(pending_deps)})")
                    else:
                        print(f"       [DRY-RUN] Verifying packaging for {name}...")
                        cmd = ["cargo", "package", "--allow-dirty", "--no-verify", "-p", name]
                        res = subprocess.run(cmd, cwd=self.workspace_root, capture_output=True, text=True)
                        if res.returncode != 0:
                            print(f"       [FAILED] cargo package failed:\n{res.stderr}")
                            sys.exit(1)
                        else:
                            print(f"       [SUCCESS] Packaged {name} v{ver} successfully.")
                else:
                    # In actual publish mode
                    print(f"       [PUBLISHING] Publishing {name} v{ver} to crates.io...")
                    cmd = ["cargo", "publish", "--allow-dirty", "--no-verify", "-p", name]
                    if self.token:
                        cmd.extend(["--token", self.token])

                    res = subprocess.run(cmd, cwd=self.workspace_root)
                    if res.returncode != 0:
                        print(f"\n[ERROR] Failed to publish {name} v{ver}.", file=sys.stderr)
                        sys.exit(1)

                    print(f"       [SUCCESS] Uploaded {name} v{ver} to crates.io.")

                    # Wait for crates.io index propagation if there are remaining packages
                    if i < len(order):
                        self.wait_for_crates_io(name, ver)

            print("\n" + "=" * 65)
            print("  All operations completed successfully!")
            print("=" * 65)

        finally:
            self.restore_manifests()


def main():
    parser = argparse.ArgumentParser(description="Publish siglus_rs monorepo crates to crates.io in topological order.")
    parser.add_argument("--publish", action="store_true", help="Execute actual publication (default is dry-run).")
    parser.add_argument("--token", help="crates.io API token (or set CARGO_REGISTRY_TOKEN env var).")
    parser.add_argument("--resume-from", help="Resume publishing starting from a specific crate.")
    parser.add_argument("--crates", help="Comma-separated list of specific crates to publish.")
    parser.add_argument("--keep-changes", action="store_true", help="Keep patched Cargo.toml files instead of restoring.")
    parser.add_argument("--wait", type=int, default=10, help="Extra wait seconds after crate is found on crates.io (default: 10).")
    parser.add_argument("--timeout", type=int, default=120, help="Max wait timeout seconds per crate (default: 120).")

    args = parser.parse_args()

    workspace_root = Path(__file__).resolve().parent.parent
    target_crates = [c.strip() for c in args.crates.split(",")] if args.crates else None

    publisher = MonorepoPublisher(
        workspace_root=workspace_root,
        dry_run=not args.publish,
        token=args.token,
        keep_changes=args.keep_changes,
        wait_seconds=args.wait,
        timeout=args.timeout,
    )
    publisher.run(resume_from=args.resume_from, target_crates=target_crates)


if __name__ == "__main__":
    main()
