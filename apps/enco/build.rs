use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, key) in [
        ("openai-compatible", "ENCO_FACTORY_OPENAI"),
        ("deepseek", "ENCO_FACTORY_DEEPSEEK"),
    ] {
        let path = root.join(format!("target/factory/{name}.wasm"));
        assert!(
            path.exists(),
            "factory artifact missing: run `cargo xtask build-factory` first"
        );
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rustc-env={key}={}", path.display());
    }
}
