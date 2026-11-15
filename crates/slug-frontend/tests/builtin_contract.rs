//! The check that a host's builtins are the ones its `slug.builtin` contract
//! declares.

use slug_frontend::builtin_contract_mismatches;
use slug_vm::{NativeArity, NativeCall, NativeFunction, NativeModule, NativeStatus};

const ERROR: &str = "export val Error = struct {\n    type:str = \"Error\",\n    msg:str,\n    code = nil,\n    data = nil,\n    cause = nil,\n}\n";

fn returns_nil(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(slug_vm::NativeOwnedValue::nil())
}

fn builtins(functions: &[(&str, NativeArity)]) -> Vec<NativeFunction> {
    let module = NativeModule::new("slug.builtin", ()).expect("valid module");
    functions
        .iter()
        .map(|(name, arity)| {
            module
                .function(*name, *arity, returns_nil)
                .expect("valid function")
        })
        .collect()
}

fn mismatches(contract: &str, functions: &[(&str, NativeArity)]) -> Vec<String> {
    builtin_contract_mismatches("contract.slug", contract, &builtins(functions))
        .expect("contract parses")
}

#[test]
fn a_matching_contract_has_no_mismatches() {
    let contract = format!(
        "{ERROR}export foreign print = fn(...values):nil\nexport foreign chan = fn<T>(capacity:num = 0):chan<T>\n"
    );
    assert_eq!(
        mismatches(
            &contract,
            &[
                ("print", NativeArity::Variadic { minimum: 0 }),
                (
                    "chan",
                    NativeArity::Range {
                        minimum: 0,
                        maximum: 1
                    }
                ),
            ],
        ),
        Vec::<String>::new()
    );
}

#[test]
fn each_difference_is_reported() {
    let contract = format!(
        "{ERROR}export foreign print = fn(...values):nil\nexport foreign len = fn(value):num\n"
    );
    assert_eq!(
        mismatches(
            &contract,
            &[
                ("len", NativeArity::Exact(2)),
                ("keys", NativeArity::Exact(1)),
            ],
        ),
        [
            "`print` is declared but not registered",
            "`len` is registered with an arity its declaration does not have",
            "`keys` is registered but not declared",
        ]
    );
}

#[test]
fn an_error_unlike_the_vms_is_reported() {
    for contract in [
        "",
        "export val Error = struct { type:str = \"Error\", msg:str }\n",
        "export val Error = struct { type:str = \"Failure\", msg:str, code = nil, data = nil, cause = nil }\n",
        "export val Error = struct { type:str, msg:str, code = nil, data = nil, cause = nil }\n",
    ] {
        assert_eq!(mismatches(contract, &[]).len(), 1, "{contract}");
    }
}
