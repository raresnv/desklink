//! Capture/control permissions are requested from explicit UI actions, never
//! by startup, display enumeration or a remote peer alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    ScreenRecording,
    Accessibility,
}
#[derive(Clone, Copy)]
pub struct SharingPermissions {
    pub screen: bool,
    pub control: bool,
}
impl SharingPermissions {
    pub fn missing(self, control: bool) -> Option<Permission> {
        if !self.screen {
            Some(Permission::ScreenRecording)
        } else if control && !self.control {
            Some(Permission::Accessibility)
        } else {
            None
        }
    }
}
pub fn snapshot() -> SharingPermissions {
    #[cfg(target_os = "macos")]
    return SharingPermissions {
        screen: core_graphics::access::ScreenCaptureAccess.preflight(),
        control: accessibility(false),
    };
    #[cfg(not(target_os = "macos"))]
    SharingPermissions {
        screen: true,
        control: true,
    }
}
pub fn request(permission: Permission) {
    #[cfg(target_os = "macos")]
    match permission {
        Permission::ScreenRecording => {
            core_graphics::access::ScreenCaptureAccess.request();
        }
        Permission::Accessibility => {
            accessibility(true);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = permission;
}
#[cfg(target_os = "macos")]
fn accessibility(prompt: bool) -> bool {
    use core_foundation::{
        base::TCFType,
        boolean::CFBoolean,
        dictionary::{CFDictionary, CFDictionaryRef},
        string::{CFString, CFStringRef},
    };
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }
    unsafe {
        // Framework-owned constant: retain under the Get rule.
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
