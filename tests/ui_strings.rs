//! Every user-facing text comes from `locales/app.toml`. This fails when a string literal is passed
//! straight to a text-taking UI call in `src/ui/`.

use std::fs;
use std::path::Path;

const TEXT_CALLS: [&str; 3] = [".child(\"", ".label(\"", ".placeholder(\""];

#[test]
fn ui_code_passes_no_literal_text_to_widgets() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("ui");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).expect("src/ui exists") {
        let path = entry.expect("readable dir entry").path();
        let source = fs::read_to_string(&path).expect("readable source file");
        for (line_no, line) in source.lines().enumerate() {
            if TEXT_CALLS.iter().any(|call| line.contains(call)) {
                found.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_no + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(found.is_empty(), "literal UI text:\n{}", found.join("\n"));
}
