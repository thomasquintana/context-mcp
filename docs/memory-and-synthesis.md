# Semantic memory, reconciliation and cited synthesis

## Semantic assertions and immutable views

`context_write_memory` and `context_reconcile_memory` take workspace, subject,
description, a `value` (`{kind:"text",statement}` or
`{kind:"typed",value_type,value}`), expected version, lineage/citations, optional
explicit supersession IDs and an idempotency key. No machine key is required:
normalized human subjects locate a logical memory, or callers may use its returned
memory ID. Expected version zero creates; updates require the known current version.
Semantic comparison applies to assertions in that logical subject/view; unrelated
subjects are not silently coalesced.

Typed values validate JSON primitive/container types. Optional operator
`memory_types` maps semantic type names to those types. Assertions, descriptions,
source lists and current conflicts are independently bounded. Every write publishes
a new immutable revision with author, origin, sources, support, rationale and basis.
Concurrent stale-version writes cannot overwrite each other. Equivalent assertions
retain combined provenance; complementary text/object facts merge without inventing
additional information. Merged content is explicitly generated and depends on prior
contributing revisions.

Models may classify only `equivalent`, `complementary` or `contradiction` through
`reconcile_role`. Unknown classifications or attempts to return a winning author/
authority are contract failures. Different values of the same scalar type are
classified as contradiction by the host. A contradiction becomes a conflicted view
unless:

- The caller explicitly supersedes known current revision IDs under current
  WriteMemory/source authority and an expected version; or
- Operator `memory_authority` gives one author strict precedence. It maps stable
  principal UUIDs to integer ranks. Ties preserve conflict; a lower-ranked incoming
  assertion is retained in history without replacing the higher-ranked view.

Model judgment never invents authority. `context_read_memory` reads a logical memory
ID or workspace/natural subject. History is explicit and paginated. Current reads
check every current contribution; historical bodies and expansions check their own
transitive source protections. Vector fields do not become returned memory bodies.

`derived_from` source references and `derived_content` IDs are informational
dependencies. `citations` support an independently authored assertion without
declaring it source-derived. All newly supplied references require current read
authority. Existing citation-only assertions can survive removal/revocation of their
support; returned citations are then unavailable. A source-derived revision cannot
leak through a shared workspace after its original source access is revoked.

## Explicit cited generation

`context_synthesize` takes workspace, question, optional exact evidence IDs, evidence
token budget and optional `save`/name/idempotency key. Empty evidence triggers bounded
current-view context retrieval. Source text is treated as data by the operator graph;
the service checks every returned citation against the supplied exact evidence IDs.
Invented IDs and empty/unbounded outputs fail before knowledge publication.

Results are generated, cited and transient by default: no answer/content record is
written into permanent application knowledge. Existing HTLK execution records and
provider retention are outside that promise. Transient retries are new computations.

Saving requires explicit `save:true` and a stable key. Saved summaries are indexed
and published only after all current source protections validate, and depend on
**all** model inputs, not merely the subset the model chose to cite. Later expansion
requires every informational dependency. Saving does not transform generated text
into original evidence. An explicit new request is required to regenerate from
surviving sources after a purge.

## Storage/concurrency boundary

Embeddings and model classifications run through the bounded shared HTLK request
queue. A single-instance publication mutex serializes derived physical-write/publication
windows with purge fencing across ownership boundaries. Staged content and derivation
edges are authoritative before index writes; unpublished records cannot be queried
as evidence. Final memory version/view changes and idempotency receipts are atomic.
Pending failed-stage cleanup remains operational state rather than searchable history.

## Verification scope

Twelve nonpaid backing-service application tests pass. New cases verify immutable
memory history, equivalent/complementary merges, preserved scalar conflicts, explicit
supersession, stale concurrent versions, configured author precedence, citation
validation, transient versus saved synthesis, full dependency edges and source
revocation with retained citation-only assertions. These processing/model roles are
pure fixtures; they establish application invariants, not real model reasoning.
Combined live model checks belong to final delivery verification.
