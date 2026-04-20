# siglus_ss_decompiler

Siglus `.ss` decompiler entry point for `Scene.pck`.

Loader path:

1. Parse `S_tnm_pack_scn_header` from `Scene.pck`.
2. Detect chunk mode from `original_source_header_size`.
3. If `scn_data_exe_angou_mod != 0`, require a 16-byte exe angou key.
4. Apply exe-key XOR when required.
5. Apply the built-in Siglus easy-angou scene key.
6. LZSS-unpack the scene chunk.
7. Parse `S_tnm_scn_header` and disassemble `CD_*` bytecode.
8. Reconstruct `.ss` control-flow patterns emitted by the original compiler.

The built-in FORM and ELEMENT names come from the recovered compiler tables:

- `sub_402D50` / `def_form_Siglus.h`
- `sub_403730` / `def_element_Siglus.h`

The tool uses only the built-in recovered compiler tables for FORM and ELEMENT constants.

## Usage

```sh
cargo run -p siglus_ss_decompiler -- path/to/Scene.pck --out-dir out_ss
```

Single scene:

```sh
cargo run -p siglus_ss_decompiler -- --scene-pck path/to/Scene.pck --scene 0 --out scene.ss
```

List scenes:

```sh
cargo run -p siglus_ss_decompiler -- --scene-pck path/to/Scene.pck --list
```

Explicit exe-angou key:

```sh
cargo run -p siglus_ss_decompiler -- path/to/Scene.pck --out-dir out_ss --key 00112233445566778899AABBCCDDEEFF
```

If `Scene.pck` requires exe-angou and no `--key` is given, the tool looks for `key.toml` near the inferred project directory and near `Scene.pck`. If no key is found, it exits with an error.

`key.toml` may use either:

```toml
key_hex = "00112233445566778899AABBCCDDEEFF"
```

or:

```toml
key = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
       0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
```

## Current known unknowns

- `switch/case/default` recovery is still not implemented as a dedicated high-level pattern. The bytecode pattern is decoded, but the emitter may leave label/goto where switch recovery is not yet proven.
- `for(init; cond; update)` is recognized only by the original control-flow shape. Reconstructing the exact original header expression is not complete.
- Exact source formatting, comments, include/macro source text, and non-`#z` user label names are not recoverable from bytecode.

## VM cross-check use

The disassembler follows the same operand consumption order as `flow_script.cpp`, including recursive `FM_LIST` argument form decoding through the `tnm_stack_pop_arg_list` layout. A bytecode decode failure therefore points to either a remaining decoder format mismatch or a VM/parser discrepancy worth checking against the runtime path.
