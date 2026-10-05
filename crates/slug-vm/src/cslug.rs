//! Versioned serialization primitives for `.cslug` VM artifacts.
//!
//! The codec stays adjacent to the VM because decoding is an executable
//! validation boundary. The initial surface establishes the fixed artifact
//! header; graph and bytecode records are added in later codec slices.

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    CallArgumentKind, CallableIdentity, Entrypoint, EntrypointArguments, ForeignResourceSignature,
    MatchMapKey, MatchPattern, MatchRest, MatchType, ModuleDeclaration, ModuleTag, PackedOpcode,
    ParameterSignature, Program, SchemaField, SelectCase, SourceSpan, SpanId, Value,
};
use crate::{
    Capture, Constant,
    bytecode::{CallArgumentsId, CompiledChunk, PackedInstruction},
};

/// The exact byte count of a version-1 `.cslug` header.
pub const HEADER_LENGTH: usize = 12;

const MAGIC: [u8; 4] = *b"CSLG";
const FORMAT_VERSION: u16 = 1;
const VM_COMPATIBILITY_VERSION: u16 = 1;
const DEFAULT_STRING_BYTES: usize = 1024 * 1024;

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
    /// A value is outside the source-derived version-1 subset.
    UnsupportedValue(String),
    /// Decoded bytecode violates a VM executable invariant.
    InvalidExecutable(String),
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
            Self::UnsupportedValue(message) => {
                write!(formatter, "unsupported .cslug value: {message}")
            }
            Self::InvalidExecutable(message) => {
                write!(formatter, "invalid .cslug executable: {message}")
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
    /// Maximum chunks in one decoded module program.
    pub chunks_per_module: usize,
    /// Maximum entries in one decoded program table or chunk pool.
    pub entries_per_table: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            artifact_bytes: 64 * 1024 * 1024,
            section_bytes: 32 * 1024 * 1024,
            modules: 4_096,
            imports: 16_384,
            string_bytes: DEFAULT_STRING_BYTES,
            chunks_per_module: 65_535,
            entries_per_table: 1_048_576,
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
    let mut graph_reader = Reader::with_limits(graph, usize::MAX, limits.string_bytes);
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
            name: graph_reader.string("import name")?,
            target: graph_reader.usize("import target module index")?,
        });
    }
    graph_reader.finish("graph section")?;
    if module_count == 0 || root >= module_count {
        return Err(Error::InvalidReference("root module index".into()));
    }
    for edge in &imports {
        if edge.importer >= module_count || edge.target >= module_count {
            return Err(Error::InvalidReference("import module index".into()));
        }
    }
    for pair in imports.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.importer == current.importer && previous.name == current.name {
            return Err(Error::InvalidReference("duplicate import edge".into()));
        }
        if (previous.importer, &previous.name, previous.target)
            >= (current.importer, &current.name, current.target)
        {
            return Err(Error::InvalidReference(
                "import edges are not strictly sorted".into(),
            ));
        }
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
    let mut reader = Reader::with_limits(bytes, usize::MAX, limits.string_bytes);
    let key = reader.string("module key")?;
    let name = reader.string("module name")?;
    let program_length = reader.usize("program length")?;
    if program_length > limits.section_bytes {
        return Err(Error::LimitExceeded {
            context: "program length",
        });
    }
    let program = decode_scalar_program(
        reader.bytes(program_length, "program record")?,
        limits.chunks_per_module,
        limits.entries_per_table,
        limits.string_bytes,
    )?;
    reader.finish("module section")?;
    Ok(ModuleRecord { key, name, program })
}

#[allow(clippy::too_many_lines)]
fn decode_scalar_program(
    bytes: &[u8],
    max_chunks: usize,
    max_entries: usize,
    max_string_bytes: usize,
) -> Result<Program, Error> {
    let mut reader = Reader::with_limits(bytes, max_entries, max_string_bytes);
    let bindings = reader.strings("bindings")?;
    let exports = reader.strings("exports")?;
    let count = reader.count("declaration count")?;
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
    let span_count = reader.count("source span count")?;
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
    let capture_list_count = reader.count("capture list count")?;
    let mut capture_lists = Vec::with_capacity(capture_list_count);
    for _ in 0..capture_list_count {
        let capture_count = reader.count("capture count")?;
        let mut captures = Vec::with_capacity(capture_count);
        for _ in 0..capture_count {
            captures.push(match reader.byte("capture kind")? {
                0 => Capture::Local(reader.usize("capture local slot")?),
                1 => Capture::Capture(reader.usize("capture enclosing slot")?),
                _ => return Err(Error::InvalidStructure("capture kind".into())),
            });
        }
        capture_lists.push(captures);
    }
    let schema_field_list_count = reader.count("schema field list count")?;
    let mut schema_fields = Vec::with_capacity(schema_field_list_count);
    for _ in 0..schema_field_list_count {
        let count = reader.count("schema field count")?;
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            fields.push(SchemaField {
                name: reader.string("schema field name")?,
                has_default: reader.bool("schema field default")?,
            });
        }
        schema_fields.push(fields);
    }
    let struct_fields = reader.strings_lists("struct field lists")?;
    let match_pattern_count = reader.count("match pattern count")?;
    let mut match_patterns = Vec::with_capacity(match_pattern_count);
    for _ in 0..match_pattern_count {
        match_patterns.push(reader.match_pattern(0)?);
    }
    let interpolations = reader.strings_lists("interpolation lists")?;
    let list_spreads = reader.bool_lists("list spread lists")?;
    let call_arguments = reader.call_argument_lists()?;
    let selected_call_count = reader.count("selected call count")?;
    let mut selected_calls = Vec::with_capacity(selected_call_count);
    for _ in 0..selected_call_count {
        let arguments = reader.usize("selected call arguments")?;
        if arguments >= call_arguments.len() {
            return Err(Error::InvalidReference("selected call arguments".into()));
        }
        selected_calls.push((
            CallArgumentsId::new(
                u32::try_from(arguments)
                    .map_err(|_| Error::InvalidReference("selected call arguments".into()))?,
            ),
            reader.usize("selected call identity")?,
        ));
    }
    let select_cases = reader.select_case_lists()?;
    let chunk_count = reader.count("chunk count")?;
    if chunk_count > max_chunks {
        return Err(Error::LimitExceeded {
            context: "chunk count",
        });
    }
    let mut chunks = Vec::with_capacity(chunk_count);
    for _ in 0..chunk_count {
        chunks.push(reader.chunk()?);
    }
    reader.finish("scalar program record")?;
    let mut program = Program::from_cslug_scalar_metadata(
        bindings,
        exports,
        declarations,
        entrypoint,
        identities,
        sources,
        spans,
        global_names,
        capture_lists,
        schema_fields,
        struct_fields,
        match_patterns,
        interpolations,
        list_spreads,
        call_arguments,
        selected_calls,
        select_cases,
    );
    program.set_cslug_chunks(chunks);
    let entry = program
        .find_chunk("main")
        .ok_or_else(|| Error::InvalidExecutable("program is missing main entry chunk".into()))?;
    program.validate(entry).map_err(Error::InvalidExecutable)?;
    Ok(program)
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
    let tag_count = reader.count("declaration tag count")?;
    let mut tags = Vec::with_capacity(tag_count);
    for _ in 0..tag_count {
        // Tag expressions execute during module initialization. Persist only
        // their static name; argument values begin empty in a fresh VM.
        tags.push(ModuleTag {
            name: reader.string("declaration tag name")?,
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
    let count = reader.count("foreign resource parameter count")?;
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
            reader.string("foreign resource name")?,
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
    entry_limit: usize,
    string_limit: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self::with_limits(bytes, usize::MAX, DEFAULT_STRING_BYTES)
    }

    const fn with_limits(bytes: &'a [u8], entry_limit: usize, string_limit: usize) -> Self {
        Self {
            bytes,
            cursor: 0,
            entry_limit,
            string_limit,
        }
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
        let value = self.u64(context)?;
        if value > usize::MAX as u64 {
            return Err(Error::NonCanonicalInteger { context });
        }
        usize::try_from(value).map_err(|_| Error::NonCanonicalInteger { context })
    }

    fn count(&mut self, context: &'static str) -> Result<usize, Error> {
        let count = self.usize(context)?;
        if count > self.entry_limit {
            return Err(Error::LimitExceeded { context });
        }
        Ok(count)
    }

    fn u64(&mut self, context: &'static str) -> Result<u64, Error> {
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
                if bytes != uleb_length(value) {
                    return Err(Error::NonCanonicalInteger { context });
                }
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn u32(&mut self, context: &'static str) -> Result<u32, Error> {
        let bytes = self.bytes(4, context)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i64(&mut self, context: &'static str) -> Result<i64, Error> {
        let encoded = self.u64(context)?;
        let value =
            i64::try_from(encoded >> 1).map_err(|_| Error::NonCanonicalInteger { context })?;
        Ok(value ^ -i64::from((encoded & 1) as u8))
    }

    fn value(&mut self, context: &'static str) -> Result<Value, Error> {
        match self.byte(context)? {
            0 => Ok(Value::Nil),
            1 => Ok(Value::Bool(false)),
            2 => Ok(Value::Bool(true)),
            3 => Ok(Value::Int(self.i64(context)?)),
            4 => {
                let bytes = self.bytes(8, context)?;
                let value = f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                if !value.is_finite() {
                    return Err(Error::UnsupportedValue("non-finite number".into()));
                }
                Ok(Value::Float(value))
            }
            5 => Ok(Value::string(self.string(context)?)),
            6 => {
                let length = self.usize(context)?;
                Ok(Value::Bytes(alloc::rc::Rc::new(
                    self.bytes(length, context)?.to_vec(),
                )))
            }
            _ => Err(Error::UnsupportedValue("value tag".into())),
        }
    }

    fn string(&mut self, context: &'static str) -> Result<String, Error> {
        let length = self.usize(context)?;
        if length > self.string_limit {
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
            1 => self.string(context).map(Some),
            _ => Err(Error::InvalidStructure(format!(
                "invalid optional {context}"
            ))),
        }
    }

    fn strings(&mut self, context: &'static str) -> Result<Vec<String>, Error> {
        let count = self.count(context)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string(context)?);
        }
        Ok(values)
    }

    fn strings_lists(&mut self, context: &'static str) -> Result<Vec<Vec<String>>, Error> {
        let count = self.count(context)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.strings(context)?);
        }
        Ok(values)
    }

    fn bool_lists(&mut self, context: &'static str) -> Result<Vec<Vec<bool>>, Error> {
        let count = self.count(context)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let items = self.count(context)?;
            let mut list = Vec::with_capacity(items);
            for _ in 0..items {
                list.push(self.bool(context)?);
            }
            values.push(list);
        }
        Ok(values)
    }

    fn call_argument_lists(&mut self) -> Result<Vec<Vec<CallArgumentKind>>, Error> {
        let count = self.count("call argument lists")?;
        let mut lists = Vec::with_capacity(count);
        for _ in 0..count {
            let argument_count = self.count("call argument count")?;
            let mut arguments = Vec::with_capacity(argument_count);
            for _ in 0..argument_count {
                arguments.push(match self.byte("call argument kind")? {
                    0 => CallArgumentKind::Positional,
                    1 => CallArgumentKind::Spread,
                    2 => CallArgumentKind::Named(self.string("call argument name")?),
                    _ => return Err(Error::InvalidStructure("call argument kind".into())),
                });
            }
            lists.push(arguments);
        }
        Ok(lists)
    }

    fn match_pattern(&mut self, depth: usize) -> Result<MatchPattern, Error> {
        if depth >= 128 {
            return Err(Error::LimitExceeded {
                context: "match pattern depth",
            });
        }
        match self.byte("match pattern kind")? {
            0 => Ok(MatchPattern::Literal(self.value("match literal")?)),
            1 => Ok(MatchPattern::Enum(self.usize("match enum operand")?)),
            2 => Ok(MatchPattern::Wildcard),
            3 => Ok(MatchPattern::Binding),
            4 => Ok(MatchPattern::Pinned(self.usize("match pinned operand")?)),
            5 => Ok(MatchPattern::At(Box::new(self.match_pattern(depth + 1)?))),
            6 => {
                let count = self.count("match alternative count")?;
                let mut patterns = Vec::with_capacity(count);
                for _ in 0..count {
                    patterns.push(self.match_pattern(depth + 1)?);
                }
                Ok(MatchPattern::Alternatives(patterns))
            }
            7 => {
                let count = self.count("match list item count")?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.match_pattern(depth + 1)?);
                }
                Ok(MatchPattern::List {
                    items,
                    rest: self.match_rest()?,
                })
            }
            8 => {
                let count = self.count("match map entry count")?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let key = match self.byte("match map key kind")? {
                        0 => MatchMapKey::String(self.string("match map key")?),
                        1 => MatchMapKey::Operand(self.usize("match map key operand")?),
                        _ => return Err(Error::InvalidStructure("match map key kind".into())),
                    };
                    entries.push((key, self.match_pattern(depth + 1)?));
                }
                Ok(MatchPattern::Map {
                    entries,
                    rest: self.match_rest()?,
                    exact: self.bool("match map exact")?,
                })
            }
            9 => Ok(MatchPattern::Constrained {
                pattern: Box::new(self.match_pattern(depth + 1)?),
                constraint: self.match_type(depth + 1)?,
            }),
            _ => Err(Error::InvalidStructure("match pattern kind".into())),
        }
    }

    fn match_rest(&mut self) -> Result<MatchRest, Error> {
        match self.byte("match rest")? {
            0 => Ok(MatchRest::None),
            1 => Ok(MatchRest::Discard),
            2 => Ok(MatchRest::Binding),
            _ => Err(Error::InvalidStructure("match rest".into())),
        }
    }

    fn match_type(&mut self, depth: usize) -> Result<MatchType, Error> {
        if depth >= 128 {
            return Err(Error::LimitExceeded {
                context: "match type depth",
            });
        }
        match self.byte("match type kind")? {
            0 => Ok(MatchType::Any),
            1 => Ok(MatchType::Nil),
            2 => Ok(MatchType::Bool),
            3 => Ok(MatchType::Num),
            4 => Ok(MatchType::Str),
            5 => Ok(MatchType::Bytes),
            6 => Ok(MatchType::Resource {
                module: self.string("match resource module")?,
                name: self.string("match resource name")?,
            }),
            7 => Ok(MatchType::Enum {
                module: self.string("match enum module")?,
                name: self.string("match enum name")?,
            }),
            8 => Ok(MatchType::List(self.optional_match_type(depth + 1)?)),
            9 => Ok(MatchType::Map(
                match self.byte("match map type presence")? {
                    0 => None,
                    1 => Some((
                        Box::new(self.match_type(depth + 1)?),
                        Box::new(self.match_type(depth + 1)?),
                    )),
                    _ => return Err(Error::InvalidStructure("match map type presence".into())),
                },
            )),
            10 => Ok(MatchType::Function),
            11 => Ok(MatchType::Task),
            12 => Ok(MatchType::Channel),
            13 => Ok(MatchType::Schema),
            14 => Ok(MatchType::Struct(
                match self.byte("match struct type presence")? {
                    0 => None,
                    1 => Some(self.usize("match struct operand")?),
                    _ => return Err(Error::InvalidStructure("match struct type presence".into())),
                },
            )),
            15 => {
                let count = self.count("match union type count")?;
                let mut types = Vec::with_capacity(count);
                for _ in 0..count {
                    types.push(self.match_type(depth + 1)?);
                }
                Ok(MatchType::Union(types))
            }
            _ => Err(Error::InvalidStructure("match type kind".into())),
        }
    }

    fn optional_match_type(&mut self, depth: usize) -> Result<Option<Box<MatchType>>, Error> {
        match self.byte("match type presence")? {
            0 => Ok(None),
            1 => Ok(Some(Box::new(self.match_type(depth)?))),
            _ => Err(Error::InvalidStructure("match type presence".into())),
        }
    }

    fn select_case_lists(&mut self) -> Result<Vec<Vec<SelectCase>>, Error> {
        let count = self.count("select case lists")?;
        let mut lists = Vec::with_capacity(count);
        for _ in 0..count {
            let case_count = self.count("select case count")?;
            let mut cases = Vec::with_capacity(case_count);
            for _ in 0..case_count {
                let has_handler = self.bool("select case handler")?;
                cases.push(match self.byte("select case kind")? {
                    0 => SelectCase::Receive { has_handler },
                    1 => SelectCase::Send { has_handler },
                    2 => SelectCase::After { has_handler },
                    3 => SelectCase::Await { has_handler },
                    4 => SelectCase::Default { has_handler },
                    _ => return Err(Error::InvalidStructure("select case kind".into())),
                });
            }
            lists.push(cases);
        }
        Ok(lists)
    }

    fn chunk(&mut self) -> Result<CompiledChunk, Error> {
        let name = self.string("chunk name")?;
        let arity = self.usize("chunk arity")?;
        let parameter_count = self.count("chunk parameter count")?;
        let mut parameters = Vec::with_capacity(parameter_count);
        for _ in 0..parameter_count {
            parameters.push(ParameterSignature {
                name: self.string("parameter name")?,
                has_default: self.bool("parameter default")?,
                variadic: self.bool("parameter variadic")?,
            });
        }
        let callable_identity = match self.byte("chunk callable identity")? {
            0 => None,
            1 => Some(self.usize("chunk callable identity")?),
            _ => return Err(Error::InvalidStructure("chunk callable identity".into())),
        };
        let locals = self.usize("chunk local count")?;
        let constant_count = self.count("constant count")?;
        let mut constants = Vec::with_capacity(constant_count);
        for _ in 0..constant_count {
            constants.push(match self.byte("constant kind")? {
                0 => Constant::Value(self.value("constant value")?),
                1 => Constant::Function(self.usize("constant function chunk")?),
                _ => return Err(Error::InvalidStructure("constant kind".into())),
            });
        }
        let instruction_count = self.count("instruction count")?;
        let mut code = Vec::with_capacity(instruction_count);
        for _ in 0..instruction_count {
            let opcode = PackedOpcode::from_cslug_tag(self.byte("opcode")?)
                .ok_or_else(|| Error::InvalidStructure("opcode".into()))?;
            let a = self.u32_uleb("instruction operand")?;
            let b = self.u32_uleb("instruction operand")?;
            let c = self.u32_uleb("instruction operand")?;
            let span = match self.usize("instruction span")? {
                0 => None,
                index => Some(SpanId::new(u32::try_from(index - 1).map_err(|_| {
                    Error::InvalidReference("instruction source span".into())
                })?)),
            };
            code.push(PackedInstruction::from_cslug(opcode, a, b, c, span));
        }
        Ok(CompiledChunk::from_cslug(
            name,
            arity,
            parameters,
            callable_identity,
            locals,
            constants,
            code,
        ))
    }

    fn u32_uleb(&mut self, context: &'static str) -> Result<u32, Error> {
        u32::try_from(self.u64(context)?).map_err(|_| Error::NonCanonicalInteger { context })
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
    use super::{Error, HEADER_LENGTH, Header, Limits, Reader, decode_envelope, encode_header};
    use crate::{PackedOpcode, Value};

    fn uleb(mut value: usize, output: &mut Vec<u8>) {
        loop {
            let mut byte = u8::try_from(value & 0x7f).expect("ULEB128 byte is bounded");
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
        let program = empty_program();
        uleb(program.len(), &mut module);
        module.extend(program);
        section(2, &module, &mut encoded);
        section(0, &[], &mut encoded);
        encoded
    }

    fn string(value: &str, output: &mut Vec<u8>) {
        uleb(value.len(), output);
        output.extend(value.as_bytes());
    }

    fn empty_program() -> Vec<u8> {
        let mut program = vec![0; 17]; // Empty scalar metadata tables.
        main_chunk(&mut program);
        program
    }

    fn main_chunk(output: &mut Vec<u8>) {
        uleb(1, output); // chunks
        string("main", output);
        uleb(0, output); // arity
        uleb(0, output); // parameters
        output.push(0); // callable identity
        uleb(0, output); // locals
        uleb(0, output); // constants
        uleb(2, output); // instructions
        output.push(PackedOpcode::Nil as u8);
        output.extend([0, 0, 0, 0]); // unused operands, no span
        output.push(PackedOpcode::Return as u8);
        output.extend([0, 0, 0, 0]); // unused operands, no span
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
    fn rejects_duplicate_import_edges() {
        let mut artifact = Vec::new();
        encode_header(&mut artifact);
        let mut graph = vec![0, 1, 2]; // root, modules, imports
        for _ in 0..2 {
            graph.push(0); // importer
            string("dep", &mut graph);
            graph.push(0); // target
        }
        section(1, &graph, &mut artifact);
        let mut module = Vec::new();
        string("root", &mut module);
        string("", &mut module);
        uleb(18, &mut module);
        module.extend([0; 18]);
        section(2, &module, &mut artifact);
        section(0, &[], &mut artifact);

        assert!(matches!(
            decode_envelope(&artifact, Limits::default()),
            Err(Error::InvalidReference(message)) if message == "duplicate import edge"
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
        uleb(0, &mut record); // capture lists
        uleb(0, &mut record); // schema field lists
        uleb(0, &mut record); // struct field lists
        uleb(1, &mut record); // match patterns
        record.push(9); // constrained pattern
        record.push(3); // binding
        record.push(4); // string constraint
        uleb(0, &mut record); // interpolation lists
        uleb(0, &mut record); // list spread lists
        uleb(0, &mut record); // call argument lists
        uleb(0, &mut record); // selected calls
        uleb(0, &mut record); // select case lists
        main_chunk(&mut record);

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
        uleb(1, &mut record); // capture lists
        uleb(2, &mut record); // captures
        record.push(0); // local
        uleb(3, &mut record);
        record.push(1); // enclosing capture
        uleb(1, &mut record);
        uleb(1, &mut record); // schema field lists
        uleb(1, &mut record); // schema fields
        string("field", &mut record);
        record.push(1); // default
        uleb(1, &mut record); // struct field lists
        uleb(1, &mut record); // struct fields
        string("field", &mut record);
        uleb(0, &mut record); // match patterns
        uleb(0, &mut record); // interpolation lists
        uleb(0, &mut record); // list spread lists
        uleb(0, &mut record); // call argument lists
        uleb(0, &mut record); // selected calls
        uleb(0, &mut record); // select case lists
        main_chunk(&mut record);

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
        assert_eq!(program.layout_metrics().metadata_pool_slots, 4);
    }

    #[test]
    fn decodes_packed_chunks_and_source_constants() {
        let mut record = vec![0; 17]; // Empty scalar metadata tables.
        uleb(2, &mut record); // chunks
        string("<fn #0>", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(0); // callable identity
        uleb(0, &mut record); // locals
        uleb(1, &mut record); // constants
        record.push(0); // source literal constant
        record.push(3); // integer value
        uleb(42, &mut record); // ZigZag 21
        uleb(2, &mut record); // instructions
        record.push(PackedOpcode::Constant as u8);
        record.extend([0, 0, 0, 0]); // constant index, unused operands, no span
        record.push(PackedOpcode::Return as u8);
        record.extend([0, 0, 0, 0]); // unused operands, no span
        string("main", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(0); // callable identity
        uleb(0, &mut record); // locals
        uleb(0, &mut record); // constants
        uleb(2, &mut record); // instructions
        record.push(PackedOpcode::Nil as u8);
        record.extend([0, 0, 0, 0]); // unused operands, no span
        record.push(PackedOpcode::Return as u8);
        record.extend([0, 0, 0, 0]); // unused operands, no span

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
        let chunk = program.chunk(0).expect("function chunk");
        assert_eq!(chunk.name, "<fn #0>");
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.code.len(), 2);
        assert_eq!(program.find_chunk("main"), Some(1));
    }

    #[test]
    fn enforces_the_chunk_limit_before_allocating_program_chunks() {
        let mut record = vec![0; 17]; // Empty scalar metadata tables.
        uleb(1, &mut record); // chunks
        string("main", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(0); // callable identity
        uleb(0, &mut record); // locals
        uleb(0, &mut record); // constants
        uleb(0, &mut record); // instructions

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

        let limits = Limits {
            chunks_per_module: 0,
            ..Limits::default()
        };
        assert!(matches!(
            decode_envelope(&artifact, limits),
            Err(Error::LimitExceeded {
                context: "chunk count"
            })
        ));
    }

    #[test]
    fn enforces_the_table_entry_limit_before_allocating_metadata() {
        let mut artifact = Vec::new();
        encode_header(&mut artifact);
        section(1, &[0, 1, 0], &mut artifact);
        let mut module = Vec::new();
        string("root", &mut module);
        string("", &mut module);
        module.extend([1, 1]); // program length and one binding entry
        section(2, &module, &mut artifact);
        section(0, &[], &mut artifact);

        let limits = Limits {
            entries_per_table: 0,
            ..Limits::default()
        };
        assert!(matches!(
            decode_envelope(&artifact, limits),
            Err(Error::LimitExceeded {
                context: "bindings"
            })
        ));
    }

    #[test]
    fn enforces_host_string_limits_inside_program_records() {
        let mut artifact = Vec::new();
        encode_header(&mut artifact);
        section(1, &[0, 1, 0], &mut artifact);
        let mut module = Vec::new();
        string("root", &mut module);
        string("", &mut module);
        module.extend([2, 1, 5]); // program length, binding count, binding length
        section(2, &module, &mut artifact);
        section(0, &[], &mut artifact);

        let limits = Limits {
            string_bytes: 4,
            ..Limits::default()
        };
        assert!(matches!(
            decode_envelope(&artifact, limits),
            Err(Error::LimitExceeded {
                context: "bindings"
            })
        ));
    }

    #[test]
    fn rejects_invalid_control_flow_before_returning_a_program() {
        let mut record = vec![0; 17]; // Empty scalar metadata tables.
        uleb(1, &mut record); // chunks
        string("main", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(0); // callable identity
        uleb(0, &mut record); // locals
        uleb(0, &mut record); // constants
        uleb(1, &mut record); // instructions
        record.push(PackedOpcode::Jump as u8);
        record.extend([1, 0, 0, 0]); // missing target, unused operands, no span

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

        assert!(matches!(
            decode_envelope(&artifact, Limits::default()),
            Err(Error::InvalidExecutable(message)) if message.contains("jumps to missing instruction")
        ));
    }

    #[test]
    fn rejects_missing_chunk_callable_identities() {
        let mut record = vec![0; 17]; // Empty scalar metadata tables.
        uleb(1, &mut record); // chunks
        string("main", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(1); // callable identity present
        uleb(0, &mut record); // missing callable identity
        uleb(0, &mut record); // locals
        uleb(0, &mut record); // constants
        uleb(0, &mut record); // instructions

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

        assert!(matches!(
            decode_envelope(&artifact, Limits::default()),
            Err(Error::InvalidExecutable(message)) if message.contains("missing callable identity")
        ));
    }

    #[test]
    fn rejects_programs_without_a_main_entry_chunk() {
        let mut record = vec![0; 17]; // Empty scalar metadata tables.
        uleb(1, &mut record); // chunks
        string("<fn #0>", &mut record);
        uleb(0, &mut record); // arity
        uleb(0, &mut record); // parameters
        record.push(0); // callable identity
        uleb(0, &mut record); // locals
        uleb(0, &mut record); // constants
        uleb(2, &mut record); // instructions
        record.push(PackedOpcode::Nil as u8);
        record.extend([0, 0, 0, 0]); // unused operands, no span
        record.push(PackedOpcode::Return as u8);
        record.extend([0, 0, 0, 0]); // unused operands, no span

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

        assert!(matches!(
            decode_envelope(&artifact, Limits::default()),
            Err(Error::InvalidExecutable(message)) if message == "program is missing main entry chunk"
        ));
    }

    #[test]
    fn decodes_only_finite_source_literal_values() {
        let mut integer = vec![3];
        uleb(5, &mut integer); // ZigZag -3
        assert_eq!(
            Reader::new(&integer).value("value").unwrap(),
            Value::Int(-3)
        );

        let mut non_finite = vec![4];
        non_finite.extend(f64::NAN.to_le_bytes());
        assert!(matches!(
            Reader::new(&non_finite).value("value"),
            Err(Error::UnsupportedValue(_))
        ));
    }
}
