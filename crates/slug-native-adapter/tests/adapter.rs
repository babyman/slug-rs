//! Exercises the adapter through the VM with Rust entries that follow the
//! uniform shim contract, so no C compiler is needed.

#![allow(unsafe_code)]

use std::{
    any::Any,
    cell::{Cell as CounterCell, RefCell},
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    rc::Rc,
};

use slug_frontend::{ModuleGraph, ModuleGraphHost};
use slug_loader::{
    ModuleActivationTransaction, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver,
    ModuleSource, StaticModuleProvider, StaticModuleResolver,
};
use slug_native_adapter::{
    ExpectedBinding,
    abi::{Cell, Cleanup, Entry, RawBinding, RawTable, SHIM_ABI_VERSION},
    install,
};
use slug_vm::{
    EmptyVmConfiguration, NativeDescriptorError, NativeFunction, NativeResourceRegistry,
    RuntimeError, RuntimeErrorKind, Value, Vm, VmConfiguration, VmHost, VmHostError,
    VmModuleExports, native_resource_registry,
};

// ---- Rust entries following the shim contract ----

thread_local! {
    static OWNED_FREES: CounterCell<usize> = const { CounterCell::new(0) };
    static RELEASES: CounterCell<usize> = const { CounterCell::new(0) };
    static CLOSES: CounterCell<usize> = const { CounterCell::new(0) };
}

fn count(counter: &'static std::thread::LocalKey<CounterCell<usize>>) -> usize {
    counter.with(CounterCell::get)
}

fn bump(counter: &'static std::thread::LocalKey<CounterCell<usize>>) {
    counter.with(|value| value.set(value.get() + 1));
}

/// # Safety
///
/// `pointer` must point to `count` cells.
unsafe fn cells<'a>(pointer: *const Cell, count: usize) -> &'a [Cell] {
    unsafe { std::slice::from_raw_parts(pointer, count) }
}

/// # Safety
///
/// `pointer` must point to `count` writable cells.
unsafe fn cells_mut<'a>(pointer: *mut Cell, count: usize) -> &'a mut [Cell] {
    unsafe { std::slice::from_raw_parts_mut(pointer, count) }
}

fn text_cell(text: *const c_char) -> Cell {
    if text.is_null() {
        return Cell::pointer(0, 0);
    }
    let length = unsafe { CStr::from_ptr(text) }.to_bytes().len() as u64;
    Cell::pointer(text.expose_provenance(), length)
}

unsafe extern "C" fn add(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 2) };
    (unsafe { cells_mut(results, 1) })[0] =
        Cell::signed(arguments[0].as_signed() + arguments[1].as_signed());
    0
}

unsafe extern "C" fn narrow(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 2) };
    // The adapter range-checks first; report a contract failure if it did not.
    let (Ok(left), Ok(right)) = (
        i8::try_from(arguments[0].as_signed()),
        u16::try_from(arguments[1].word),
    ) else {
        return 9;
    };
    (unsafe { cells_mut(results, 1) })[0] = Cell::signed(i64::from(left) + i64::from(right));
    0
}

unsafe extern "C" fn scale(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 2) };
    #[allow(clippy::cast_possible_truncation)]
    let factor = arguments[0].as_float() as f32;
    (unsafe { cells_mut(results, 1) })[0] =
        Cell::float(f64::from(factor) * arguments[1].as_float());
    0
}

unsafe extern "C" fn flip(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 1) };
    (unsafe { cells_mut(results, 1) })[0] = Cell::unsigned(u64::from(arguments[0].word == 0));
    0
}

unsafe extern "C" fn measure(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 2) };
    let text = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(arguments[0].address()),
            usize::try_from(arguments[0].length).unwrap(),
        )
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(arguments[1].address()),
            usize::try_from(arguments[1].length).unwrap(),
        )
    };
    let total = text.len() * 1000 + bytes.iter().map(|byte| usize::from(*byte)).sum::<usize>();
    (unsafe { cells_mut(results, 1) })[0] = Cell::unsigned(total as u64);
    0
}

unsafe extern "C" fn free_owned(context: *mut c_void) {
    let results = unsafe { cells_mut(context.cast(), 1) };
    let pointer = std::ptr::with_exposed_provenance_mut::<c_char>(results[0].address());
    if !pointer.is_null() {
        drop(unsafe { CString::from_raw(pointer) });
        bump(&OWNED_FREES);
    }
}

fn owned_result(text: CString, results: *mut Cell, cleanup: *mut Cleanup) {
    (unsafe { cells_mut(results, 1) })[0] = text_cell(text.into_raw());
    let cleanup = unsafe { &mut *cleanup };
    cleanup.release = Some(free_owned);
    cleanup.context = results.cast();
}

unsafe extern "C" fn greet(
    arguments: *const Cell,
    results: *mut Cell,
    cleanup: *mut Cleanup,
) -> i32 {
    let arguments = unsafe { cells(arguments, 1) };
    let name = unsafe {
        CStr::from_ptr(std::ptr::with_exposed_provenance::<c_char>(
            arguments[0].address(),
        ))
    };
    let text = format!("hello, {}", name.to_str().unwrap());
    owned_result(CString::new(text).unwrap(), results, cleanup);
    0
}

unsafe extern "C" fn bad_owned(_: *const Cell, results: *mut Cell, cleanup: *mut Cleanup) -> i32 {
    owned_result(CString::new(vec![0xff, 0xfe]).unwrap(), results, cleanup);
    0
}

unsafe extern "C" fn broken_owned(
    _: *const Cell,
    results: *mut Cell,
    cleanup: *mut Cleanup,
) -> i32 {
    owned_result(CString::new("lost").unwrap(), results, cleanup);
    3
}

unsafe extern "C" fn borrowed(_: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    (unsafe { cells_mut(results, 1) })[0] = text_cell(c"static".as_ptr());
    0
}

unsafe extern "C" fn nothing(_: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    (unsafe { cells_mut(results, 1) })[0] = text_cell(std::ptr::null());
    0
}

unsafe extern "C" fn bad_text(_: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    (unsafe { cells_mut(results, 1) })[0] = text_cell(c"\xff".as_ptr());
    0
}

unsafe extern "C" fn huge(_: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    (unsafe { cells_mut(results, 1) })[0] = Cell::unsigned(u64::MAX);
    0
}

unsafe extern "C" fn broken(_: *const Cell, _: *mut Cell, _: *mut Cleanup) -> i32 {
    3
}

struct Counter {
    name: String,
    line: CString,
    count: i64,
}

unsafe extern "C" fn open(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let arguments = unsafe { cells(arguments, 1) };
    let name = unsafe {
        CStr::from_ptr(std::ptr::with_exposed_provenance::<c_char>(
            arguments[0].address(),
        ))
    }
    .to_str()
    .unwrap()
    .to_owned();
    let address = if name == "missing" {
        0
    } else {
        Box::into_raw(Box::new(Counter {
            name,
            line: CString::default(),
            count: 0,
        }))
        .expose_provenance()
    };
    (unsafe { cells_mut(results, 1) })[0] = Cell::pointer(address, 0);
    0
}

fn counter<'a>(cell: Cell) -> &'a mut Counter {
    unsafe { &mut *std::ptr::with_exposed_provenance_mut::<Counter>(cell.address()) }
}

unsafe extern "C" fn next(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let counter = counter(unsafe { cells(arguments, 1) }[0]);
    counter.count += 1;
    counter.line = CString::new(format!("{}-{}", counter.name, counter.count)).unwrap();
    let results = unsafe { cells_mut(results, 3) };
    results[0] = Cell::signed(7);
    results[1] = text_cell(counter.line.as_ptr());
    results[2] = Cell::signed(counter.count);
    0
}

unsafe extern "C" fn close_raw(arguments: *const Cell, results: *mut Cell, _: *mut Cleanup) -> i32 {
    let cell = unsafe { cells(arguments, 1) }[0];
    drop(unsafe {
        Box::from_raw(std::ptr::with_exposed_provenance_mut::<Counter>(
            cell.address(),
        ))
    });
    bump(&CLOSES);
    (unsafe { cells_mut(results, 1) })[0] = Cell::signed(5);
    0
}

unsafe extern "C" fn release(arguments: *const Cell, _: *mut Cell, _: *mut Cleanup) -> i32 {
    let cell = unsafe { cells(arguments, 1) }[0];
    drop(unsafe {
        Box::from_raw(std::ptr::with_exposed_provenance_mut::<Counter>(
            cell.address(),
        ))
    });
    bump(&RELEASES);
    0
}

// ---- A shim table built in Rust ----

struct Spec {
    name: &'static str,
    signature: &'static str,
    entry: Entry,
    arguments: u32,
    results: u32,
}

const fn spec(
    name: &'static str,
    signature: &'static str,
    entry: Entry,
    arguments: u32,
    results: u32,
) -> Spec {
    Spec {
        name,
        signature,
        entry,
        arguments,
        results,
    }
}

#[allow(clippy::too_many_lines)] // one row per fixture binding
fn fixture_specs() -> Vec<Spec> {
    vec![
        spec("add", "slug-shim/1;fx;add;fx_add;(i64,i64)->i64", add, 2, 1),
        spec(
            "narrow",
            "slug-shim/1;fx;narrow;fx_narrow;(i8,u16)->i32",
            narrow,
            2,
            1,
        ),
        spec(
            "scale",
            "slug-shim/1;fx;scale;fx_scale;(f32,f64)->f64",
            scale,
            2,
            1,
        ),
        spec(
            "flip",
            "slug-shim/1;fx;flip;fx_flip;(bool)->bool",
            flip,
            1,
            1,
        ),
        spec(
            "measure",
            "slug-shim/1;fx;measure;fx_measure;(str,bytes)->u64",
            measure,
            2,
            1,
        ),
        spec(
            "greet",
            "slug-shim/1;fx;greet;fx_greet;(cstr)->cstr owned(fx_free)",
            greet,
            1,
            1,
        ),
        spec(
            "badOwned",
            "slug-shim/1;fx;badOwned;fx_bad;()->cstr owned(fx_free)",
            bad_owned,
            0,
            1,
        ),
        spec(
            "brokenOwned",
            "slug-shim/1;fx;brokenOwned;fx_broken;()->cstr owned(fx_free)",
            broken_owned,
            0,
            1,
        ),
        spec(
            "borrowed",
            "slug-shim/1;fx;borrowed;fx_borrowed;()->cstr",
            borrowed,
            0,
            1,
        ),
        spec(
            "nothing",
            "slug-shim/1;fx;nothing;fx_nothing;()->cstr",
            nothing,
            0,
            1,
        ),
        spec(
            "badText",
            "slug-shim/1;fx;badText;fx_bad_text;()->cstr",
            bad_text,
            0,
            1,
        ),
        spec("huge", "slug-shim/1;fx;huge;fx_huge;()->u64", huge, 0, 1),
        spec(
            "broken",
            "slug-shim/1;fx;broken;fx_broken;()->i64",
            broken,
            0,
            1,
        ),
        spec(
            "open",
            "slug-shim/1;fx;open;fx_open;(cstr)->handle<Counter:fx_counter>",
            open,
            1,
            1,
        ),
        spec(
            "next",
            "slug-shim/1;fx;next;fx_next;(handle<Counter:fx_counter>,out cstr,out i64)->i32",
            next,
            1,
            3,
        ),
        spec(
            "closeRaw",
            "slug-shim/1;fx;closeRaw;fx_close;(close handle<Counter:fx_counter>)->i32",
            close_raw,
            1,
            1,
        ),
        spec(
            "release:Counter",
            "slug-shim/1;fx;release:Counter;fx_release;(handle<Counter:fx_counter>)->void",
            release,
            1,
            0,
        ),
    ]
}

/// Owns the strings and array a [`RawTable`] points at.
struct Table {
    _strings: Vec<CString>,
    _bindings: Vec<RawBinding>,
    raw: RawTable,
}

fn table(specs: &[Spec]) -> Table {
    let mut strings = Vec::new();
    let mut string = |text: &str| {
        let text = CString::new(text).unwrap();
        let pointer = text.as_ptr();
        strings.push(text);
        pointer
    };
    let bindings = specs
        .iter()
        .map(|spec| RawBinding {
            module: string(spec.signature.split(';').nth(1).unwrap()),
            name: string(spec.name),
            signature: string(spec.signature),
            entry: Some(spec.entry),
            argument_count: spec.arguments,
            result_count: spec.results,
        })
        .collect::<Vec<_>>();
    let raw = RawTable {
        abi_version: SHIM_ABI_VERSION,
        binding_count: u32::try_from(bindings.len()).unwrap(),
        bindings: bindings.as_ptr(),
    };
    Table {
        _strings: strings,
        _bindings: bindings,
        raw,
    }
}

fn expected(specs: &[Spec]) -> Vec<ExpectedBinding<'static>> {
    specs
        .iter()
        .map(|spec| ExpectedBinding {
            module: spec.signature.split(';').nth(1).unwrap(),
            name: spec.name,
            signature: spec.signature,
        })
        .collect()
}

fn install_table(
    table: &Table,
    expected: &[ExpectedBinding<'_>],
    owner: &Rc<dyn Any>,
) -> Result<Vec<NativeFunction>, String> {
    // SAFETY: `table` and its strings outlive the returned functions in each
    // test, and every entry follows the contract for its listed signature.
    unsafe { install(&raw const table.raw, expected, owner) }.map_err(|error| error.to_string())
}

// ---- A minimal host with one static Slug module ----

const FX_SOURCE: &str = "\
export resource Counter
export foreign add = fn(a:num, b:num):num
export foreign narrow = fn(a:num, b:num):num
export foreign scale = fn(a:num, b:num):num
export foreign flip = fn(value:bool):bool
export foreign measure = fn(text:str, data:bytes):num
export foreign greet = fn(name:str):str
export foreign badOwned = fn():str
export foreign brokenOwned = fn():str
export foreign borrowed = fn():str
export foreign nothing = fn():str|nil
export foreign badText = fn():str
export foreign huge = fn():num
export foreign broken = fn():num
export foreign open = fn(name:str):Counter|nil
export foreign next = fn(counter:Counter):list
export foreign closeRaw = fn(counter:Counter):num|nil
";

#[derive(Clone)]
struct Host(Rc<HostState>);

struct HostState {
    resolver: StaticModuleResolver,
    graph: ModuleGraph,
    configuration: EmptyVmConfiguration,
    foreign: RefCell<HashMap<(String, String), NativeFunction>>,
    resources: NativeResourceRegistry,
}

impl Host {
    fn new(functions: Vec<NativeFunction>) -> Self {
        let provider = StaticModuleProvider::new(
            "fx",
            ModuleSource {
                key: ModuleKey::new("static:fx"),
                diagnostic_name: "static:fx".into(),
                text: FX_SOURCE.into(),
                activation: None,
            },
        )
        .unwrap();
        let foreign = functions
            .into_iter()
            .map(|function| {
                (
                    (function.module_name().into(), function.name().into()),
                    function,
                )
            })
            .collect();
        Self(Rc::new(HostState {
            resolver: StaticModuleResolver::new([provider]).unwrap(),
            graph: ModuleGraph::new(),
            configuration: EmptyVmConfiguration,
            foreign: RefCell::new(foreign),
            resources: native_resource_registry(),
        }))
    }

    fn run(&self, body: &str) -> Result<Value, RuntimeError> {
        let source = format!("val fx = import(\"fx\")\n{body}\n");
        let program = self
            .0
            .graph
            .compile_source(self, "main.slug", &source)
            .unwrap_or_else(|error| panic!("compile `{body}`: {error}"));
        Vm::with_host(Rc::new(self.clone())).run_named(&program, "main")
    }
}

impl ModuleResolver for Host {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.0.resolver.resolve(request)
    }
}

impl ModuleGraphHost for Host {
    fn stage_activation(
        &self,
        _source: &ModuleSource,
    ) -> Result<Option<Box<dyn ModuleActivationTransaction>>, ModuleLoadError> {
        Ok(None)
    }

    /// Non-empty so the module graph supplies a virtual `slug.builtin`.
    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::from([("adapterTest".into(), Value::Nil)])
    }

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: Rc<dyn VmHost> = Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for Host {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let importer = importer.map(ModuleKey::new);
        let instance = self
            .0
            .graph
            .initialize(self, ModuleRequest::new(importer.as_ref(), name))
            .map_err(|error| VmHostError::new(error.to_string()))?;
        Ok(VmModuleExports {
            exports: instance.live_exports(),
        })
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.0
            .foreign
            .borrow()
            .get(&(module.into(), name.into()))
            .cloned()
    }

    fn define_foreign_batch(
        &self,
        _functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        Ok(())
    }

    fn define_native_global(&self, _name: String, _value: Value) {}

    fn native_resources(&self) -> NativeResourceRegistry {
        self.0.resources.clone()
    }

    fn configuration(&self) -> &dyn VmConfiguration {
        &self.0.configuration
    }

    fn warn(&self, _message: String) {}

    fn shutdown(&self) {
        let _ = self.0.resources.finalize_all_for_shutdown();
    }
}

fn fixture_host() -> (Host, Table) {
    let specs = fixture_specs();
    let table = table(&specs);
    let owner: Rc<dyn Any> = Rc::new(());
    let functions = install_table(&table, &expected(&specs), &owner).expect("install fixture");
    (Host::new(functions), table)
}

fn native_code(error: &RuntimeError) -> Option<&str> {
    error.native.as_ref().map(|native| native.code.as_str())
}

// ---- Tests ----

#[test]
fn encodes_and_decodes_scalars_text_and_bytes() {
    let (host, _table) = fixture_host();
    let frees = count(&OWNED_FREES);
    let value = host
        .run(
            "[fx.add(20, 22), fx.narrow(-128, 65535), fx.scale(1.5, 2), fx.scale(3, 0.5), \
             fx.flip(true), fx.measure(\"héllo\", 0x\"0102\"), fx.greet(\"slug\"), \
             fx.borrowed(), fx.nothing()]",
        )
        .expect("run scalar program");
    assert_eq!(
        value.to_string(),
        "[42, 65407, 3, 1.5, false, 6003, \"hello, slug\", \"static\", nil]"
    );
    assert_eq!(count(&OWNED_FREES), frees + 1, "owned text is freed once");
}

#[test]
fn reports_boundary_failures_with_closed_codes() {
    let (host, _table) = fixture_host();
    for (body, code) in [
        ("fx.narrow(128, 0)", "native.range"),
        ("fx.narrow(-129, 0)", "native.range"),
        ("fx.narrow(0, -1)", "native.range"),
        ("fx.narrow(0, 65536)", "native.range"),
        ("fx.scale(1e39, 1)", "native.range"),
        ("dynamic(1.5) /> fx.add(2)", "native.type"),
        ("fx.flip(dynamic(1))", "native.type"),
        ("fx.measure(dynamic(0x\"01\"), 0x\"01\")", "native.type"),
        ("fx.greet(\"a\\0b\")", "native.interior_nul"),
        ("fx.badText()", "native.encoding"),
        ("fx.huge()", "native.range"),
    ] {
        // `dynamic` hides the static type, so the adapter sees the value.
        let program = format!("val dynamic = fn(value) {{ value }}\n{body}");
        let error = host.run(&program).expect_err(body);
        assert_eq!(error.kind, RuntimeErrorKind::Native, "{body}: {error}");
        assert_eq!(native_code(&error), Some(code), "{body}: {error}");
    }
}

#[test]
fn runs_cleanup_after_decode_failures_and_contract_failures() {
    let (host, _table) = fixture_host();
    let frees = count(&OWNED_FREES);
    let error = host.run("fx.badOwned()").expect_err("invalid owned text");
    assert_eq!(native_code(&error), Some("native.encoding"));
    assert_eq!(count(&OWNED_FREES), frees + 1);

    let error = host.run("fx.brokenOwned()").expect_err("contract failure");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(error.message.contains("`fx.brokenOwned`"), "{error}");
    assert!(error.message.contains("status 3"), "{error}");
    assert_eq!(count(&OWNED_FREES), frees + 2);

    let error = host.run("fx.broken()").expect_err("contract failure");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
}

#[test]
fn handles_open_read_close_idempotently() {
    let (host, _table) = fixture_host();
    let (closes, releases) = (count(&CLOSES), count(&RELEASES));
    let value = host
        .run(
            "val counter = fx.open(\"alpha\")\n\
             if (counter != nil) {\n\
               val first = fx.next(counter)\n\
               val second = fx.next(counter)\n\
               [first, second, fx.closeRaw(counter), fx.closeRaw(counter), fx.open(\"missing\")]\n\
             }",
        )
        .expect("run handle program");
    assert_eq!(
        value.to_string(),
        "[[7, \"alpha-1\", 1], [7, \"alpha-2\", 2], 5, nil, nil]"
    );
    assert_eq!(count(&CLOSES), closes + 1, "explicit close calls C once");
    assert_eq!(count(&RELEASES), releases, "explicit close skips release");

    let error = host
        .run(
            "val counter = fx.open(\"beta\")\n\
             if (counter != nil) { fx.closeRaw(counter)\nfx.next(counter) }",
        )
        .expect_err("use after close");
    assert_eq!(native_code(&error), Some("native.resource_closed"));
    assert!(
        error.message.contains("native resource is closed"),
        "{error}"
    );
}

#[test]
fn releases_open_handles_exactly_once() {
    let (host, _table) = fixture_host();
    let releases = count(&RELEASES);
    host.run(
        "val left = fx.open(\"left\")\n\
         val right = fx.open(\"right\")\n\
         if (left != nil) { fx.next(left) }",
    )
    .expect("leave handles open");
    assert_eq!(
        count(&RELEASES),
        releases + 2,
        "each open handle is released"
    );
    let _ = host.0.resources.finalize_all_for_shutdown();
    assert_eq!(count(&RELEASES), releases + 2, "release never runs twice");
}

#[test]
fn keeps_the_owner_alive_while_functions_exist() {
    let specs = fixture_specs();
    let table = table(&specs);
    let owner: Rc<dyn Any> = Rc::new(());
    let weak = Rc::downgrade(&owner);
    let functions = install_table(&table, &expected(&specs), &owner).expect("install fixture");
    drop(owner);
    assert!(weak.upgrade().is_some(), "functions keep the owner alive");
    drop(functions);
    assert!(
        weak.upgrade().is_none(),
        "the owner is dropped with the functions"
    );
}

#[test]
fn rejects_tables_that_do_not_match_the_expected_bindings() {
    let owner: Rc<dyn Any> = Rc::new(());
    let rejection = |specs: Vec<Spec>, expected: Vec<ExpectedBinding<'static>>| {
        install_table(&table(&specs), &expected, &owner).expect_err("table must be rejected")
    };
    let add = || spec("add", "slug-shim/1;fx;add;fx_add;(i64,i64)->i64", add, 2, 1);

    let stale = ExpectedBinding {
        module: "fx",
        name: "add",
        signature: "slug-shim/1;fx;add;fx_add;(i32,i32)->i64",
    };
    let message = rejection(vec![add()], vec![stale]);
    assert!(message.contains("`fx.add` is stale"), "{message}");
    assert!(message.contains("(i32,i32)->i64"), "{message}");
    assert!(message.contains("(i64,i64)->i64"), "{message}");

    let message = rejection(vec![], expected(&[add()]));
    assert_eq!(message, "shim is missing binding `fx.add`");

    let message = rejection(vec![add()], vec![]);
    assert_eq!(message, "shim has unexpected binding `fx.add`");

    let message = rejection(vec![add(), add()], expected(&[add()]));
    assert_eq!(message, "shim has duplicate binding `fx.add`");

    let mut miscounted = add();
    miscounted.results = 2;
    let message = rejection(vec![miscounted], expected(&[add()]));
    assert!(
        message.contains("does not match its signature"),
        "{message}"
    );

    let orphan = spec(
        "next",
        "slug-shim/1;fx;next;fx_next;(handle<Counter:fx_counter>,out cstr,out i64)->i32",
        next,
        1,
        3,
    );
    let message = rejection(
        vec![orphan],
        expected(&[spec(
            "next",
            "slug-shim/1;fx;next;fx_next;(handle<Counter:fx_counter>,out cstr,out i64)->i32",
            next,
            1,
            3,
        )]),
    );
    assert!(
        message.contains("has no `release:Counter` binding"),
        "{message}"
    );

    let bad_release = || {
        spec(
            "release:Counter",
            "slug-shim/1;fx;release:Counter;fx_release;(i64)->void",
            release,
            1,
            0,
        )
    };
    let message = rejection(vec![bad_release()], expected(&[bad_release()]));
    assert!(
        message.contains("exactly one `handle<Counter>`"),
        "{message}"
    );

    let mut version = table(&[add()]);
    version.raw.abi_version = 2;
    let message =
        install_table(&version, &expected(&[add()]), &owner).expect_err("version mismatch");
    assert_eq!(message, "shim ABI version 2 is not the supported version 1");

    // SAFETY: a NULL table is explicitly allowed.
    let message = unsafe { install(std::ptr::null(), &[], &owner) }
        .expect_err("NULL table")
        .to_string();
    assert_eq!(message, "shim table function returned NULL");
}

const EVERY_FORM: &str = r#"
[native]
profile = "core"
headers = ["every.h"]

[native.resources.every.Thing]
type = "thing_t"
release = { symbol = "thing_free", signature = "(handle<Thing>) -> i32" }

[native.functions.every]
scalars = { symbol = "s", signature = "(bool, i8, i16, i32, i64, u8, u16, u32, u64, f32, f64) -> void" }
text = { symbol = "t", signature = "(str, bytes, cstr) -> cstr owned(free)" }
outs = { symbol = "o", signature = "(out bool, out u64, out f32, out cstr owned(free), out cstr, out handle<Thing>) -> handle<Thing>" }
close = { symbol = "c", signature = "(close handle<Thing>, i32) -> i32" }
twice = { symbol = "twice", signature = "(i64) -> i64", macro = true }
"#;

#[test]
fn installs_every_canonical_form_the_manifest_crate_emits() {
    let manifest =
        slug_native_manifest::NativeManifest::from_clutch_source(EVERY_FORM).expect("manifest");
    let signatures = manifest
        .bindings()
        .map(slug_native_manifest::Binding::canonical_signature)
        .collect::<Vec<_>>();
    let specs = manifest
        .bindings()
        .zip(&signatures)
        .map(|(binding, signature)| Spec {
            name: Box::leak(binding.name.clone().into_boxed_str()),
            signature: Box::leak(signature.clone().into_boxed_str()),
            entry: broken,
            arguments: u32::try_from(binding.signature.argument_cells()).unwrap(),
            results: u32::try_from(binding.signature.result_cells()).unwrap(),
        })
        .collect::<Vec<_>>();
    let owner: Rc<dyn Any> = Rc::new(());
    let functions =
        install_table(&table(&specs), &expected(&specs), &owner).expect("install every form");
    let mut names = functions
        .iter()
        .map(|function| function.name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["close", "outs", "scalars", "text", "twice"]);
}

#[test]
fn rejects_signatures_it_cannot_encode() {
    let owner: Rc<dyn Any> = Rc::new(());
    for (signature, message) in [
        (
            "slug-shim/2;fx;f;f;()->void",
            "unsupported signature prefix",
        ),
        ("slug-shim/1;fx;f;f;(str)->str", "arguments only"),
        ("slug-shim/1;fx;f;f;(out str)->void", "invalid parameter"),
        ("slug-shim/1;fx;f;f;(close i64)->void", "invalid parameter"),
        ("slug-shim/1;fx;f;f;(int)->void", "unknown type `int`"),
        (
            "slug-shim/1;fx;f;f;(i64)->void;inline",
            "unexpected trailing fields",
        ),
        ("slug-shim/1;fx;f;f;i64->void", "no `(...)->` shape"),
    ] {
        let specs = [spec("f", signature, broken, 1, 0)];
        let error = install_table(&table(&specs), &expected(&specs), &owner).expect_err(signature);
        assert!(error.contains(message), "{signature}: {error}");
    }
}
