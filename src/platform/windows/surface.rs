use skia::gl_context::PlatformDisplayData;

#[derive(Copy, Clone)]
pub struct NativeDisplay;

#[cfg(target_os = "windows")]
impl NativeDisplay {
    pub fn new() -> NativeDisplay {
        NativeDisplay
    }

    pub fn platform_display_data(&self) -> PlatformDisplayData {
        PlatformDisplayData::new()
    }
}
