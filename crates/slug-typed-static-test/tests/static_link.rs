#[test]
fn two_typed_shims_link_into_one_image_and_run() {
    assert_eq!(
        slug_typed_static_test::run().expect("run statically linked typed providers"),
        "[42, \"hello, static\", 1, 2, nil, \"alpha\", \"beta\"]"
    );
}
