use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use slug_vm::{
    CallArgumentKind, Chunk, EmptyVmConfiguration, NativeDescriptorError, NativeFunction,
    NativeResourceRegistry, Op, ProgramBuilder, Value, Vm, VmConfiguration, VmHost, VmHostError,
    VmModuleExports,
};

struct RecordingHost {
    imports: RefCell<Vec<(Option<String>, String)>>,
    warnings: RefCell<Vec<String>>,
    shutdown: Cell<bool>,
    configuration: EmptyVmConfiguration,
    native_resources: NativeResourceRegistry,
}

impl RecordingHost {
    fn new() -> Self {
        Self {
            imports: RefCell::new(Vec::new()),
            warnings: RefCell::new(Vec::new()),
            shutdown: Cell::new(false),
            configuration: EmptyVmConfiguration,
            native_resources: NativeResourceRegistry::new(),
        }
    }
}

impl VmHost for RecordingHost {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        self.imports
            .borrow_mut()
            .push((importer.map(str::to_owned), name.into()));
        match name {
            "slug.builtin" => Err(VmHostError::not_found("no host builtins")),
            "first" | "second" => Ok(VmModuleExports {
                exports: Value::Map(vec![(Value::string("answer"), Value::Int(42))].into()),
            }),
            _ => Err(VmHostError::not_found(format!("missing {name}"))),
        }
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn foreign_function(&self, _module: &str, _name: &str) -> Option<NativeFunction> {
        None
    }

    fn define_foreign_batch(
        &self,
        _functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        Ok(())
    }

    fn define_native_global(&self, _name: String, _value: Value) {}

    fn native_resources(&self) -> NativeResourceRegistry {
        self.native_resources.clone()
    }

    fn configuration(&self) -> &dyn VmConfiguration {
        &self.configuration
    }

    fn warn(&self, message: String) {
        self.warnings.borrow_mut().push(message);
    }

    fn shutdown(&self) {
        self.shutdown.set(true);
    }
}

#[test]
fn host_callback_drives_live_imports_warnings_and_shutdown() {
    let mut main = Chunk::new("main", 0);
    let first = main.constant(Value::string("first"));
    let second = main.constant(Value::string("second"));
    main.emit(Op::Constant(first))
        .emit(Op::Constant(second))
        .emit(Op::Import(vec![
            CallArgumentKind::Positional,
            CallArgumentKind::Positional,
        ]))
        .emit(Op::Return);
    let mut builder = ProgramBuilder::new();
    builder.add_chunk(main);
    let program = builder.finish();
    let host = Rc::new(RecordingHost::new());
    let mut vm = Vm::with_host(host.clone());

    assert_eq!(
        vm.run(&program, 0).expect("host import succeeds"),
        Value::Map(vec![(Value::string("answer"), Value::Int(42))].into()),
    );
    assert_eq!(
        host.imports.borrow().as_slice(),
        [(None, "first".into()), (None, "second".into()),],
    );
    assert_eq!(host.warnings.borrow().len(), 1);
    vm.shutdown();
    assert!(host.shutdown.get());
}

#[test]
fn program_builder_retains_frontend_module_metadata() {
    let mut builder = ProgramBuilder::new();
    builder.set_bindings(vec!["answer".into()]);
    builder.set_exports(vec!["answer".into()]);
    let program = builder.finish();

    assert_eq!(program.bindings(), ["answer"]);
    assert_eq!(program.exports(), ["answer"]);
}
