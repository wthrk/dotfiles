# AGENTS.md

## Critical Planning Gate

When handling a `planning request` in this repository, the only source of truth is `docs/secret-recovery/implementation-guidelines.md`. Follow that document for implementation units, role assignments in the planning/implementation/review phases, review cycles, and implementation policy.

For any `planning request`, check the fixed implementation units section in `docs/secret-recovery/implementation-guidelines.md` before any other document, and reference the predefined implementation units without redefining them.

Do not create, paraphrase, summarize, or replace planning procedures in chat. Do not override this repository-specific source of truth with generic planning habits or default workflows.

## Project Overview

This repository is a Nix flake project for managing dotfiles for macOS user environments. It provides the `dotfiles` CLI, Home Manager / nix-darwin modules, and local flake helpers.

Primary structure:

- Rust workspace: `rust/dotfiles-cli`, `rust/dotfiles-core`, `rust/xtask`, and test crates under `rust/tests/`
- Nix flake and modules: `flake.nix`, `nix/home.nix`, `nix/darwin.nix`, `nix/modules/`
- User configuration: zsh / Neovim settings under `config/`
- Bootstrap entrypoint: `scripts/bootstrap.sh`

For ongoing secret-recovery work, first use `docs/README.md` as the document entrypoint, then use `docs/secret-recovery/tasks.md` as the progress-management entrypoint. Before implementation changes or reviews, read `docs/secret-recovery/implementation-guidelines.md` and apply its fixed implementation units, role assignments, and implementation policy.
Use `docs/README.md` and `docs/secret-recovery/README.md` as document entrypoints, and follow each file's explicit scope (what to write, what not to write, and references) defined in those README files and `docs/docs-governance.md`.

## Translation Synchronization

- `AGENTS_ja.md` must remain semantically aligned with `AGENTS.md`.
- If `AGENTS.md` is edited, update `AGENTS_ja.md` in the same change.
- During review, verify semantic equivalence between both documents.

## Applying Document Instructions

- When executing a prompt, extract the document instructions that apply to the current request before starting work, and continue applying them throughout execution.
- Do not provide proposals, edits, or reports that violate document instructions. If instructions conflict, confirm with the user before starting work.

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

- Keep existing `configs` / `mappings` / `autocmds` structure under `config/nvim/lua/omy/`.
- Prefer local changes over large structural rewrites.

## Security

- Do not commit machine secrets, credentials, API tokens, Docker auth state, SSH private keys, or app session files.
- Do not put machine-specific mutable state in Home Manager modules unless intentionally declaring it.
- Homebrew taps are pinned by flake inputs; do not introduce mutable tap operations unless a design change requires it.

## Commit Rules

- Delegate commit-related tasks to a sub-agent, and only instruct it: "Read `AGENTS.md` and perform commit work."
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
