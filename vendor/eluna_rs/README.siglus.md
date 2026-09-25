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

Traversal contexts also share inherited mesh chains and parameter sets until
they change. The stencil ancestor lookup uses borrowed paths and draw keys;
building draw metadata no longer copies an entire traversal context.
`src/runtime.rs` avoids an extra copy of the variable map and provides
`swap_scene`, allowing the host to retain the previous frame for both physics
passes without copying the scene. These optimizations preserve scene output.

Physics corrections also address discontinuous hair and bust motion:

- Mesh icons encode width, height, origin X and origin Y. Evaluate anchor
  deformation in the owning layer's coordinate frame, and put each local
  patch into the ancestor chain only once.
- Restore exported pendulum positions, velocities and bend state together
  with the equilibrium bias, avoiding an artificial startup impulse.
- Preserve published physics outputs across zero-time variable updates, so
  mouth updates cannot mirror or clamp those outputs again.

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
reproduced on the unmodified crate; the other 93 original tests and the new mesh regressions pass with this patch.

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
`EMOTE_BENCH_FRAMES` sets the sample count. `EMOTE_BENCH_TIMELINES` accepts
one `flags,name` entry per line to exercise simultaneous timelines, such as
`0,喜ぶ02`, `1,ポーズA`, and `2,待機ループ00`. The report includes frame-time
percentiles as well as average time for each update phase.

The host's idle redraw regression can be checked with the real model:

```sh
SIGLUS_EMOTE_TEST_PROJECT=/path/to/game \
  cargo test -p siglus_scene_vm --lib emote_continuous_frame -- --ignored
```

The physics regression loads both normal and close-up After models, runs 600
frames each, and checks startup equilibrium, per-frame continuity, and that
mouth updates leave physics outputs unchanged:

```sh
SIGLUS_EMOTE_TEST_PROJECT=/path/to/game \
  cargo test --release -p siglus_scene_vm --lib after_idle_physics -- --ignored
```
