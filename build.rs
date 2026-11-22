// Windows resources: the executable icon, and on release builds a manifest
// that asks for administrator rights at launch (changing DNS needs them).
// Debug builds skip the manifest so `cargo run` works from a normal shell.
// The manifest is the only source of the UAC level (/MANIFESTUAC:NO stops the
// linker from adding its own asInvoker entry).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.compile().expect("embed Windows resources");

    // The manifest goes through the linker with -bins so it only reaches the
    // app executable, never the test harness (which would then need elevation).
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("app.manifest");
        std::fs::write(&path, MANIFEST).expect("write manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", path.display());
        println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    }
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;
