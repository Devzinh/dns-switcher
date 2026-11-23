// Windows resources for the executable: icon, version information (shown in
// Properties > Details and used by antivirus reputation), and a manifest.
//
// The manifest runs the app with the user's normal rights (asInvoker). Changing
// DNS is done by the DNS Switcher service that the setup program installs, so
// opening the app never needs a UAC prompt.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico")
        .set("ProductName", "DNS Switcher")
        .set("FileDescription", "DNS Switcher")
        .set("CompanyName", "DNS Switcher contributors")
        .set("LegalCopyright", "Copyright (c) 2026 DNS Switcher contributors. MIT License.")
        .set("OriginalFilename", "dns_switcher.exe")
        .set("InternalName", "dns_switcher")
        .set("Comments", "Open source: https://github.com/Devzinh/dns-switcher")
        .set("ProductVersion", &version)
        .set("FileVersion", &version)
        .set_manifest(&MANIFEST.replace("{VERSION}", &format!("{version}.0")));
    res.compile().expect("embed Windows resources");
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="DnsSwitcher.App" version="{VERSION}" processorArchitecture="*"/>
  <description>DNS Switcher</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;
