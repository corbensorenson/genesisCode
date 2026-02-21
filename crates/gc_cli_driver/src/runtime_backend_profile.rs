#[cfg(feature = "profile-backend")]
pub(crate) fn active_runtime_backend_profile() -> &'static str {
    "backend"
}

#[cfg(all(not(feature = "profile-backend"), feature = "profile-gpu"))]
pub(crate) fn active_runtime_backend_profile() -> &'static str {
    "gpu"
}

#[cfg(all(
    not(feature = "profile-backend"),
    not(feature = "profile-gpu"),
    feature = "profile-gfx"
))]
pub(crate) fn active_runtime_backend_profile() -> &'static str {
    "gfx"
}

#[cfg(all(
    not(feature = "profile-backend"),
    not(feature = "profile-gpu"),
    not(feature = "profile-gfx")
))]
pub(crate) fn active_runtime_backend_profile() -> &'static str {
    "headless"
}

pub(crate) fn gpu_device_backend_enabled() -> bool {
    cfg!(feature = "gpu-device-backend")
}

pub(crate) fn gfx_desktop_backend_enabled() -> bool {
    cfg!(feature = "gfx-desktop-backend")
}

#[cfg(test)]
mod tests {
    use super::{
        active_runtime_backend_profile, gfx_desktop_backend_enabled, gpu_device_backend_enabled,
    };

    #[test]
    fn backend_feature_flags_match_active_profile() {
        match active_runtime_backend_profile() {
            "headless" => {
                assert!(!gpu_device_backend_enabled());
                assert!(!gfx_desktop_backend_enabled());
            }
            "gpu" => {
                assert!(gpu_device_backend_enabled());
                assert!(!gfx_desktop_backend_enabled());
            }
            "gfx" => {
                assert!(!gpu_device_backend_enabled());
                assert!(gfx_desktop_backend_enabled());
            }
            "backend" => {
                assert!(gpu_device_backend_enabled());
                assert!(gfx_desktop_backend_enabled());
            }
            other => panic!("unexpected runtime backend profile label: {other}"),
        }
    }
}
