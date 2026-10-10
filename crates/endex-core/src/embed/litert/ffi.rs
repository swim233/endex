//! Raw LiteRT-LM C API (embedding subset), resolved at runtime from `liblitert-lm.so`.
//! Signatures follow `c/engine.h` and `c/embedding_engine.h` of LiteRT-LM v0.18.0.

use std::ffi::{c_char, c_float, c_int, c_void};
use std::path::Path;

use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};

#[repr(C)]
pub struct InputData {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Settings {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Options {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Response {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Engine {
    _opaque: [u8; 0],
}

pub const INPUT_TEXT: c_int = 0;
pub const INPUT_IMAGE: c_int = 1;
pub const LOG_ERROR: c_int = 4;
pub const OVERFLOW_TRUNCATE: c_int = 1;

macro_rules! api {
    ($($name:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Api {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
            _lib: Library,
        }

        impl Api {
            /// # Safety
            /// `path` must point to a LiteRT-LM build whose C API matches the declarations above.
            pub unsafe fn load(path: &Path) -> Result<Self, libloading::Error> {
                // RTLD_LOCAL keeps LiteRT's bundled absl/protobuf/tflite symbols out of the
                // global namespace, so they cannot clash with ONNX Runtime's.
                let lib = unsafe { Library::open(Some(path), RTLD_NOW | RTLD_LOCAL)? };
                Ok(Self {
                    $($name: unsafe {
                        *lib.get(concat!("litert_lm_", stringify!($name), "\0").as_bytes())?
                    },)*
                    _lib: lib,
                })
            }
        }
    };
}

api! {
    set_min_log_level: fn(c_int);
    input_data_create: fn(c_int, *const c_void, usize) -> *mut InputData;
    input_data_delete: fn(*mut InputData);
    embedding_engine_settings_create:
        fn(*const c_char, *const c_char, *const c_char, *const c_char) -> *mut Settings;
    embedding_engine_settings_delete: fn(*mut Settings);
    embedding_engine_settings_set_cache_dir: fn(*mut Settings, *const c_char);
    embedding_options_create: fn() -> *mut Options;
    embedding_options_delete: fn(*mut Options);
    embedding_options_set_normalize: fn(*mut Options, bool);
    embedding_options_set_output_size: fn(*mut Options, c_int);
    embedding_options_set_input_overflow_strategy: fn(*mut Options, c_int);
    embedding_response_delete: fn(*mut Response);
    embedding_response_get_size: fn(*const Response) -> usize;
    embedding_response_get_values: fn(*const Response) -> *const c_float;
    embedding_engine_create: fn(*const Settings) -> *mut Engine;
    embedding_engine_delete: fn(*mut Engine);
    embedding_engine_compute_embedding:
        fn(*mut Engine, *const *const InputData, usize, *const Options) -> *mut Response;
}
