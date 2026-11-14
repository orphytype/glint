//! Puts the app's icon and its version details into `glint.exe`. Explorer, the Start menu,
//! pinned shortcuts, Task Manager and Windows 11's "other system tray icons" list all read the
//! picture and the name ("Glint", not "glint.exe") from the executable, not from anything
//! the running program does. The icon is `assets/glint.ico`, written from the same painter as
//! the tray icon (see `src/desktop/icon.rs`).
//!
//! A machine with no resource compiler still builds a working app, just with the default icon, so
//! a failure here is a warning and never an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/glint.ico");

    // The build script runs on the host, the resources are for the target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/glint.ico")
        // `FileDescription` is what Windows shows as the program's name in the tray's "other
        // icons" list, in Task Manager and in the "open with" dialogs.
        .set("FileDescription", "Glint")
        .set("ProductName", "Glint")
        .set("InternalName", "glint")
        .set("OriginalFilename", "glint.exe")
        .set("LegalCopyright", "GPL-3.0-only")
        .set("Comments", "An unofficial Yandex Music client. Not affiliated with or endorsed by Yandex.");
    // `FileVersion` and `ProductVersion` come from the package's version.

    // Only the app itself gets the resources, not its test programs.
    if let Err(e) = resource.compile_for("glint") {
        println!("cargo:warning=Glint could not embed its icon and version info ({e}); the exe keeps the default icon");
    }
}
