# Pill Packer

Portable codecs for **PIL (Positional Interchange Layout)** and **PIL PACs**.
Encode typed values by field position, distinguish absent fields from nulls,
and read packed records without expanding every field into an object.

[Format & Specification](https://trietrunk.com/PIL)

## Libraries

| Library | Location | Available APIs |
| --- | --- | --- |
| Rust | `rust/` | PIL encoding/decoding, borrowed scans, PAC reading, optional gzip and JSON definition import |
| JavaScript / WASM | `npm/`, `rust/wasm/` | Rust-backed validation, portable PAC/header readers, frame composition, bounded gzip and column transforms |
| Go / C | `go/`, `rust/ffi/`, `include/` | Rust C ABI bindings for definition import and scanning |
| Go Codec | `go/codec/` | PIL encode/decode and streaming writes, PAC factoring/writing/reading, compression and column transforms |

Definitions describe ordered fields, binary types, and string dictionaries.
Their CBOR structure is described by [CDDL](https://github.com/AuthenticityLabs/pill-packer/blob/main/schemas/definition.cddl); runtime
validators enforce semantic rules. The document API preserves unknown annotations.
Applications own routing, storage, delivery, and consistency policies.

This is an early implementation. Packages are not yet published to registries.
The Go codec is a portable Go implementation; it is not a Rust FFI wrapper.
Rust, Go and JavaScript APIs do not yet have full feature parity. Cross-language
fixtures verify their shared wire formats. Arbitrary CDDL-to-PIL compilation is
not supported.

Pill Packer owns PIL field layouts, adaptive integers, arrays, PAC common-value
factoring, PAC headers, gzip compression/decompression, numeric base residuals,
and byte shuffling. Applications supply schemas and compression settings. Sealing
schedules, storage placement, aggregation semantics, retention, replication and
cache eviction remain application responsibilities. Numeric column transforms
are reusable codec primitives; using them does not make an application-specific
container a PIL PAC.

The Go codec import is `github.com/AuthenticityLabs/pill-packer/go/codec`.
JavaScript helpers are exported as `pill-packer/layout`, `pill-packer/pac`,
`pill-packer/compression`, `pill-packer/gzip`, `pill-packer/columns` and
`pill-packer/cbor`. The package root exposes the Rust/WASM reader.

## Validation and Trusted Reception

Full validation remains the default: Rust `FrameIndex::scan` and `PacIndex::scan`,
and JavaScript `scan` and `pac`, validate field values as well as framing.
`scan_structural` / `scanStructural` and `scan_pac_structural` / `pacStructural`
retain bounds, offsets, masks, type widths, and PAC framing checks while skipping
semantic field validation. Use these when the producer has already validated the
values and the receiver has established transport integrity.

Rust `checksum::crc32c` and JavaScript/WASM `verifyCRC32C(bytes, expected)` provide
CRC32C (Castagnoli) verification. The expected checksum must come from the trusted
transport or storage manifest; CRC32C detects corruption, not authenticity.
Compression retains its own framing and integrity checks.

For a validated member-offset directory, `indexedPACBounds(header, index, stride,
offset)` constructs member ranges without scanning PIL fields. It checks directory
shape and monotonic in-bounds offsets. The application verifies both the PAC and
the directory before using this path, binds them to the same definition and block,
and owns storage publication ordering. These APIs do not change the wire format.

## Fixed-Width PAC Design

Planned, not yet a supported packing mode. A Definition may request
`"pac": {"layout": "fixed"}`. This is a packing contract, separate from scalar
field types. The byte stride belongs to each PAC because common-value factoring
can change member widths. Variable-length PACs remain the default.

An optional `FixedStride` PAC header extension will carry a nonzero U32
little-endian `member_stride`. The extension appears in the decoded semantic
header, inside the compressed body when compression is used. Its type identifier
will be assigned with the implementation. No extra per-record header is needed.

- Address member `i` as `members_start + i × member_stride`; require
  `member_count × member_stride == decoded_size − members_start`.
- Empty PACs have no addressable members; omit stride metadata unless a stride
  is supplied by the packing contract.
- This guarantees record offsets, not equal field offsets within every record.
  Fixed field positions require a stronger presence/layout contract.
- Every member remains a valid PIL frame. No implicit padding, changed field
  states, or new meaning for absent/null values.
- Verify equal encoded lengths after common-value factoring. If factoring breaks
  uniformity, omit factoring; if the original frames still differ, reject a
  fixed-width request. Never silently change it to variable-width packing.
- Fixed-width scalar types alone cannot guarantee fixed-width records: optional
  fields and adaptive integers can change encoded lengths. Variable fields can
  participate only when their encoded frame lengths satisfy the same contract.
- The Definition requests the contract; the PAC records the verified stride.
  Header validation and member validation must agree before exposing random access.
- Direct addressing is into the decoded PAC. Whole-PAC compression still requires
  decompression; compressed-byte seeking requires a separate block design.

Readers without the optional optimization can scan the ordinary PIL frames.
Implementation must cover Go packing, Rust/WASM validation and random access,
JavaScript header parsing, malformed strides, common prefixes and compression.

**TODO:** Explore a tool that accepts CDDL with PIL layout conventions or
annotations and generates a definition-specific WASM encoder/decoder. Preserve
wire compatibility and validation, and benchmark the potential performance gains
against the generalized codec.

## Nested-Object Experiment

The Rust [nested-object example](rust/examples/nested_objects.rs) composes a
purchase, customer, address, and variable-length item list from existing PIL
frames. Their logical types are declared together in
[nested_objects.cddl](rust/examples/nested_objects.cddl), using named CDDL rules.
The layout bindings are handwritten for this experiment; this is not an
arbitrary CDDL importer or a supported public `OBJECT` wire type.

Nested objects occupy `BYTES` fields; object lists use `ARRAY<BYTES>`, with each
body containing another PIL frame. The containing layout supplies the child
layout, without a per-child definition identifier. Event payloads contain no
CBOR. The four existing definition documents still use CBOR. A production
implementation needs portable child-layout bindings and generic recursive
validation before other codecs can interpret these byte fields as objects.

```sh
cargo test --locked --manifest-path rust/Cargo.toml --all-features --example nested_objects
cargo run --locked --release --manifest-path rust/Cargo.toml --all-features --example nested_objects
```

Five tests check semantic round trips against CDDL, Unicode, absent/null/empty
values, empty and long lists, malformed nested frames, required fields, scalar
widths, and zero-allocation borrowed traversal after warming reusable offsets.
The encoder and the test's logical-object projection still allocate; only the
borrowed reader has the zero-allocation guarantee tested here.

A deterministic 10,000-purchase sample (one to five items per purchase) produced:

| Representation | Payload Bytes | Gzip-6 Bytes |
| --- | ---: | ---: |
| Nested PIL frames | 1,262,230 | 179,101 |
| CBOR sequence with named fields | 2,122,480 | 197,720 |
| Compact JSONL | 2,806,680 | 190,934 |

These are identical logical records, with no indexes, PAC factoring, or shared
compression dictionary. The four PIL layout documents add 450 bytes once; the
CDDL source is 251 bytes. PIL reduced raw size substantially, but its compressed
advantage over JSONL was only about 6.2% on this sample. This is a codec experiment,
not a production storage or throughput claim. The example also prints a local
microbenchmark for full nested structural validation and borrowed projection;
CDDL validation is a separate conformance check, outside that timed path.

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
npm --prefix npm install
npm --prefix npm run build
npm --prefix npm test

# Portable Go codec (no C toolchain required)
go -C go test ./codec

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

## Format Reference

**Project:** Version zero; not a frozen interoperability standard.

This is the primary PIL reference. It describes the implemented format unless
explicitly marked **proposed**. Trie-Trunk owns its replication and storage binding.

PIL exchanges structured state under a shared, immutable Definition. Field
positions replace names, dictionaries replace repeated strings, and cumulative
end offsets locate variable values. A frame may carry a complete object or a
partial change; that distinction belongs to the application. PIL does not
prescribe storage, routing, subscriptions, or query execution.

### 1. Definition

The wire Definition is CBOR. [definition.cddl](schemas/definition.cddl) describes its
portable structure and extensible annotations; codec validation additionally
enforces semantic rules. This JSON illustration uses the same data model:

```json
{
  "pil": 0,
  "definition": "payment/transactions",
  "version": 0,
  "fields": [
    { "name": "event_id", "type": "UUID" },
    { "name": "id", "type": "UUID" },
    { "name": "revision_id", "type": "U64" },
    { "name": "state", "type": "STRING", "values": ["pending", "authorized", "canceled"] },
    { "name": "description", "type": "STRING" },
    { "name": "@fields", "type": "CBOR" }
  ]
}
```

- `definition` is a nonempty identity; `version` is an unsigned integer.
- `fields` is a nonempty **ordered array**. Position identifies each field.
  Names must be nonempty and unique; map-key order has no field-order meaning.
- `values`, when supplied, is an ordered dictionary of unique strings for a
  scalar `STRING` field. Array position is the zero-based dictionary ID.
- Optional `adaptive_integers` is `"none"`, `"16"`, or `"32"` (§4).
- The current codec requires `pil: 0` for fixed integers and `pil: 1` for enabled
  adaptive integers. These existing layout selectors are distinct from the
  Definition's `version` and PAC/0. Transport bindings have their own selectors.

Publish Definitions immutably. Changing types, order, fields, dictionary contents,
or integer policy requires a new Definition binding, even for an additive
change. Adding entries to a declared `@fields` value does not change its Definition.
A frame contains no Definition identifier: its enclosing binding must supply the
exact Definition before decoding. A structurally compatible wrong Definition
cannot be detected from a standalone frame alone.

Applications may attach resource/transport/profile annotations. Generic PIL does
not require event IDs, object IDs, revisions, a trie, or a resource binding.
Bindings using a digest retain the exact original Definition document bytes;
reserializing a semantic equivalent need not preserve its digest. JSON imports
produce CBOR once at registration; they do not yet canonicalize differently
ordered but equivalent JSON sources to one digest.

### 2. Types and Byte Conventions

All multibyte numbers are little endian. Signed integers use two's complement;
floats use IEEE-754. UUIDs are the exception: their 16 bytes follow standard UUID
network/textual-hex order. UUIDv7's first six bytes hold its millisecond timestamp;
the generic UUID primitive does not require version 7.

| Type | Value representation |
|---|---|
| `BOOL` | 1 byte: `00` false, `01` true; other values invalid |
| `I8/I16/I32/I64`, `U8/U16/U32/U64` | 1/2/4/8 bytes, subject to §4 |
| `F32`, `F64` | 4/8 bytes |
| `UUID` | 16 bytes |
| `TIMESTAMP_MS`, `TIMESTAMP_US`, `TIMESTAMP_NS` | Signed 8-byte Unix time; Definition supplies the unit |
| `REF16` | Opaque unsigned 2-byte shared-value reference, 0–65,535 |
| `STRING` | UTF-8 bytes, or a DIRECT dictionary ID |
| `BYTES` | Arbitrary bytes |
| `CBOR` | Exactly one complete CBOR data item |
| `ARRAY<T>` | Homogeneous scalar elements; layout in §6 |

There is no bare `TIMESTAMP` type or implicit unit inference. REF16's target and
scope must be supplied by the binding; PIL reserves no REF16 value and does not
assume that the target is a trie leaf. A dictionary ID instead refers to its own
field's Definition dictionary and uses the smallest width covering its largest ID:
1–256 entries → U8; 257–65,536 → U16; up to 2³² → U32; larger → U64.
No dictionary ID is an extension sentinel. Out-of-range IDs are invalid.

### 3. Frames and Field States

```text
[field-state mask]
[integer-width mask, if applicable]
[DIRECT values]
[variable End Offsets]
[contiguous variable values]
```

For N fields, the state mask occupies `ceil(N / 4)` bytes. Each field has two
bits, first field in bits 7–6, then 5–4, 3–2, 1–0. Unused low bits are zero.

| Bits | State | Meaning and bytes |
|---|---|---|
| `00` | ABSENT | No value supplied; no value bytes |
| `01` | NULL | Explicit null; no value bytes |
| `10` | DIRECT | Fixed-width value or dictionary ID in the DIRECT section |
| `11` | OFFSET | One cumulative End Offset and a value in the variable section |

ABSENT and NULL are distinct. Applications decide how absence affects a complete
object or a patch; PIL does not turn NULL into deletion. Ordinary fields currently
permit both states; `@fields` forbids NULL. Required/nullable annotations are not
implemented (§10).

Fixed-width types accept DIRECT, not OFFSET. Unmapped STRING, BYTES, CBOR, and
arrays accept OFFSET, not DIRECT. A mapped STRING accepts either an in-range
DIRECT ID or a literal OFFSET string, including a string present in its dictionary.
The encoder does not automatically replace literal strings with dictionary IDs.

Within each section, values appear in Definition order for fields using that
section. Offsets count bytes from the beginning of the variable section:

```text
start(first variable) = 0
start(next variable)  = previous End Offset
length                = End Offset - start
```

Offsets are nondecreasing; equal offsets permit empty strings/bytes. Values are
contiguous with no padding. The final End Offset is the variable section length.
Frame length is the prefix through the offset table plus that final offset; with
no OFFSET fields, it ends after DIRECT values. There is no standalone frame-length
prefix. Discovering the boundary requires parsing the masks and offset encodings,
not decoding the variable payloads. Consecutive frames can share a Definition.

Example, fixed layout `[state STRING(values=[pending,authorized]), note STRING]`:

```text
state = authorized, note = "OK"
B0 | 01 | 02 00 | 4F 4B
mask direct  end   UTF-8
```

`B0` is `10 11 00 00`: DIRECT, OFFSET, then zero padding. Total: six bytes.

### 4. Adaptive Integers

The Definition controls shortening; declared types and valid ranges remain strict.

| `adaptive_integers` | Eligible scalar fields |
|---|---|
| `none` / omitted | None |
| `16` | I16/U16, I32/U32, I64/U64 |
| `32` | I32/U32, I64/U64 |

Each present DIRECT eligible integer contributes a selector to a **separate**
packed width mask immediately after the state mask. ABSENT/NULL fields contribute
no selector. Selectors are MSB-first in field order, may cross bytes, and have
zero final padding. Full-width values still contribute selectors.

| Declared width | Selector | Stored widths |
|---|---|---|
| 16 bits | 1 bit | `0`=1 byte, `1`=2 bytes |
| 32 bits | 2 bits | `00`=1, `01`=2, `10`=4; `11` invalid |
| 64 bits | 2 bits | `00`=1, `01`=2, `10`=4, `11`=8 |

Use the smallest lossless width. Signed values are shortened two's complement
and sign-extended when read. Reject nonminimal encodings. U8/I8, REF16, dictionary
IDs, timestamps, floats, and array elements never adapt. There is no automatic
per-frame fallback. The selector-byte overhead can erase a sparse record's saving
or grow a full-width record; enable this deliberately per Definition.

### 5. Extended Unsigned Integers

Counts, End Offsets, and PAC variable lengths use this canonical encoding:

| Value | Encoding | Bytes |
|---|---|---:|
| 0…65,534 | U16(value) | 2 |
| 65,535…4,294,967,294 | U16(65,535), U32(value) | 6 |
| 4,294,967,295…2⁶⁴−1 | U16(65,535), U32(4,294,967,295), U64(value) | 14 |

Encoders and decoders require the shortest form. This length encoding is separate
from adaptive scalar integers and dictionary IDs.

### 6. Arrays

Arrays always occupy one OFFSET field. The parent field's boundaries contain the
entire array, including its count and offset table.

```text
Fixed-width T:   [extended count][value₁]...[valueₙ]
Variable T:     [extended count][extended end₁]...[extended endₙ][values...]
```

For STRING/BYTES/CBOR elements, ends are cumulative byte offsets relative to the
**element-value section**, not the array start. The final end must equal that
section's length; fixed-width size must equal `encoded_count_size + count × width`.

Example STRING array: `[3][5][10][19][JasonSmithAlexander]` (conceptual integers).
An empty array is the two-byte count zero alone, distinct from ABSENT and NULL.
Elements cannot be absent or null; `ARRAY<CBOR>` rejects a top-level null element
but permits nulls inside complex items. Native arrays are not nested; CBOR supplies
complex/nested structures. There is no ARRAY<PIL> type: use a sequence or PAC for
Definition-bound PIL records. Native arrays do not require PAC.

### 7. CBOR and Dynamic Fields

Each CBOR range contains exactly one complete item, without trailing bytes.
Ordinary CBOR structure remains opaque to PIL beyond validation.

Field names beginning with `@` are reserved. The only implemented reserved field
is explicitly declared `@fields: CBOR`; omitting it disallows dynamic root fields.
When present it must be an OFFSET CBOR map with unique text keys, none beginning
with `@` or colliding with any defined field. Its entries represent additional
root attributes; `@fields` is not itself an application property.

The extension contract for partial state is: ABSENT leaves the dynamic attribute
set unchanged; a supplied map replaces that set, including `{}` to clear it.
NULL is invalid. The codec validates the representation; applying patches is an
application responsibility. Promoting a dynamic key into a defined field requires
a new Definition and removal of that key from `@fields` for frames using it.

### 8. PAC — Prefixed Array Container

PAC optionally groups ordered PIL records under one exact Definition. A common
frame can factor shared values at **arbitrary field positions**; positions need
not form a contiguous Definition prefix. PAC/0 is implemented in Go, Rust/WASM,
and the browser binding.

#### Decoded Binary Layout

| Offset | Bytes | Value |
|---|---:|---|
| 0 | 4 | U32 decoded container length, including header |
| 4 | 4 | U32 flags: bit 0 = common frame; other bits rejected in decoded PACs |
| 8 | 8 | U64 total decoded PAC length, including header |
| 16 | 8 | U64 header length / first member position |
| 24 | 8 | U64 member count |
| 32 | 32 | Exact Definition digest (SHA-256 of bound Definition bytes) |
| 64 | variable | Extended extension count, then extension entries |
| following | variable | Extended common-frame length, common PIL frame if present |
| header end | variable | Consecutive PIL member frames |

An extension is `[type U32][schema version U16][flags U16][extended length][bytes]`.
Current extension versions are zero; flag bit 0 means required. Types are singleton.
Skip unrecognized optional extensions by length; reject unsupported required ones,
unsupported versions of recognized types, and duplicates. Type 1 carries a UTF-8
source name. Other extension identifiers and their interpretation are owned by
application bindings. The host passes its supported identifiers to the decoder.

Header length includes extensions and common frame. Zero common length means no
prefix and requires the common flag to be clear. Empty PACs are valid. Member
count, self-delimited member lengths, and total PAC length must agree exactly.
Stored and decoded container lengths are bounded by U32. Readers must impose
resource limits before allocation; actual record counts are also bounded by the
decoded extent and member lengths.

#### Common-Field Semantics

- Any non-ABSENT common field applies to every member, including common NULL.
- That position must be ABSENT in every member; overrides are invalid.
- Positions ABSENT in the common frame retain their normal member meaning.
- Compare dictionary values and REF16 targets only under the same exact bindings.
- Group consecutive records without reordering search results to improve packing.

A packed member is structurally a PIL frame but needs its common frame to recover
the original logical object. Readers can borrow common/member field spans without
materializing maps or expanding repeated bytes. Standalone export must restore
inherited fields. A native storage projection may additionally need companion
index/trie data and must declare that requirement through its extension.

PAC is neither a pagination protocol nor proof of cache completeness. It does not
require compression or provide timestamp-base shortening.

#### Compressed Representation

Compression wraps the **entire decoded PAC**, including its header, extensions,
common frame, and records. A compressed PAC has this separate 16-byte header:

| Offset | Bytes | Value |
|---|---:|---|
| 0 | 4 | U32 stored length, including this wrapper |
| 4 | 4 | U32 codec discriminator: `2` = gzip; other compressed values rejected |
| 8 | 4 | U32 decoded PAC length |
| 12 | 4 | U32 record count |
| 16 | remaining | One gzip member containing the complete decoded PAC |

The discriminator shares its position with decoded PAC flags: `0` or `1` means
a decoded PAC; `2` means this compressed representation. Common-frame flags and
all semantic metadata exist only inside the compressed body. No outer routing,
Definition, common-prefix, or placement fields are added.

The library validates stored/decoded bounds, gzip CRC, absence of trailing data,
and agreement between wrapper length/count and the decoded header. Expansion
returns the original decoded PAC bytes exactly. The decoded header replaces the
wrapper; no wrapper fields are copied into or patched onto it. Borrowed PIL
scanners consume that decoded representation. The outer wrapper is not persisted
in uncompressed client storage.

### 9. Validation and Transport Boundaries

Reject invalid masks/padding, type-state combinations, dictionary IDs, integer
selectors, noncanonical lengths, decreasing/out-of-range offsets, invalid UTF-8,
malformed arrays/CBOR, extension collisions, PAC overrides, and inconsistent
bindings/counts/lengths. Check arithmetic and resource limits before allocating
or reading. Distinguish truncated input that can receive more bytes from malformed
values within an already complete bounded frame.

HTTP/JMS negotiation, storage addresses, and routing are application/transport
bindings. A transport must supply the exact definition before dependent data.
HTTP content compression applies to a transport representation; PAC compression
belongs to each container and is decoded by the library. Neither requires a
particular datastore, time partition, event identity, or subscription protocol.
Compression does not imply encryption.

### 10. Proposed, Not Implemented

The core Definition currently has no `header`, `required`, `nullable`, or
application `property` annotation. These were discussed as follows:

- **Message headers:** a contiguous leading group in the ordered Definition,
  covered by the ordinary state mask, with DIRECT bytes before the body.
  Only fixed-width representations; no variable pointers, arrays, or adaptive
  header integers. Mapped STRING headers would require dictionary-only encoding.
- **Header requirements:** optional headers could be ABSENT; `required` would
  require DIRECT even in partial messages. NULL/OFFSET would be invalid.
  Application properties would remain ordinary body fields.
- **Independent body compression:** keep the state mask and header values visible;
  encode the body codec and stored/decoded lengths before compressed body bytes.
  Body adaptive selectors, DIRECT values, offsets, and values would be inside.
  An outer transport could still compress everything.
- **Field validation:** nullable/required application contracts are not enforced
  by the current generic Definition; do not assume annotations change its layout.
- **Time-base packing:** shared epochs and shortened timestamps/UUIDv7 values
  remain deferred. Current PAC does not implement them.

These are design directions, not accepted input features. Unknown annotations
must not be relied on for validation or wire-layout changes. Introducing any of
them requires coordinated codec/binding tests and performance measurements.

### Public Rust APIs

`Definition::new` builds the ordered layout; `DefinitionDocument` retains the full
annotated source. `encode`/`FrameEncoder` write PIL, `decode`/`decode_exact` read it,
and `FrameIndex` scans into reusable spans. `ArrayView` and `CborRef` support
borrowed access. `pac::PacIndex` validates decoded PAC member spans and common
fields without materializing objects. `compression::inflate` is feature-gated.

Development schema checks can also validate consumer profiles:

```sh
cargo run --locked --manifest-path rust/Cargo.toml --example check_definition -- schema.cddl definition.cbor
```

This checks CDDL structure plus PIL layout semantics. It does not execute a
consumer's additional semantic validators. The CDDL tool is a dev dependency.

## License

[Apache-2.0](LICENSE).
