use std::rc::Rc;

use slug_frontend::ModuleLoader;
use slug_nil_loader::NilLoader;
use slug_vm::{Configuration, Value, Vm};

#[test]
fn in_memory_nil_host_evaluates_without_desktop_capabilities() {
    let loader = ModuleLoader::with_resolver(Rc::new(NilLoader), Configuration::default());
    let program = loader
        .compile_source("memory:entry", "40 + 2\n")
        .expect("compile in-memory entry source");
    let mut vm = Vm::with_host(Rc::new(loader));

    assert_eq!(
        vm.run_named(&program, "main")
            .expect("evaluate in-memory entry source"),
        Value::Int(42)
    );
}

#[test]
fn in_memory_nil_host_rejects_external_imports_as_checked_errors() {
    let loader = ModuleLoader::with_resolver(Rc::new(NilLoader), Configuration::default());
    let program = loader
        .compile_source("memory:entry", "import(\"outside.module\")\n")
        .expect("compile unresolved runtime import");
    let mut vm = Vm::with_host(Rc::new(loader));

    let error = vm
        .run_named(&program, "main")
        .expect_err("nil loader must reject external imports");
    assert!(
        error
            .to_string()
            .contains("module `outside.module` was not found")
    );
}
