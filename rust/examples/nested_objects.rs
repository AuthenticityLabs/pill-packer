//! Experimental composition: nested PIL frames in BYTES / ARRAY<BYTES> fields.
//! Hand-bound to nested_objects.cddl, not a CDDL compiler or a public OBJECT type.
//! Definitions and record payloads use PIL; CBOR is only a comparison baseline.
use ciborium::Value as Cbor;
use pil_codec::{
    ABSENT, Array, ArrayView, Definition, Error, Field, FrameIndex, NULL, Scalar, Type, Value,
    decode_exact, encode,
};

const SCHEMA: &str = include_str!("nested_objects.cddl");

struct Layouts {
    purchase: Definition,
    customer: Definition,
    address: Definition,
    item: Definition,
}
impl Layouts {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            purchase: Definition::new(
                "purchase",
                0,
                vec![
                    Field::new("id", Scalar::U64),
                    Field::new("customer", Scalar::Bytes),
                    Field::new("items", Type::Array(Scalar::Bytes)),
                    Field::new("note", Scalar::String),
                ],
            )?,
            customer: Definition::new(
                "customer",
                0,
                vec![
                    Field::new("id", Scalar::U64),
                    Field::new("name", Scalar::String),
                    Field::new("address", Scalar::Bytes),
                ],
            )?,
            address: Definition::new(
                "address",
                0,
                vec![
                    Field::new("city", Scalar::String),
                    Field::new("postal", Scalar::String),
                ],
            )?,
            item: Definition::new(
                "item",
                0,
                vec![
                    Field::new("sku", Scalar::String),
                    Field::new("quantity", Scalar::U16),
                    Field::new("unit_price", Scalar::U32),
                ],
            )?,
        })
    }
}
#[derive(Clone, Copy)]
enum Note<'a> {
    Absent,
    Null,
    Text(&'a str),
}
struct Item {
    sku: String,
    quantity: u16,
    price: u32,
}
struct Purchase {
    id: u64,
    customer_id: u64,
    name: String,
    city: String,
    postal: String,
    items: Vec<Item>,
}
impl Purchase {
    fn sample(id: u64, count: usize) -> Self {
        Self {
            id,
            customer_id: id % 1000,
            name: format!("Customer {}", id % 1000),
            city: "Montréal 🌳".into(),
            postal: "H2Y 1C6".into(),
            items: (0..count)
                .map(|n| Item {
                    sku: format!("SKU-{:04}", (id + n as u64) % 500),
                    quantity: 1 + (n % 5) as u16,
                    price: 1999 + (id % 100) as u32,
                })
                .collect(),
        }
    }
    fn encode(&self, d: &Layouts, note: Note<'_>) -> Result<Vec<u8>, Error> {
        let address = encode(
            &d.address,
            &[Value::String(&self.city), Value::String(&self.postal)],
        )?;
        let customer = encode(
            &d.customer,
            &[
                Value::U64(self.customer_id),
                Value::String(&self.name),
                Value::Bytes(&address),
            ],
        )?;
        let items = self
            .items
            .iter()
            .map(|i| {
                encode(
                    &d.item,
                    &[
                        Value::String(&i.sku),
                        Value::U16(i.quantity),
                        Value::U32(i.price),
                    ],
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let values: Vec<_> = items.iter().map(|i| Value::Bytes(i)).collect();
        encode(
            &d.purchase,
            &[
                Value::U64(self.id),
                Value::Bytes(&customer),
                Value::Array(Array::Items(&values)),
                match note {
                    Note::Absent => Value::Absent,
                    Note::Null => Value::Null,
                    Note::Text(s) => Value::String(s),
                },
            ],
        )
    }
    fn logical_value(&self, note: Note<'_>) -> Cbor {
        let mut fields = vec![
            ("id", self.id.into()),
            (
                "customer",
                map(vec![
                    ("id", self.customer_id.into()),
                    ("name", self.name.clone().into()),
                    (
                        "address",
                        map(vec![
                            ("city", self.city.clone().into()),
                            ("postal", self.postal.clone().into()),
                        ]),
                    ),
                ]),
            ),
            (
                "items",
                Cbor::Array(
                    self.items
                        .iter()
                        .map(|i| {
                            map(vec![
                                ("sku", i.sku.clone().into()),
                                ("quantity", i.quantity.into()),
                                ("unit_price", i.price.into()),
                            ])
                        })
                        .collect(),
                ),
            ),
        ];
        match note {
            Note::Absent => (),
            Note::Null => fields.push(("note", Cbor::Null)),
            Note::Text(s) => fields.push(("note", s.into())),
        }
        map(fields)
    }
}
fn map(fields: Vec<(&str, Cbor)>) -> Cbor {
    Cbor::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn cbor_bytes(value: &Cbor) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::ser::into_writer(value, &mut out).unwrap();
    out
}

// Allocation is limited to reusable offset vectors, warmed once per nesting level.
// No record tree or string copies are needed for this traversal.
#[derive(Default)]
struct Reader {
    purchase: FrameIndex,
    customer: FrameIndex,
    address: FrameIndex,
    item: FrameIndex,
}
#[derive(Debug, PartialEq)]
struct Summary<'a> {
    name: &'a str,
    city: &'a str,
    items: usize,
    quantity: u64,
}
fn scan(index: &mut FrameIndex, d: &Definition, bytes: &[u8]) -> Result<(), Error> {
    if index.scan(d, bytes, bytes.len())? != bytes.len() {
        return Err(Error::Invalid("trailing nested frame bytes"));
    }
    Ok(())
}
fn field<'a>(index: &FrameIndex, bytes: &'a [u8], n: usize) -> Result<&'a [u8], Error> {
    let [a, b] = index.spans()[n];
    if a == ABSENT || a == NULL {
        return Err(Error::Invalid("required nested field missing or null"));
    }
    Ok(&bytes[a as usize..b as usize])
}
impl Reader {
    fn read<'a>(&mut self, d: &Layouts, bytes: &'a [u8]) -> Result<Summary<'a>, Error> {
        scan(&mut self.purchase, &d.purchase, bytes)?;
        field(&self.purchase, bytes, 0)?;
        let customer = field(&self.purchase, bytes, 1)?;
        scan(&mut self.customer, &d.customer, customer)?;
        field(&self.customer, customer, 0)?;
        let name = std::str::from_utf8(field(&self.customer, customer, 1)?)
            .map_err(|_| Error::Invalid("invalid name"))?;
        let address = field(&self.customer, customer, 2)?;
        scan(&mut self.address, &d.address, address)?;
        let city = std::str::from_utf8(field(&self.address, address, 0)?)
            .map_err(|_| Error::Invalid("invalid city"))?;
        field(&self.address, address, 1)?;
        let items = ArrayView::parse(Scalar::Bytes, field(&self.purchase, bytes, 2)?)?;
        let mut quantity = 0;
        for item in items.iter() {
            let Value::Bytes(item) = item? else {
                return Err(Error::Invalid("expected nested item"));
            };
            scan(&mut self.item, &d.item, item)?;
            field(&self.item, item, 0)?;
            quantity += u16::from_le_bytes(
                field(&self.item, item, 1)?
                    .try_into()
                    .map_err(|_| Error::Invalid("quantity width"))?,
            ) as u64;
            field(&self.item, item, 2)?;
        }
        Ok(Summary {
            name,
            city,
            items: items.len(),
            quantity,
        })
    }
}
// Allocating projection solely for semantic round-trip tests and baseline JSON.
fn project(d: &Layouts, kind: &str, bytes: &[u8]) -> Result<Cbor, Error> {
    let def = match kind {
        "purchase" => &d.purchase,
        "customer" => &d.customer,
        "address" => &d.address,
        "item" => &d.item,
        _ => return Err(Error::Invalid("unknown layout")),
    };
    let frame = decode_exact(def, bytes)?;
    let mut fields = Vec::new();
    for (f, v) in def.fields().iter().zip(frame.values()) {
        let value = match *v {
            Value::Absent => continue,
            Value::Null => Cbor::Null,
            Value::U64(n) => n.into(),
            Value::U32(n) => n.into(),
            Value::U16(n) => n.into(),
            Value::String(s) => s.into(),
            Value::Bytes(b) => project(d, &f.name, b)?,
            Value::Array(Array::Encoded(a)) => Cbor::Array(
                a.iter()
                    .map(|v| match v? {
                        Value::Bytes(b) => project(d, "item", b),
                        _ => Err(Error::Invalid("item type")),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err(Error::Invalid("unexpected prototype value")),
        };
        fields.push((f.name.as_str(), value));
    }
    Ok(map(fields))
}

#[cfg(all(feature = "json", feature = "gzip"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    fn gzip(b: &[u8]) -> usize {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        e.write_all(b).unwrap();
        e.finish().unwrap().len()
    }
    let d = Layouts::new()?;
    let one = Purchase::sample(42, 2);
    let p = one.encode(&d, Note::Text("approved"))?;
    let logical = one.logical_value(Note::Text("approved"));
    assert_eq!(project(&d, "purchase", &p)?, logical);
    cddl::validate_cbor_from_slice(SCHEMA, &cbor_bytes(&logical), None)?;
    println!(
        "Single record: PIL={} CBOR={} compact JSON={} bytes",
        p.len(),
        cbor_bytes(&logical).len(),
        serde_json::to_vec(&logical)?.len()
    );
    let mut pil = Vec::new();
    let mut cbor = Vec::new();
    let mut jsonl = Vec::new();
    let mut bounds = Vec::new();
    for id in 0..10_000 {
        let value = Purchase::sample(id, 1 + (id % 5) as usize);
        let note = match id % 3 {
            0 => Note::Absent,
            1 => Note::Null,
            _ => Note::Text("approved"),
        };
        let b = value.encode(&d, note)?;
        // Every generated record is compared with its independent logical input.
        let logical = value.logical_value(note);
        assert_eq!(project(&d, "purchase", &b)?, logical);
        bounds.push((pil.len(), pil.len() + b.len()));
        pil.extend_from_slice(&b);
        cbor.extend_from_slice(&cbor_bytes(&logical));
        serde_json::to_writer(&mut jsonl, &logical)?;
        jsonl.push(b'\n');
    }
    println!("10,000 records; same logical data; payload only; no PAC factoring");
    for (name, b) in [
        ("PIL frames", &pil),
        ("CBOR sequence", &cbor),
        ("compact JSONL", &jsonl),
    ] {
        println!("{name}: raw={} gzip6={} bytes", b.len(), gzip(b));
    }
    let definitions = [&d.purchase, &d.customer, &d.address, &d.item]
        .iter()
        .map(|d| d.to_pil().unwrap().len())
        .sum::<usize>();
    println!(
        "One-time four-layout metadata: {definitions} bytes (native PIL definition documents; CDDL source {} bytes)",
        SCHEMA.len()
    );
    let mut reader = Reader::default();
    reader.read(&d, &pil[bounds[0].0..bounds[0].1])?;
    let start = std::time::Instant::now();
    let mut checksum = 0u64;
    for _ in 0..20 {
        for &(a, b) in &bounds {
            let s = reader.read(&d, std::hint::black_box(&pil[a..b]))?;
            checksum += s.quantity + s.name.len() as u64 + s.city.len() as u64;
        }
    }
    println!(
        "Warm nested validation + borrowed projection: {:.0} ns/record (200,000 reads; checksum {checksum}; local microbenchmark)",
        start.elapsed().as_nanos() as f64 / 200_000.0
    );
    Ok(())
}
#[cfg(not(all(feature = "json", feature = "gzip")))]
fn main() {
    println!("Run with --features json,gzip to compare nested PIL, CBOR and JSON.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };
    struct Counting;
    thread_local! { static TRACK:Cell<bool>=const {Cell::new(false)}; static ALLOCS:Cell<usize>=const {Cell::new(0)}; }
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            TRACK.with(|t| {
                if t.get() {
                    ALLOCS.with(|n| n.set(n.get() + 1))
                }
            });
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            TRACK.with(|t| {
                if t.get() {
                    ALLOCS.with(|n| n.set(n.get() + 1))
                }
            });
            unsafe { System.realloc(p, l, n) }
        }
    }
    #[test]
    fn nested_roundtrip_matches_cddl_including_presence_and_unicode() {
        let d = Layouts::new().unwrap();
        for n in [0, 1, 3, 128] {
            for note in [
                Note::Absent,
                Note::Null,
                Note::Text(""),
                Note::Text("approved 🌳"),
            ] {
                let mut p = Purchase::sample(42, n);
                p.name = "é".repeat(600);
                let b = p.encode(&d, note).unwrap();
                let expected = p.logical_value(note);
                assert_eq!(project(&d, "purchase", &b).unwrap(), expected);
                cddl::validate_cbor_from_slice(SCHEMA, &cbor_bytes(&expected), None).unwrap();
                let result = Reader::default().read(&d, &b).unwrap();
                assert_eq!(result.name, p.name);
                assert_eq!(result.items, n);
            }
        }
    }
    #[test]
    fn borrowed_traversal_allocates_nothing_after_warmup() {
        let d = Layouts::new().unwrap();
        let b = Purchase::sample(42, 12).encode(&d, Note::Absent).unwrap();
        let mut r = Reader::default();
        r.read(&d, &b).unwrap();
        ALLOCS.with(|n| n.set(0));
        TRACK.with(|t| t.set(true));
        let result = r.read(&d, &b);
        TRACK.with(|t| t.set(false));
        let result = result.unwrap();
        assert_eq!(ALLOCS.with(Cell::get), 0);
        let p = result.name.as_ptr() as usize;
        assert!(p >= b.as_ptr() as usize && p + result.name.len() <= b.as_ptr() as usize + b.len());
    }
    #[test]
    fn rejects_every_truncated_outer_record() {
        let d = Layouts::new().unwrap();
        let b = Purchase::sample(42, 2).encode(&d, Note::Absent).unwrap();
        let mut r = Reader::default();
        for n in 0..b.len() {
            assert!(r.read(&d, &b[..n]).is_err(), "accepted truncation {n}");
        }
        let mut b = b;
        b.push(0);
        assert!(r.read(&d, &b).is_err());
    }
    #[test]
    fn validates_nested_bytes_instead_of_treating_them_as_opaque() {
        let d = Layouts::new().unwrap();
        let b = Purchase::sample(42, 2).encode(&d, Note::Absent).unwrap();
        let root = decode_exact(&d.purchase, &b).unwrap();
        let Value::Bytes(customer) = root.values()[1] else {
            panic!()
        };
        for n in 0..customer.len() {
            let outer = encode(
                &d.purchase,
                &[
                    Value::U64(42),
                    Value::Bytes(&customer[..n]),
                    root.values()[2],
                    Value::Absent,
                ],
            )
            .unwrap();
            assert!(Reader::default().read(&d, &outer).is_err());
        }
        let mut child = customer.to_vec();
        child.push(0);
        let outer = encode(
            &d.purchase,
            &[
                Value::U64(42),
                Value::Bytes(&child),
                root.values()[2],
                Value::Absent,
            ],
        )
        .unwrap();
        assert!(Reader::default().read(&d, &outer).is_err());
        let bad_item = encode(
            &d.item,
            &[Value::String("ABC"), Value::Absent, Value::U32(10)],
        )
        .unwrap();
        let outer = encode(
            &d.purchase,
            &[
                Value::U64(42),
                Value::Bytes(customer),
                Value::Array(Array::Items(&[Value::Bytes(&bad_item)])),
                Value::Absent,
            ],
        )
        .unwrap();
        assert!(Reader::default().read(&d, &outer).is_err());
    }
    #[test]
    fn cddl_rejects_invalid_item_and_pil_rejects_wrong_scalar_type() {
        let d = Layouts::new().unwrap();
        assert!(
            encode(
                &d.item,
                &[Value::String("ABC"), Value::U32(70_000), Value::U32(10)]
            )
            .is_err()
        );
        let mut value = Purchase::sample(42, 1).logical_value(Note::Absent);
        let Cbor::Map(fields) = &mut value else {
            panic!()
        };
        let Cbor::Array(items) = &mut fields[2].1 else {
            panic!()
        };
        let Cbor::Map(item) = &mut items[0] else {
            panic!()
        };
        item[1].1 = 70_000u64.into();
        assert!(cddl::validate_cbor_from_slice(SCHEMA, &cbor_bytes(&value), None).is_err());
    }
}
