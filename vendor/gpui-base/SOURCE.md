# Source

Package: `gpui-base` 0.9.1 from crates.io.
Repository: `longbridge/gpui-kit`, revision `39ab849a93b5f15507b969164d49891db8d5b470`.
Registry checksum: `f3e9ae5237737cfd110d0ff513fcd9c20f97d98ebabd0a73164068177f0aca4f`.

Local changes expose editor descriptions (`src/input/base/state.rs`), editor errors,
modal dialog state (`src/dialog.rs`), focus-trap owner lookup (`src/focus_trap.rs`,
`src/lib.rs`), readable document text, measured text geometry, selection actions, and
canonical custom object semantics (`src/text/*`). Existing custom text providers retain
their own text runs. The local manifest adds the existing `unicode-bidi` 0.3.18
dependency.
