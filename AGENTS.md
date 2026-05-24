# AGENTS.md

## Orchestrator Role — Absolute Prohibitions

The main agent acts as the orchestrator for all task-execution requests in this repository. While acting as orchestrator, the following are absolutely prohibited — no exception for urgency, simplicity, or user instruction:

- Directly editing any file (using Edit tool, Write tool, or any file-write operation)
- Reading target implementation code, specs, tests, or review artifacts for implementation judgement
- Running tests, build commands, or verification commands
- Performing implementation, review judgement, progress judgement, or completion judgement directly
- Asking the user for additional delegation permission when the user request is already a task-execution command

The only permitted orchestrator actions are:
1. Read `docs/tasks/tasks.md` to select the single active work item
2. Launch fresh subagent(s) for required delegated roles (implementation executor, reviewer, progress-judgement, completion-judgement)
3. Record role launch/use failure in the governing record if launch fails

These prohibitions apply to all task types: secret-recovery implementation, documentation remediation, refactoring, and any other work. There is no "simple fix" exception.

## Critical Planning Gate

When handling a `planning request` for `secret-recovery`, the only source of truth is `docs/secret-recovery/implementation-guidelines.md`. Follow that document for implementation units, role assignments in the planning/implementation/review phases, review cycles, and implementation policy.

For any `secret-recovery planning request`, check the fixed implementation units section in `docs/secret-recovery/implementation-guidelines.md` before any other document, and reference the predefined implementation units without redefining them.

Do not create, paraphrase, summarize, or replace planning procedures in chat. Do not override this repository-specific source of truth with generic planning habits or default workflows.
For secret-recovery planning, implementation, verification, review, and follow-up work, the main agent is orchestration-only when distinct implementation and required review roles can actually be assigned and executed. While proceeding under that orchestration flow, the current actor is strictly orchestration-only and must not directly edit files, run implementation work, or perform review/progress/completion judgement. Secret-recovery review must use multiple reviewer roles when the governing documents require them. If the current execution environment cannot launch or use an assigned role, the current executor must first release completed subagents and attempt a fresh role launch. Launch limits or thread limits never justify reusing a subagent that is already assigned to another task item or role. Launch/use failure never authorizes the current orchestrator/current executor to self-execute any delegated role work. If a fresh launch still cannot be performed, follow the active work item's governing documents for the minimum failure record required to keep the work executable, and do not revert to self-execution or documentation-only progress handling.

## Project Overview

This repository is a Nix flake project for managing dotfiles for macOS user environments. It provides the `dotfiles` CLI, Home Manager / nix-darwin modules, and local flake helpers.

Primary structure:

- Rust workspace: `rust/dotfiles-cli`, `rust/dotfiles-core`, `rust/xtask`, and test crates under `rust/tests/`
- Nix flake and modules: `flake.nix`, `nix/home.nix`, `nix/darwin.nix`, `nix/modules/`
- User configuration: zsh / Neovim settings under `config/`
- Bootstrap entrypoint: `scripts/bootstrap.sh`

At the start of work in this repository, immediately read `docs/README.md`, then read `docs/tasks/README.md` and `docs/tasks/tasks.md` as the single task entrypoint and active-item selection source before any other interpretation. On any resumed session, cleared context, or continuation request, read `docs/tasks/README.md` and `docs/tasks/tasks.md` first, identify the active work item, then follow the references required by that item (including `docs/tasks/<area>/...`) as execution-governing sources for implementation/review/completion. Before implementation changes or reviews for `secret-recovery` work, read `docs/secret-recovery/implementation-guidelines.md` and apply its fixed implementation units, role assignments, and implementation policy.
Task-management interpretation for progress/continuation requests is defined by `docs/docs-governance.md`; progress changes must be reflected in the execution-governing artifacts explicitly required by the active work item (including `docs/tasks/<area>/tasks.md` only when that file is actually referenced by the active item).
Use `docs/README.md` and `docs/secret-recovery/README.md` as document entrypoints, and follow each file's explicit scope (what to write, what not to write, and references) defined in those README files and `docs/docs-governance.md`.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.
- If a new `AGENTS.md` is added anywhere in this repository, add the sibling `AGENTS_ja.md` in the same change.
- Do not create or leave a directory-scoped `AGENTS.md` without its sibling `AGENTS_ja.md`.

## Required References

- Before work in this repository, you must read `docs/README.md`.
- Before task-governance, task-ledger, or progress changes, you must read `docs/task-governance/README.md`.
- Before changes to area task artifacts, you must read `docs/tasks/README.md`.
- Before implementation, verification, or review that can touch sensitive data, you must read `docs/task-governance/security-obligations.md`.
- Before secret-recovery implementation or review work, you must read `docs/secret-recovery/implementation-guidelines.md`.

## Applying Document Instructions

- When executing a prompt, extract the document instructions that apply to the current request before starting work, and continue applying them throughout execution.
- Do not provide proposals, edits, or reports that violate document instructions. If instructions conflict, confirm with the user before starting work.
- For any task-execution request in this repository (including progress/continuation/complete-type commands), execution must start with orchestration: select the active work item from `docs/tasks/tasks.md` and fix delegation boundaries before any implementation/review/completion action.
- After active-item selection, the orchestrator may only do one of the following: launch the required delegated roles, or record launch/use failure in the governing record specified by the active item.
- Before role-launch success/failure handling is finished, the orchestrator must not read target code/spec/tests/review artifacts for implementation judgement, must not run tests, and must not edit files.
- When the user request is already a task-execution command, the orchestrator must not ask the user for additional delegation permission. When a task-execution command is received, the orchestrator must autonomously launch the required subagent roles without waiting for additional user authorization. Delegation is a mandatory action, not an optional one — the absence of an explicit delegation request is not a reason to delay or avoid launching subagents.
- For secret-recovery work, an explicit documentation-fix request targets the named documents directly and must not be reinterpreted as implementation progress.
- For secret-recovery progress/continuation requests, select the active work item from `docs/tasks/tasks.md`, then follow the execution-governing references required by that item under the selected area: treat work items whose primary artifact is executable code as code-first implementation work, and treat work items whose primary artifact is documentation as documentation-primary work that may advance through documentation diffs when required confirmation/review evidence is recorded.
- For secret-recovery progress/continuation requests, treat each selected work item's `対象コードパス` from its required referenced governing materials as the implementation starting point, not an edit boundary. You may follow directly necessary callers, callees, shared types, ports/adapters, and corresponding tests.
- For selected secret-recovery work items, do not treat documentation-only diffs as implementation progress in the target code paths. Until code changes exist in relevant executable paths, do not advance implementation-facing progress artifacts (`確認`, `レビュー`, `実装状態` progression, or implementation-complete interpretation).
- In secret-recovery progress updates, explicitly separate `文書整合` from `実装` and mark `コード差分なし` when no relevant executable-path diff exists.
- For selected secret-recovery work items, if relevant executable-path code diffs do not exist, hard-stop `確認` / `レビュー` as `未着手`, keep `実装状態` at `未実装` or `実装中`, and do not treat those artifacts as implementation-ready or review-ready.
- For selected secret-recovery work items, any forward transition of `実装状態` / `確認` / `レビュー` is invalid unless the same change set also updates prerequisite evidence (relevant code-diff identifier and required confirmation/review artifacts).
- For selected secret-recovery work items, `コード差分なし` records are temporary documentation records only and must never justify any forward transition of `実装状態` / `確認` / `レビュー`.
- For selected secret-recovery work items whose primary artifact is executable code, documentation-only work must remain subordinate and must not be used as the primary response to a progress request.
- For selected secret-recovery work items whose declared primary artifact is documentation, documentation-only progress handling is allowed when required confirmation and review evidence are recorded.
- For selected secret-recovery work items, documentation edits are always allowed when they are directly required to keep implementation artifacts consistent; for documentation-primary work items declared in the ledger, documentation edits are also allowed as the primary deliverable without a separate explicit documentation-fix request.
- The documentation-edit allowances above apply only to delegated execution roles. Repository-wide, while the current actor is orchestration-only, any direct file edits are prohibited.

## Communication

- Unless the user explicitly specifies another language, respond in Japanese.
- Write code review findings, PR summaries, and verification notes in Japanese.
- Keep technical identifiers, command names, file paths, commit types, and upstream quotations in their original form when needed.

## Setup

Perform work in the flake development shell.

```sh
direnv allow .
```

If `direnv` is not enabled:

```sh
nix develop
```

Do not manually edit generated dotfiles outside this repository or machine-specific dotfiles. The source of truth is this repository and the local flake generated under `~/.config/dotfiles`.

Do not write to the developer's real `~/.config/dotfiles` to manually validate local flake generation. Validate new-machine-equivalent behavior and target-path behavior using sandbox verification in this repository, especially runtime tests.

## Branches

- Check the current branch before starting work and decide whether it is appropriate for the request.
- If it is not appropriate, switch to or create a suitable working branch before editing.
- When creating a new working branch, verify that `main` is up to date with upstream and update it if needed before branching.
- Do not assume local `main` is current. Fetch upstream before branching.

## Instruction Compliance

- Before editing or broad verification, identify which instructions apply and implement within those constraints.
- For code changes, verify diffs against code-style rules before completion. Successful formatter/lint/test runs are not a substitute for instruction compliance.
- The agent must not send a final response before the assigned work is actually completed within the allowed scope.
- The agent must not use a short or simple answer to stop progress when assigned work remains incomplete.
- The agent must continue execution until the assigned remediation or work item is complete, unless blocked by a real constraint that must be explicitly surfaced to the user.
- These completion and continuation rules are binding workflow requirements for all future sessions in this repository, not optional guidance.

## Development Commands

Use `xtask` for maintenance operations.

```sh
cargo xtask check
cargo xtask apply
cargo xtask apply home-manager
```

Public CLI execution when needed:

```sh
cargo run --package dotfiles-cli -- init
cargo run --package dotfiles-cli -- switch home
cargo run --package dotfiles-cli -- switch darwin
cargo run --package dotfiles-cli -- switch all
```

## Testing

Choose only verification relevant to changed targets. Do not mechanically run broad checks that do not validate your changes.

For Markdown-only changes, do not run `cargo xtask check` or `cargo xtask check static` unless the change affects generated documentation, command validation described in docs, or the user explicitly requests it. Instead, run `git diff --check`, necessary rendering checks, and changed-link validation.

For changes to code, Nix, shell, workflow, bootstrap, or generated artifacts, run the default verification.

```sh
cargo xtask check
```

Re-run already successful verification only when any of the following applies: diffs changed afterward, final diff is not covered, or the user requests rerun. Read-only checks such as `git status` or `git diff` are not rerun reasons.

Always run verification from the flake dev shell. If outside the dev shell, run via `direnv exec .`.

```sh
direnv exec . cargo test ...
direnv exec . cargo xtask check static
```

Default verification covers static checks only and does not include zsh startup behavior or Tart VM runtime integration.

Targeted checks:

```sh
cargo xtask check static
cargo xtask check zsh
```

For changes affecting zsh configuration, startup behavior, TAB binding, fzf-tab, autosuggestions, syntax highlighting, or PATH handling, run `cargo xtask check zsh`.

For changes affecting bootstrap, first-run behavior, host switching, or cross-machine assumptions, run runtime checks.

```sh
cargo xtask check runtime
```

Run all checks:

```sh
cargo xtask check all
```

Runtime checks require Darwin VM toolchains (Tart/Packer/Ansible) in the dev shell. For static-check details, see `rust/tests/checks/src/static_checks.rs`; for zsh invariants, see `rust/tests/checks/src/zsh.rs`.

## Architecture Constraints

- This repository adopts Hexagonal Architecture. The layer model, per-layer allowed artifacts, forbidden artifacts, and visibility rules are defined in `docs/architecture/hexagonal-implementation-rules.md`.
- Before implementing or reviewing any code under `rust/`, read `docs/architecture/hexagonal-implementation-rules.md` and apply its layer-based rules.
- `adapter` layer files must expose only port trait implementations as `pub`. Any other `pub` item is a layer violation.
- `application` layer files must not import adapter concrete types and must not contain `println!` or stdin reads.
- Layer-based constraints override file-name-specific rules. Resolving a named violation (e.g., V1〜V16) without eliminating the underlying layer violation does not constitute resolution.

## Code Style

- For non-trivial modules, scripts, command entrypoints, and verification-flow definition files, add a file-level comment or language-standard doc comment explaining the role.
- Repository-authored explanatory comments must be written in Japanese. English is allowed only when surrounding context is fixed in English, for upstream quotations, or for external format requirements.
- Comments must describe persistent intent, invariants, constraints, and non-obvious operational context, and must not be mere code paraphrases, personal notes, or vague TODO/FIXME items.
- When comments are needed, concretize one of the following: lifecycle boundaries, external contracts, signal-safety requirements, wire-format rules, security properties, or user-operation constraints.
- In function/type/module doc comments, present the primary contract first, then separate conditions and failure-time contracts afterward.
- When behavior changes, update nearby comments in the same patch and do not leave misleading stale comments.

Rust:

- Workspace edition is Rust 2024.
- Put public CLI logic in `rust/dotfiles-cli`, maintenance commands in `rust/xtask`, and shared helpers in `rust/dotfiles-core`.
- Do not mix responsibilities. Do not overload dispatcher files with terminal I/O, signal policy, crypto helpers, wire formats, and tests.
- Use `anyhow` through the repository's Result alias, and propagate with context instead of panicking.
- Prefer iterators and `collect` for simple transforms/extractions.
- Do not branch on `match collection.len()` when slice patterns, `is_empty`, or domain states can express intent.
- Do not introduce unnecessary `mut`; verify necessity in `git diff`.
- Do not pass closed sets as raw strings; use enums/newtypes.
- Do not introduce `unsafe` in repository-authored Rust.
- Do not use `unwrap` or `expect`, including tests.
- Leave no warnings.

Nix:

- Put user configuration in Home Manager and host/system configuration in nix-darwin.
- Keep the public flake API stable unless an explicit destructive migration is requested.
- Do not embed real usernames, real hostnames, or machine-specific paths in reusable modules.
- Follow the flake formatter used by `cargo xtask check static` for Nix formatting.

Shell/zsh:

- Treat `scripts/bootstrap.sh` as installation-critical; keep it portable and maintain a state that passes `bash -n` syntax validation.
- Keep zsh behavior aligned with assumptions in `rust/tests/checks/src/zsh.rs` (TAB, fzf-tab, autosuggestions, syntax highlighting, PATH exclusion).
- Keep user-local mutable state, such as app-management shell injection and Docker auth, outside this repository.

Lua/Neovim:

- Use `config/nvim/lua/omy/` as the base area, and restructure `configs` / `mappings` / `autocmds` when needed for strict compliance.
- Do not optimize for smallest diff or preserving inherited structure.
- If current structure conflicts with architecture/spec/work definition, redesign to compliant structure, including zero-base rewrites of module/document boundaries when needed.

## Security

- Do not commit machine secrets, credentials, API tokens, Docker auth state, SSH private keys, or app session files.
- Do not put machine-specific mutable state in Home Manager modules unless intentionally declaring it.
- Homebrew taps are pinned by flake inputs; do not introduce mutable tap operations unless a design change requires it.

## Commit Rules

- Delegate commit-related tasks to a sub-agent, and only instruct it: "Read `AGENTS.md` and perform commit work."
- The actor launched by that exact instruction is the terminal commit sub-agent for the current cycle. It must execute commit work directly and must not recursively delegate commit work again.
- Commit work is explicitly forbidden until commit-start conditions are met in governing records: diff identifier present, required review roles recorded, and aggregated review result is pass. Follow `docs/task-governance/workflow.md#6-コミット着手ゲート`, `docs/task-governance/implementation-review-judgement.md#コミット連動規則`, `docs/task-governance/progress-judgement.md#コミット可否との連動`, and `docs/task-governance/task-completion-judgement.md#コミット許可条件`.
- Reviewer no-findings in chat, oral confirmation, or sub-agent messages alone never make commit work eligible.
- For documentation remediation, do not require unrelated coarse-grained progress or duplicate ledger synchronization as a commit gate.
- Use Conventional Commits format: `<type>(<scope>): <description>`.
- Preferred types are `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `build`.
- Write descriptions in Japanese; keep type/scope in ASCII.
- One logical change per commit by default; split when needed.
- Always check `git status` and `git diff` when deciding commit boundaries.
- If skipping or being unable to run verification is significant, state it in the commit body.
- The commit sub-agent must not run verification commands. Verification responsibility belongs to the parent agent.

## Pull Request Rules

- Delegate PR-related tasks to a sub-agent, and only instruct it: "Read `AGENTS.md` and perform PR work."
- Do not push directly to `main`. Merge changes through a feature branch and PR.
- Use branch name format `<type>/<scope>-<short-kebab-description>` and keep it lowercase.
- In PRs, describe permanent changes, rationale, and performed verification; do not include chat history or work logs.
- Explicitly mention user-visible behavior changes, bootstrap changes, module API changes, generated-artifact deletions, and migration steps.
- If changing user-visible commands, bootstrap behavior, module boundaries, or zsh key behavior, update `README.md`.
- If expected checks are skipped, not run, or run outside the dev shell, explicitly note that in the PR.
