# Source and manifest policy

Source repository: <https://github.com/longbridge/gpui-kit>

Source release: `gpui-kit` 0.11.0, tag `v0.11.0`,
revision `e97fe438129c5e4ccab504f2e6fafd9a4e396bba`

The complete `crates/kit` directory is copied without implementation changes.
`Cargo.toml.orig` preserves the original manifest. `SOURCE.json` records SHA-256
hashes for every original file, the source workspace manifest, both retained
licenses, and the generated local manifest. `LICENSE-APACHE` and
`LICENSE-DOCS.md` come from the repository root.

The local `Cargo.toml` expands the source workspace's edition, Clippy lints and
dependency declarations. Source workspace siblings (`gpui-base`,
`gpui-component`, `gpui-kit-assets`) are the same 0.11.0 release on crates.io, as in
the published `gpui-kit` 0.11.0. Moving to another release means refreshing the
facade copy and this tag together.

The behavioral manifest changes are limited to platform feature selection:

- Native non-mobile platform dependencies retain upstream defaults and features.
- That target predicate explicitly excludes WASM, preventing its native default
  features from also applying to a browser build.
- The WASM target explicitly declares both the platform and Web dependencies,
  with `default-features = false`. The same applies to optional GPUI Fast
  platform/Web dependencies. All other inherited feature requests are retained.
- The new, non-default `multithreaded-web` feature forwards `multithreaded` to
  the ordinary platform and Web crates, and weakly forwards it to Fast crates
  only when Fast is selected. It preserves an explicit route to upstream's
  worker/atomics functionality and its existing toolchain/runtime requirements.
  No toolchain override or bootstrap environment variable is supplied here.

`MANIFEST.diff` shows the complete difference from the original, including
workspace normalization. Reproduce with Python 3.11 or newer and an unmodified
checkout at the release tag:

```sh
python3.13 vendor/gpui-kit/normalize-manifest.py /path/to/gpui-kit-checkout
```

The script performs no Cargo invocation, network access, or compilation.
