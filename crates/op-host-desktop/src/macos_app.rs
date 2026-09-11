//! macOS app identity for the non-bundled dev binary.
//!
//! Run bare (`./target/release/openpencil-desktop`), the app shows
//! up in the Dock as the raw executable name with a blank icon —
//! there is no `Info.plist` to read `CFBundleName` / the icon from.
//! [`apply`] sets both at runtime: the Dock / menu-bar name becomes
//! "Norka" and the Dock tile gets the brand icon. A packaged
//! `.app` carries them in its bundle, so this is a dev-run nicety.

/// Set the running app's Dock name + icon. macOS only; a no-op
/// elsewhere. Call once at startup, on the main thread.
#[cfg(target_os = "macos")]
pub fn apply() {
    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData, NSProcessInfo, NSString};

    // `apply` runs before the event loop, on the main thread.
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    // Dock / menu-bar name — overrides the `argv[0]` basename.
    let name = NSString::from_str(op_editor_ui::PRODUCT_NAME);
    unsafe { NSProcessInfo::processInfo().setProcessName(&name) };

    // Dock icon — decode the embedded PNG into an `NSImage`.
    let data = NSData::with_bytes(include_bytes!("../assets/icon.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

/// Tell AppKit that the window's backing pixels are sRGB.
///
/// `NSWindow` otherwise follows the current screen's colour space. On a
/// Display-P3 screen that makes the compositor interpret the GL framebuffer's
/// sRGB channel values as P3, visibly shifting colours such as Ant Design blue.
/// Call this after winit creates the window and before the GL surface is built.
#[cfg(target_os = "macos")]
pub fn configure_srgb_window(window: &winit::window::Window) -> bool {
    use objc2_app_kit::{NSColorSpace, NSView};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return false;
    };

    // SAFETY: raw-window-handle's `ns_view` is the live NSView owned by
    // `window`. This function runs synchronously on winit's main thread and
    // does not retain the borrowed view beyond this call.
    let ns_view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = ns_view.window() else {
        return false;
    };
    // SAFETY: AppKit window mutation is performed on the main thread during
    // `ApplicationHandler::resumed`, before any render context is created.
    unsafe {
        let srgb = NSColorSpace::sRGBColorSpace();
        ns_window.setColorSpace(Some(&srgb));
    }
    true
}

/// No-op on non-macOS platforms — a packaged build carries the
/// name + icon in its own bundle / resources.
#[cfg(not(target_os = "macos"))]
pub fn apply() {}

/// Non-macOS windows do not use AppKit colour-space metadata.
#[cfg(not(target_os = "macos"))]
pub fn configure_srgb_window(_window: &winit::window::Window) -> bool {
    true
}
