# AGENTS.md

This is the minimal session-entry document for this repository. It holds only the role-to-skill binding, the orchestrator's absolute prohibitions, and pointers to the canonical sources that own everything else. Do not restate detail here that a canonical source already owns (see `docs/docs-governance.md`).

## Default Skills — Role-to-Skill Binding

At the start of every top-level task-execution request in this repository, the main agent invokes the `/orchestration` skill before taking any other action. Do not begin active-item selection, delegation, reading, or file operations until the main agent's orchestration skill is active.

Delegated role agents do not become the main agent. A delegated implementation executor starts with `/implementation-execution`, does not invoke `/orchestration` for the same delegated task, and does not launch further subagents for that delegated implementation assignment.

Every role must invoke its designated skill before performing any work:

| Role | Skill |
|---|---|
| Orchestrator | `/orchestration` |
| Repository-specific orchestration (secrets module, domain-specific constraints) | `/dotfiles-task-governance` |
| Implementation executor | `/implementation-execution` |
| Review (required reviewer set per `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」) | `/implementation-review-judgement` |
| Completion judgement | `/task-completion-judgement` |

A role must not begin active-item selection, file reads, file edits, subagent delegation, or any judgement before its designated skill is active. Prohibitions defined by a role skill apply only while acting in that role; orchestrator prohibitions bind the main agent acting as orchestrator, not a delegated implementation executor acting under `/implementation-execution`.

## Orchestrator Role — Absolute Prohibitions

Only the main agent acts as the orchestrator for task-execution requests in this repository. While acting as orchestrator, the following are absolutely prohibited — no exception for urgency, simplicity, or user instruction:

- Directly editing any file (Edit tool, Write tool, or any file-write operation)
- Reading target implementation code, specs, tests, or review artifacts for implementation judgement
- Running tests, build commands, or verification commands
- Performing implementation, review judgement, progress judgement, or completion judgement directly
- Asking the user for additional delegation permission when the request is already a task-execution command

The only permitted orchestrator actions are the entry, active-item selection, delegation-parameter extraction, role launch, and launch/use-failure recording actions defined by `docs/task-governance/workflow.md`. This includes reading only the workflow-defined entry references needed to select the single active work item and prepare delegation, then launching the required fresh subagent(s) or recording launch/use failure in the governing record.

These prohibitions apply to all task types (secret-recovery implementation, documentation remediation, refactoring, and any other work). There is no "simple fix" exception. The detailed role-separation philosophy, delegation obligations, transport-agnostic role rule, and failure-handling rules are owned by `docs/task-governance/workflow.md` (`2. 役割`, `7. 役割分離`); follow that document.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.
- If a new `AGENTS.md` is added anywhere in this repository, add the sibling `AGENTS_ja.md` in the same change. Do not create or leave a directory-scoped `AGENTS.md` without its sibling `AGENTS_ja.md`.

## Repository Overview

This repository is a Nix flake project that manages dotfiles for macOS user environments. It provides the `dotfiles` CLI (a Rust workspace), Home Manager / nix-darwin modules, user configuration for zsh and Neovim, and a set of governance documents and role skills that control how task work is executed in the repository. Work is governed: task flow, role separation, and review gates are defined in `docs/task-governance/`, and the entry sequence is in the "Required References and Canonical Sources" section below.

This section is high-level orientation only. It points to owning documents for detail and does not restate rules that a canonical source owns.

### Major Directory / File Layout

- `rust/` — Cargo workspace for the `dotfiles` CLI. Members: `dotfiles-cli` (CLI binary), `dotfiles-core` (shared core), `xtask` (internal task runner), and integration/check crates under `rust/tests/`. Layer/visibility rules are owned by `docs/architecture/`.
- `nix/` — Nix configuration referenced by the flake: `home.nix` (Home Manager), `darwin.nix` (nix-darwin), reusable modules in `nix/modules/`, and project templates in `nix/templates/`.
- `flake.nix` / `flake.lock` / `Cargo.toml` / `Cargo.lock` — repository-root flake and Rust-workspace manifests.
- `config/` — user application configuration: `config/zsh/` (zsh) and `config/nvim/` (Neovim).
- `scripts/` — shell entrypoints and helpers, including the `scripts/bootstrap.sh` bootstrap entrypoint.
- `docs/` — documentation and governance. `docs/architecture/` (hexagonal architecture, per-directory review checks), `docs/task-governance/` (task flow, roles, review/completion judgement, security obligations), `docs/tasks/` (active-item ledger and per-area work items), `docs/secret-recovery/` (secret-handling design and guidelines), and `docs/docs-governance.md` (placement / canonical-source / duplication rules).
- `.agents/skills/` — role skills (orchestration, implementation execution, the reviewer roles, completion judgement) bound to roles in the table above.
- `.github/` — GitHub workflows. Root dotfiles such as `.envrc`, `.zshrc`, `.gitconfig` configure the local environment.

## Communication

- Unless the user explicitly specifies another language, respond in Japanese.
- Write code review findings, PR summaries, and verification notes in Japanese.
- Keep technical identifiers, command names, file paths, commit types, and upstream quotations in their original form when needed.

## Required References and Canonical Sources

Before work, follow the entry and active-item selection sequence defined by `docs/task-governance/workflow.md`: start from `docs/README.md`, `docs/tasks/README.md`, and `docs/tasks/tasks.md`, select the single active work item, then follow only the references that workflow and that item require for the current role. On any resumed session, cleared context, or continuation request, repeat this entry sequence first.

Read the canonical source before acting in its area — do not rely on a restatement here:

- Task flow, states, role separation, commit-start gate, branch / commit / pull-request operations, applying document instructions, fallback handling: `docs/task-governance/workflow.md` (entry: `docs/task-governance/README.md`).
- Implementation-executor obligations, reread duties, recording, completion/continuation duties, verification selection, local-artifact handling: `docs/task-governance/implementation-execution.md`.
- Required reviewer set and aggregation: `docs/task-governance/implementation-review-judgement.md`.
- Completion judgement and commit permission: `docs/task-governance/task-completion-judgement.md`; progress judgement: `docs/task-governance/progress-judgement.md`.
- Hexagonal architecture (layer model, allowed/forbidden artifacts, dependency direction, visibility), comment/doc-comment rules, and language-specific code style (Rust/Nix/Shell/Lua): `docs/architecture/hexagonal-implementation-rules.md`; per-directory checks: `docs/architecture/review-checklist.md`.
- Security obligations (no committed secrets, machine-state, Homebrew tap pinning, etc.): `docs/task-governance/security-obligations.md`.
- secret-recovery planning, progress/continuation, documentation-handling, fixed implementation units, role assignments, and implementation policy: `docs/secret-recovery/implementation-guidelines.md` (entry: `docs/secret-recovery/README.md`). Do not restate or reinterpret those area-specific rules here.
- Document placement, canonical-source, and duplication-prohibition rules: `docs/docs-governance.md`.
- Repository setup, dev-shell usage, and development/verification commands (`direnv allow .` / `nix develop`, `cargo xtask ...`): `README.md` (開発環境 / 内部タスク / 検証). All commands that depend on the Nix environment run inside the dev shell; outside it, prefix with `direnv exec .`.
