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

