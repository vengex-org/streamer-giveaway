use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let env = |key: &str| std::env::var(key).expect("cargo sets package variables");
    let version = env("CARGO_PKG_VERSION");
    // Windows wants four numbers; a pre-release suffix ("-rc.1") is left out of them.
    let numbers: Vec<&str> = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .collect();
    let [major, minor, patch] = numbers[..] else {
        panic!("version {version} is not major.minor.patch");
    };
    let icon = PathBuf::from(env("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // The resource compiler reads backslashes as escapes; it takes forward slashes as they are.
    let icon = icon
        .display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let description = env("CARGO_PKG_DESCRIPTION").replace('"', "'");
    // Resource 1 is the icon GPUI shows in the title bar and taskbar. The version block fills in
    // the file's Properties > Details and the name Task Manager shows.
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "vengex-org"
      VALUE "FileDescription", "Streamer Giveaway"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "streamer-giveaway"
      VALUE "LegalCopyright", "Copyright (c) 2026 vengex-org contributors"
      VALUE "OriginalFilename", "streamer-giveaway.exe"
      VALUE "ProductName", "Streamer Giveaway"
      VALUE "ProductVersion", "{version}"
      VALUE "Comments", "{description}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let path = PathBuf::from(env("OUT_DIR")).join("app.rc");
    std::fs::write(&path, rc).expect("resource file is written");
    embed_resource::compile(&path, embed_resource::NONE)
        .manifest_optional()
        .expect("icon and version resources compile");
}
