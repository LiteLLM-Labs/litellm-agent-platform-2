//! Wires in provider folders from src/sdk/providers/<name>/, so a new provider
//! needs zero edits outside its own folder.

use std::{fs, path::Path};

fn main() {
    let providers_dir = Path::new("src/sdk/providers");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("providers_generated.rs");

    let mut providers: Vec<ProviderModule> = fs::read_dir(providers_dir)
        .expect("src/sdk/providers not found")
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_dir() && path.join("mod.rs").exists() {
                Some(ProviderModule {
                    name: path.file_name()?.to_str()?.to_owned(),
                    has_llm: path.join("llm").join("mod.rs").exists(),
                })
            } else {
                None
            }
        })
        .collect();
    providers.sort_by(|a, b| a.name.cmp(&b.name));

    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mods: String = providers
        .iter()
        .map(|provider| {
            let name = &provider.name;
            format!("#[path = \"{manifest}/src/sdk/providers/{name}/mod.rs\"]\npub mod {name};\n")
        })
        .collect();
    let inits: String = providers
        .iter()
        .filter(|provider| provider.has_llm)
        .map(|provider| format!("    {}::llm::init(registry);\n", provider.name))
        .collect();

    let generated = format!(
        "{mods}\npub fn register_all(registry: &mut crate::sdk::providers::llm::ProviderRegistry) {{\n{inits}}}\n"
    );
    fs::write(&dest, generated).unwrap();

    println!("cargo:rerun-if-changed=src/sdk/providers");
}

struct ProviderModule {
    name: String,
    has_llm: bool,
}
