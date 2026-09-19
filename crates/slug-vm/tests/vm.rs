use std::sync::{Arc, Mutex};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

#[cfg(feature = "concurrency")]
use slug_vm::SelectCase;
use slug_vm::VmProgress;
use slug_vm::{
    CallArgumentKind, Capture, CaptureListId, Chunk, GlobalNameId, MatchMapKey, MatchPatternId,
    MatchRest, ModuleLoader, NativeArity, NativeCall, NativeError, NativeModule, NativeOwnedValue,
    NativeResourceType, NativeStatus, Op, Program, RuntimeErrorKind, SchemaField, SchemaFieldsId,
    SourceSpan, SpanId, StructFieldsId, Value, Vm, compile,
};

fn program_with_main(main: Chunk) -> Program {
    let mut program = Program::new();
    program.add_chunk(main);
    program
}

#[cfg(feature = "concurrency")]
fn native_make_channel(call: &mut NativeCall<'_>) -> NativeStatus {
    let capacity = match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
        Ok(value) => match usize::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                return call.raise(NativeError::new(
                    "native.type",
                    "channel capacity must not be negative or too large",
                ));
            }
        },
        Err(error) => return call.raise(error),
    };
    let channel = call.plain_channel(capacity);
    call.return_value(channel)
}

#[cfg(feature = "concurrency")]
fn vm_with_channel_constructor() -> Vm {
    let mut vm = Vm::new();
    let module = NativeModule::new("test.channels", ()).expect("native module is valid");
    vm.define_native(
        module
            .function("make_channel", NativeArity::Exact(1), native_make_channel)
            .expect("native channel constructor is valid"),
    )
    .expect("native channel constructor is unique");
    vm
}

mod native_resource_fixture {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    struct Payload {
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
        panic_on_first_close: bool,
    }

    struct State {
        first: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        second: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
    }

    pub fn module(closed: Rc<Cell<usize>>, destroyed: Rc<Cell<usize>>) -> NativeModule {
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let module = NativeModule::new(
            "test.shared_resources",
            State {
                first: first.clone(),
                second: second.clone(),
                closed,
                destroyed,
            },
        )
        .unwrap();
        *first.borrow_mut() = Some(module.resource_type("first", close, destroy).unwrap());
        *second.borrow_mut() = Some(module.resource_type("second", close, destroy).unwrap());
        module
    }

    pub fn install(vm: &mut Vm, module: &NativeModule) {
        for (name, arity, callback) in [
            (
                "make_resource",
                NativeArity::Exact(0),
                make_resource as for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
            ),
            ("wrong_resource", NativeArity::Exact(1), wrong_resource),
            ("close_resource", NativeArity::Exact(1), close_resource),
            ("busy_then_close", NativeArity::Exact(1), busy_then_close),
            ("retry_close", NativeArity::Exact(1), retry_close),
            (
                "fail_with_resource",
                NativeArity::Exact(0),
                fail_with_resource,
            ),
            (
                "make_panicking_resource",
                NativeArity::Exact(0),
                make_panicking_resource,
            ),
        ] {
            vm.define_native(module.function(name, arity, callback).unwrap())
                .unwrap();
        }
    }

    fn close(payload: &mut Payload) {
        if payload.panic_on_first_close {
            payload.panic_on_first_close = false;
            panic!("first close fails");
        }
        payload.closed.set(payload.closed.get() + 1);
    }

    fn destroy(payload: Payload) {
        let Payload { destroyed, .. } = payload;
        destroyed.set(destroyed.get() + 1);
    }

    fn make_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: false,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.return_value(value),
            Err(error) => call.raise(error),
        }
    }

    fn make_panicking_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: true,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.return_value(value),
            Err(error) => call.raise(error),
        }
    }

    fn wrong_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .second
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        match call.with_resource(0, &resource_type, |_| ()) {
            Ok(()) => call.return_value(NativeOwnedValue::nil()),
            Err(error) => call.raise(error),
        }
    }

    fn close_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn busy_then_close(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        let nested = match call.with_resource(0, &resource_type, |_| {
            call.close_resource(0, &resource_type)
        }) {
            Ok(result) => result,
            Err(error) => return call.raise(error),
        };
        if nested.is_ok() {
            return call.raise(NativeError::new(
                "test.expected_busy",
                "overlapping close unexpectedly succeeded",
            ));
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn retry_close(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        if call.close_resource(0, &resource_type).is_ok() {
            return call.raise(NativeError::new(
                "test.expected_close_failure",
                "first close unexpectedly succeeded",
            ));
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn fail_with_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: false,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.raise(
                NativeError::new("test.resource_error", "resource in error data").with_data(value),
            ),
            Err(error) => call.raise(error),
        }
    }
}

// Feature-oriented VM test modules. Shared bytecode and native-resource
// fixtures remain here because several independent execution boundaries use
// them.
#[path = "vm/bytecode.rs"]
mod bytecode;
#[path = "vm/calls_and_native.rs"]
mod calls_and_native;
#[path = "vm/collections.rs"]
mod collections;
#[cfg(feature = "concurrency")]
#[path = "vm/concurrency.rs"]
mod concurrency;
#[path = "vm/lifecycle.rs"]
mod lifecycle;
#[path = "vm/runtime.rs"]
mod runtime;
