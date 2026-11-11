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
3. **Verification and polish — completed.** Independently review handbook against source
   and tested contracts; validate JSON, CLI flags, links/anchors, setup ordering,
   credential handling and operation coverage. Run required locked audit/notices,
   dependency/strict/actual-service verification through tools/verify.py, and
   reproducible release validation. Use only isolated owned disposable services;
   no paid calls. Fix observed problems, record actual evidence and commit.
4. **Final review and PR — completed.** Review status, complete base diff and all branch
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
- Task 3: all 53 nonpaid Rust tests pass under tools/verify.py --coverage against
  fresh owned context-mcp-ci PostgreSQL/MinIO/Typesense/Keycloak on alternate ports
  57432, 61000/61001, 60108, 60080. Includes actual HTTP/stdio/CLI, issuer, native
  sandbox metadata, storage, application invariants and slow-start regression.
  Format, strict Clippy/rustdoc, advisory/license/source/bans, unchanged per-crate
  coverage floors/budgets, coverage checker and core package rebuild pass.
  Measured lines: core 210/233; storage 916/1001; workflows 726/881; auth 397/421;
  engine 4378/5110; process 199/737; application 805/1282. No thresholds changed.
  Locked workspace build, notices.py --check, audit_release.py and full release.py
  pass. After final prose edits, release.py --skip-build refreshed the deterministic
  source archive; QA verifies it includes the exact guide and excludes temporary
  plans. All owned service containers/volumes removed; preexisting containers left
  running. No paid provider calls or new full-format compatibility claims.
  Final handbook: 2,438 lines/about 14,500 words. QA passes 55 JSON parses, 39
  schema-checked argument examples, 30 shell syntax blocks, 68 local links/anchors,
  all 23 tool rows, all 12 command rows and every subcommand flag against actual help.
  Isolated Compose config/config-generator/profile-generator checks pass with fixture
  env references and existing image inspection only. QA initially assumed macOS
  /var was canonical; corrected that check and clarified canonical staging paths.
  Also clarified matching client/server catalogs and passing argument-only examples
  to the CLI. No application code changed. Next: review complete diff/commits and PR.
- Task 4: fetched origin/main and reviewed all three delivery commits plus the
  complete five-file base diff (handbook, README/deployment navigation, AGENTS.md,
  temporary working plan). Working tree clean before publication; diff check and
  tracked-input audit pass. Pushed docs/human-user-guide with its own remote tracking
  branch and created a new PR against main:
  https://github.com/thomasquintana/context-mcp/pull/2
  At the observed fd971b0 head, hosted quality runs 43109982495 and 43109973623 were
  queued/in progress, not completed. All local verification above passed; hosted
  completion is not claimed. Commit/push this completion-only plan record, then
  confirm clean local/remote state and return the guide, plan and PR locations.

## Current next action

All four tasks are complete. Commit and push the final plan record, verify the
working tree matches origin/docs/human-user-guide and inspect latest PR checks.
Deliver the handbook/plan locations, PR URL and actual local/hosted verification
status. No additional application changes or paid campaigns are needed.
