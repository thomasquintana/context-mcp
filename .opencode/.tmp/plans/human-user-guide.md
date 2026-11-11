# Human user guide working plan

## Alignment protocol

Read this plan after every compaction, before selecting work, after every task,
and whenever general guidance is needed. Update task status, completion evidence,
and follow-ups after every completed task. Commit each completed verified task.
Finish with all intended changes committed, push the new branch, and create a new
pull request. Follow AGENTS.md and the permanent contracts throughout.

Branch: `docs/human-user-guide`.
Base: `origin/main` at `d583760` (merged HTLK upgrade PR #1).

## Goal and constraints

Produce a complete, detailed, task-oriented handbook for human users and operators
in `docs/user-guide.md`, prominently linked from README.md and deployment.md.
Explain concepts before implementation detail. Include a first successful workflow,
plain-English assistant requests, CLI commands, exact tool arguments, annotated
results, reference tables and troubleshooting. Use consistent example IDs and clear
placeholder conventions. Distinguish implementation defaults, reference settings,
hard bounds and operator choices.

Preserve single-instance operation, current source authority, immutable provenance,
generation fences, transitive purge, permanent content-free upload fences, accepted
sandbox receipt/history boundaries and operator-managed replaceable providers.
Do not modify application behavior or sibling projects. Do not invent client config
syntax, public releases, production backup guarantees or provider compatibility.
Credentials remain environment/private-file references. Paid tests are not authorized
by this documentation request. Existing measured evidence is not a newly executed test.

## Tasks

1. **Branch and plan — completed.** Inspect clean status/diff/history and merged PR;
   branch from current origin/main; save this plan and add branch-specific alignment
   instructions to AGENTS.md. Verify diff and tracked-input audit; commit setup.
2. **Write handbook — completed.** Cover introduction/concepts, prerequisites/client
   connections, first workflow, ingestion/uploads/URLs/jobs, query/navigation/history,
   media, memories/reconciliation, synthesis, sharing, purge, full installation and
   operations, troubleshooting and complete reference (23 tools, 12 CLI commands,
   resources, permission/limit/ID tables). Link README/deployment and correct only
   narrowly related contradictions. Validate examples against current implementation;
   record review evidence and commit the completed writing task.
3. **Verification and polish — pending.** Independently review handbook against source
   and tested contracts; validate JSON, CLI flags, links/anchors, setup ordering,
   credential handling and operation coverage. Run required locked audit/notices,
   dependency/strict/actual-service verification through tools/verify.py, and
   reproducible release validation. Use only isolated owned disposable services;
   no paid calls. Fix observed problems, record actual evidence and commit.
4. **Final review and PR — pending.** Review status, complete base diff and all branch
   commits, ensure intended changes committed, push, create a new PR against main
   with actual verification details, and report URL and observed hosted status.

## Research and writing checklist

- Explain hierarchy and distinguish asset revisions, content IDs, memory revisions,
  logical versions, ACL versions and generations.
- Cover existing-server users separately from operators; no resource inventory/login
  tool exists. Explain how starting IDs and credentials are supplied.
- Carry returned IDs between examples; show job polling before retrieval/publication.
- Query returns a ContextMap; synthesis generates an answer. Saved synthesis is not
  a memory revision. All model inputs become saved-answer dependencies.
- Differentiate retry, reprocess, replacement upload and explicit URL refresh.
- ACL replacement is complete; empty explicit grants do not disable inheritance.
- Container setup needs separate processing image/profile, correct UID/GID and
  identical host staging path. Generated container config has OIDC disabled.
- Public output schemas are generic objects; examples must match runtime results.
- Explain purge preconditions, asynchronous completion, transitive history/cross-owner
  deletion, citation-only survivors and HTLK/provider-history exclusions.
- Document reference media sampling/bounds and short-lived signed byte URLs.
- No tested backup/restore runbook or production TLS recipe exists. Explain actual
  boundaries and label operator recommendations accurately.
- All tools, CLI commands, resources, errors and practical limits need a usable
  reference. Link permanent technical documents for deeper contracts/evidence.

## Completion log

- Setup: starting checkout clean on feat/htlk-runtime-upgrade at 8766090. PR #1 is
  merged. Fetched origin/main at d583760 and created docs/human-user-guide from it.
  Saved this plan; branch-specific AGENTS.md alignment keeps it discoverable after
  compaction. No application state or services touched.
- Task 1 verified: git diff --check and python3 tools/audit_release.py pass;
  reviewed status, diff and recent history. Commit only AGENTS.md and this plan.
- Task 2: wrote docs/user-guide.md (20 sections, 2,416 lines, about 14,400 words),
  linked prominently from README.md and deployment.md. Covers all planned user and
  operator workflows, precise lifetime/provenance/deletion boundaries, full tool/CLI
  references, configuration/permissions/limits, troubleshooting and glossary.
  Cross-checked dispatch/schemas, CLI, client, setup generators/Compose, memory,
  synthesis, media and index repair implementations. One-off external QA script
  parses 55 JSON blocks/heredocs, validates 39 argument examples against the exported
  schemas with placeholder UUID substitution, checks 30 shell blocks with bash -n,
  resolves 68 local links/anchors, and confirms all 23 tools/12 CLI subcommands.
  Hash example independently matches SHA-256 of the exact five bytes hello.
  git diff --check passes. No live model calls or application behavior changes.
  Follow-up: final source/CLI review and required isolated nonpaid release gates.

## Current next action

Commit the verified handbook task, reread this plan, then perform task 3. Existing
user containers occupy default service ports; use a separate context-mcp-ci stack
on alternate ports and remove only that owned disposable stack after verification.
Do not run paid model/format campaigns to validate prose. Update this section and
completion evidence after the task.
