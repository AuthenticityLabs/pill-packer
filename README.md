# Pill Packer

Portable codecs for **PIL (Positional Interchange Layout)** and **PIL PACs**.
Encode typed values by field position, distinguish absent fields from nulls,
and read packed records without expanding every field into an object.

[Format & Specification](https://trietrunk.com/PIL)

## Libraries

| Library | Location | Available APIs |
| --- | --- | --- |
| Rust | `rust/` | PIL encoding/decoding, borrowed scans, PAC reading, optional gzip and JSON definition import |
| JavaScript / WASM | `npm/`, `rust/wasm/` | Reader and definition bindings backed by Rust |
| Go / C | `go/`, `rust/ffi/`, `include/` | Definition import and frame scanning through the Rust C ABI |

Definitions describe ordered fields, binary types, and string dictionaries.
Their CBOR structure is described by [CDDL](https://github.com/AuthenticityLabs/pill-packer/blob/main/schemas/definition.cddl); runtime
validators enforce semantic rules. The document API preserves unknown annotations.
Applications own routing, storage, delivery, and consistency policies.

This is an early implementation. Packages are not yet published to registries.
JavaScript and Go do not expose the complete Rust encoding API. PAC writing and
common-value selection currently live in Trie-Trunk. Arbitrary CDDL-to-PIL
compilation is not supported.

## Build & Test

Requires Rust 1.88+, Go 1.24+ and a C toolchain for Go bindings, and Node.js 22+
for JavaScript package tooling. Run from a source checkout:

```sh
git clone https://github.com/AuthenticityLabs/pill-packer.git
cd pill-packer

# Rust codec and example
cargo test --locked --all-features --manifest-path rust/Cargo.toml
cargo run --locked --manifest-path rust/Cargo.toml --example roundtrip

# JavaScript / WASM
rustup target add wasm32-unknown-unknown
npm --prefix npm run build
npm --prefix npm test

# Native library and Go wrapper
cargo build --locked --release --manifest-path rust/ffi/Cargo.toml
cargo test --locked --manifest-path rust/ffi/Cargo.toml
go -C go test -race ./...
```

The native build produces static and shared libraries plus the public
[header](include/pill_packer.h). Go links the static library from this checkout;
build it for the host platform before using the wrapper. The Go import path is
`github.com/AuthenticityLabs/pill-packer/go` (package `pillpacker`).

Close Go definition handles explicitly. Calls on one handle are serialized;
independent handles can run concurrently. WASM instances are worker-local.
Definitions must be supplied before their records can be decoded.

## License

[Apache-2.0](LICENSE).
