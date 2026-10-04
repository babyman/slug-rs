//! Versioned serialization primitives for `.cslug` VM artifacts.
//!
//! The codec stays adjacent to the VM because decoding is an executable
//! validation boundary. The initial surface establishes the fixed artifact
//! header; graph and bytecode records are added in later codec slices.

use alloc::{string::String, vec::Vec};

/// The exact byte count of a version-1 `.cslug` header.
pub const HEADER_LENGTH: usize = 12;

const MAGIC: [u8; 4] = *b"CSLG";
const FORMAT_VERSION: u16 = 1;
const VM_COMPATIBILITY_VERSION: u16 = 1;

/// The fixed header that identifies a version-1 `.cslug` artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    format_version: u16,
    vm_compatibility_version: u16,
    flags: u32,
}

impl Header {
    /// Returns the version-1 header accepted by this VM.
    #[must_use]
    pub const fn version_1() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            vm_compatibility_version: VM_COMPATIBILITY_VERSION,
            flags: 0,
        }
    }

    /// Returns the artifact binary-schema version.
    #[must_use]
    pub const fn format_version(self) -> u16 {
        self.format_version
    }

    /// Returns the VM/bytecode compatibility version.
    #[must_use]
    pub const fn vm_compatibility_version(self) -> u16 {
        self.vm_compatibility_version
    }

    /// Writes this header in its canonical 12-byte representation.
    #[must_use]
    pub fn encode(self) -> [u8; HEADER_LENGTH] {
        let mut bytes = [0; HEADER_LENGTH];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.vm_compatibility_version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }

    /// Decodes and validates a version-1 artifact header.
    ///
    /// # Errors
    ///
    /// Returns a checked codec error for truncated input, bad magic, an
    /// unsupported version, or version-1 flags other than zero.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let header = bytes
            .get(..HEADER_LENGTH)
            .ok_or(Error::Truncated { context: "header" })?;
        if header[..4] != MAGIC {
            return Err(Error::BadHeader("expected CSLG magic".into()));
        }
        let format_version = u16::from_le_bytes([header[4], header[5]]);
        if format_version != FORMAT_VERSION {
            return Err(Error::UnsupportedFormatVersion(format_version));
        }
        let vm_compatibility_version = u16::from_le_bytes([header[6], header[7]]);
        if vm_compatibility_version != VM_COMPATIBILITY_VERSION {
            return Err(Error::UnsupportedVmCompatibility(vm_compatibility_version));
        }
        let flags = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        if flags != 0 {
            return Err(Error::UnsupportedFlags(flags));
        }
        Ok(Self {
            format_version,
            vm_compatibility_version,
            flags,
        })
    }
}

/// Checked failures while encoding or decoding a `.cslug` artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Input ended before a required field was fully available.
    Truncated { context: &'static str },
    /// The artifact's fixed header is malformed.
    BadHeader(String),
    /// The artifact schema is not supported by this VM.
    UnsupportedFormatVersion(u16),
    /// The artifact requires a different VM/bytecode representation.
    UnsupportedVmCompatibility(u16),
    /// The artifact requests unsupported format features.
    UnsupportedFlags(u32),
}

impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated { context } => write!(formatter, "truncated .cslug {context}"),
            Self::BadHeader(message) => write!(formatter, "invalid .cslug header: {message}"),
            Self::UnsupportedFormatVersion(version) => {
                write!(formatter, "unsupported .cslug format version {version}")
            }
            Self::UnsupportedVmCompatibility(version) => {
                write!(
                    formatter,
                    "unsupported .cslug VM compatibility version {version}"
                )
            }
            Self::UnsupportedFlags(flags) => {
                write!(formatter, "unsupported .cslug flags {flags:#x}")
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
impl std::error::Error for Error {}

/// Writes the canonical version-1 header to an artifact buffer.
pub fn encode_header(output: &mut Vec<u8>) {
    output.extend(Header::version_1().encode());
}

#[cfg(test)]
mod tests {
    use super::{Error, HEADER_LENGTH, Header, encode_header};

    #[test]
    fn encodes_and_decodes_the_version_one_header() {
        let mut encoded = Vec::new();
        encode_header(&mut encoded);

        assert_eq!(encoded.len(), HEADER_LENGTH);
        assert_eq!(encoded, b"CSLG\x01\0\x01\0\0\0\0\0");
        assert_eq!(Header::decode(&encoded), Ok(Header::version_1()));
    }

    #[test]
    fn rejects_every_header_mismatch_as_a_checked_error() {
        assert_eq!(
            Header::decode(b"CSL"),
            Err(Error::Truncated { context: "header" })
        );

        let mut wrong_magic = Header::version_1().encode();
        wrong_magic[0] = b'X';
        assert_eq!(
            Header::decode(&wrong_magic),
            Err(Error::BadHeader("expected CSLG magic".into()))
        );

        let mut wrong_format = Header::version_1().encode();
        wrong_format[4] = 2;
        assert_eq!(
            Header::decode(&wrong_format),
            Err(Error::UnsupportedFormatVersion(2))
        );

        let mut wrong_vm = Header::version_1().encode();
        wrong_vm[6] = 2;
        assert_eq!(
            Header::decode(&wrong_vm),
            Err(Error::UnsupportedVmCompatibility(2))
        );

        let mut flags = Header::version_1().encode();
        flags[8] = 1;
        assert_eq!(Header::decode(&flags), Err(Error::UnsupportedFlags(1)));
    }
}
