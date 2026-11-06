//! A record: `[header, body]` (0006 §3), and the global revision ID scheme (0015 §2).

use crate::body::{Body, BodyError};
use crate::cbor::{self, CborError, Value};
use crate::hash::Hash;
use crate::header::{Header, HeaderError};

/// A header with the body it commits to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    header: Header,
    body: Body,
}

/// Why bytes are not a record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecordError {
    /// Not canonical CBOR.
    #[error(transparent)]
    Cbor(#[from] CborError),
    /// The header is wrong.
    #[error(transparent)]
    Header(#[from] HeaderError),
    /// The body is wrong or does not match the header.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// Not a two-element array.
    #[error("record: not [header, body]")]
    Shape,
}

impl Record {
    /// Pairs a header with a body, checking the commitment.
    pub fn new(header: Header, body: Body) -> Result<Self, RecordError> {
        body.verify(&header.commitment)?;
        Ok(Self { header, body })
    }

    /// A header for `body`, with the commitment filled in from it.
    #[must_use]
    pub fn seal(mut header: Header, body: Body) -> Self {
        header.commitment = body.commitment();
        Self { header, body }
    }

    /// A header whose commitment the caller computed from `body` already
    /// ([`crate::store::Draft::seal_committed`]); not checked here.
    pub(crate) fn committed(header: Header, body: Body) -> Self {
        Self { header, body }
    }

    /// The header.
    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// The body, to erase parts of. The commitment is unchanged by erasure.
    pub fn body_mut(&mut self) -> &mut Body {
        &mut self.body
    }

    /// Takes the record apart.
    #[must_use]
    pub fn into_parts(self) -> (Header, Body) {
        (self.header, self.body)
    }

    /// The Merkle leaf of the record: that of its header.
    #[must_use]
    pub fn leaf(&self) -> Hash {
        self.header.leaf()
    }

    /// The record as a CBOR value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::Array(vec![self.header.to_value(), self.body.to_value()])
    }

    /// The canonical bytes.
    ///
    /// # Panics
    ///
    /// Never: a header and a body encode.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_value()).expect("a record encodes")
    }

    /// Parses canonical bytes, checking the commitment.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        Self::from_value(&cbor::decode(bytes)?)
    }

    /// Reads a record from a CBOR value, checking the commitment.
    pub fn from_value(v: &Value) -> Result<Self, RecordError> {
        let Value::Array(pair) = v else {
            return Err(RecordError::Shape);
        };
        if pair.len() != 2 {
            return Err(RecordError::Shape);
        }
        Self::new(Header::from_value(&pair[0])?, Body::from_value(&pair[1])?)
    }
}

/// The bits of a global revision or log ID that hold the per-provider counter.
pub const COUNTER_BITS: u32 = 40;
/// The largest counter a provider number can be combined with.
pub const MAX_COUNTER: u64 = (1 << COUNTER_BITS) - 1;
/// The largest provider number: the ID is a 64-bit integer, so 24 bits remain.
pub const MAX_PROVIDER: u64 = (1 << (64 - COUNTER_BITS)) - 1;
/// The provider number of the instance's own records.
pub const LOCAL_PROVIDER: u64 = 0;

/// `provider << 40 | counter`: a global revision or log ID (0015 §2).
///
/// Returns `None` when either half is out of range.
#[must_use]
pub fn global_id(provider: u64, counter: u64) -> Option<u64> {
    (provider <= MAX_PROVIDER && counter <= MAX_COUNTER)
        .then_some((provider << COUNTER_BITS) | counter)
}

/// The `(provider, counter)` halves of a global ID.
#[must_use]
pub fn split_global_id(id: u64) -> (u64, u64) {
    (id >> COUNTER_BITS, id & MAX_COUNTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::ATTESTATION;

    fn header() -> Header {
        Header {
            version: 1,
            partition: 7,
            offset: 3,
            appended_at: 1_790_000_000_000_042,
            payload_type: "scatter:v0/changeset".into(),
            key: Some("Q6".into()),
            commitment: [0; 32],
            revid: global_id(0, 900),
            logid: None,
            page_id: Some(12),
        }
    }

    fn body() -> Body {
        Body::core(
            &Value::Map(vec![(Value::text("op"), Value::text("put"))]),
            &Value::Null,
            &Value::Map(vec![(Value::text("actor"), Value::text("local:42"))]),
        )
        .unwrap()
    }

    #[test]
    fn seal_then_round_trip() {
        let r = Record::seal(header(), body());
        assert_eq!(r.header().commitment, r.body().commitment());
        let bytes = r.encode();
        assert_eq!(bytes[0], 0x82);
        let back = Record::decode(&bytes).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.leaf(), r.header().leaf());
    }

    #[test]
    fn erasure_keeps_the_record_valid() {
        let mut r = Record::seal(header(), body());
        let leaf = r.leaf();
        r.body_mut().erase(ATTESTATION).unwrap();
        let back = Record::decode(&r.encode()).unwrap();
        assert_eq!(back.leaf(), leaf);
        assert!(back.body().attestation().is_erased());
    }

    #[test]
    fn mismatch_is_rejected() {
        assert!(matches!(
            Record::new(header(), body()),
            Err(RecordError::Body(BodyError::Commitment))
        ));
        assert!(matches!(
            Record::from_value(&Value::Array(vec![Value::Int(1)])),
            Err(RecordError::Shape)
        ));
    }

    #[test]
    fn global_ids() {
        assert_eq!(global_id(0, 900), Some(900));
        assert_eq!(global_id(3, 5), Some((3 << 40) | 5));
        assert_eq!(split_global_id((3 << 40) | 5), (3, 5));
        assert_eq!(global_id(0, MAX_COUNTER + 1), None);
        assert_eq!(global_id(MAX_PROVIDER + 1, 0), None);
        assert_eq!(split_global_id(u64::MAX), (MAX_PROVIDER, MAX_COUNTER));
    }
}
