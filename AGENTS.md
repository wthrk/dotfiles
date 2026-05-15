# AGENTS.md

## Project Overview

This repository is a Nix flake managed dotfiles project for macOS user environments. It provides the `dotfiles` CLI, Home Manager and nix-darwin modules, and local flake helpers.

Main areas:

- Rust workspace: `rust/dotfiles-cli`, `rust/dotfiles-core`, `rust/xtask`, and validation crates under `rust/tests/`.
- Nix flake and modules: `flake.nix`, `nix/home.nix`, `nix/darwin.nix`, and `nix/modules/`.
- User configuration: zsh and Neovim config under `config/`.
- Bootstrap entry point: `scripts/bootstrap.sh`.

When continuing secret recovery work, use `docs/secret-recovery/tasks.md` and GitHub issue `#11` as the progress-management entry point.

## Translation Sync

- `AGENTS_ja.md` must be an accurate Japanese translation of `AGENTS.md`.
- When editing `AGENTS.md`, update `AGENTS_ja.md` in the same change.
- During review, verify that `AGENTS_ja.md` remains semantically equivalent to `AGENTS.md`.

## Communication

- Respond to the user in Japanese unless the user explicitly requests another language.
- Write code review findings, PR summaries, and validation notes in Japanese.
- Keep technical identifiers, command names, file paths, commit types, and quoted upstream text in their original language when clearer.

## Setup

Use the flake dev shell for repository work:

```sh
direnv allow .
```

If `direnv` is not active:

```sh
nix develop
```

Do not hand-edit generated or machine-local dotfiles outside the repository. The source of truth is this repository plus the generated local flake under `~/.config/dotfiles`.

Do not manually test local flake generation by writing to the developer's real `~/.config/dotfiles`. Validate local flake generation through the repository's sandboxed checks, especially the runtime tests for fresh-machine and target-path behavior.

## Branching

- Before starting work, always inspect the current branch and decide whether it is appropriate for the requested change.
- If the current branch is not appropriate, create or switch to an appropriate work branch before editing files.
- Whenever creating a new work branch, first confirm that `main` is up to date with its upstream, update it if needed, and branch from the latest `main`.
- Do not assume the local `main` is current. Fetch the upstream state before using `main` as a branch base.

## Instruction Compliance

- Before editing files or running broad validation, identify the project instructions that apply to the requested work and keep the implementation within those constraints.
- For code changes, review the changed constructs against the Code Style rules before finalizing. Do not treat formatter, lint, or test success as a substitute for instruction compliance.

## Development Commands

Use `xtask` for repository maintenance:

```sh
cargo xtask check
cargo xtask apply
cargo xtask apply home-manager
```

Run the public CLI from the workspace when needed:

```sh
cargo run --package dotfiles-cli -- init
cargo run --package dotfiles-cli -- switch home
cargo run --package dotfiles-cli -- switch darwin
cargo run --package dotfiles-cli -- switch all
```

## Testing

Choose validation that is relevant to the files and behavior changed. Do not run broad repository checks mechanically when they do not exercise the change.

For Markdown-only documentation changes, do not run `cargo xtask check` or `cargo xtask check static` unless the change also affects generated docs, documented commands that need verification, or the user explicitly asks for it. Use targeted checks such as `git diff --check`, reviewing rendered Markdown when useful, and verifying links or referenced files when they changed.

For code, Nix, shell, workflow, bootstrap, or generated-file changes, run the default validation suite before finishing normal changes:

```sh
cargo xtask check
```

Do not rerun an already-passing validation command unless the working tree changed after that validation, the earlier command did not cover the final diff, or the user explicitly asks for a rerun. Read-only inspection such as `git status`, `git diff`, log review, PR metadata checks, or commit/push preparation is not a reason to rerun validation.

Always run validation commands from the flake dev shell. If the current shell is not already the flake dev shell, invoke validation through `direnv exec .`, for example `direnv exec . cargo test ...` or `direnv exec . cargo xtask check static`. Do not run validation commands directly from the ambient shell and then treat the result as repository validation.

Default checks run static validation only. They do not run zsh startup/behavior checks or Tart VM runtime integration.

Focused checks:

```sh
cargo xtask check static
cargo xtask check zsh
```

Run the focused zsh check when a change affects zsh configuration, shell startup behavior, TAB bindings, fzf-tab, autosuggestions, syntax highlighting, or PATH handling.

Use runtime checks when a change affects bootstrap, first-run behavior, host switching, or cross-machine assumptions:

```sh
cargo xtask check runtime
```

Run all checks, including runtime integration:

```sh
cargo xtask check all
```

Runtime checks require Darwin VM tooling from the dev shell, including Tart/Packer/Ansible. Static check details live in `rust/tests/checks/src/static_checks.rs`; zsh invariants live in `rust/tests/checks/src/zsh.rs`.

## Code Style

Comments:

- Files that define non-trivial modules, scripts, command entry points, or validation flows must have a file-level comment or language-native documentation comment explaining the file's role.
- Write repository-authored explanatory comments in Japanese, matching the existing Rust, Nix, and shell comment style. Use English only when the surrounding file is already English, the text is copied from upstream, or an external format requires it.
- Comments must explain durable project intent, invariants, constraints, or non-obvious operational context. Do not restate the code, write personal work notes, or leave vague TODO/FIXME comments.
- Suppress low-value comments before they enter the patch. A comment is low-value if it only says that a helper "does X", repeats the function name, describes ordinary control flow, or uses vague phrases such as "normal error path", "safe path", "cleanup", "properly", "handle", or "temporary" without naming the concrete invariant being protected.
- When a comment is needed, write the invariant directly: name the lifecycle boundary, external contract, signal-safety requirement, wire-format rule, security property, or user interaction constraint that the code must preserve. If the comment cannot identify one of those, rewrite it until it can.
- Before finalizing a patch that adds or edits comments, review every added `+//`, `+///`, `+#`, or equivalent comment line in `git diff`. Remove or rewrite any comment that fails the rules above before running validation.
- When changing behavior, update nearby comments in the same patch. Delete misleading comments instead of preserving stale history inline.
- Public command flows and non-obvious private helpers must document concrete operation timing, required inputs, and interaction boundaries in language-native documentation comments. Do not leave these details implied only by code, prompts, or tests.
- When code implements an externally documented wire format, lifecycle step, or operational constraint, keep the code comment focused on the document-backed invariant and update the document instead of encoding the full procedure only in comments.

Rust:

- Workspace edition is Rust 2024.
- Keep public CLI logic in `rust/dotfiles-cli`, repository maintenance commands in `rust/xtask`, and shared helpers in `rust/dotfiles-core`.
- Keep module boundaries aligned with responsibility. Do not let a command dispatcher file accumulate terminal IO, process/signal policy, platform adapters, wire-format parsing, cryptographic helpers, and tests for all of them. Split mixed files into focused sibling modules before adding more behavior.
- During review, flag responsibility mixing explicitly when a file owns unrelated concerns or when a patch makes an existing mixed file worse. A useful review comment must name the concerns that should move and the target module boundary.
- Use `anyhow` through repository result aliases; propagate context instead of panicking.
- Prefer iterator adapters and `collect` over creating a mutable list and pushing in a loop when the loop only filters or transforms items.
- Do not branch with `match collection.len()`. Use slice patterns, `is_empty`, or domain-specific state instead.
- Prefer immutable, declarative construction. Introduce `mut` only when an API requires mutation or when in-place mutation is materially clearer than expression-based construction. Before finalizing Rust changes, inspect added `let mut` and `mut` parameters in `git diff`; keep only the ones required by a mutable API call or by unavoidable in-place state.
- Do not pass roles, states, modes, kinds, or other closed sets as raw strings. Model them as enums or newtypes, and use serde/display conversion only at IO boundaries.
- Do not write `unsafe` blocks or unsafe functions in repository-authored Rust. Use safe standard-library APIs or safe crates instead, including for signal handling, file descriptors, FFI-adjacent behavior, and platform integration. Before finalizing Rust changes, verify that `git diff` adds no `unsafe` token.
- Do not use `unwrap` or `expect`, including in tests. Return `Result` from tests and use `?`, or assert on explicit error/success conditions.
- Keep warnings clean. The check suite treats warnings as errors.

Nix:

- Keep user configuration in Home Manager modules and host/system configuration in nix-darwin modules.
- Preserve the public flake API unless the requested change explicitly includes a breaking migration.
- Do not introduce concrete user names, host names, or machine-specific paths into reusable modules. These must flow through `dotfiles.user`, `dotfiles.host`, or generated local flakes.
- Format Nix through the flake formatter used by `cargo xtask check static`.

Shell/zsh:

- Treat `scripts/bootstrap.sh` as install-critical. Keep it portable and syntax-checkable with `bash -n`.
- Keep zsh behavior compatible with `rust/tests/checks/src/zsh.rs`, especially TAB bindings, fzf-tab placement, autosuggestions, syntax highlighting, and PATH exclusions for legacy language managers.
- User-local runtime state such as app-managed shell injection and Docker authentication belongs outside this repo.

Lua/Neovim:

- Keep configuration under `config/nvim/lua/omy/` organized by the existing `configs`, `mappings`, and `autocmds` structure.
- Prefer small module-local changes over reshaping the Neovim layout.

## Security

- Do not commit machine secrets, credentials, API tokens, Docker auth state, SSH private keys, or app session files.
- Keep mutable per-machine state out of Home Manager modules unless it is intentionally declarative.
- Homebrew taps are pinned through flake inputs; avoid mutable tap behavior unless the repository design changes explicitly.

## Commit Guidelines

- Commit-related work must be delegated to a sub-agent with only this instruction: read `AGENTS.md` and handle the commit work. Do not summarize rules or repository state for the sub-agent.
- Use Conventional Commits: `<type>(<scope>): <description>`.
- Use common types such as `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, and `build`.
- Write the description in Japanese. Keep the type and optional scope in ASCII, for example `docs: エージェント向け作業規約を追加`.
- Do not write commit messages as work logs, agent notes, or chat summaries.
- Keep each commit focused on one logical change. Split commits when changes can be reviewed, reverted, or explained independently.
- Separate behavior changes from mechanical formatting, documentation updates from code changes, generated output from source changes, and refactors from functional fixes.
- Keep changes together only when splitting would leave an intermediate commit broken, misleading, or without required tests/docs.
- PR grouping and commit grouping are separate decisions. One PR does not imply one commit unless the user explicitly requests it.
- When deciding commit boundaries or messages, inspect `git status` and `git diff`. Do not rely on memory, chat context, or assumptions.
- Mention validation in the commit body when it matters, especially when runtime checks were skipped or could not be run.
- Commit sub-agents must not run validation commands. Validation is the parent agent's responsibility before commit delegation. The commit sub-agent may inspect `git status`, `git diff`, and existing file contents to decide commit boundaries and messages, then create the commit from the current working tree.

## Pull Request Guidelines

- PR-related work must be delegated to a sub-agent with only this instruction: read `AGENTS.md` and handle the PR work. Do not summarize rules or repository state for the sub-agent.
- Do not push directly to `main`. All repository changes must go through a feature branch and PR.
- Name branches as `<type>/<scope>-<short-kebab-description>`, using the same type vocabulary as Conventional Commits. Keep names lowercase and free of personal notes or chat context.
- PRs must describe the durable repository change, why it is needed, and the validation performed. Do not describe chat history or agent workflow.
- Keep PRs focused. A single PR may contain multiple logical commits; summarize that structure when it helps review.
- When deciding PR scope, title, or description, inspect `git status`, `git diff`, and staged changes if applicable. Do not infer PR content from conversation alone.
- Call out user-visible behavior changes, bootstrap changes, module API changes, generated output removal, and migration steps explicitly.
- Include screenshots or command output summaries only when they clarify review. Do not paste noisy logs.
- Update `README.md` when changing user-visible commands, bootstrap behavior, module boundaries, or zsh key behavior.
- If any expected check is skipped, blocked, or run outside the dev shell, state that clearly in the PR.
