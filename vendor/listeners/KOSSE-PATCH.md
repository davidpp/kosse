# Vendored `listeners` 0.6.1 (patched)

This is the published crates.io release of [`listeners` 0.6.1](https://crates.io/crates/listeners/0.6.1) (MIT, see `LICENSE`), used through `[patch.crates-io]` in the workspace `Cargo.toml`. Two registry artefacts were removed (`.cargo-ok`, the crate's own `Cargo.lock`, which Cargo ignores for a dependency). Everything else is byte-identical except this patch to the macOS backend:

- `src/platform/macos/mod.rs`: `get_all` no longer drops a socket when its process name cannot be read; the name is empty instead.
- `src/platform/macos/proc_name.rs`: names are decoded lossily. macOS truncates process names to 31 bytes, which can cut a multi-byte UTF-8 character; strict decoding then failed and, combined with the above, the listening socket silently disappeared.

Why vendoring rather than another backend, and the reproduction, are recorded in `docs/evidence/S02.md`. Verify the patch against the registry copy with:

```sh
diff -ru ~/.cargo/registry/src/index.crates.io-*/listeners-0.6.1 vendor/listeners
```

Remove this directory and the `[patch.crates-io]` entry once an upstream release stops gating sockets on a decodable name.
