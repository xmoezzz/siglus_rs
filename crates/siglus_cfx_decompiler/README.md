# siglus_cfx_decompiler

Rust D3DX9 compiled-effect (`.cfx` / `.fxo`) decompiler for Shader Model 2.0 bytecode.

This project is focused on DX9 SM2 effects used by older engines. It scans embedded VS/PS bytecode, parses shader CTAB constant tables, emits ASM listings, emits reconstructed HLSL, and parses the compiled effect container enough to recover technique/pass/state mapping.


Bundled sample input for this workspace:

```text
crates/siglus_cfx_decompiler/assets/shader.cfx
```

## Usage

```bash
cargo run --release -- path/to/input.cfx --out output
```

No extra mode flags are required. Raw single-shader bytecode is auto-detected.

## Output

```text
output/
  summary.txt
  original_names.txt
  technique_map.txt
  hlsl/
    <technique>__<pass>__vs.hlsl
    <technique>__<pass>__ps.hlsl
    unmapped__*.hlsl
    *.ctab.txt
  asm/
    <same-name>.asm
  bytecode/
    <same-name>.bin
  techniques/
    <technique>.txt
```

The primary names come from the compiled effect's technique/pass names. If a shader blob is not referenced by a parsed pass, it is emitted as `unmapped__ps/vs_index_offset.*`.

## What can and cannot be recovered

Recovered from the binary when present:

- technique names
- pass names
- pass state records
- vertex/pixel shader object references
- CTAB uniform names
- CTAB sampler names
- CTAB struct member names
- shader semantic declarations

Usually not present in SM2 bytecode:

- original `.fx` / `.hlsl` source file name
- original HLSL entry function name
- original local variable names
- original input/output struct type names

Those are only recoverable if the compiled effect carries debug/source annotations or the engine stored them as strings.

## Notes

The effect container parser follows the D3DX9 compiled-effect layout used by Wine/ReactOS: effect version tag, offset to the effect table, top-level parameter table, technique table, pass table, state records, and object payloads. It does not silently invent missing metadata.

The HLSL writer is SM2-focused. Straight-line shaders are expression-folded. Shaders with control flow are emitted as structured instruction-shaped HLSL rather than fake source.

## HLSL rewrite notes

This build rewrites every generated HLSL file through the direct SM2 instruction-shaped writer instead of the previous folded expression writer.  This avoids huge inline expressions and keeps the generated code closer to the bytecode instruction order.

Technique outputs are named with the effect technique index and pass index:

```text
tNNNN_<technique>__pNN_<pass>__vs.hlsl
tNNNN_<technique>__pNN_<pass>__ps.hlsl
```

The index prefix is intentional.  The compiled effect contains duplicate technique names, so names without the index overwrite previous files and lose shaders.

Pixel shader input registers are emitted as `COLORn` for `v#` and `TEXCOORDn` for `t#`; the previous writer trusted declaration usage too literally and produced invalid `POSITION0` pixel inputs for many SM2 shaders.

## Generated comment policy

Generated HLSL and ASM files do not include tool-authored banner/comment lines. Shader comment tokens such as CTAB are consumed for metadata and skipped in the ASM listing rather than emitted as comments.


Output layout now preserves original HLSL when available:

- output/hlsl/                 public HLSL, original when present, otherwise rewritten
- output/hlsl_original/        untouched reference HLSL copied from reference_hlsl
- output/hlsl_rewritten/       rewritten/decompiled HLSL generated from bytecode
- output/wgsl/                 rewritten WGSL currently used by the pipeline map
- output/wgsl_rewritten/       same rewritten WGSL kept explicitly as rewrite output
