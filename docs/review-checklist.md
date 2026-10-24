# Review Checklist

Criteria for reviewing changes before merge, whether by a human or an AI agent.
`AGENTS.md` is the source of truth for constraints; this checklist operationalizes it.
Evaluate only the rules relevant to what actually changed in the diff.

Severity when reporting findings:

- `[must]` — blocks merge: constraint violation, spec deviation, panic path, security issue
- `[should]` — design/readability improvement; does not block
- `[nit]` — style/naming preference; does not block

## Constraints (category: constraint)
- [ ] No agent-specific logic in core code (only the `agent` field value is agent-aware)
- [ ] Terminal CLIs (`wezterm cli`, ...) run only inside `src/terminal/` backends
- [ ] Module direction respected: `ui → app → {state, terminal}`; no reverse `use`
- [ ] paneward writes no state files and sends no input to agent panes
- [ ] Missing state directory / missing terminal backend is an empty state, not an error or panic

## Spec Fidelity (category: fidelity)
- [ ] State parsing matches `docs/spec/state-format.md` (required fields, status values, `version` handling)
- [ ] Unknown fields are ignored; unknown `version` / `status` and broken files are skipped, not fatal
- [ ] Files for panes that no longer exist are hidden
- [ ] Terminal backend behavior matches `docs/spec/` memos (CLI arguments, output parsing)

## Robustness (category: robustness)
- [ ] No `unwrap()` / `expect()` / panicking index outside tests, unless an invariant is documented
- [ ] External command failures (terminal CLI missing, non-zero exit) surface as a message in the UI, never a crash
- [ ] The terminal is restored (raw mode, alternate screen) on every exit path, including errors and panics

## Performance (category: perf)
- [ ] No blocking I/O or subprocess inside the render path; refresh happens on file events or a timer
- [ ] File watching does not busy-loop; idle CPU stays near zero
- [ ] Perf-motivated complexity cites a measurement

## Tests & Docs (category: test/docs)
- [ ] Pure logic (state parsing, sorting, filtering, formatting, backend output parsing) has unit tests
- [ ] Tests use temporary directories, never the real state directory
- [ ] `docs/manual-testing.md` updated if terminal-dependent behavior changed
- [ ] README updated for user-facing changes (commands, keys, layouts)

## General Rust Quality (category: style)
- [ ] Guard clauses / `let ... else` over nested `if`; small functions
- [ ] Names state intent; boolean names read as predicates (`is_live`, `has_session`)
- [ ] Items private by default; no needless `pub`
- [ ] No speculative code: no unused abstractions, extension points, or "for later" options
- [ ] `cargo fmt`, `cargo clippy -- -D warnings` and `cargo deny check` clean
