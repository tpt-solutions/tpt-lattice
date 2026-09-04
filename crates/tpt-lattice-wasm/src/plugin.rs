//! Sandboxed user-defined-function (UDF) plugins for power users.
//!
//! A plugin is a WebAssembly module loaded at runtime. It is instantiated with
//! an **empty** import object, so it cannot touch the DOM, network, storage, or
//! engine state — it is a pure, isolated computation sandbox. The host calls
//! the plugin's exported `call(ptr, len) -> f64` with its numeric arguments
//! written into the plugin's *own* linear memory (via the plugin's exported
//! `alloc`/`dealloc`), and reads back an `f64` result.
//!
//! Plugin ABI (every plugin must export these four symbols):
//! - `alloc(size: i32) -> i32`   : reserve `size` bytes, return a pointer
//! - `dealloc(ptr: i32, size: i32)` : free a previous allocation
//! - `call(ptr: i32, len: i32) -> f64` : invoke the function; `ptr`/`len`
//!   describe an `f64[len]` argument array in plugin memory
//! - `memory` : the plugin's exported linear memory
//!
//! Only the `wasm32` build can actually load and run plugins (it has a browser
//! `WebAssembly` runtime and `js_sys`). The host build provides inert stubs so
//! the crate still compiles and the engine can be unit-tested with native
//! closures via `Evaluator::add_external`.

use tpt_lattice_core::{CellValue, LatticeError};
use tpt_lattice_evaluator::Evaluator;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use js_sys::{Float64Array, Function, Reflect, Uint8Array, WebAssembly};

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Loaded plugin instances, keyed by uppercase function name.
    static PLUGINS: RefCell<HashMap<String, JsValue>> = RefCell::new(HashMap::new());
}

/// Instantiate `bytes` as a sandboxed plugin and register it under `name`.
///
/// Returns an error string (suitable for surfacing to the UI) if the bytes are
/// not a valid wasm module, or if the module does not export the required ABI.
#[cfg(target_arch = "wasm32")]
pub fn register_plugin(name: &str, bytes: &[u8]) -> Result<(), String> {
    let uint8 = Uint8Array::new_with_length(bytes.len() as u32);
    uint8.copy_from(bytes);
    // Validate the bytes parse as a wasm module before instantiating.
    let module = match WebAssembly::Module::new(&uint8) {
        Ok(m) => m,
        Err(_) => return Err("invalid wasm module".to_string()),
    };
    // Empty import object => the plugin cannot import anything. This is what
    // makes it a true sandbox: no ambient capabilities are available to it.
    let imports = js_sys::Object::new();
    let instance = match WebAssembly::Instance::new(&module, &imports) {
        Ok(i) => i,
        Err(e) => return Err(format!("plugin instantiation failed: {e:?}")),
    };
    let exports = instance.exports();
    for sym in ["alloc", "dealloc", "call", "memory"] {
        if Reflect::get(&exports, &JsValue::from_str(sym)).is_err() {
            return Err(format!("plugin is missing required export '{sym}'"));
        }
    }
    PLUGINS.with(|p| p.borrow_mut().insert(name.to_string(), JsValue::from(instance)));
    Ok(())
}

/// Remove a previously registered plugin.
#[cfg(target_arch = "wasm32")]
pub fn unregister_plugin(name: &str) {
    PLUGINS.with(|p| p.borrow_mut().remove(name));
}

/// Names of all currently loaded plugins.
#[cfg(target_arch = "wasm32")]
pub fn list_plugins() -> Vec<String> {
    PLUGINS.with(|p| p.borrow().keys().cloned().collect())
}

/// Invoke a registered plugin with already-evaluated arguments. Non-numeric
/// arguments are coerced (numbers pass through; booleans become 0/1; numeric
/// text is parsed); any other value is a `#VALUE!` error. Never panics: a
/// malformed plugin returns a `#ERROR!` rather than unwinding the engine.
#[cfg(target_arch = "wasm32")]
pub fn invoke_plugin(name: &str, args: &[CellValue]) -> CellValue {
    let instance = PLUGINS.with(|p| p.borrow().get(name).cloned());
    let Some(instance) = instance else {
        return CellValue::Error(LatticeError::name_error(name));
    };
    let inst: WebAssembly::Instance = instance.unchecked_into();
    let exports = inst.exports();
    let get = |sym: &str| -> Option<JsValue> {
        Reflect::get(&exports, &JsValue::from_str(sym))
            .ok()
            .filter(|v| !v.is_undefined())
    };
    let call = match get("call") {
        Some(v) => v.unchecked_into::<Function>(),
        None => return CellValue::Error(LatticeError::internal("plugin missing 'call' export")),
    };
    let alloc = match get("alloc") {
        Some(v) => v.unchecked_into::<Function>(),
        None => return CellValue::Error(LatticeError::internal("plugin missing 'alloc' export")),
    };
    let dealloc = match get("dealloc") {
        Some(v) => v.unchecked_into::<Function>(),
        None => return CellValue::Error(LatticeError::internal("plugin missing 'dealloc' export")),
    };
    let memory = match get("memory") {
        Some(v) => v.unchecked_into::<WebAssembly::Memory>(),
        None => return CellValue::Error(LatticeError::internal("plugin missing 'memory' export")),
    };

    let mut nums: Vec<f64> = Vec::with_capacity(args.len());
    for a in args {
        let n = match a {
            CellValue::Number(n) => *n,
            CellValue::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            CellValue::Text(s) => match s.parse::<f64>() {
                Ok(n) if n.is_finite() => n,
                _ => return CellValue::Error(LatticeError::type_error("Number", "Text")),
            },
            _ => return CellValue::Error(LatticeError::type_error("Number", "other")),
        };
        nums.push(n);
    }

    let n = nums.len() as i32;
    let bytes = (nums.len() * 8) as i32;
    let ptr = match alloc.call1(&JsValue::NULL, &JsValue::from_f64(bytes as f64)) {
        Ok(v) => v.as_f64().unwrap_or(0.0) as i32,
        Err(_) => return CellValue::Error(LatticeError::internal("plugin alloc() failed")),
    };

    let buf = memory.buffer();
    let view = Float64Array::new_with_byte_offset_and_length(&buf, ptr as u32, nums.len() as u32);
    for (i, x) in nums.iter().enumerate() {
        view.set_index(i as u32, *x);
    }

    let result = match call.call2(
        &JsValue::NULL,
        &JsValue::from_f64(ptr as f64),
        &JsValue::from_f64(n as f64),
    ) {
        Ok(v) => v.as_f64().unwrap_or(f64::NAN),
        Err(_) => {
            let _ = dealloc.call2(
                &JsValue::NULL,
                &JsValue::from_f64(ptr as f64),
                &JsValue::from_f64(bytes as f64),
            );
            return CellValue::Error(LatticeError::internal("plugin call() failed"));
        }
    };

    let _ = dealloc.call2(
        &JsValue::NULL,
        &JsValue::from_f64(ptr as f64),
        &JsValue::from_f64(bytes as f64),
    );
    CellValue::Number(result).sanitize()
}

/// Ensure every loaded plugin has a registered external function on `engine`.
/// Called whenever a fresh evaluator is created (new sheet, fork) so plugins
/// survive across sheets.
#[cfg(target_arch = "wasm32")]
pub fn register_plugins_on(engine: &mut Evaluator) {
    let names: Vec<String> = PLUGINS.with(|p| p.borrow().keys().cloned().collect());
    for name in names {
        if !engine.list_externals().iter().any(|n| n == &name) {
            let owned = name.clone();
            engine.add_external(&name, Box::new(move |args| invoke_plugin(&owned, args)));
        }
    }
}

// ----- host (non-wasm32): inert stubs ---------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub fn register_plugin(_name: &str, _bytes: &[u8]) -> Result<(), String> {
    Err("plugin loading requires the wasm build".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn unregister_plugin(_name: &str) {}

#[cfg(not(target_arch = "wasm32"))]
pub fn list_plugins() -> Vec<String> {
    Vec::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn invoke_plugin(_name: &str, _args: &[CellValue]) -> CellValue {
    CellValue::Error(LatticeError::name_error(_name))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn register_plugins_on(_engine: &mut Evaluator) {}
