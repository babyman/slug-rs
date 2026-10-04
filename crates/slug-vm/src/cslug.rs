//! Versioned serialization primitives for `.cslug` VM artifacts.
//!
//! The codec stays adjacent to the VM because decoding is an executable
//! validation boundary. The initial surface establishes the fixed artifact
//! header; graph and bytecode records are added in later codec slices.

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    CallableIdentity, Entrypoint, EntrypointArguments, ForeignResourceSignature, ModuleDeclaration,
    ModuleTag, Program, SourceSpan,
};

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
    /// An unsigned LEB128 integer is malformed or not shortest-form.
    NonCanonicalInteger { context: &'static str },
    /// A bounded artifact resource would exceed the caller's configured limit.
    LimitExceeded { context: &'static str },
    /// Section ordering or framing does not match the version-1 envelope.
    InvalidStructure(String),
    /// A graph index or duplicate graph key is invalid.
    InvalidReference(String),
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
            Self::NonCanonicalInteger { context } => {
                write!(formatter, "invalid .cslug integer for {context}")
            }
            Self::LimitExceeded { context } => {
                write!(formatter, ".cslug limit exceeded: {context}")
            }
            Self::InvalidStructure(message) => {
                write!(formatter, "invalid .cslug structure: {message}")
            }
            Self::InvalidReference(message) => {
                write!(formatter, "invalid .cslug reference: {message}")
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

/// Resource bounds enforced before the envelope reader allocates or recurses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum complete artifact byte length.
    pub artifact_bytes: usize,
    /// Maximum one section payload byte length.
    pub section_bytes: usize,
    /// Maximum closed-graph members.
    pub modules: usize,
    /// Maximum closed-graph import edges.
    pub imports: usize,
    /// Maximum UTF-8 string byte length.
    pub string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            artifact_bytes: 64 * 1024 * 1024,
            section_bytes: 32 * 1024 * 1024,
            modules: 4_096,
            imports: 16_384,
            string_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ImportEdge {
    pub(crate) importer: usize,
    pub(crate) name: String,
    pub(crate) target: usize,
}

#[derive(Debug)]
#[allow(dead_code)] // Exposed to the graph loader once full program decoding lands.
pub(crate) struct Envelope {
    pub(crate) root: usize,
    pub(crate) modules: Vec<ModuleRecord>,
    pub(crate) imports: Vec<ImportEdge>,
}

#[derive(Debug)]
#[allow(dead_code)] // Module fields are consumed by the next graph-loader slice.
pub(crate) struct ModuleRecord {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) program: Program,
}

/// Validates the `.cslug` header and closed-graph envelope without decoding
/// program records.
///
/// # Errors
///
/// Returns a checked error for malformed, noncanonical, oversized, or invalid
/// graph-envelope input.
pub fn validate_envelope(bytes: &[u8], limits: Limits) -> Result<(), Error> {
    let _ = decode_envelope(bytes, limits)?;
    Ok(())
}

pub(crate) fn decode_envelope(bytes: &[u8], limits: Limits) -> Result<Envelope, Error> {
    if bytes.len() > limits.artifact_bytes {
        return Err(Error::LimitExceeded {
            context: "artifact bytes",
        });
    }
    Header::decode(bytes)?;
    let mut reader = Reader::new(&bytes[HEADER_LENGTH..]);
    let graph = read_section(&mut reader, 1, limits.section_bytes, "graph section")?;
    let mut graph_reader = Reader::new(graph);
    let root = graph_reader.usize("root module index")?;
    let module_count = graph_reader.usize("module count")?;
    let import_count = graph_reader.usize("import count")?;
    if module_count > limits.modules {
        return Err(Error::LimitExceeded {
            context: "module count",
        });
    }
    if import_count > limits.imports {
        return Err(Error::LimitExceeded {
            context: "import count",
        });
    }
    let mut imports = Vec::with_capacity(import_count);
    for _ in 0..import_count {
        imports.push(ImportEdge {
            importer: graph_reader.usize("importer module index")?,
            name: graph_reader.string(limits.string_bytes, "import name")?,
            target: graph_reader.usize("import target module index")?,
        });
    }
    graph_reader.finish("graph section")?;
    if module_count == 0 || root >= module_count {
        return Err(Error::InvalidReference("root module index".into()));
    }
    let mut modules = Vec::with_capacity(module_count);
    for _ in 0..module_count {
        let payload = read_section(&mut reader, 2, limits.section_bytes, "module section")?;
        modules.push(decode_module(payload, limits)?);
    }
    let end = read_section(&mut reader, 0, 0, "end section")?;
    if !end.is_empty() {
        return Err(Error::InvalidStructure("end section is not empty".into()));
    }
    reader.finish("artifact")?;
    for edge in &imports {
        if edge.importer >= module_count || edge.target >= module_count {
            return Err(Error::InvalidReference("import module index".into()));
        }
    }
    for pair in modules.windows(2) {
        if pair[0].key >= pair[1].key {
            return Err(Error::InvalidReference(
                "module keys are not strictly sorted".into(),
            ));
        }
    }
    Ok(Envelope {
        root,
        modules,
        imports,
    })
}

fn decode_module(bytes: &[u8], limits: Limits) -> Result<ModuleRecord, Error> {
    let mut reader = Reader::new(bytes);
    let key = reader.string(limits.string_bytes, "module key")?;
    let name = reader.string(limits.string_bytes, "module name")?;
    let program_length = reader.usize("program length")?;
    if program_length > limits.section_bytes {
        return Err(Error::LimitExceeded {
            context: "program length",
        });
    }
    let program = decode_scalar_program(reader.bytes(program_length, "program record")?)?;
    reader.finish("module section")?;
    Ok(ModuleRecord { key, name, program })
}

fn decode_scalar_program(bytes: &[u8]) -> Result<Program, Error> {
    let mut reader = Reader::new(bytes);
    let bindings = reader.strings("bindings")?;
    let exports = reader.strings("exports")?;
    let count = reader.usize("declaration count")?;
    let mut pending = Vec::with_capacity(count);
    for _ in 0..count {
        pending.push(decode_declaration(&mut reader)?);
    }
    let identities = reader
        .strings("callable identities")?
        .into_iter()
        .map(CallableIdentity::new)
        .collect::<Vec<_>>();
    let declarations = pending
        .into_iter()
        .map(|declaration| declaration.finish(&identities))
        .collect::<Result<Vec<_>, _>>()?;
    let entrypoint = match reader.byte("entrypoint")? {
        0 => None,
        1 => {
            let arguments = match reader.byte("entrypoint arguments")? {
                0 => EntrypointArguments::None,
                1 => EntrypointArguments::List,
                2 => EntrypointArguments::Map,
                _ => return Err(Error::InvalidStructure("entrypoint arguments".into())),
            };
            let callable_identity = reader.usize("entrypoint callable identity")?;
            if callable_identity >= identities.len() {
                return Err(Error::InvalidReference(
                    "entrypoint callable identity".into(),
                ));
            }
            Some(Entrypoint::new(arguments, callable_identity))
        }
        _ => return Err(Error::InvalidStructure("entrypoint presence".into())),
    };
    let sources = reader
        .strings("source paths")?
        .into_iter()
        .map(Arc::from)
        .collect::<Vec<Arc<str>>>();
    let span_count = reader.usize("source span count")?;
    let mut spans = Vec::with_capacity(span_count);
    for _ in 0..span_count {
        let source = reader.usize("source span path")?;
        let path = sources
            .get(source)
            .cloned()
            .ok_or_else(|| Error::InvalidReference("source span path".into()))?;
        spans.push(SourceSpan {
            path,
            line: reader.u32("source span line")?,
            column: reader.u32("source span column")?,
        });
    }
    let global_names = reader.strings("global names")?;
    reader.finish("scalar program record")?;
    Ok(Program::from_cslug_scalar_metadata(
        bindings,
        exports,
        declarations,
        entrypoint,
        identities,
        sources,
        spans,
        global_names,
    ))
}

struct PendingDeclaration {
    declaration: ModuleDeclaration,
    foreign_identity: Option<usize>,
}

impl PendingDeclaration {
    fn finish(mut self, identities: &[CallableIdentity]) -> Result<ModuleDeclaration, Error> {
        if let Some(index) = self.foreign_identity {
            self.declaration.foreign_callable_identity = Some(
                identities
                    .get(index)
                    .cloned()
                    .ok_or_else(|| Error::InvalidReference("foreign callable identity".into()))?,
            );
        }
        Ok(self.declaration)
    }
}

fn decode_declaration(reader: &mut Reader<'_>) -> Result<PendingDeclaration, Error> {
    let bindings = reader.strings("declaration bindings")?;
    let mutable = reader.bool("declaration mutable")?;
    let exported = reader.bool("declaration exported")?;
    let foreign = reader.bool("declaration foreign")?;
    let (foreign_arity, foreign_identity) = if foreign {
        let minimum = reader.usize("foreign minimum arity")?;
        let maximum = match reader.byte("foreign maximum arity")? {
            0 => None,
            1 => Some(reader.usize("foreign maximum arity")?),
            _ => return Err(Error::InvalidStructure("foreign maximum arity".into())),
        };
        if maximum.is_some_and(|maximum| maximum < minimum) {
            return Err(Error::InvalidStructure("foreign arity range".into()));
        }
        (
            Some((minimum, maximum)),
            Some(reader.usize("foreign callable identity")?),
        )
    } else {
        (None, None)
    };
    let resource_type = reader.optional_string("resource type")?;
    let foreign_resource_signature = match reader.byte("foreign resource signature")? {
        0 => None,
        1 => Some(decode_resource_signature(reader)?),
        _ => return Err(Error::InvalidStructure("foreign resource signature".into())),
    };
    let documentation = reader.optional_string("declaration documentation")?;
    let tag_count = reader.usize("declaration tag count")?;
    let mut tags = Vec::with_capacity(tag_count);
    for _ in 0..tag_count {
        // Tag expressions execute during module initialization. Persist only
        // their static name; argument values begin empty in a fresh VM.
        tags.push(ModuleTag {
            name: reader.string(1024 * 1024, "declaration tag name")?,
            arguments: Vec::new(),
        });
    }
    Ok(PendingDeclaration {
        declaration: ModuleDeclaration {
            bindings,
            mutable,
            exported,
            foreign,
            foreign_arity,
            resource_type,
            foreign_callable_identity: None,
            foreign_resource_signature,
            documentation,
            tags,
        },
        foreign_identity,
    })
}

fn decode_resource_signature(reader: &mut Reader<'_>) -> Result<ForeignResourceSignature, Error> {
    let count = reader.usize("foreign resource parameter count")?;
    let mut parameters = Vec::with_capacity(count);
    for _ in 0..count {
        parameters.push(decode_resource_identity(reader)?);
    }
    let result = decode_resource_identity(reader)?;
    Ok(ForeignResourceSignature::new(parameters, result))
}

fn decode_resource_identity(
    reader: &mut Reader<'_>,
) -> Result<Option<(Option<String>, String)>, Error> {
    match reader.byte("foreign resource identity")? {
        0 => Ok(None),
        1 => Ok(Some((
            reader.optional_string("foreign resource module")?,
            reader.string(1024 * 1024, "foreign resource name")?,
        ))),
        _ => Err(Error::InvalidStructure("foreign resource identity".into())),
    }
}

fn read_section<'a>(
    reader: &mut Reader<'a>,
    expected_kind: u8,
    max_payload: usize,
    context: &'static str,
) -> Result<&'a [u8], Error> {
    let kind = reader.byte(context)?;
    if kind != expected_kind {
        return Err(Error::InvalidStructure(format!("expected {context}")));
    }
    let length = reader.usize(context)?;
    if length > max_payload {
        return Err(Error::LimitExceeded { context });
    }
    reader.bytes(length, context)
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn byte(&mut self, context: &'static str) -> Result<u8, Error> {
        let byte = *self
            .bytes
            .get(self.cursor)
            .ok_or(Error::Truncated { context })?;
        self.cursor += 1;
        Ok(byte)
    }

    fn bytes(&mut self, length: usize, context: &'static str) -> Result<&'a [u8], Error> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or(Error::LimitExceeded { context })?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(Error::Truncated { context })?;
        self.cursor = end;
        Ok(value)
    }

    fn usize(&mut self, context: &'static str) -> Result<usize, Error> {
        let mut value = 0u64;
        let mut shift = 0u32;
        let mut bytes = 0usize;
        loop {
            let byte = self.byte(context)?;
            bytes += 1;
            if bytes > 10 || shift >= 64 {
                return Err(Error::NonCanonicalInteger { context });
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                if bytes != uleb_length(value) || value > usize::MAX as u64 {
                    return Err(Error::NonCanonicalInteger { context });
                }
                return Ok(value as usize);
            }
            shift += 7;
        }
    }

    fn u32(&mut self, context: &'static str) -> Result<u32, Error> {
        let bytes = self.bytes(4, context)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn string(&mut self, limit: usize, context: &'static str) -> Result<String, Error> {
        let length = self.usize(context)?;
        if length > limit {
            return Err(Error::LimitExceeded { context });
        }
        let bytes = self.bytes(length, context)?;
        core::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidStructure(format!("invalid UTF-8 {context}")))
    }

    fn bool(&mut self, context: &'static str) -> Result<bool, Error> {
        match self.byte(context)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidStructure(format!(
                "invalid boolean {context}"
            ))),
        }
    }

    fn optional_string(&mut self, context: &'static str) -> Result<Option<String>, Error> {
        match self.byte(context)? {
            0 => Ok(None),
            1 => self.string(1024 * 1024, context).map(Some),
            _ => Err(Error::InvalidStructure(format!(
                "invalid optional {context}"
            ))),
        }
    }

    fn strings(&mut self, context: &'static str) -> Result<Vec<String>, Error> {
        let count = self.usize(context)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string(1024 * 1024, context)?);
        }
        Ok(values)
    }

    fn finish(&self, context: &'static str) -> Result<(), Error> {
        if self.cursor == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::InvalidStructure(format!(
                "trailing bytes in {context}"
            )))
        }
    }
}

const fn uleb_length(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use super::{Error, HEADER_LENGTH, Header, Limits, decode_envelope, encode_header};

    fn uleb(mut value: usize, output: &mut Vec<u8>) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    fn section(kind: u8, payload: &[u8], output: &mut Vec<u8>) {
        output.push(kind);
        uleb(payload.len(), output);
        output.extend(payload);
    }

    fn minimal_envelope() -> Vec<u8> {
        let mut encoded = Vec::new();
        encode_header(&mut encoded);
        section(1, &[0, 1, 0], &mut encoded);
        let mut module = Vec::new();
        uleb(4, &mut module);
        module.extend(b"root");
        uleb(0, &mut module);
        uleb(8, &mut module);
        module.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        section(2, &module, &mut encoded);
        section(0, &[], &mut encoded);
        encoded
    }

    fn string(value: &str, output: &mut Vec<u8>) {
        uleb(value.len(), output);
        output.extend(value.as_bytes());
    }

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

    #[test]
    fn validates_the_bounded_closed_graph_envelope() {
        let artifact = minimal_envelope();
        let envelope = decode_envelope(&artifact, Limits::default()).expect("valid envelope");

        assert_eq!(envelope.root, 0);
        assert_eq!(envelope.modules[0].key, "root");
        assert_eq!(envelope.modules[0].name, "");
        assert!(envelope.modules[0].program.bindings().is_empty());
        assert!(envelope.imports.is_empty());
    }

    #[test]
    fn rejects_noncanonical_or_unordered_envelope_data() {
        let mut noncanonical = minimal_envelope();
        noncanonical[HEADER_LENGTH + 1] = 0x83;
        noncanonical.insert(HEADER_LENGTH + 2, 0);
        assert!(matches!(
            decode_envelope(&noncanonical, Limits::default()),
            Err(Error::NonCanonicalInteger {
                context: "graph section"
            })
        ));

        let mut wrong_order = Vec::new();
        encode_header(&mut wrong_order);
        section(2, &[], &mut wrong_order);
        assert!(matches!(
            decode_envelope(&wrong_order, Limits::default()),
            Err(Error::InvalidStructure(_))
        ));
    }

    #[test]
    fn decodes_declaration_tag_names_without_live_arguments() {
        let mut record = Vec::new();
        uleb(0, &mut record); // bindings
        uleb(0, &mut record); // exports
        uleb(1, &mut record); // declarations
        uleb(0, &mut record); // declaration bindings
        record.extend([0, 0, 0]); // mutable, exported, foreign
        record.push(0); // resource type
        record.push(0); // foreign resource signature
        record.push(0); // documentation
        uleb(1, &mut record); // tags
        string("audit", &mut record);
        uleb(0, &mut record); // callable identities
        record.push(0); // no entrypoint
        uleb(0, &mut record); // source paths
        uleb(0, &mut record); // source spans
        uleb(0, &mut record); // global names

        let mut artifact = Vec::new();
        encode_header(&mut artifact);
        section(1, &[0, 1, 0], &mut artifact);
        let mut module = Vec::new();
        string("root", &mut module);
        string("", &mut module);
        uleb(record.len(), &mut module);
        module.extend(record);
        section(2, &module, &mut artifact);
        section(0, &[], &mut artifact);

        let envelope = decode_envelope(&artifact, Limits::default()).expect("valid artifact");
        let tags = &envelope.modules[0].program.declarations()[0].tags;
        assert_eq!(tags[0].name, "audit");
        assert!(tags[0].arguments.is_empty());
    }

    #[test]
    fn decodes_source_paths_and_spans() {
        let mut record = vec![0, 0, 0, 0, 0];
        uleb(1, &mut record); // source paths
        string("main.slug", &mut record);
        uleb(1, &mut record); // source spans
        uleb(0, &mut record); // source index
        record.extend(4u32.to_le_bytes());
        record.extend(8u32.to_le_bytes());
        uleb(1, &mut record); // global names
        string("value", &mut record);

        let mut artifact = Vec::new();
        encode_header(&mut artifact);
        section(1, &[0, 1, 0], &mut artifact);
        let mut module = Vec::new();
        string("root", &mut module);
        string("", &mut module);
        uleb(record.len(), &mut module);
        module.extend(record);
        section(2, &module, &mut artifact);
        section(0, &[], &mut artifact);

        let envelope = decode_envelope(&artifact, Limits::default()).expect("valid artifact");
        let program = &envelope.modules[0].program;
        assert_eq!(program.source_count(), 1);
        assert_eq!(program.span_count(), 1);
    }
}
