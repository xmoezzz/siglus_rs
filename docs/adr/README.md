# Architecture decision records

ADRs record significant architectural choices, their technical context, and
their consequences. They describe project contracts rather than temporary
progress, build logs, or conversation history. Original engine documentation
remains under `manual/`.

Each ADR includes its status, date, scope, implementation state, context,
decision, and consequences. Use sequential numbers and descriptive filenames.

Statuses:

- **Proposed**: a design under consideration.
- **Accepted**: the architectural direction has been selected.
- **Rejected**: a considered design that was not selected.
- **Superseded**: replaced by another ADR, linked from both records.

Acceptance of a decision does not establish implementation, compilation, or
runtime validation. Record those separately and link concrete evidence when
available. Material changes to an accepted decision require a superseding ADR.

| ADR | Decision | Status | Implementation |
| --- | --- | --- | --- |
| [0001](0001-unicode-text-layout.md) | Unicode text layout as an unofficial extension | Accepted | Cell spacing implemented, shaping and bidi planned |
