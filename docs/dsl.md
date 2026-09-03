# Promptr DSL Specification

Status: Draft  
Version: 0.1

Runtime decisions that resolve cross-document boundaries are recorded in
[`adr/0001-runtime-semantics.md`](adr/0001-runtime-semantics.md).

## 1. Overview

Promptr is a persistent manager for named prompt graphs. Its DSL creates text
fragments, composes them into prompts, searches the resulting catalog, and
deterministically renders any node as XML.

The language is intentionally small:

```text
Promptr = Persistent Named DAG
        + Tiny DSL
        + XML Renderer
        + Searchable TUI
```

The DSL is the product's command language. The REPL, TUI, CLI, persistence
layer, and editor integration are adapters around the same domain operations;
none of them may define independent mutation semantics.

This document uses **MUST**, **MUST NOT**, **SHOULD**, and **MAY** as normative
terms.

## 2. Core model

### 2.1 Objects

Promptr has exactly two node kinds:

```text
Node
├── Fragment(content: XmlText)
└── Prompt(children: Vec<NodeId>)
```

- A **Fragment** is a named text leaf.
- A **Prompt** is a named, ordered sequence of references to other nodes.
- A **NodeId** is a stable internal identity and is never written directly in
  the DSL.
- A **Symbol** is the node's global, unique, user-visible identity.
- A child entry is an **edge occurrence**. Two entries may refer to the same
  NodeId.

Files are not part of the domain model. An editor may expose Fragment content
through a temporary file, and SQLite may persist it, but the Fragment itself is
an `XmlText` value.

### 2.2 Symbol identity

Every node has exactly one Symbol, and every Symbol identifies exactly one
node:

```text
symbol(a) = symbol(b)  iff  a = b
```

The Symbol is also the node's XML element name:

```text
XML tag = Symbol
```

There are no namespaces, aliases, qualified names, or anonymous nodes. Internal
identity and serialized identity therefore agree.

### 2.3 Graph structure

The composition graph is a directed acyclic graph (DAG):

- Prompt-to-node edges are directed.
- Child order is significant.
- Duplicate children are legal.
- Shared subgraphs are legal.
- Cycles, including direct self-reference, are illegal.
- Fragment nodes have no outgoing edges.
- Prompt nodes have at least one outgoing edge.

For example, this is valid:

```promptr
A: [B, B];
```

Both occurrences refer to the same node `B`. They are not two different
objects with the same name.

## 3. Language invariants

Every committed state MUST satisfy all of the following:

1. Every node is named.
2. Every Symbol is globally unique.
3. A Symbol is both the DSL name and XML tag of its node.
4. Fragment is the only leaf-node kind.
5. Prompt children are ordered.
6. Repeated child references are legal and preserve multiplicity.
7. Every reference resolves when its declaring command commits.
8. The composition graph is acyclic.
9. Deletion cannot create a dangling reference.
10. Fragment content cannot create XML structure.
11. Every committed Fragment is renderable as XML 1.0 text.
12. Rendering the same graph state produces the same byte sequence.

Mutation commands MUST be atomic: either the entire command commits or the
observable state remains unchanged.

## 4. Lexical syntax

### 4.1 Symbols

Version 0.1 uses a strict ASCII subset of XML names:

```regex
[A-Za-z_][A-Za-z0-9_-]*
```

Examples:

```text
Coding
BestPractice
cpp23
tool_call
Agent-Legible
```

The following are invalid:

```text
foo.bar
foo:bar
hello world
123Foo
```

In particular, `:` is forbidden so XML namespace syntax cannot re-enter the
language through element names.

Symbols are case-sensitive. `Notice` and `notice` are distinct Symbols.

### 4.2 Keywords

Command keywords are case-insensitive in keyword positions. They are
contextual rather than globally reserved, so a Symbol may have the same text as
a keyword when the grammar expects a Symbol:

```promptr
FRAGMENT Fragment;
OUTPUT Output;
```

### 4.3 String literals

Search queries, descriptions, and tags use double-quoted UTF-8 string literals.
Version 0.1 recognizes the following escapes:

```text
\"  \\  \n  \r  \t
```

Other escape sequences are invalid.

### 4.4 Statements and whitespace

- Ordinary statements end with `;`.
- Whitespace is insignificant outside string literals.
- Version 0.1 defines no comment syntax.

An interactive REPL may recognize a line beginning with `!` as a host
meta-action before invoking the DSL parser. That line is not DSL source and is
therefore not described by the lexical or grammar rules below.

## 5. Grammar

The normative surface grammar is:

```ebnf
input              = statement ;

statement          = fragment_statement
                   | prompt_statement
                   | rename_statement
                   | delete_statement
                   | metadata_statement
                   | list_statement
                   | print_statement
                   | output_statement
                   | search_statement
                   | find_statement ;

fragment_statement = "FRAGMENT" symbol ";" ;

prompt_statement   = symbol ":" "[" symbol
                     { "," symbol } "]" ";" ;

rename_statement   = "RENAME" symbol "TO" symbol ";" ;
delete_statement   = "DELETE" symbol ";" ;

metadata_statement = "METADATA" symbol
                     ( "DESCRIPTION" string
                     | "TAGS" tag_list ) ";" ;

tag_list           = "[" [ string { "," string } ] "]" ;

list_statement     = "LIST"
                     [ "FRAGMENTS" | "PROMPTS" ] ";" ;

print_statement    = "PRINT" symbol ";" ;
output_statement   = "OUTPUT" symbol ";" ;

search_statement   = "SEARCH" string
                     [ "FROM" search_field ] ";" ;

find_statement     = "FIND" string "ON" symbol
                     [ "FROM" search_field ] ";" ;

search_field       = "TITLE" | "CONTENT" | "MIXED" ;

symbol             = letter_or_underscore
                     { letter | digit | "_" | "-" } ;

letter_or_underscore = letter | "_" ;
letter             = "A" ... "Z" | "a" ... "z" ;
digit              = "0" ... "9" ;
```

An empty Prompt such as `A: [];` is syntactically and semantically invalid.
Anonymous composition is not part of the grammar:

```text
A: [B, [C, D]];
```

The statement above is invalid; the DSL itself does not recognize the prose as
a comment.

The nested composition must be named:

```promptr
Inner: [C, D];
A: [B, Inner];
```

## 6. Command semantics

### 6.1 `FRAGMENT`

```promptr
FRAGMENT <symbol>;
```

`FRAGMENT` creates or edits a Fragment through the configured editor provider.
The built-in editor is the default. An external editor is an explicitly
configured alternative.

| Existing binding | Result |
| --- | --- |
| No node | Open an empty staging document; create the Fragment after successful validation. |
| Fragment | Open its current content; replace the content after successful validation. |
| Prompt | Fail with a node-kind error. |

The provider receives the current content, or an empty draft for a new
Fragment, and returns either saved text or cancellation. An external provider
uses a temporary staging file and configured argv. If editing is cancelled, an
external editor exits unsuccessfully, the staging file cannot be read, the text
is invalid, or validation fails, the database MUST remain unchanged. A
successful edit is committed atomically; the implementation MUST NOT overwrite
durable content in place before validation.

Fragment content MAY be empty. It MUST be valid UTF-8 and contain only
characters permitted in XML 1.0 text. Structural XML metacharacters are legal
content and are escaped during rendering.

### 6.2 Prompt declaration

```promptr
<symbol>: [<symbol>, ...];
```

A Prompt declaration creates a Prompt or replaces an existing Prompt's complete
ordered child list.

| Existing binding | Result |
| --- | --- |
| No node | Create a Prompt after validation. |
| Prompt | Atomically replace all children after validation. |
| Fragment | Fail with a node-kind error. |

Before committing, the implementation MUST verify that:

1. the child list is non-empty;
2. every referenced Symbol exists;
3. replacing the edges would not introduce a cycle.

The declaration is replacement, not incremental editing. There are no separate
`PROMPT ADD`, `PROMPT REMOVE`, or `PROMPT EDIT` commands.

### 6.3 `RENAME`

```promptr
RENAME <old> TO <new>;
```

`RENAME` changes the canonical Symbol of an existing node while preserving its
NodeId, node kind, content, children, and all incoming references.

It fails if `old` does not exist, `new` is invalid, or `new` is already bound.
The operation is atomic. Since graph edges store NodeIds, a rename does not
rewrite graph topology.

### 6.4 `METADATA`

```promptr
METADATA <symbol> DESCRIPTION <string>;
METADATA <symbol> TAGS [<string>, ...];
```

Both forms replace one complete user-metadata field on an existing Fragment or
Prompt. They do not create nodes and do not change identity, graph edges, child
ordering, or canonical XML bytes.

- `DESCRIPTION` replaces the previous description. `""` clears it.
- `TAGS` replaces the complete unordered tag set. `[]` clears it.
- Tag spelling is preserved and tag identity is bytewise case-sensitive.
- Empty or whitespace-only tags are invalid. Repeated equal tag strings denote
  one set member and therefore do not create duplicate stored tags.

Both statements compile to the same typed `SetMetadata` domain operation used
by other frontends. The replacement and revision update are atomic.

### 6.5 `DELETE`

```promptr
DELETE <symbol>;
```

`DELETE` removes an unreferenced node. If any Prompt refers to the target, the
command fails and reports the referring Prompts. Duplicate references need not
produce duplicate parent names, but an implementation MAY report occurrence
counts.

Version 0.1 has no `FORCE` or `CASCADE` mode.

### 6.6 `LIST`

```promptr
LIST;
LIST FRAGMENTS;
LIST PROMPTS;
```

- `LIST;` lists all nodes.
- `LIST FRAGMENTS;` lists only Fragments.
- `LIST PROMPTS;` lists only Prompts.

Results MUST have a deterministic order. Version 0.1 orders them by Symbol
using bytewise ascending order.

### 6.7 `PRINT`

```promptr
PRINT <symbol>;
```

`PRINT` produces a human-oriented inspection view. It MAY include node kind,
direct children, size, previews, reference information, or truncation. Its
format is not a machine-stable interface.

For example:

```text
Coding: [Notice, Comment, Others]
```

or:

```text
Notice: Fragment
  size: 1.4 KiB
  preview: "Always preserve userspace compatibility..."
```

### 6.8 `OUTPUT`

```promptr
OUTPUT <symbol>;
```

`OUTPUT` produces complete machine-oriented XML for the selected node. It MUST
never truncate, summarize, decorate, paginate, or mix diagnostics into standard
output.

Rendering is recursively defined as:

```text
render(Fragment(symbol, text)) =
    "<" + symbol + ">" + escape_xml_text(text) + "</" + symbol + ">"

render(Prompt(symbol, children)) =
    "<" + symbol + ">"
    + concat(render(child) for child in children)
    + "</" + symbol + ">"
```

The canonical v0.1 output:

- is UTF-8 without a byte-order mark;
- contains no XML declaration;
- introduces no formatting whitespace;
- escapes `&`, `<`, and `>` in Fragment text;
- expands every edge occurrence in order;
- ends with exactly one LF byte after the root element.

No formatting whitespace is inserted because such whitespace would become XML
text and could alter the prompt. Human-friendly tree and XML previews belong to
`PRINT` or the TUI, not to the machine contract of `OUTPUT`.

For:

```promptr
FRAGMENT Notice;
FRAGMENT Comment;
Others: [Notice];
Coding: [Notice, Comment, Others];
OUTPUT Coding;
```

the structural result is:

```xml
<Coding><Notice>...</Notice><Comment>...</Comment><Others><Notice>...</Notice></Others></Coding>
```

### 6.9 `SEARCH`

```promptr
SEARCH <string> [FROM TITLE | CONTENT | MIXED];
```

`SEARCH` searches the global Fragment catalog. Version 0.1 search results are
Fragment objects, not edge occurrences.

- `TITLE` searches Symbols.
- `CONTENT` searches Fragment content.
- `MIXED` searches and ranks using both fields.
- The built-in default field is `MIXED`. An invocation may select a different
  default through typed configuration; an explicit `FROM` clause always wins.

The built-in matcher is fuzzy matching. Typed runtime configuration may select
case-sensitive exact substring matching. Matcher scoring and presentation are
not a stable language contract, but ties MUST be resolved deterministically.

The domain search API SHOULD model search scope, searched field, and matching
algorithm as separate dimensions even though version 0.1 exposes only the
field:

```text
Search = Scope x Field x Matcher
```

This permits future matchers such as `EXACT`, `FTS`, or `SEMANTIC` without
changing the meaning of `FROM`.

### 6.10 `FIND`

```promptr
FIND <string> ON <prompt> [FROM TITLE | CONTENT | MIXED];
```

`FIND` searches only Fragments reachable from the specified Prompt. The `ON`
target MUST exist and MUST be a Prompt.

Results are deduplicated by NodeId. If a Fragment is reachable through multiple
paths or repeated edges, it appears once in the result set. The interface MAY
show its occurrence count and paths separately:

```text
A    2 occurrences
  Root -> A
  Root -> X -> A
```

An occurrence count counts paths in the fully expanded output tree, not merely
distinct graph edges.

### 6.11 REPL shell meta-action (not DSL)

```text
! <shell command>
```

This syntax is recognized only by the interactive REPL host before DSL parsing.
It is a REPL meta-action, not a statement in the Promptr grammar. Script,
`eval`, `check`, and the Rust domain API MUST NOT recognize or execute it.

Everything after `!` through the end of the line is opaque shell text. Because
the line never reaches the DSL parser, semicolons, quotes, substitutions, and
pipelines have no DSL meaning inside it.

The command is executed by the configured platform shell. Promptr reports its
exit status but does not infer or commit domain changes from shell execution.
The REPL SHOULD visibly distinguish this external process action from ordinary
DSL commands.

## 7. Persistence and transactions

All successful mutations are immediately durable. There is no separate saved
versus unsaved object state, and the REPL does not own an in-memory shadow
catalog.

Therefore version 0.1 has no `SAVE` or `DROP` commands:

- `SAVE` would introduce an unnecessary second persistence state.
- `DROP` would either duplicate deletion or create a product-level anonymous
  node, violating the naming invariant.

A mutation first prepares interactive or external input without holding a
writer lock. It then starts `BEGIN IMMEDIATE` and, inside that transaction,
recompiles and revalidates the command against the transaction snapshot,
checks any captured node/catalog revision, applies all persistent changes, and
commits. The in-transaction compile and revision check are authoritative: no
mutation may rely only on validation performed before the write transaction.
This ordering removes the time-of-check/time-of-use (TOCTOU) gap while keeping
user think-time outside the writer lock. Errors MUST NOT expose a partially
updated symbol table, node payload, metadata value, or edge list.

SQLite is the recommended persistence implementation, with a logical schema
equivalent to:

```text
nodes(id, symbol UNIQUE, kind)
fragments(node_id PRIMARY KEY, content)
prompt_edges(parent_id, ordinal, child_id,
             PRIMARY KEY(parent_id, ordinal))
```

The database representation is non-normative. Implementations MAY use another
store if they preserve the same semantics, ordering, integrity, and atomicity.

## 8. Error model

Errors are typed domain outcomes, not successful textual results. At minimum,
the implementation distinguishes:

| Error | Example |
| --- | --- |
| Invalid syntax | Missing `;` or malformed list |
| Invalid Symbol | `123Foo` or `foo:bar` |
| Unknown Symbol | A referenced child does not exist |
| Symbol conflict | Rename or creation targets an existing Symbol |
| Node-kind mismatch | `FRAGMENT` targets a Prompt |
| Empty Prompt | `A: [];` |
| Cycle detected | `A: [A];` or an indirect cycle |
| Referenced node | `DELETE` targets a child of another Prompt |
| Invalid Fragment text | Invalid UTF-8 or XML 1.0 character |
| Invalid metadata | Empty/whitespace-only tag or unknown target |
| Editor failure | Editor exits nonzero or staging content cannot be read |
| Storage failure | The transaction cannot commit |

Diagnostics SHOULD identify the failed statement, the primary Symbol, and
actionable related nodes or paths. Diagnostics MUST go to standard error in CLI
mode so `OUTPUT` standard output remains clean.

## 9. Complete example

```promptr
FRAGMENT Notice;
FRAGMENT Comment;
FRAGMENT IndustryPractice;
FRAGMENT AcademicResearch;

BestPractice: [IndustryPractice, AcademicResearch];
Coding: [Notice, Comment, BestPractice];

LIST FRAGMENTS;
PRINT Coding;
SEARCH "compatibility" FROM CONTENT;
FIND "research" ON Coding FROM MIXED;

METADATA Coding DESCRIPTION "Reusable coding prompt";
METADATA Coding TAGS ["coding", "systems"];

RENAME Comment TO Documentation;
Coding: [Notice, Documentation, BestPractice];

OUTPUT Coding;
```

The following fails without modifying the graph because it would introduce a
cycle:

```promptr
BestPractice: [Coding];
```

The following also fails while `Coding` refers to `Notice`:

```promptr
DELETE Notice;
```

## 10. Explicit non-features in v0.1

The following are deliberately outside the language core:

- namespaces and qualified names;
- aliases;
- anonymous composition;
- `SAVE`, `DROP`, and unsaved working sets;
- forced or cascading deletion;
- import/export syntax;
- persistent `OUTPUT` roots;
- arbitrary key-value metadata beyond `description` and `tags`;
- shell execution in DSL scripts or `eval`;
- semantic-search dependencies;
- plugins and remote synchronization;
- alternate rendering formats.

These omissions are semantic boundaries, not promises that the corresponding
product capabilities can never exist. Future features must preserve the v0.1
identity, graph, and rendering invariants unless introduced through an explicit
versioned language change.

## 11. Compatibility rule

Once released, valid v0.1 programs and their committed effects MUST retain
their meaning. New syntax may be added only when it does not reinterpret an
existing valid statement. Changes to canonical `OUTPUT` bytes require a
versioned rendering mode rather than an implicit behavior change.
