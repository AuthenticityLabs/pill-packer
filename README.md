# Pill Packer

Portable codecs for **PIL (Positional Interchange Layout)** and **PAC (Packed Array Container)**.
Encode typed values by field position, distinguish absent fields from nulls,
and read packed records without expanding every field into an object.

[Format & Specification](https://trietrunk.com/PIL)

## Pack API

Pill Packer owns portable encoding, packing, compression, and validation.
Applications own grouping, storage, retention, and routing. The Go codec and
`pill-packer/pack` JavaScript module implement the byte-oriented pack API;
Rust/WASM provides validation, scanning, and compression primitives. This is
not yet a uniform high-level API across all bindings.

| Operation | Behavior |
| --- | --- |
| Define | Prepare a PIL `.rx` Definition for reuse |
| Fill | Compose a validated PIL record from ordered fields |
| Pack | Group records, optionally factor common fields, and choose addressing |
| Open / Get | Open a container and recover a standalone member by ordinal or exact member offset |
| Compress / Decompress | Explicit whole-PAC gzip wrapper and restoration |
| Unpack | Restore standalone records and named application headers |
| Push / Pop | Rebuild a pack with a tail record added or removed |

Named headers carry application-supplied byte values. The codec interprets only
its structural controls. Compression is explicit; returning an uncompressed pack
when compression expands it requires `onlyIfSmaller`. Whole-pack compression
must be undone before indexed member access.

Three addressing forms are supported by the current format:

- No directory: member boundaries are found by scanning self-delimiting frames.
- Embedded directory: one member-relative start offset per record, with a
  caller-selected and checked width of 2, 3, or 4 bytes.
- Fixed width: the pack declares a verified encoded record width and member
  count. Every member must actually have that width after common factoring.

Fixed-capacity strings do not by themselves imply fixed-width records: omitted
fields and other variable fields still affect encoded lengths. Caller-provided
offsets are checked against the actual packed members. A directory occupies
`recordCount × addressBytes`; it is additional storage, not a free replacement
for an application's index. Application-managed external offsets remain possible
without requiring the generic library to own a sidecar file.

JavaScript example (given a prepared `codec`, `.rx` bytes, and PIL records):

```js
import {define, pack, compress, decompress} from 'pill-packer/pack';

const definition = await define(rxBytes, codec);
const unpacked = pack(definition, records, {
  index: {addressBytes: 4},
  packing: {commonValues: true},
  headers: {source: new TextEncoder().encode('example')},
});
const packed = compress(unpacked, {algorithm: 'gzip', level: 1});
const restored = await decompress(packed);
const first = restored.get(0);
```

`member()` borrows bytes from a JavaScript UPack; `get()` returns an owned record
with inherited common fields restored. An unindexed JavaScript lookup scans
preceding members. Go's parsed PAC and Rust's validating `PacIndex` expose member
tables built during parsing; do not describe every binding's open as zero-scan.
Tail changes rebuild/revalidate the pack and may change factoring and offsets.
Published packs referenced by application indexes must remain immutable.

## Verification and Remaining Work

Pack tests live in `npm/pack.test.mjs`, `go/codec/pack_test.go`, and
`rust/tests/pac.rs`; cross-binding fixtures cover shared representations.
Measure parsing, validation, member access, restoration, and compression
separately, including copies and directory bytes. Historical application
benchmarks do not prove that one addressing mode is universally faster.

Remaining work includes high-level API parity across bindings, expansion of the
supported CDDL subset, and application integration measurements. Definition-specific
compiled WASM codecs remain future work: accept CDDL and generate specialized
codecs, then compare them with the general interpreter before adopting them.

## Current Libraries

| Library | Location | Available APIs |
| --- | --- | --- |
| Rust | `rust/` | PIL encoding/decoding, borrowed scans, PAC reading, optional gzip and JSON definition import |
| JavaScript / WASM | `npm/`, `rust/wasm/` | Rust-backed validation, portable PAC/header readers, frame composition, bounded gzip and column transforms |
| Go / C | `go/`, `rust/ffi/`, `include/` | Rust C ABI bindings for definition import and scanning |
| Go Codec | `go/codec/` | PIL encode/decode and streaming writes, PAC factoring/writing/reading, compression and column transforms |

Definitions describe ordered fields, binary types, and string dictionaries.
They are stored as binary PIL files with the `.rx` extension, including their
structured CDDL schema annotations. Their structure is described by [CDDL](https://github.com/AuthenticityLabs/pill-packer/blob/main/schemas/definition.cddl); runtime
validators enforce semantic rules. The document API preserves unknown annotations.
Applications own routing, storage, delivery, and consistency policies.

This is an early implementation. Packages are not yet published to registries.
The Go scalar codec is implemented in Go; its compiled nested-schema support
uses the shared Rust runtime through the native bindings.
Rust, Go and JavaScript APIs do not yet have full feature parity. Cross-language
fixtures verify their shared wire formats. The optional Rust `schema` feature
compiles a supported CDDL subset into PIL layouts and a JSON-editable schema.
Unsupported constructs fail explicitly; full CDDL support is not claimed.

Pill Packer owns PIL field layouts, adaptive integers, arrays, PAC common-value
factoring, PAC headers, gzip compression/decompression, numeric base residuals,
and byte shuffling. Applications supply schemas and choose whether and when to
compress. Version zero defines gzip as its only PAC compression codec; callers
can select gzip effort through the compression API. Adding other PAC codecs
requires a versioned discriminator and implementations in each binding. Sealing
schedules, storage placement, aggregation, retention, replication, and eviction
remain application responsibilities. Numeric column transforms are reusable
codec primitives, while their use in an enclosing container is defined by that
container's own format.

The Go codec import is `github.com/AuthenticityLabs/pill-packer/go/codec`.
JavaScript helpers are exported as `pill-packer/layout`, `pill-packer/pac`,
`pill-packer/gzip`, `pill-packer/columns`, `pill-packer/object`, `pill-packer/pack`, and
`pill-packer/cbor`. The package root exposes the Rust/WASM reader.

## Command-Line Reader

Build and install the `rx` executable:

```sh
cargo install --path rust --locked --features cli --bin rx
rx read definition.rx records.pil
rx read definition.rx records.pac
rx read definition.rx records.pac.gz --pretty
rx read definition.rx
rx definition.rx --pretty
rx write definition.json definition.rx
```

`.rx` names a definition document; `.pil` names a record stream and `.pac`
names a record pack. Every `.rx` document, including the structured CDDL schema
inside its `cddl` annotation, is encoded entirely in PIL. JSON and `.cddl` files
are editable authoring inputs. There is no JSON or CDDL text envelope in `.rx`.

Records print as JSONL by default. `--pretty` prints each record as indented
JSON; omit the data path to print the Definition itself. Absent fields are
omitted and explicit nulls remain null. Bytes are JSON octet arrays, UUIDs are
dashed hexadecimal strings, timestamps retain their declared integer units,
and integer output preserves the full signed/unsigned 64-bit range. PAC common
fields are merged before rendering. The reader validates definition digests,
required extensions, field values, framing, compression bounds and checksums.
It accepts adjacent PACs, Go-compressed PACs, and external gzip streams. `-`
reads records from stdin or writes binary definition bytes to stdout.

`rx write` accepts a JSON layout Definition (including application annotations)
or the JSON-editable CDDL schema model. It writes binary PIL; `--pretty` applies
to JSON read output. To compile CDDL source, name its root rule explicitly:

```sh
rx write purchase.cddl purchase.rx --root purchase
rx read purchase.rx --pretty > purchase.json
# Edit the schema JSON, then compile it back to PIL:
rx write purchase.json purchase.rx
```

The CDDL compiler supports named nonrecursive rules, closed text-keyed records,
required/optional members, homogeneous arrays with occurrence bounds, choices,
literals, uint, bool, float64, text, bytes, null, bounded 64-bit integer ranges,
and supported `.size` constraints. It rejects recursive rules, dynamic maps,
generics, group rules, tuple arrays, unbounded `int`/`nint`, and unsupported
controls. JSON byte values are octet arrays. The PIL document contains an ordinary
root Definition plus a `cddl` annotation with the editable schema. Rust, Go, and
WASM load the same compiled schema runtime and validate nested layouts. The
layouts are derived together; full schema checks run during encoding and
ordinary decoding, while `decode_trusted_json` performs structural decoding
only.

A schema's `fixed_strings` map binds qualified field paths (for example,
`purchase.customer.name`) to byte capacities. Bindings must cover a bounded
CDDL text size. `STRING(n)` uses a 1/2/4-byte little-endian length prefix for
capacities up to 255/65,535/U32 maximum, followed by exactly `n` UTF-8 bytes
with zero padding. Shorter strings, trailing spaces and embedded NULs survive;
overflow, malformed UTF-8 and nonzero padding are errors. This storage choice
does not change the CDDL size constraint.

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

## Current Variable-Length Records

PIL records and PAC members remain variable length. A Definition specifies field
types, ordering, dictionaries and schema constraints; it does not imply that
all fields occur in every record. ABSENT and NULL retain their distinct meanings,
and absent fields do not receive fabricated values or reserved payload slots.
Fixed-width scalar types and fixed-capacity strings remain supported when
present, without imposing a fixed record stride.

CDDL schema validation uses the shared runtime across language bindings.
Application-specific validation remains the responsibility of the consuming
system and is not duplicated in the CDDL compiler.

## Nested-Object Experiment

The Rust [nested-object example](rust/examples/nested_objects.rs) composes a
purchase, customer, address, and variable-length item list from existing PIL
frames. Their logical types are declared together in
[nested_objects.cddl](rust/examples/nested_objects.cddl), using named CDDL rules.
This original experiment uses handwritten layout bindings. The `schema` feature
now compiles the supported CDDL subset; `OBJECT` separately carries open values
using the built-in native PIL value layout.

Nested objects occupy `BYTES` fields; object lists use `ARRAY<BYTES>`, with each
body containing another PIL frame. The containing layout supplies the child
layout, without a per-child definition identifier. Record payloads contain no
CBOR. Definitions now also use native PIL. The CDDL compiler supplies portable
child-layout bindings and generic recursive
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
compression dictionary. The original benchmark used 450 bytes for four layout
documents; that older Definition encoding is not the current PIL wire size. The
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

# Native library, Go codec, and Go wrapper
cargo build --locked --release --manifest-path rust/ffi/Cargo.toml
cargo test --locked --manifest-path rust/ffi/Cargo.toml
go -C go test ./codec
go -C go test -race ./...
```

The native build produces static and shared libraries plus the public
[header](include/pill_packer.h). Go links the static library from this checkout;
build it for the host platform before using the wrapper. The Go import path is
`github.com/AuthenticityLabs/pill-packer/go` (package `pillpacker`).

Close Go definition handles explicitly. Calls on one handle are serialized;
independent handles can run concurrently. WASM instances are worker-local.
Definitions must be supplied before their records can be decoded.

## PIL Specification

This specification defines PIL Definitions, records, and Packed Array Containers
(PACs). Byte positions start at zero. “Must” and “reject” state encoding and
validation requirements. Application storage and delivery bindings are separate.

### Quick Start

Install rx from the pill-packer repository root:

```sh
cargo install --path rust --locked --features cli --bin rx
```

Download the JSON schema and matching data from “Try the Worked Example” at
https://trietrunk.com/PIL/. Save them in one folder, then compile the schema:

```sh
rx write pil-example.json pil-example.rx
rx read pil-example.rx --pretty
rx read pil-example.rx pil-example.pil
rx read pil-example.rx pil-example.pac
rx read pil-example.rx pil-example.compressed.pac --pretty
```

Omitting the data path prints the Definition as JSON. Supplying it prints records
as JSONL; --pretty indents the output. The record example is explained byte by
byte in §3; §11 covers compilation and all command forms.

Contents: 1. Definition; 2. Types; 3. Record Layout; 4. Integer Widths;
5. Lengths and Offsets; 6. Arrays; 7. Objects; 8. PAC Layout;
9. Validation; 10. Schema Mapping; 11. Command-Line Reference.

PIL exchanges structured state under a shared, immutable Definition. Field
positions replace names, dictionaries replace repeated strings, and cumulative
end offsets locate variable values. A frame may carry a complete object or a
partial change; that distinction belongs to the application. PIL does not
prescribe storage, routing, subscriptions, or query execution.

### 1. Definition

The wire Definition is PIL; the codec reads its layout automatically. [definition.cddl](schemas/definition.cddl) describes its
portable structure and extensible annotations; codec validation additionally
enforces semantic rules. This JSON view defines the four-field record used in §3:

```json
{
  "pil": 0,
  "definition": "example/records",
  "version": 0,
  "fields": [
    { "name": "id", "type": "U16" },
    { "name": "count", "type": "U32" },
    { "name": "state", "type": "STRING", "values": ["pending", "authorized", "canceled"] },
    { "name": "note", "type": "STRING" }
  ]
}
```

- `definition` is a nonempty identity; `version` is an unsigned integer.
- `fields` is a nonempty **ordered array**. Position identifies each field.
  Names must be nonempty and unique; map-key order has no field-order meaning.
- `values`, when supplied, is an ordered dictionary of unique strings for a
  scalar `STRING` field. Array position is the zero-based dictionary ID.
- Optional `adaptive_integers` is `"none"`, `"16"`, or `"32"` (§4).
- Use `pil: 0` for fixed integers and `pil: 1` for enabled
  adaptive integers. These layout selectors are distinct from the
  Definition's `version` and PAC/0. Transport bindings have their own selectors.

Publish Definitions immutably. Changing types, order, fields, dictionary contents,
or integer policy requires a new Definition binding, even for an additive
change. Adding entries to a declared `@fields` value does not change its Definition.
A frame contains no Definition identifier: its enclosing binding must supply the
exact Definition before decoding. A structurally compatible wrong Definition
cannot be detected from a standalone frame alone.

Applications may attach resource/transport/profile annotations. Generic PIL
does not prescribe application identities, revisions, tree structures, or
resource bindings.
Bindings using a digest retain the exact original Definition document bytes;
reserializing a semantic equivalent need not preserve its digest. JSON imports
produce PIL once at registration. Definition map entries are sorted by their
encoded keys; ordered field arrays and dictionary arrays retain their order.
Forwarding preserves the exact accepted document bytes and annotations.

### 2. Types and Byte Conventions

All multibyte numbers are little endian. Signed integers use two's complement;
floats use IEEE-754. UUIDs are the exception: their 16 bytes follow standard UUID
network/textual-hex order. UUIDv7's first six bytes hold its millisecond timestamp;
the generic UUID primitive does not require version 7.

| Type | Value representation |
|---|---|
| `BOOL` | 1 byte: `00` false, `01` true; other values invalid |
| `I8/I16/I32/I64`, `U8/U16/U32/U64` | 1/2/4/8 bytes, subject to §4 |
| `U24` | Exactly 3 bytes, little-endian; 0–16,777,215; not adaptive |
| `F32`, `F64` | 4/8 bytes |
| `UUID` | 16 bytes |
| `TIMESTAMP_MS`, `TIMESTAMP_US`, `TIMESTAMP_NS` | Signed 8-byte Unix time; Definition supplies the unit |
| `REF16` | Opaque unsigned 2-byte shared-value reference, 0–65,535 |
| `STRING` | UTF-8 bytes, or a DIRECT dictionary ID |
| `STRING(n)` | Fixed UTF-8 byte capacity, explicit length, then zero padding |
| `OBJECT` | One native PIL value, including nested objects and arrays |
| `BYTES` | Arbitrary bytes |
| `ARRAY<T>` | Homogeneous scalar elements; layout in §6 |

There is no bare `TIMESTAMP` type or implicit unit inference. REF16's target and
scope must be supplied by the binding; PIL reserves no REF16 value and does not
interpret the target. A dictionary ID instead refers to its own
field's Definition dictionary and uses the smallest width covering its largest ID.

1–256 entries → U8; 257–65,536 → U16; up to 2³² → U32; larger → U64.
No dictionary ID is an extension sentinel. Out-of-range IDs are invalid.

CDDL bounded unsigned integers above U16 and no greater than 16,777,215 lower to
`U24`. Rust, Go, the portable TypeScript reader, and WASM use the same three-byte
representation. Bounds remain part of validation.

For STRING(n), n is a byte capacity from 1 through 4,294,967,295. The DIRECT
value is [length][UTF-8 content][zero padding], occupying length_width + n bytes.
The length prefix is U8 for n ≤ 255, U16 for n ≤ 65,535, otherwise U32. Length
counts content bytes, excluding padding. Content must fit the capacity and be
valid UTF-8; padding must be zero. Trailing spaces and embedded NUL bytes are
content, not padding. STRING(0) is not a type; use STRING for variable text.
CDDL value constraints and this physical storage capacity are separate (§10).

### 3. Record Layout and Field States

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
object or a patch; PIL does not turn NULL into deletion. Ordinary scalar layouts
permit both states; `@fields` forbids NULL. A compiled CDDL schema can further
constrain presence and nullability. Arbitrary annotations do not add validation
rules (§10).

Fixed-width types accept DIRECT, not OFFSET. Unmapped STRING, BYTES, OBJECT, and
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

Worked example using the Definition in §1 (adaptive integers disabled):

```json
{"id":42,"count":1500,"state":"authorized","note":"OK"}
```

```text
Byte position:   0    1–2      3–6          7    8–9     10–11
Hex bytes:       AB | 2A 00 | DC 05 00 00 | 01 | 02 00 | 4F 4B
Section:        mask    DIRECT values          ends     variable
Field:                  id       count   state           note
```

| Byte position | Interpretation |
|---|---|
| 0 | AB = binary 10 10 10 11: id, count, state are DIRECT; note is OFFSET |
| 1–2 | Little-endian U16 value 42 |
| 3–6 | Little-endian U32 value 1500 |
| 7 | Dictionary ID 1 = authorized in the state field's dictionary |
| 8–9 | End offset 2 relative to the variable section at byte 10 |
| 10–11 | UTF-8 bytes 4F 4B = OK |

Total: 12 bytes. No field names, Definition identifier, or copy of "authorized"
appear in this record. Field positions are defined by the ordered fields array;
byte positions depend on states and encoded widths. With state = null, the mask
becomes A7 (10 10 01 11), its dictionary byte disappears, and the record is
11 bytes. Null and absent do not reserve a direct-value slot.

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

For STRING/BYTES/OBJECT elements, ends are cumulative byte offsets relative to the
**element-value section**, not the array start. The final end must equal that
section's length; fixed-width size must equal `encoded_count_size + count × width`.

Example STRING array: `[3][5][10][19][JasonSmithAlexander]` (conceptual integers).
An empty array is the two-byte count zero alone, distinct from ABSENT and NULL.
Elements cannot be absent or null; `ARRAY<OBJECT>` rejects a top-level native null element
but permits nulls inside complex items. Use `OBJECT` for native nested structures,
or CDDL-compiled nested frames in `BYTES`/`ARRAY<BYTES>`. There is no ARRAY<PIL> type: use a sequence or PAC for
Definition-bound PIL records. Native arrays do not require PAC.

### 7. Native Objects and Dynamic Fields

`OBJECT` uses the same built-in `pil/value` layout as Definition documents.
The ordered fields are `kind:U8`, `unsigned:U64`, `negative:I64`, `float:F64`,
`text:STRING`, `bytes:BYTES`, and `children:ARRAY<BYTES>`. Kinds 0–9 represent
null, false, true, unsigned integer, negative integer, float, text, bytes,
array, and map. Only the payload selected by `kind` is present. Each child is
another native PIL value; maps alternate text/integer keys and values, reject
duplicate keys, and sort encoded keys when writing. Codecs bound values to
16 MiB and nesting to 64 levels. This layout needs no external Definition.

CBOR is an external ingestion format only. `CBOR` and `ARRAY<CBOR>` are rejected
as PIL field types; ingress adapters decode them to native PIL objects before
storage or runtime processing.

Field names beginning with `@` are reserved. The reserved extension field
is explicitly declared `@fields: OBJECT`; omitting it disallows dynamic root fields.
When present it must be an OFFSET native PIL map with unique text keys, none beginning
with `@` or colliding with any defined field. Its entries represent additional
root attributes; `@fields` is not itself an application property.

The extension contract for partial state is: ABSENT leaves the dynamic attribute
set unchanged; a supplied map replaces that set, including `{}` to clear it.
NULL is invalid. The codec validates the representation; applying patches is an
application responsibility. Promoting a dynamic key into a defined field requires
a new Definition and removal of that key from `@fields` for frames using it.

### 8. PAC — Packed Array Container

PAC optionally groups ordered PIL records under one exact Definition. A common
frame can factor shared values at **arbitrary field positions**; positions need
not form a contiguous Definition prefix. The following tables define the PAC/0 wire layout.

#### Decoded Binary Layout

| Offset | Bytes | Value |
|---|---:|---|
| 0 | 4 | U32 decoded container length, including header |
| 4 | 4 | U32 flags: bit 2 = required layout controls; bit 0 = common frame; other bits rejected |
| 8 | 8 | U64 total decoded PAC length, including header |
| 16 | 8 | U64 header length / first member position |
| 24 | 8 | U64 member count |
| 32 | 32 | Exact Definition digest (SHA-256 of bound Definition bytes) |
| 64 | variable | Extended extension count, then extension entries |
| following | 1 | Directory address width: 0, 2, 3, or 4 bytes |
| following | 4 | U32 fixed record width; zero means variable members |
| following | variable | Extended named-header count, then name/value entries |
| following | variable | Extended common-frame length, common PIL frame if present |
| following | count × address width | Member-relative start offsets, if enabled |
| header end | variable | Consecutive PIL member frames |

An extension is `[type U32][schema version U16][flags U16][extended length][bytes]`.
Extension schema versions in this specification are zero; flag bit 0 means required. Types are singleton.
Skip unrecognized optional extensions by length; reject unsupported required ones,
unsupported versions of recognized types, and duplicates. Type 1 carries a UTF-8
source name. Other extension identifiers and their interpretation are owned by
application bindings. The host passes its supported identifiers to the decoder.

Named headers are `[extended name length][UTF-8 name][extended value length][bytes]`,
strictly sorted by UTF-8 name bytes with unique, nonempty names. Directory offsets
start at zero, increase strictly, and stay within the member body. Fixed-width
members must fill exactly `count × width` bytes; any directory must agree with
that stride.

Header length includes extensions, layout controls, named headers, common frame,
and the optional directory. Zero common length means no
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
inherited fields. A consumer may require companion indexes or metadata and must
declare that requirement through its extension.

PAC is not a pagination protocol or proof of delivery completeness. It does not
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

The discriminator shares its position with decoded PAC flags: `4` or `5` means
a decoded PAC; `2` means this compressed representation. Common-frame flags and
all semantic metadata exist only inside the compressed body. No outer routing,
Definition, common-prefix, or placement fields are added.

The library validates stored/decoded bounds, gzip CRC, absence of trailing data,
and agreement between wrapper length/count and the decoded header. Expansion
returns the original decoded PAC bytes exactly. The decoded header replaces the
wrapper; no wrapper fields are copied into or patched onto it. Borrowed PIL
scanners consume that decoded representation. Applications choose whether to
retain the compressed or uncompressed representation.

### 9. Validation and Transport Boundaries

Reject invalid masks/padding, type-state combinations, dictionary IDs, integer
selectors, noncanonical lengths, decreasing/out-of-range offsets, invalid UTF-8,
malformed arrays/objects, extension collisions, PAC overrides, and inconsistent
bindings/counts/lengths. Check arithmetic and resource limits before allocating
or reading. Distinguish truncated input that can receive more bytes from malformed
values within an already complete bounded frame.

HTTP/JMS negotiation, storage addresses, and routing are application/transport
bindings. A transport must supply the exact definition before dependent data.
HTTP content compression applies to a transport representation; PAC compression
belongs to each container and is decoded by the library. Neither requires a
particular datastore, time partition, record identity, or subscription protocol.
Compression does not imply encryption.

### 10. Schema Mapping and Application Bindings

CDDL is an authoring schema. Compilation produces a PIL Definition containing
ordered field layouts and a structured `cddl` annotation; .rx contains PIL bytes,
not embedded CDDL or JSON text. Its named child layouts are resolved from the
same schema. Schema-bound nested records use BYTES and arrays of child records
use ARRAY<BYTES>; their contents are child PIL frames with no repeated Definition
identifier. Arbitrary OBJECT values instead use the native value layout in §7.

The compiler accepts named nonrecursive rules, closed text-keyed records,
required/optional members, homogeneous arrays with occurrence bounds, choices,
literals, uint, bool, float64, text, bytes, null, bounded 64-bit integer ranges,
and supported `.size` constraints. It rejects recursive rules, dynamic maps,
generics, group rules, tuple arrays, unbounded int/nint, and unsupported controls.
An accepted schema must agree with its derived root layout. Full encoding and
decoding validate schema constraints, including nested children. Unsupported
constructs are errors, not silently relaxed constraints.

A schema's `fixed_strings` map binds qualified field paths, such as
`purchase.customer.name`, to UTF-8 byte capacities. These bindings must cover the
bounded CDDL text size and select STRING(n) storage (§2). They do not change the
CDDL constraint. Other annotations have no implicit encoding behavior.

Named PAC headers carry opaque application values; numeric extensions carry
explicit contracts. The application supplies the meaning of REF16 references,
resource roles, record identities, and transport negotiation. The codec does not
provide authorization, retention, delivery completeness, or subscription policy.

### 11. rx Command-Line Reference

```text
rx read <definition> [<pil/pac>|-] [--pretty]
rx <definition> [--pretty]
rx write <definition.json> <definition.rx>
rx write <schema.cddl> <definition.rx> --root <rule>
```

- `read` without a data path prints the Definition as JSON. The definition input
  may be binary .rx or its JSON authoring representation.
- `read` with a data path prints one JSON object per record (JSONL). It reads PIL
  streams, PACs, compressed PACs, and gzip-wrapped inputs. Common fields are
  resolved when rendering PAC members. The PAC digest must match the Definition.
- `--pretty` indents JSON output. It is a read option; write rejects it.
- `write` compiles a Definition from JSON or CDDL into binary PIL. It does not
  encode event JSON into PIL records. Use the codec library for record encoding.
- `--root` selects the root rule when compiling CDDL. JSON read output preserves
  the structured schema and can be edited and supplied to write.
- `-` reads record bytes from stdin or writes binary Definitions to stdout.
- Byte strings render as arrays of octets; UUIDs use dashed hexadecimal;
  timestamp numbers retain their declared units. Absent fields are omitted;
  explicit null remains null. Definitions and individual frames/PACs are limited to
  64 MiB; a stream can contain multiple records or containers.

Examples (download the JSON schema and PIL/PAC samples from /PIL/; write creates .rx):

```sh
rx write pil-example.json pil-example.rx
rx read pil-example.rx --pretty
rx read pil-example.rx pil-example.pil
rx read pil-example.rx pil-example.pac
rx read pil-example.rx pil-example.compressed.pac --pretty
rx read pil-example.rx - < pil-example.pil
rx write purchase.cddl purchase.rx --root purchase
rx read purchase.rx --pretty > purchase.json
rx write purchase.json purchase.rx
```

## Rust APIs

`Definition::new` builds the ordered layout; `DefinitionDocument` retains the full
annotated source. `encode`/`FrameEncoder` write PIL, `decode`/`decode_exact` read it,
and `FrameIndex` scans into reusable spans. `ArrayView` supports borrowed array access; native objects use
`ObjectRef`. CBOR helpers are for external ingress. `pac::PacIndex` validates decoded PAC member spans and common
fields without materializing objects. `compression::inflate` is feature-gated.

Development schema checks can also validate consumer profiles:

```sh
cargo run --locked --manifest-path rust/Cargo.toml --example check_definition -- schema.cddl definition.rx
```

This checks CDDL structure plus PIL layout semantics. It does not execute a
consumer's additional semantic validators. The CDDL tool is a dev dependency.

## License

[Apache-2.0](LICENSE).

### Shared Nested Schema Runtime

CDDL authoring compiles to binary PIL `.rx` documents containing their complete
structured schema. Rust `DefinitionDocument`, the Go bindings and `go/codec`
layouts, and the Rust WASM codec use the same interpreter for nested records,
arrays, named types, choices, sparse members, and constraints. Registration
rejects disagreement between the schema and its compiled root layout. Full
frame and resolved PAC validation checks nested children; explicit structural
APIs remain available for trusted, integrity-checked data.

Go exposes `EncodeJSON` and `DecodeJSON`; WASM's `Codec` exposes `encodeJSON` and
`decodeJSON`. Pass JSON text so 64-bit numbers never pass through a JavaScript
floating-point object. Native `.rx` bytes and annotations remain digest-bound.
Go's schema support links the Rust FFI library: build `rust/ffi` in release mode
before testing or consuming the Go package. CDDL source parsing is a separate
`schema` feature used by `rx`; runtime schema interpretation is enabled by
default. Keep CDDL compiler tests in `rust/tests/schema.rs`, `.rx` CLI behavior
in `rust/tests/rx.rs`, bootstrap Definition conformance in
`rust/tests/documents.rs`, and binding parity in `go/codec/schema_test.go` plus
`npm/test.mjs`. Application-specific profile tests belong with the application
that owns those semantics.
