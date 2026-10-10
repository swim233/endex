//! Safe RAII wrapper over the LiteRT-LM embedding engine.

mod ffi;

use std::ffi::{CString, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;

use anyhow::{Context, Result, bail, ensure};

use super::{DIM, Device, Embedder, PrefixStyle, normalize};

/// The library is loaded once per process and never unloaded: unloading a large C++ runtime
/// with live worker threads is not safe.
static API: Mutex<Option<(PathBuf, &'static ffi::Api)>> = Mutex::new(None);

fn api(lib: &Path) -> Result<&'static ffi::Api> {
    let mut slot = API.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((loaded, api)) = slot.as_ref() {
        ensure!(
            loaded == lib,
            "LiteRT-LM already loaded from {}, cannot load {}",
            loaded.display(),
            lib.display()
        );
        return Ok(api);
    }
    // SAFETY: the declarations in `ffi` match the LiteRT-LM v0.18 C API.
    let api: &'static ffi::Api = Box::leak(Box::new(
        unsafe { ffi::Api::load(lib) }
            .with_context(|| format!("loading LiteRT-LM library {}", lib.display()))?,
    ));
    // SAFETY: plain setter without preconditions.
    unsafe { (api.set_min_log_level)(ffi::LOG_ERROR) };
    *slot = Some((lib.to_owned(), api));
    Ok(api)
}

fn c_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).context("path contains a NUL byte")
}

pub struct EngineConfig<'a> {
    pub library: &'a Path,
    pub model: &'a Path,
    pub cache_dir: &'a Path,
    pub device: Device,
}

/// One loaded embedding model on one backend; `&mut self` serializes calls into it.
pub struct Engine {
    api: &'static ffi::Api,
    raw: NonNull<ffi::Engine>,
    options: NonNull<ffi::Options>,
}

impl Engine {
    pub fn new(config: &EngineConfig) -> Result<Self> {
        let api = api(config.library)?;
        let model = c_path(config.model)?;
        let cache_dir = c_path(config.cache_dir)?;
        let backend = CString::new(config.device.as_str())?;
        // SAFETY: all pointers are valid NUL-terminated strings for the duration of the calls;
        // the settings object is deleted on every path after the engine has been created.
        unsafe {
            let settings = (api.embedding_engine_settings_create)(
                model.as_ptr(),
                backend.as_ptr(),
                backend.as_ptr(),
                std::ptr::null(),
            );
            let settings = NonNull::new(settings).context("creating LiteRT-LM settings")?;
            (api.embedding_engine_settings_set_cache_dir)(settings.as_ptr(), cache_dir.as_ptr());
            let raw = (api.embedding_engine_create)(settings.as_ptr());
            (api.embedding_engine_settings_delete)(settings.as_ptr());
            let Some(raw) = NonNull::new(raw) else {
                bail!(
                    "LiteRT-LM could not create a {} engine for {}",
                    config.device.as_str(),
                    config.model.display()
                );
            };
            let Some(options) = NonNull::new((api.embedding_options_create)()) else {
                (api.embedding_engine_delete)(raw.as_ptr());
                bail!("creating LiteRT-LM embedding options");
            };
            (api.embedding_options_set_normalize)(options.as_ptr(), true);
            (api.embedding_options_set_output_size)(options.as_ptr(), DIM as c_int);
            // Inputs over the largest signature (1024 tokens) would fail otherwise; chunks
            // stay far below it, so this only affects unusually long queries.
            (api.embedding_options_set_input_overflow_strategy)(
                options.as_ptr(),
                ffi::OVERFLOW_TRUNCATE,
            );
            Ok(Self { api, raw, options })
        }
    }

    fn compute(&mut self, kind: c_int, bytes: &[u8]) -> Result<Vec<f32>> {
        let api = self.api;
        // SAFETY: the input copies `bytes`; input, response and engine pointers are checked for
        // NULL and released exactly once; the values slice is copied before the response is freed.
        unsafe {
            let input = (api.input_data_create)(kind, bytes.as_ptr().cast(), bytes.len());
            let input = NonNull::new(input).context("creating LiteRT-LM input")?;
            let inputs = [input.as_ptr().cast_const()];
            let response = (api.embedding_engine_compute_embedding)(
                self.raw.as_ptr(),
                inputs.as_ptr(),
                inputs.len(),
                self.options.as_ptr(),
            );
            (api.input_data_delete)(input.as_ptr());
            let response = NonNull::new(response).context("LiteRT-LM failed to embed input")?;
            let len = (api.embedding_response_get_size)(response.as_ptr());
            let values = (api.embedding_response_get_values)(response.as_ptr());
            let vector = if values.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(values, len).to_vec()
            };
            (api.embedding_response_delete)(response.as_ptr());
            Ok(vector)
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: both pointers were created by this library and are not used afterwards.
        unsafe {
            (self.api.embedding_options_delete)(self.options.as_ptr());
            (self.api.embedding_engine_delete)(self.raw.as_ptr());
        }
    }
}

pub struct LiteRtEmbedder {
    engine: Engine,
    prefix: PrefixStyle,
    device: Device,
}

impl LiteRtEmbedder {
    pub fn new(config: &EngineConfig, prefix: PrefixStyle) -> Result<Self> {
        Ok(Self {
            engine: Engine::new(config)?,
            prefix,
            device: config.device,
        })
    }

    fn embed(&mut self, kind: c_int, bytes: &[u8]) -> Result<Vec<f32>> {
        let mut vector = self.engine.compute(kind, bytes)?;
        ensure!(
            vector.len() == DIM,
            "expected a {DIM}-dim embedding, got {}",
            vector.len()
        );
        normalize(&mut vector)?;
        Ok(vector)
    }
}

impl Embedder for LiteRtEmbedder {
    fn device(&self) -> Device {
        self.device
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        let text = self.prefix.query(text);
        self.embed(ffi::INPUT_TEXT, text.as_bytes())
    }

    fn embed_document(&mut self, text: &str) -> Result<Vec<f32>> {
        let text = self.prefix.document(text);
        self.embed(ffi::INPUT_TEXT, text.as_bytes())
    }

    fn embed_image(&mut self, bytes: &[u8]) -> Result<Vec<f32>> {
        self.embed(ffi::INPUT_IMAGE, bytes)
    }
}
