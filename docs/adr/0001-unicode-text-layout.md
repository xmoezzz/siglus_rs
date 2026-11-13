# ADR 0001: Unicode text layout as an unofficial extension

- Status: Accepted
- Date: 2026-11-13
- Scope: Text layout in `siglus_rs`, initially in `siglus_scene_vm`
- Implementation: Cell-spacing stage implemented. Full shaping and bidi remain planned.
- Related issue: [#97](https://github.com/xmoezzz/siglus_rs/issues/97)

## Context

The current Siglus message and text-grid paths use a fixed cell model. ASCII and halfwidth
katakana receive narrow cells, while most other characters receive wide cells.
That model produces excessive spacing for translated alphabetic text, including
Latin letters with diacritics. The same classification is duplicated across
message layout, text rendering, backlog layout, and reconstructed choice glyphs.

A Vietnamese-specific character range adjustment would address only part of
this problem. Greek, Cyrillic, other alphabetic scripts, combining sequences,
and complex scripts also require layout beyond the existing cell assumption.
Unicode code points, UTF-16 code units, user-perceived characters, and rendered
glyphs are different units and cannot share one indexing rule.

Original-compatible behavior remains necessary for existing Japanese games,
script-visible string operations, and native save formats.

## Decision

### An unofficial extension

Implement language-independent Unicode text layout as a `siglus_rs` extension.
**This is not official SiglusEngine behavior and is not a claim of compatibility
with the original engine's typography.** Documentation and any exposed setting
must identify it as an unofficial extension.

Apply Unicode cell spacing automatically in the Siglus runtime. No per-game
configuration file, activation step, operating-system locale, or font-name
heuristic selects it. `CommandContext::new` initializes both runtime and UI
font caches with the same display policy on every host. Cache/UI resets and
child message windows retain it. Native-width classification remains available
internally for original-semantics reference and other runtimes. Native VM string
operations retain their original rules.

There is no language whitelist. Support is defined by Unicode text properties
and layout requirements, not by a Vietnamese special case or a list of selected
European languages. Every language affected by the existing width assumption
is in scope. Missing fonts or unimplemented layout capabilities must remain
visible limitations rather than being reported as supported.

### Shared layout policy and results

Use one display layout policy and a common layout result throughout the text
paths. The policy covers character classification, shaping, advances, spacing,
line breaking, direction, alignment, and reveal boundaries. Glyph rasterization
and graphics backends consume the result rather than reclassifying characters.

The contract covers:

- Message text, speaker names, and ruby annotations.
- Stage text objects, message choices, and button choices.
- Backlog layout and any text reconstructed while restoring a save.
- Input text display, caret movement, selection, and IME geometry where these
  depend on display layout.
- Text measurement and hit regions used by these paths.

Preserve the distinct semantics of ruby, vertical writing, and explicitly
positioned glyphs. A shared policy does not require them to use identical
alignment or spacing rules.

### Unicode support beyond a width table

Unicode mode must account for all of the following:

- Extended grapheme clusters, so combining marks do not become separate cells
  or independent typewriter steps.
- Font advances and shaping for the relevant text runs. Narrow/wide Unicode
  properties alone do not determine the final width of a glyph.
- Script-appropriate shaping, joining, mark positioning, and bidirectional
  layout, including mixed-direction text where required.
- Unicode line-breaking behavior, with appropriate CJK tailoring rather than
  treating every character as an independent wrapping opportunity.
- Font fallback that preserves shaped clusters and consistent measurement
  and rendering. Glyph coverage alone does not establish layout support.

Do not implement Unicode mode by marking every non-CJK character as halfwidth.
Do not normalize or rewrite stored script strings to make the layout easier.
Any normalized working representation must retain a mapping to the source.

### Preserve VM and save semantics

Display layout changes must not alter native string indexing, UTF-16 slicing,
script-visible length/width commands, opcode behavior, read flags, or the native
save schema. Keep native string semantics separate from the display policy.

Layout results must map glyphs and grapheme clusters back to their source
UTF-16 ranges. Use those mappings for presentation, interaction, and reveal
without changing the source string or VM program position semantics.

Existing saves that contain final glyph positions restore those positions
verbatim. Do not silently reflow historical text under a different policy.
Before enabling saving for Unicode layouts that cannot be represented by the
native snapshot, define a lossless reconstruction contract. Any necessary
extension metadata belongs in separately identified runner-owned storage,
bound to the relevant game and snapshot. It must not be inserted into native
records or silently discarded. Missing required reconstruction metadata must produce an explicit result.

### Platform and engine boundaries

Keep layout independent of windows and GPU APIs. Desktop, mobile, browser,
PS Vita, and Switch hosts consume the same owned layout data. Platform font
availability may differ, but backend selection must not silently change the
layout contract. Unsupported capabilities must be identified per platform.

Other maintained VMs retain their own original text semantics. A reusable
Unicode layout component may serve them through explicit integration, without
making their native width or string rules aliases of Siglus rules.

## Delivery and acceptance

The implemented cell-spacing stage lives in `src/text_layout.rs` under
`siglus_scene_vm`. It centralizes Unicode column widths, display-only NFC
projection, source byte/UTF-16 cluster ranges, and editbox grapheme boundaries.
Alphabetic and numeric characters use Unicode width classification, with resolved
font advances for newly handled narrow characters. Measurement uses the same
fallback face as rasterization. This avoids overlapping wide glyphs in legacy
CJK fonts that contain full-cell Greek and Cyrillic glyphs. Editbox interaction
and IME geometry carry the same em-unit advances as rendering. Other characters
use East Asian ambiguous widths to preserve Japanese ellipses, dashes, and
symbols. Ruby whose measured widths are unchanged retains the original
distribution and integer rounding.
The runtime uses it for messages, names, ruby, object strings, button choices,
backlog measurement, text raster layout, and choice reconstruction. Positioned
glyph snapshots retain their recorded geometry. Native string-width operations
remain separate in `runtime/string_semantics.rs`.

The internal `UnicodeCells` policy implements this stage. It does not establish
contextual shaping, bidi, Unicode line breaking, complex mark attachment,
cluster-aware fallback, joined emoji rendering, or normalization across separate
script append operations. Source mappings are working-layout data, not new
native save fields. Lossless reconstruction metadata for future shaped layouts
remains pending.
These limitations also apply to scripts whose glyph order and joining need a
shaping engine, even when their characters now receive narrower cells.

Deliver in stages under this single architectural scope:

1. Define source mappings, cache identity, invalidation rules, and the common
   layout result. Centralize display decisions while preserving native string
   operations and saved glyph coordinates.
2. Implement Unicode cluster, shaping, font fallback, direction, wrapping, and
   interaction behavior. A Latin-only width patch is an intermediate milestone,
   not completion of Unicode support.
3. Connect every listed presentation and reconstruction path, with explicit
   save and platform capability behavior.

Validation must distinguish implementation, compilation, and observed behavior.
Use focused evidence for accented and decomposed Latin text, Greek and Cyrillic,
joining and bidirectional scripts, complex combining scripts, mixed CJK text,
fallback fonts, and the applicable save/reconstruction paths. These are
representative capability cases, not a language whitelist. Retain original
compatibility evidence separately. Do not claim all-language or all-platform
support from a width predicate, a successful build, or one screenshot.

## Consequences

Translation patches can share one Unicode layout extension instead of accumulating
language-specific exceptions. The extension requires more than glyph coverage,
and introduces font, cache, and reconstruction contracts. Native VM string
operations and positioned save glyphs remain independent of display-width rules.

## References

- [Unicode Standard Annex #11: East Asian Width](https://www.unicode.org/reports/tr11/)
- [Unicode Standard Annex #29: Unicode Text Segmentation](https://www.unicode.org/reports/tr29/)
- [Unicode Standard Annex #14: Unicode Line Breaking Algorithm](https://www.unicode.org/reports/tr14/)
- [Unicode Standard Annex #9: Unicode Bidirectional Algorithm](https://www.unicode.org/reports/tr9/)
