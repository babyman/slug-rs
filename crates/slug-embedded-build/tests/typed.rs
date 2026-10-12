//! Typed provider selection and its checks before linking.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_embedded_build::{CToolchain, ProviderKind, TargetDescriptor};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

const HEADER: &str = "#include <stdint.h>\nint64_t sensor_read(int64_t channel);\n";
const SOURCE: &str =
    "#include \"native/sensor.h\"\nint64_t sensor_read(int64_t channel) { return channel * 10; }\n";
const CONTRACT: &str = "export foreign read = fn(channel:num):num\n";
const MANIFEST: &str = "\
[modules]
\"vendor.sensor\" = { source = \"modules/sensor.slug\" }

[native]
profile = \"core\"
headers = [\"native/sensor.h\"]
sources = [\"native/sensor.c\"]

[native.functions.\"vendor.sensor\"]
read = { symbol = \"sensor_read\", signature = \"(i64 channel) -> i64\" }
";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slug-embedded-typed-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let clutch = root.join("sensor.clutch");
        fs::create_dir_all(clutch.join("modules")).expect("create clutch modules");
        fs::create_dir_all(clutch.join("native")).expect("create clutch native");
        fs::write(
            root.join("app.slug"),
            "val sensor = import(\"vendor.sensor\")\nval main = fn() { sensor.read(4) }\n",
        )
        .expect("write entry");
        let fixture = Self(root);
        fixture.write("sensor.clutch/native/sensor.h", HEADER);
        fixture.write("sensor.clutch/native/sensor.c", SOURCE);
        fixture.write("sensor.clutch/modules/sensor.slug", CONTRACT);
        fixture.write("sensor.clutch/clutch.toml", MANIFEST);
        fixture
    }

    fn write(&self, path: &str, contents: &str) {
        fs::write(self.0.join(path), contents).expect("write fixture file");
    }

    fn descriptor(&self, firmware: &str, provider: &str) -> Result<TargetDescriptor, String> {
        TargetDescriptor::parse(
            &self.0,
            &format!(
                "format = 1\n\
                 [firmware]\nentry = \"app.slug\"\nboard_package = \"test\"\n{firmware}\n\
                 [limits]\nartifact_bytes = 16384\nsection_bytes = 8192\nmodules = 4\nimports = 3\n\
                 string_bytes = 256\nchunks_per_module = 16\nentries_per_table = 128\n\
                 [[providers]]\nmodule = \"vendor.sensor\"\n\
                 contract = \"sensor.clutch/modules/sensor.slug\"\n{provider}"
            ),
        )
        .map_err(|error| error.to_string())
    }

    fn typed(&self) -> TargetDescriptor {
        self.descriptor("", "kind = \"typed\"\nclutch = \"sensor.clutch\"\n")
            .expect("valid typed descriptor")
    }

    fn output(&self) -> PathBuf {
        let output = self.0.join("out");
        fs::create_dir_all(&output).expect("create output");
        output
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn typed_error(fixture: &Fixture) -> String {
    fixture
        .typed()
        .typed_providers()
        .expect_err("typed provider must be rejected")
        .to_string()
}

#[test]
fn parses_typed_providers_and_target_profiles() {
    let fixture = Fixture::new();
    let descriptor = fixture.typed();
    assert_eq!(
        descriptor.providers[0].kind,
        ProviderKind::Typed {
            clutch: fs::canonicalize(fixture.path().join("sensor.clutch")).unwrap()
        }
    );
    assert_eq!(descriptor.profiles, ["core"]);
    let events = fixture
        .descriptor(
            "profiles = [\"core\", \"events\"]",
            "kind = \"typed\"\nclutch = \"sensor.clutch\"\n",
        )
        .expect("profiles");
    assert_eq!(events.profiles, ["core", "events"]);

    for (firmware, provider, message) in [
        ("", "kind = \"zig\"\n", "unknown provider kind `zig`"),
        (
            "",
            "kind = \"typed\"\nlibrary = \"x\"\n",
            "unknown field `library` in provider",
        ),
        (
            "",
            "kind = \"typed\"\n",
            "missing required field `clutch` in provider",
        ),
        (
            "",
            "library = \"x\"\nclutch = \"sensor.clutch\"\n",
            "unknown field `clutch`",
        ),
        (
            "profiles = []",
            "library = \"x\"\n",
            "non-empty array of `core` or `events`",
        ),
        (
            "profiles = [\"gpu\"]",
            "library = \"x\"\n",
            "non-empty array of `core` or `events`",
        ),
    ] {
        let error = fixture.descriptor(firmware, provider).expect_err(message);
        assert!(error.contains(message), "{message}: {error}");
    }
}

#[test]
fn builds_a_typed_provider_with_its_registration() {
    let fixture = Fixture::new();
    let descriptor = fixture.typed();
    let providers = descriptor.typed_providers().expect("typed providers");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, "sensor");
    assert_eq!(providers[0].modules, ["vendor.sensor"]);

    let output = fixture.output();
    let build = descriptor
        .build_typed_providers(&CToolchain::host(), &output)
        .expect("build typed provider");
    assert_eq!(build.libraries, ["slug_typed_sensor"]);
    assert!(output.join("libslug_typed_sensor.a").is_file());
    assert!(build.registration.contains("fn slug_shim_table_sensor()"));
    assert!(
        build
            .registration
            .contains("signature: \"slug-shim/1;vendor.sensor;read;sensor_read;(i64)->i64\"")
    );
    assert!(
        build
            .cargo_directives()
            .contains(&"cargo:rustc-link-lib=static=slug_typed_sensor".to_owned())
    );
    assert!(descriptor.validate_provider_libraries(&[]).is_ok());
}

#[test]
fn rejects_uncovered_or_incompatible_declarations_before_building() {
    let fixture = Fixture::new();
    fixture.write(
        "sensor.clutch/modules/sensor.slug",
        "export foreign read = fn(channel:num):num\nexport foreign reset = fn():nil\n",
    );
    assert!(typed_error(&fixture).contains("foreign `vendor.sensor.reset` has no native binding"));

    fixture.write("sensor.clutch/modules/sensor.slug", "");
    assert!(
        typed_error(&fixture)
            .contains("native binding `vendor.sensor.read` has no foreign declaration")
    );

    fixture.write(
        "sensor.clutch/modules/sensor.slug",
        "export foreign read = fn(channel:str):num\n",
    );
    assert!(typed_error(&fixture).contains("parameter `channel` is annotated `str`"));
}

#[test]
fn rejects_unsupported_profiles_and_unselected_modules() {
    let fixture = Fixture::new();
    fixture.write(
        "sensor.clutch/clutch.toml",
        &MANIFEST.replace("profile = \"core\"", "profile = \"events\""),
    );
    assert!(typed_error(&fixture).contains("needs the `events` profile"));

    fixture.write(
        "sensor.clutch/clutch.toml",
        &format!(
            "{MANIFEST}\n[native.functions.\"vendor.other\"]\n\
             ping = {{ symbol = \"sensor_read\", signature = \"(i64 channel) -> i64\" }}\n"
        ),
    );
    assert!(typed_error(&fixture).contains("binds module `vendor.other`"));

    fixture.write(
        "sensor.clutch/clutch.toml",
        &MANIFEST.replace("modules/sensor.slug\" }", "modules/other.slug\" }"),
    );
    fixture.write("sensor.clutch/modules/other.slug", CONTRACT);
    assert!(
        typed_error(&fixture).contains("must be provided by the clutch from the selected contract")
    );
}

#[test]
fn a_header_mismatch_fails_the_build() {
    let fixture = Fixture::new();
    fixture.write(
        "sensor.clutch/native/sensor.h",
        "#include <stdint.h>\nint32_t sensor_read(int32_t channel);\n",
    );
    let error = fixture
        .typed()
        .build_typed_providers(&CToolchain::host(), &fixture.output())
        .expect_err("header mismatch")
        .to_string();
    assert!(error.contains("embedded target typed-build"), "{error}");
    assert!(error.contains("sensor_shim.c failed"), "{error}");
}
