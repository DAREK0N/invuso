//! Android-only helpers over JNI (AGENTS.md 5, stage 4).
//!
//! Neither Dioxus nor the WebView exposes the app's files directory, so it is
//! read from `Context.getFilesDir()` via the JavaVM that `ndk-context`
//! provides; no Kotlin code is needed.

use std::path::PathBuf;

use jni::objects::{JObject, JString};
use jni::signature::RuntimeMethodSignature;
use jni::strings::JNIString;

/// Absolute path of `Context.getFilesDir()`.
///
/// Asked from Android instead of assembled from the package name: under a
/// secondary Android user the directory is `/data/user/<n>/<package>`.
pub fn files_dir() -> Result<PathBuf, String> {
    with_env(|env| {
        let context_ptr = ndk_context::android_context().context() as jni::sys::jobject;
        if context_ptr.is_null() {
            return Err("Android context unavailable".to_string());
        }
        // SAFETY: ndk-context holds a valid global reference to the Activity
        // for the lifetime of the process.
        let context = unsafe { JObject::from_raw(env, context_ptr) };

        let file_signature =
            RuntimeMethodSignature::from_str("()Ljava/io/File;").map_err(|e| e.to_string())?;
        let dir = env
            .call_method(
                &context,
                JNIString::new("getFilesDir"),
                file_signature.method_signature(),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|e| e.to_string())?;

        let string_signature =
            RuntimeMethodSignature::from_str("()Ljava/lang/String;").map_err(|e| e.to_string())?;
        let path = env
            .call_method(
                &dir,
                JNIString::new("getAbsolutePath"),
                string_signature.method_signature(),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|e| e.to_string())?;
        let path = env.cast_local::<JString>(path).map_err(|e| e.to_string())?;
        let path = path.try_to_string(env).map_err(|e| e.to_string())?;

        Ok(PathBuf::from(path))
    })
}

fn java_vm() -> Result<jni::JavaVM, String> {
    if let Ok(vm) = jni::JavaVM::singleton() {
        return Ok(vm);
    }
    let vm_ptr = ndk_context::android_context().vm();
    if vm_ptr.is_null() {
        return Err("Android JavaVM unavailable".to_string());
    }
    // SAFETY: ndk-context provides the valid, process-wide JavaVM pointer;
    // `from_raw` caches it as jni's singleton.
    Ok(unsafe { jni::JavaVM::from_raw(vm_ptr as *mut jni::sys::JavaVM) })
}

fn with_env<T>(op: impl FnOnce(&mut jni::Env<'_>) -> Result<T, String>) -> Result<T, String> {
    let vm = java_vm()?;
    let mut result = None;
    vm.attach_current_thread(|env| {
        result = Some(op(env));
        Ok::<(), jni::errors::Error>(())
    })
    .map_err(|e| e.to_string())?;
    result.unwrap_or_else(|| Err("JNI callback did not run".to_string()))
}
