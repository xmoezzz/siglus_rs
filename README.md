# Siglus.rs

Siglus.rs is an unofficial Rust implementation and multi-platform port of SiglusEngine.

This project is non-commercial and intended for research purposes.

## Run

```bash
cargo run --release -p siglus_scene_vm --bin siglus_engine -- --project-dir ~/Documents/siglus_rs-main/testcase
```

## Resource decryption key

SiglusEngine games require a secondary key to decrypt protected resources.

Create `key.toml` in the game root:

```toml
key = [0x00, 0x11, ...] # 16 bytes
```

For most trial versions, a secondary key is usually not required, and a 16-byte zero key is enough:

```toml
key = [
  0x00, 0x00, 0x00, 0x00,
  0x00, 0x00, 0x00, 0x00,
  0x00, 0x00, 0x00, 0x00,
  0x00, 0x00, 0x00, 0x00,
]
```

For full retail versions, a game-specific secondary key is usually required.

There are several practical ways to obtain the key:

1. Static extraction, when the game executable is not encrypted or obfuscated:

   https://github.com/xmoezzz/siglus_static_key_tool

2. Dynamic extraction. The general idea can be found in this older repository:

   https://github.com/xmoezzz/SiglusExtract

3. Known-key databases maintained by some extractor tools.



