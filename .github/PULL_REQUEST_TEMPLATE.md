## Summary

<!-- What does this PR change, and why? -->

## Checklist

- [ ] `cargo test` passes (incl. WPT)
- [ ] No `unsafe` / does not weaken `forbid(unsafe_code)`
- [ ] Zero-copy / CoW path preserved for ASCII-canonical inputs (if applicable)
- [ ] Hot-path change: Criterion deltas noted below (if applicable)
- [ ] Public API / breaking change called out (pre-release OK, must be explicit)

## Benchmark notes (optional)

```text
<!-- paste relative Criterion deltas -->
```
