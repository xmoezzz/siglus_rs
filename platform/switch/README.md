# Switch platform scaffold

This directory holds the Nintendo Switch port scaffold described in [ROADMAP.md](ROADMAP.md). Nothing here builds yet; PR3 wires it into CI.

## Toolchain (PR3 prerequisite, not installed on dev machines yet)

1. Install devkitA64 + devkitPro portlibs (`dkp-pacman -S devkitA64 libnx switch-tools switch-portlibs`).
2. `rustup component add rust-src` — the custom target is built from source with `-Zbuild-std=core,alloc,std,panic_abort`.
3. Cargo config for the target lives in `rust/`; the linker wrapper is `aarch64-none-elf-gcc` from devkitA64 so libnx `crt0`/`--start-group` flags apply.

## Layout

- `rust/aarch64-switch.json` — custom target spec (aarch64-none-elf base, newlib, `switch` os).
- `build_switch.sh` — stub build entry; becomes `cargo build -p siglus_engine --target rust/aarch64-switch.json` plus `elf2hbl`/NRO packaging in PR3.
- `ROADMAP.md` — staged PR plan.

## Why the target spec looks like this

`no_std` is **not** chosen for the engine; `std` comes from newlib via `build-std`. The `no_std`-style pieces are only the libnx `crt0` startup and panic/allocator glue, which the existing `main` shim pattern already isolates.
