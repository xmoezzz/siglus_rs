# Siglus.rs

Another SiglusEngine Implementation in Rust with cross-platform support.

You can inspect and dump the decoded text:

```bash
cargo run -p siglus_scene_vm --bin gameexe_tool -- --project /path/to/game summary
cargo run -p siglus_scene_vm --bin gameexe_tool -- --project /path/to/game dump-ini
```

Environment variables (hex strings):

- `SIGLUS_EXE_ANGOU_HEX` (16 bytes)
- `SIGLUS_BASE_ANGOU_CODE_HEX` (often 256 bytes)
- `SIGLUS_GAME_ANGOU_CODE_HEX` (often 256 bytes)
- `SIGLUS_ANGOU_CHAIN_ORDER` (comma-separated: `exe,base,game`)

key.toml (project root) overrides:

```toml
key = [0x00, 0x11, ...]               # 16 bytes
base_angou_code = [0x00, 0x11, ...]   # optional
game_angou_code = [0x00, 0x11, ...]   # optional
chain_order = ["exe", "base", "game"] # optional
```

Hex string variants are also accepted:

```toml
key_hex = "001122...ff"
base_angou_hex = "001122...ff"
game_angou_hex = "001122...ff"
```
