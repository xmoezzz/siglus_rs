# Patched crates for the Switch build

The Switch build uses the ordinary crates.io dependency graph: the target
(`rust/aarch64-switch.json`, `env = "newlib"`) builds `std` with
`-Zbuild-std`, so no dependency needs a `no_std` port. This directory holds
only crates that do not compile with the pinned nightly
(`nightly-2026-01-20`) and have no fixed release yet.

| Crate | Patch | Why |
|---|---|---|
| `libc` 0.2.178 (std's) | `libc-0.2.178-horizon-aarch64-stat.patch` | libc's `horizon` types are the 3DS's (32-bit newlib). On aarch64, devkitA64's `struct stat` has 16-bit `dev_t`/`ino_t` and 64-bit `blksize_t`/`blkcnt_t`, so std read `st_mode`/`st_size` at the wrong offsets (`is_dir()` always false, `create_dir_all` failing on existing directories, bogus file sizes) and `dirent` names two bytes off. |
| `unwinding` 0.2.10 | `unwinding-0.2.10-catch-unwind-i32.patch` | `core::intrinsics::catch_unwind` now returns `i32` rather than `bool`; upstream 0.2.10 (the newest release, and `trunk` as of this writing) still treats it as `bool`. |

## libc (always applied, not vendored)

The Makefile downloads the crates.io release named by `LIBC_VERSION` (the
version pinned in the nightly's `library/Cargo.lock`), checks it against
`LIBC_SHA256`, applies the patch under `runtime/build/` and passes it to
the `-Zbuild-std` build with `--config patch.crates-io.libc.path=...`.
Cargo then warns that the patch is unused by the workspace graph (the
workspace locks a newer libc); std does use it. When the pinned nightly
changes, update `LIBC_VERSION`/`LIBC_SHA256` and re-check the layouts
against devkitA64 (`offsetof` over `struct stat` and `struct dirent`).

## Rules

- A vendored crate is its crates.io release plus exactly the listed `.patch`.
  `./verify.sh` downloads the release, checks its crates.io checksum,
  applies the patch and diffs the result against the vendored copy.
  `./verify.sh --upstream` also reports the newest release, so the copy can
  be deleted once upstream ships the fix.
- Patches are applied only to the Switch build, never workspace-wide. Nothing in
  the current NRO depends on `unwinding` (it is needed by libnx Rust bindings
  such as `nx`), so the Makefile passes it only on request:

  ```sh
  make -C platform/switch/runtime UNWINDING_PATCH=1
  ```

  which adds `--config 'patch.crates-io.unwinding.path="platform/switch/patches/unwinding-0.2.10"'`
  to the Rust build.
- These directories are not workspace members or path dependencies, so
  `cargo fmt` never rewrites them. Keep it that way: edit the `.patch`, then
  regenerate the vendored copy from the release.

## Updating or adding a patch

```sh
curl -sSfL -o x.crate https://static.crates.io/crates/<name>/<name>-<ver>.crate
tar xzf x.crate
cd <name>-<ver> && patch -p1 < ../<patch> && cd ..
rm -rf platform/switch/patches/<name>-<ver>
mv <name>-<ver> platform/switch/patches/
```

Then add the crate to `CRATES` in `verify.sh` (name, version, the `.crate`
SHA-256 from the crates.io index, patch file) and run `./verify.sh`.
