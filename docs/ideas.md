# Ideas

## `promote!` macro for library API

Declarative macro for building pipelines when using cargo-promote as a
library dependency. Reduces boilerplate struct construction for
downstream consumers.

```rust
// Instead of verbose struct nesting:
let pipeline = promote!(dev, staging => confirm, prod => confirm);
```

Scope: `src/macros.rs` or inline in `src/lib.rs`. Only worth adding
when there's a real downstream consumer.
