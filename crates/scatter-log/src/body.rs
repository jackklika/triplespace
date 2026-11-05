//! The record body (0006 §5; 0015 §2): an array of salted parts, each of which can be
//! erased on its own without touching the header or the Merkle tree.
//!
//! A present part is `[salt, bytes]`; an erased part is `[null, leaf]`, the leaf being
//! the hash the part had, so that the body commitment in the header still verifies.

use crate::cbor::{self, CborError, Value};
use crate::hash::{self, Hash};

/// The index of the `content` part.
pub const CONTENT: usize = 0;
/// The index of the `comment` part.
pub const COMMENT: usize = 1;
/// The index of the `attestation` part.
pub const ATTESTATION: usize = 2;
/// The number of parts every Triplespace payload type has.
pub const CORE_PARTS: usize = 3;
/// The most parts a body may have: the part index is one byte in the leaf preimage.
pub const MAX_PARTS: usize = 256;

/// A 16-byte random salt.
pub type Salt = [u8; 16];

/// One part of a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A part that is still in the log.
    Present {
        /// The salt that hides the bytes behind the leaf.
        salt: Salt,
        /// The part's canonical CBOR.
        bytes: Vec<u8>,
    },
    /// A part that was erased; only its leaf remains.
    Erased {
        /// The hash the part had.
        leaf: Hash,
    },
}

impl Part {
    /// A present part with a fresh random salt.
    ///
    /// # Panics
    ///
    /// If the operating system's random source fails.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self::Present {
            salt: fresh_salt(),
            bytes,
        }
    }

    /// A present part with a fresh salt around a CBOR value. The encoding is trimmed to its
    /// length: a part is held as long as its record is, and the encoder's buffer grows by
    /// doubling, so it could otherwise carry up to as much again unused.
    pub fn of(value: &Value) -> Result<Self, CborError> {
        let mut bytes = cbor::encode(value)?;
        bytes.shrink_to_fit();
        Ok(Self::new(bytes))
    }

    /// The part's bytes, when present.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Present { bytes, .. } => Some(bytes),
            Self::Erased { .. } => None,
        }
    }

    /// The part's decoded value, when present.
    pub fn value(&self) -> Result<Option<Value>, CborError> {
        self.bytes().map(cbor::decode).transpose()
    }

    /// Whether the part was erased.
    #[must_use]
    pub fn is_erased(&self) -> bool {
        matches!(self, Self::Erased { .. })
    }

    /// The part's leaf at index `index`: computed when present, kept when erased.
    #[must_use]
    pub fn leaf(&self, index: u8) -> Hash {
        match self {
            Self::Present { salt, bytes } => hash::part_leaf(index, salt, bytes),
            Self::Erased { leaf } => *leaf,
        }
    }

    /// The content hash of the part's bytes (tag `0x03`), when present.
    #[must_use]
    pub fn content_hash(&self) -> Option<Hash> {
        self.bytes().map(hash::content_hash)
    }

    /// The part as a CBOR value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Present { salt, bytes } => Value::Array(vec![
                Value::Bytes(salt.to_vec()),
                Value::Bytes(bytes.clone()),
            ]),
            Self::Erased { leaf } => Value::Array(vec![Value::Null, Value::Bytes(leaf.to_vec())]),
        }
    }

    /// Reads a part from a CBOR value.
    pub fn from_value(v: &Value) -> Result<Self, BodyError> {
        let Value::Array(pair) = v else {
            return Err(BodyError::Shape("a part is a two-element array"));
        };
        if pair.len() != 2 {
            return Err(BodyError::Shape("a part is a two-element array"));
        }
        match (&pair[0], &pair[1]) {
            (Value::Bytes(salt), Value::Bytes(bytes)) => {
                let salt: Salt = salt
                    .as_slice()
                    .try_into()
                    .map_err(|_| BodyError::Shape("a salt is 16 bytes"))?;
                Ok(Self::Present {
                    salt,
                    bytes: bytes.clone(),
                })
            }
            (Value::Null, Value::Bytes(leaf)) => {
                let leaf: Hash = leaf
                    .as_slice()
                    .try_into()
                    .map_err(|_| BodyError::Shape("a leaf is 32 bytes"))?;
                Ok(Self::Erased { leaf })
            }
            _ => Err(BodyError::Shape("a part is [salt, bytes] or [null, leaf]")),
        }
    }
}

/// Why a value is not a body, or a body does not match its header.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BodyError {
    /// Not canonical CBOR.
    #[error(transparent)]
    Cbor(#[from] CborError),
    /// Not an array of parts of the right shapes.
    #[error("body: {0}")]
    Shape(&'static str),
    /// Fewer than the three core parts, or more than 256.
    #[error("body: {0} parts")]
    PartCount(usize),
    /// A part index past the end of the body.
    #[error("body: no part {0}")]
    NoPart(usize),
    /// The commitment the parts produce is not the one the header carries.
    #[error("body: the commitment does not match the header")]
    Commitment,
}

/// A record body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    parts: Vec<Part>,
}

impl Body {
    /// A body from its parts: at least the three core parts, at most 256.
    pub fn new(parts: Vec<Part>) -> Result<Self, BodyError> {
        if !(CORE_PARTS..=MAX_PARTS).contains(&parts.len()) {
            return Err(BodyError::PartCount(parts.len()));
        }
        Ok(Self { parts })
    }

    /// The three core parts from their values, each with a fresh salt.
    pub fn core(content: &Value, comment: &Value, attestation: &Value) -> Result<Self, BodyError> {
        Self::new(vec![
            Part::of(content)?,
            Part::of(comment)?,
            Part::of(attestation)?,
        ])
    }

    /// Adds a part after the ones so far (a type's extra parts).
    pub fn push(&mut self, part: Part) -> Result<(), BodyError> {
        if self.parts.len() >= MAX_PARTS {
            return Err(BodyError::PartCount(self.parts.len() + 1));
        }
        self.parts.push(part);
        Ok(())
    }

    /// The parts in order.
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// Part `index`, if the body has one.
    #[must_use]
    pub fn part(&self, index: usize) -> Option<&Part> {
        self.parts.get(index)
    }

    /// The `content` part.
    #[must_use]
    pub fn content(&self) -> &Part {
        &self.parts[CONTENT]
    }

    /// The `comment` part.
    #[must_use]
    pub fn comment(&self) -> &Part {
        &self.parts[COMMENT]
    }

    /// The `attestation` part.
    #[must_use]
    pub fn attestation(&self) -> &Part {
        &self.parts[ATTESTATION]
    }

    /// The leaves of the parts in order.
    #[must_use]
    pub fn leaves(&self) -> Vec<Hash> {
        self.parts
            .iter()
            .enumerate()
            .map(|(i, p)| p.leaf(u8::try_from(i).expect("at most 256 parts")))
            .collect()
    }

    /// The body commitment: header field 6.
    #[must_use]
    pub fn commitment(&self) -> Hash {
        hash::commitment(&self.leaves())
    }

    /// The content hash of the `content` part, when present.
    #[must_use]
    pub fn content_hash(&self) -> Option<Hash> {
        self.content().content_hash()
    }

    /// Erases part `index`, leaving its leaf. Erasing an erased part is a no-op.
    pub fn erase(&mut self, index: usize) -> Result<(), BodyError> {
        let i8 = u8::try_from(index).map_err(|_| BodyError::NoPart(index))?;
        let part = self.parts.get_mut(index).ok_or(BodyError::NoPart(index))?;
        if let Part::Present { .. } = part {
            *part = Part::Erased {
                leaf: part.leaf(i8),
            };
        }
        Ok(())
    }

    /// Checks the body against a header's commitment.
    pub fn verify(&self, commitment: &Hash) -> Result<(), BodyError> {
        if self.commitment() == *commitment {
            Ok(())
        } else {
            Err(BodyError::Commitment)
        }
    }

    /// The body as a CBOR value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::Array(self.parts.iter().map(Part::to_value).collect())
    }

    /// The canonical bytes.
    ///
    /// # Panics
    ///
    /// Never: every part encodes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_value()).expect("a body encodes")
    }

    /// Parses canonical bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, BodyError> {
        Self::from_value(&cbor::decode(bytes)?)
    }

    /// Reads a body from a CBOR value.
    pub fn from_value(v: &Value) -> Result<Self, BodyError> {
        let Value::Array(parts) = v else {
            return Err(BodyError::Shape("a body is an array of parts"));
        };
        Self::new(
            parts
                .iter()
                .map(Part::from_value)
                .collect::<Result<_, _>>()?,
        )
    }
}

/// Sixteen bytes from the operating system's random source.
///
/// # Panics
///
/// If the random source fails: a salt that is not random does not hide anything.
#[must_use]
pub fn fresh_salt() -> Salt {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("the OS random source works");
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Body::core(
            &Value::Map(vec![(Value::text("op"), Value::text("put"))]),
            &Value::text("a comment"),
            &Value::Map(vec![(Value::text("actor"), Value::text("local:42"))]),
        )
        .unwrap()
    }

    #[test]
    fn erasing_keeps_the_commitment() {
        let mut b = body();
        let c = b.commitment();
        let content_hash = b.content_hash().unwrap();
        b.erase(ATTESTATION).unwrap();
        assert!(b.attestation().is_erased());
        assert_eq!(b.commitment(), c);
        assert_eq!(b.content_hash(), Some(content_hash));
        b.verify(&c).unwrap();
        b.erase(ATTESTATION).unwrap();
        assert_eq!(b.commitment(), c);
        assert!(matches!(b.erase(3), Err(BodyError::NoPart(3))));
        assert!(matches!(b.verify(&[0; 32]), Err(BodyError::Commitment)));
    }

    #[test]
    fn round_trips_through_cbor() {
        let mut b = body();
        b.erase(COMMENT).unwrap();
        let bytes = b.encode();
        assert_eq!(bytes[0], 0x83);
        assert_eq!(Body::decode(&bytes).unwrap(), b);
        assert!(b.comment().value().unwrap().is_none());
        assert_eq!(
            b.content().value().unwrap(),
            Some(Value::Map(vec![(Value::text("op"), Value::text("put"))]))
        );
    }

    #[test]
    fn shapes_are_checked() {
        assert!(matches!(Body::new(vec![]), Err(BodyError::PartCount(0))));
        let salted = Value::Array(vec![Value::Bytes(vec![1; 15]), Value::Bytes(vec![])]);
        assert!(matches!(
            Part::from_value(&salted),
            Err(BodyError::Shape(_))
        ));
        let erased = Value::Array(vec![Value::Null, Value::Bytes(vec![1; 32])]);
        assert_eq!(
            Part::from_value(&erased).unwrap(),
            Part::Erased { leaf: [1; 32] }
        );
        assert!(matches!(
            Part::from_value(&Value::Array(vec![Value::Int(1), Value::Int(2)])),
            Err(BodyError::Shape(_))
        ));
    }

    #[test]
    fn salts_differ() {
        assert_ne!(fresh_salt(), fresh_salt());
        let a = Part::new(b"x".to_vec());
        let b = Part::new(b"x".to_vec());
        assert_ne!(a.leaf(0), b.leaf(0));
        assert_eq!(a.content_hash(), b.content_hash());
    }
}
