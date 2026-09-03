# ADR 0001: Runtime and DSL semantics for v0.1

- Status: Accepted
- Date: 2026-09-03
- Scope: Promptr v0.1 language and runtime

## Context

The draft language and system documents described several features
inconsistently. In particular, the DSL grammar included shell escape even
though the runtime treated it as a REPL-only action; metadata was present in the
storage/TUI design but excluded from the language; editor defaults differed;
and the transaction pipeline placed revision validation before acquiring a
writer reservation, leaving a time-of-check/time-of-use (TOCTOU) gap.

Schema-version refusal also needs a precise rationale. Refusing to guess the
meaning of a newer persisted format protects compatibility and data semantics,
but must not become a generic “fail-closed” posture that turns recoverable
operational conditions into user work.

## Decision

### 1. Shell `!` is a REPL meta-action

A line beginning with `!` is recognized by the interactive REPL host before DSL
parsing. It is not a token, production, AST node, command, or effect in the
Promptr DSL. Script, `eval`, `check`, and the Rust domain API do not execute it.
The shell remains the appropriate host for combining external commands with a
Promptr script.

### 2. v0.1 metadata uses replacement statements

The DSL supports:

```promptr
METADATA Coding DESCRIPTION "Reusable coding prompt";
METADATA Coding TAGS ["coding", "systems"];
```

Both target an existing Fragment or Prompt and compile to the typed
`SetMetadata` domain operation shared by all frontends. Description assignment
replaces the complete description; an empty string clears it. Tag assignment
replaces the complete unordered tag set; an empty list clears it. Metadata does
not affect node identity, graph topology, child ordering, or canonical XML.
Arbitrary metadata bags remain outside v0.1.

### 3. The built-in editor is the default

`FRAGMENT` obtains content from an editor provider. The built-in editor works
without configuration and is the default. An external editor is an explicitly
configured alternative represented as argv, avoiding shell-dependent quoting.
Both providers return staged text or cancellation and have no store handle.

### 4. Authoritative validation occurs inside the write transaction

Interactive/external preparation occurs without a writer lock. A write then
uses this order:

```text
prepare external input
  -> BEGIN IMMEDIATE
  -> recompile and revalidate against the transaction snapshot
  -> check captured node/catalog revisions
  -> interpret mutations and buffer values
  -> COMMIT
  -> publish buffered values
```

The initial compile is useful for diagnostics, capability checks, and planning
preparation, but cannot authorize a mutation. Acquiring the SQLite writer
reservation before authoritative recompilation and revision checks prevents a
concurrent writer from changing the checked state before use. User think-time
remains outside the writer lock.

### 5. Newer-schema refusal is a compatibility invariant

An executable does not guess how to interpret a database or configuration whose
schema version is newer than it supports. Normal loading stops with an
observable diagnostic containing the found version, maximum supported version,
relevant path, and application version. The diagnostic gives a maintenance
path: upgrade Promptr; use `config check` for configuration, `db status` for a
database, or `doctor` for either. These inspection paths avoid constructing the
normal typed configuration or domain store.

This rule is specific to versioned persisted-format semantics. It is not a
general security principle and does not justify rejecting recoverable busy,
index, terminal, or other operational states.

## Consequences

- The DSL parser stays deterministic and platform-neutral; REPL shell handling
  cannot leak into batch or library execution.
- Metadata is automatable through scripts while retaining one typed domain
  mutation path.
- A fresh installation can edit Fragments without discovering or configuring an
  external program.
- Write transactions hold the reservation slightly longer because semantic
  analysis is repeated, in exchange for eliminating the TOCTOU window.
- Newer formats produce actionable, machine-identifiable diagnostics rather
  than speculative partial reads or vague refusal.

## Superseded alternatives

- Keeping `shell_escape` in the DSL grammar.
- Treating metadata as TUI-only or excluding tags from v0.1.
- Selecting `$VISUAL`, `$EDITOR`, or a platform editor by default.
- Checking revisions before starting the write transaction and trusting the
  result afterward.
- Describing every newer-schema mismatch as a generalized security
  “fail-closed” requirement.
