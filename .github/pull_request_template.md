# Pull Request

## Summary
<!-- What does this change do and why? -->

## Type of Change
- [ ] Bug fix (game logic, Python bindings, or API correctness)
- [ ] New feature (game mechanics, API extensions, or tooling)
- [ ] Performance improvement (hot path optimization, memory usage, or benchmark improvements)
- [ ] Test coverage (edge cases, property testing, or benchmark scenarios)
- [ ] Documentation (game rules, API usage, or development workflows)
- [ ] Breaking change (API modifications that affect existing usage)

## Changes Made
<!-- Specific technical changes -->

## Testing
- [ ] Rust unit tests pass: `cd rust && cargo test --lib`
- [ ] Python integration tests pass: `pytest python/tests`
- [ ] Added tests for edge cases or new functionality
- [ ] Verified critical game invariants (if applicable)

## Performance Impact
<!-- Required for changes to hot paths, data structures, or Python bindings -->
- [ ] Benchmarked before/after with `cargo bench`
- [ ] Performance impact: <!-- Quantify: e.g., "15% faster", "same", "2% slower but fixes critical bug" -->

### Benchmark Results
```
Before: [relevant benchmark results]
After:  [relevant benchmark results]
```

## Code Quality
- [ ] Rust clippy warnings resolved: `cargo clippy`
- [ ] Python linting passes: `ruff check python/`
- [ ] Uses appropriate error handling (Result types vs unwrap)
- [ ] Follows existing patterns for data structures and performance

## API Consistency
<!-- For changes affecting Python interface or public APIs -->
- [ ] Maintains PettingZoo Parallel environment interface compatibility
- [ ] Observation space format unchanged (or documented if changed)
- [ ] Direction/action mappings work correctly between Rust and Python

## Breaking Changes
<!-- Describe impact on existing PyRat users and migration path -->

## Additional Notes
<!-- Technical details, design decisions, or specific areas for reviewer focus -->