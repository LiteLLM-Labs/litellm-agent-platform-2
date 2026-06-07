fn main() {
    // Force recompile when any migration file changes so migrate!() always
    // embeds fresh checksums and stale incremental artifacts can't cause
    // version-mismatch failures across test binaries.
    println!("cargo:rerun-if-changed=src/db/managed_agents/migrations");
    if let Ok(entries) = std::fs::read_dir("src/db/managed_agents/migrations") {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }
}
