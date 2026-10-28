use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src");
    let mut sources = WalkDir::new("src")
        .into_iter()
        .map(|entry| entry.map(walkdir::DirEntry::into_path))
        .collect::<Result<Vec<_>, _>>()?;
    sources.retain(|path| path.extension().is_some_and(|extension| extension == "rs"));
    sources.sort();
    let mut hasher = DefaultHasher::new();
    for path in sources {
        path.hash(&mut hasher);
        fs::read(path)?.hash(&mut hasher);
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?);
    let digits = hasher.finish().to_string();
    let mut fingerprint = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            fingerprint.push('_');
        }
        fingerprint.push(digit);
    }
    fs::write(
        output.join("rule_fingerprint.rs"),
        format!("const RULE_FINGERPRINT: u64 = {fingerprint};\n"),
    )?;
    Ok(())
}
