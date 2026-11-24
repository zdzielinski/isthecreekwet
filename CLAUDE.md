# General Rules

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

# Rust Specific Rules

## Build & Test Commands
- Build: `cargo build`
- Test: `cargo test`
- Test with output: `cargo test -- --nocapture`
- Lint: `cargo clippy -- -D warnings`
- Format: `cargo fmt`
- Check (fast, no codegen): `cargo check`
- Audit dependencies: `cargo audit`
- Documentation: `cargo doc --open`

## Rust Edition and Toolchain
- Edition: 2021 (always specify in Cargo.toml)
- MSRV: 1.91 (stable, pinned in rust-toolchain.toml)
- Do NOT use nightly features

## Error Handling
- Libraries: use `thiserror` — derive `Error` for all custom error types
- Applications/binaries: use `anyhow` — propagate with `?`, add context with `.context()`
- Never use `.unwrap()` or `.expect()` in library code
- `.expect()` is acceptable in binary main() for setup failures (file not found, etc.)
- Never use `.unwrap()` in tests — use `?` with `#[tokio::test]` or return `Result<(), Box<dyn Error>>`
- Propagate errors with `?` unless there is a documented reason to handle locally

## Clippy Policy
- Treat all warnings as errors: `cargo clippy -- -D warnings`
- Run clippy before committing, not just before merging
- Do not use `#[allow(clippy::...)]` without a comment explaining why

## Code Style
- All public items (structs, enums, functions, traits, modules) must have doc comments (`///`)
- Private items: doc comments are encouraged but not required
- Prefer explicit type annotations on public function signatures
- Do not use wildcard imports (`use foo::*`) except in test modules

## Dependency Policy
- Prefer minimal dependencies — justify each new crate in the PR description
- Pin exact versions for production binaries (`=1.2.3` in Cargo.toml)
- Use `^` (caret) for library crates (allows compatible updates)
- Run `cargo audit` before every release
- Do not add a crate to solve a problem that can be solved with std in under 20 lines

## Testing
- Unit tests: `#[cfg(test)]` module at the bottom of each file
- Integration tests: `tests/` directory
- Test naming: `test_<function_name>_<scenario>` pattern
- Use `cargo test -- --nocapture` when debugging output
- Mock external dependencies using trait objects, not concrete types
- Aim for 80%+ coverage on library crates; focus on edge cases not happy paths

## Memory Safety and Unsafe
- No `unsafe` blocks without a `// SAFETY:` comment explaining the invariants upheld
- If `unsafe` is required, isolate it in a dedicated module (e.g., `src/ffi/` or `src/sys/`)
- Prefer `Arc<Mutex<T>>` over raw pointers for shared state across threads
- Do not use `Rc<RefCell<T>>` in async code (not `Send`)
- If you need `unsafe`, ask before writing it — we may have a safe alternative

## Module Structure
- `src/lib.rs` or `src/main.rs` — crate root, minimal logic
- `src/error.rs` — all custom error types
- `src/types.rs` — shared types and newtypes
- Keep modules focused: one concern per file, not one per type

## When to Use Each Command
- `cargo check` — fastest feedback during iteration, catches type errors and borrow checker issues
- `cargo build` — full build, use before testing or running
- `cargo clippy -- -D warnings` — run after any code change before committing
- `cargo test` — after all code changes; use `-- --nocapture` when debugging test output
- `cargo fmt` — before every commit; enforced in CI

## Error Handling — Library Crate (this project)
- Define all error types in `src/error.rs`
- Use `thiserror::Error` derive macro for all custom errors
- Error variants must have descriptive messages via `#[error("...")]`
- Wrap external crate errors with `#[from]` — do not re-invent conversion
- Return `Result<T, crate::Error>` from all fallible public functions

Example pattern:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration key not found: {key}")]
    KeyNotFound { key: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),
}
```

## Async Runtime: tokio
- Runtime: tokio with `#[tokio::main]` for the binary entry point
- Test runtime: `#[tokio::test]` for all async test functions
- Use `tokio::spawn` for tasks that should run concurrently with the current task
- Use `tokio::select!` for racing futures — not nested `.await` chains
- Use `tokio::sync::Mutex` (not `std::sync::Mutex`) in async contexts
- CPU-bound work: wrap in `tokio::task::spawn_blocking` to avoid blocking the executor
- Do NOT mix `tokio` and `async-std` runtimes in the same binary

Example spawn pattern:
```rust
// Preferred: structured concurrency with JoinSet
let mut set = tokio::task::JoinSet::new();
for item in items {
    set.spawn(process(item));
}
while let Some(result) = set.join_next().await {
    handle(result?)?;
}
```

## Polymorphism Policy
- Prefer generics when: performance matters, trait bounds are simple, single-crate use
- Prefer trait objects (`Box<dyn Trait>`) when: return types need erasure, heterogeneous collections, cross-crate boundaries
- Ask before converting between the two — it affects the public API surface

# Custom Rules

- Ensure that we increase the wrangler.toml CACHE_VERSION variable when we are working on a new branch.
