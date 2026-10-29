//! Borrowed native PIL values for storage adapters.
use crate::{Error, FrameIndex};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectRef<'a> {
    bytes: &'a [u8],
}
impl<'a> ObjectRef<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        crate::object::decode_value(bytes)?;
        Ok(Self { bytes })
    }
    pub fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }
    pub fn is_null(self) -> bool {
        self.kind() == 0
    }
    fn kind(self) -> u8 {
        self.bytes[2]
    }
    fn field(self, i: usize) -> Option<&'a [u8]> {
        let mut index = FrameIndex::default();
        index
            .scan(crate::object::definition(), self.bytes, self.bytes.len())
            .ok()?;
        let [s, e] = *index.spans().get(i)?;
        self.bytes.get(s as usize..e as usize)
    }
    pub fn unsigned(self) -> Option<u64> {
        if self.kind() != 3 {
            return None;
        };
        Some(u64::from_le_bytes(self.field(1)?.try_into().ok()?))
    }
    pub fn bytes(self) -> Option<&'a [u8]> {
        (self.kind() == 7).then(|| self.field(5)).flatten()
    }
    pub fn text(self) -> Option<&'a str> {
        if self.kind() != 6 {
            return None;
        };
        std::str::from_utf8(self.field(4)?).ok()
    }
}
pub struct ObjectMap<'a> {
    pairs: std::vec::IntoIter<(ObjectRef<'a>, ObjectRef<'a>)>,
}
impl<'a> ObjectMap<'a> {
    pub fn new(bytes: &'a [u8], _depth: usize) -> Result<Self, Error> {
        let value = ObjectRef::parse(bytes)?;
        if value.kind() != 9 {
            return Err(Error::Invalid("expected PIL object"));
        }
        let children = value
            .field(6)
            .ok_or(Error::Invalid("missing PIL map children"))?;
        let array = crate::ArrayView::parse(crate::Scalar::Bytes, children)?;
        let mut refs = Vec::new();
        for child in array.iter() {
            let crate::Value::Bytes(b) = child? else {
                return Err(Error::Invalid("PIL map child"));
            };
            refs.push(ObjectRef { bytes: b });
        }
        let pairs = refs
            .chunks_exact(2)
            .map(|p| (p[0], p[1]))
            .collect::<Vec<_>>()
            .into_iter();
        Ok(Self { pairs })
    }
}
impl<'a> Iterator for ObjectMap<'a> {
    type Item = Result<(ObjectRef<'a>, ObjectRef<'a>), Error>;
    fn next(&mut self) -> Option<Self::Item> {
        self.pairs.next().map(Ok)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.pairs.size_hint()
    }
}
