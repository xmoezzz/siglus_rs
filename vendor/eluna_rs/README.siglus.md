# Eluna local patch

Source: the crates.io `eluna_rs` 0.1.0 release (MPL-2.0).
`src/` is copied from that release; `Cargo.toml` is its original manifest.

The changes in `src/emote.rs` borrow PSB objects instead of recursively
cloning them in schema queries and static/animated layer traversal. These
objects are immutable during evaluation. Motion priority tables are also
shared through `Arc` across traversal contexts, instead of cloning the same
map for every layer. A table is built once per motion evaluation, so priority
animation and nested player scopes keep their original behavior. These
recursive copies made large models take over 100 ms per frame.

The particle pass now copies only actual emitters, and the 3-D model pass
creates a layer snapshot only when a target-directed model needs it.
Ordinary 2-D layers still follow the same evaluation and rendering order.

Keep this patch until an upstream release includes equivalent fixes.
Do not reformat the upstream files: keeping the diff small makes updating
the vendored source easier.

Run the upstream unit tests with:

```sh
cargo test --manifest-path vendor/eluna_rs/Cargo.toml
```

The published 0.1.0 source already fails two of its 95 tests:
`runtime::reverse_parity_tests::timeline_hold_markers_do_not_pollute_authored_ranges`
and `vertex::tests::builds_single_cell_strip`. The same two failures were
reproduced on the unmodified crate; the other 93 tests pass with this patch.

The host benchmark uses real model files without adding game assets to the
repository:

```sh
EMOTE_BENCH_TIMELINE='喜ぶ02' EMOTE_BENCH_TRACE=1 \
  cargo run --release -p siglus_scene_vm --example emote_bench -- \
  /path/to/game 'bup_sn01_03制服＋エプロン.psb' \
  'bup_sn01_頭部.psb' 'bup_共通tl.psb'
```

`EMOTE_BENCH_TRACE` hashes the complete evaluated scene each frame, outside
the measured update time, for comparisons with the unmodified dependency.
