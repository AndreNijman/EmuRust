#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/libmupen64plus.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    const LIBMUPEN64PLUS: &str = "libmupen64plus.dylib";
    #[cfg(target_os = "linux")]
    const LIBMUPEN64PLUS: &str = "libmupen64plus.so";
    #[cfg(target_os = "windows")]
    const LIBMUPEN64PLUS: &str = "libmupen64plus.dll";

    #[test]
    fn load_core_lib() {
        unsafe {
            let lib = libmupen64plus::new(LIBMUPEN64PLUS).unwrap();

            let plugin_get_version: libloading::Symbol<ptr_PluginGetVersion> =
                lib.__library.get(b"PluginGetVersion").unwrap();
            let plugin_get_version = plugin_get_version.unwrap();

            let mut plugin_type = 0;
            let mut plugin_version = 0;
            let mut api_version = 0;
            let plugin_name = std::ptr::null_mut();
            let mut capabilities = 0;
            plugin_get_version(
                &mut plugin_type,
                &mut plugin_version,
                &mut api_version,
                plugin_name,
                &mut capabilities,
            );

            assert!(plugin_type == m64p_plugin_type_M64PLUGIN_CORE);
        }
    }
}
