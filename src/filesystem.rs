use std::{
    cell::RefCell,
    io::{BufRead, Write},
    rc::Rc,
};

use crate::{
    ClutchPluginRegistrar, NativeArity, NativeCall, NativeDescriptorError, NativeModule,
    NativeOwnedValue, NativeResourceType, NativeStatus, Vm,
};

enum OpenFile {
    Reader(std::io::BufReader<std::fs::File>),
    Writer(std::fs::File),
}

struct FileHandle(Option<OpenFile>);

struct FilesystemState {
    file: Rc<RefCell<Option<NativeResourceType<FileHandle>>>>,
}

fn file_resource_type(call: &NativeCall<'_>) -> NativeResourceType<FileHandle> {
    call.state::<FilesystemState>()
        .expect("slug.io.fs has matching native state")
        .file
        .borrow()
        .as_ref()
        .expect("slug.io.fs file resource type is registered")
        .clone()
}

fn release_file(handle: &mut FileHandle) {
    handle.0.take();
}

fn destroy_file(_handle: FileHandle) {}

fn native_open_read(call: &mut NativeCall<'_>) -> NativeStatus {
    native_open_file(call, false, false)
}

fn native_open_write(call: &mut NativeCall<'_>) -> NativeStatus {
    native_open_file(call, true, false)
}

fn native_open_append(call: &mut NativeCall<'_>) -> NativeStatus {
    native_open_file(call, true, true)
}

fn native_open_file(call: &mut NativeCall<'_>, write: bool, append: bool) -> NativeStatus {
    let path = match call.argument(0).and_then(crate::NativeValueRef::as_str) {
        Ok(path) => path.to_owned(),
        Err(error) => return call.raise(error),
    };
    let opened = if write {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!append)
            .append(append)
            .open(&path)
            .map(OpenFile::Writer)
    } else {
        std::fs::File::open(&path).map(|file| OpenFile::Reader(std::io::BufReader::new(file)))
    };
    let handle = match opened {
        Ok(handle) => FileHandle(Some(handle)),
        Err(error) => {
            return call.raise(crate::NativeError::new(
                "native.io",
                format!("cannot open {path:?}: {error}"),
            ));
        }
    };
    match call.resource(&file_resource_type(call), handle) {
        Ok(handle) => call.return_value(handle),
        Err(error) => call.raise(error),
    }
}

fn native_read_line(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = call.with_resource(0, &file_resource_type(call), |handle| {
        let Some(OpenFile::Reader(reader)) = handle.0.as_mut() else {
            return Err(crate::NativeError::new(
                "native.mode",
                "file is not open for reading",
            ));
        };
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Some(line))
            }
            Err(error) => Err(crate::NativeError::new(
                "native.io",
                format!("cannot read file: {error}"),
            )),
        }
    });
    match result {
        Ok(Ok(Some(line))) => call.return_value(NativeOwnedValue::string(line)),
        Ok(Ok(None)) => call.return_value(NativeOwnedValue::nil()),
        Ok(Err(error)) | Err(error) => call.raise(error),
    }
}

fn native_write_file(call: &mut NativeCall<'_>) -> NativeStatus {
    let content = match call.argument(1).and_then(crate::NativeValueRef::as_str) {
        Ok(content) => content.to_owned(),
        Err(error) => return call.raise(error),
    };
    let result = call.with_resource(0, &file_resource_type(call), |handle| {
        let Some(OpenFile::Writer(file)) = handle.0.as_mut() else {
            return Err(crate::NativeError::new(
                "native.mode",
                "file is not open for writing",
            ));
        };
        file.write_all(content.as_bytes()).map_err(|error| {
            crate::NativeError::new("native.io", format!("cannot write file: {error}"))
        })?;
        i64::try_from(content.len()).map_err(|_| {
            crate::NativeError::new("native.range", "written byte count exceeds integer range")
        })
    });
    match result {
        Ok(Ok(written)) => call.return_value(NativeOwnedValue::integer(written)),
        Ok(Err(error)) | Err(error) => call.raise(error),
    }
}

fn native_close_file(call: &mut NativeCall<'_>) -> NativeStatus {
    if let Err(error) = call.close_resource(0, &file_resource_type(call)) {
        return call.raise(error);
    }
    call.return_value(NativeOwnedValue::nil())
}

fn functions(module_name: &str) -> Result<Vec<crate::NativeFunction>, NativeDescriptorError> {
    let file = Rc::new(RefCell::new(None));
    let filesystem = NativeModule::new(module_name, FilesystemState { file: file.clone() })?;
    *file.borrow_mut() = Some(filesystem.resource_type("File", release_file, destroy_file)?);
    [
        (
            "openRead",
            NativeArity::Exact(1),
            native_open_read as for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
        ),
        ("openWrite", NativeArity::Exact(1), native_open_write),
        ("openAppend", NativeArity::Exact(1), native_open_append),
        ("readLine", NativeArity::Exact(1), native_read_line),
        ("write", NativeArity::Exact(2), native_write_file),
        ("close", NativeArity::Exact(1), native_close_file),
    ]
    .into_iter()
    .map(|(name, arity, callback)| filesystem.function(name, arity, callback))
    .collect()
}

/// Registers the existing built-in filesystem module for the CLI host.
///
/// # Errors
///
/// Returns an error when a filesystem foreign declaration already exists.
pub fn register_filesystem_foreign(vm: &mut Vm) -> Result<(), NativeDescriptorError> {
    vm.define_foreign_batch(functions("slug.io.fs")?)
}

/// Stages the filesystem capability for one clutch module.
///
/// # Errors
///
/// Returns an error when the clutch scope is not `slug.io.fs` or one of its
/// function registrations is invalid.
pub fn initialize_filesystem_plugin(
    registrar: &mut ClutchPluginRegistrar,
) -> Result<(), NativeDescriptorError> {
    if registrar.module_name() != "slug.io.fs" {
        return Err(NativeDescriptorError::new(
            "filesystem plugin may only serve module slug.io.fs",
        ));
    }
    for function in functions(registrar.module_name())? {
        registrar.define_foreign(function)?;
    }
    Ok(())
}
