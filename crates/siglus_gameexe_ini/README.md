# siglus_gameexe_ini

Expand `Gameexe.dat` to a UTF-8 `Gameexe.ini` text file.

This crate is standalone and only depends on the existing `siglus_assets` crate in the workspace.

## Formats handled

The original Siglus compiler writes an 8-byte header:

```text
int32 version
int32 exe_angou_mode
```

For that original format, the body is decoded in the same order as the runtime:

```text
if exe_angou_mode != 0:
    XOR body with 16-byte exe key
XOR body with fixed GAMEEXE_KEY
LZSS unpack
UTF-16LE decode
```

Some extracted or intermediate files may be headerless and start directly with an LZSS body. This tool handles that explicitly as a separate format instead of treating the first LZSS `arc_size` as an unsupported header version.

## Usage

```sh
cargo run -p siglus_gameexe_ini --release -- \
  /path/to/Gameexe.dat \
  --out /path/to/Gameexe.ini \
  --force \
  --verbose
```

With an external exe-angou key:

```sh
cargo run -p siglus_gameexe_ini --release -- \
  /path/to/Gameexe.dat \
  --out /path/to/Gameexe.ini \
  --key 00112233445566778899AABBCCDDEEFF
```

Or with `key.toml`:

```toml
key = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
       0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
```

Key lookup order:

1. `--key HEX`
2. `--key-file key.toml`
3. `--project-dir/key.toml`
4. `Gameexe.dat` directory `key.toml`
5. current directory `key.toml`

No environment variables are read.
