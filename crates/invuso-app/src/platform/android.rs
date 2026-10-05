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

/// [`ImageSource`](super::ImageSource) over the bridge in `MainActivity.kt`.
pub struct AndroidImageSource;

/// Status codes of `MainActivity.receiptImageResult`.
const RESULT_SAVED: i32 = 0;
const RESULT_CANCELLED: i32 = 1;

type PickResult = Result<super::PickOutcome, String>;

/// The pick waiting for its answer from Kotlin. A newer pick replaces it,
/// which drops the old sender and so ends the old one as cancelled.
static PENDING_PICK: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<PickResult>>> =
    std::sync::Mutex::new(None);

impl super::ImageSource for AndroidImageSource {
    fn supports(&self, kind: super::ImageKind) -> bool {
        match kind {
            super::ImageKind::Gallery => true,
            super::ImageKind::Camera => can_take_photo().unwrap_or(false),
        }
    }

    fn pick(
        &self,
        kind: super::ImageKind,
        dest: &std::path::Path,
    ) -> impl std::future::Future<Output = PickResult> + use<> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let started = match PENDING_PICK.lock() {
            Ok(mut pending) => {
                *pending = Some(sender);
                request_image(kind, dest)
            }
            Err(_) => Err("image picker state poisoned".to_string()),
        };
        async move {
            started?;
            receiver.await.unwrap_or(Ok(super::PickOutcome::Cancelled))
        }
    }
}

/// `MainActivity.canTakeReceiptPhoto()`: Android 10+ with a camera app.
fn can_take_photo() -> Result<bool, String> {
    with_activity(|env, activity| {
        let signature = RuntimeMethodSignature::from_str("()Z").map_err(|e| e.to_string())?;
        env.call_method(
            activity,
            JNIString::new("canTakeReceiptPhoto"),
            signature.method_signature(),
            &[],
        )
        .and_then(|value| value.z())
        .map_err(|e| e.to_string())
    })
}

/// `MainActivity.requestReceiptImage(kind, dest)`; the answer arrives in
/// [`Java_dev_dioxus_main_MainActivity_receiptImageResult`].
fn request_image(kind: super::ImageKind, dest: &std::path::Path) -> Result<(), String> {
    let kind = match kind {
        super::ImageKind::Camera => 0,
        super::ImageKind::Gallery => 1,
    };
    let dest = dest.to_string_lossy().into_owned();
    with_activity(|env, activity| {
        let dest = JString::from_str(env, dest).map_err(|e| e.to_string())?;
        let signature = RuntimeMethodSignature::from_str("(ILjava/lang/String;)V")
            .map_err(|e| e.to_string())?;
        env.call_method(
            activity,
            JNIString::new("requestReceiptImage"),
            signature.method_signature(),
            &[
                jni::objects::JValue::Int(kind),
                jni::objects::JValue::Object(&dest),
            ],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
}

/// Native half of `MainActivity.receiptImageResult(status, message)`,
/// called by Kotlin once the image is copied, or the pick ended otherwise.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_MainActivity_receiptImageResult<'caller>(
    mut unowned_env: jni::EnvUnowned<'caller>,
    _activity: JObject<'caller>,
    status: jni::sys::jint,
    message: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|env| -> Result<(), jni::errors::Error> {
        let result = match status {
            RESULT_SAVED => Ok(super::PickOutcome::Saved),
            RESULT_CANCELLED => Ok(super::PickOutcome::Cancelled),
            _ if message.is_null() => Err("image could not be copied".to_string()),
            _ => Err(message.try_to_string(env)?),
        };
        if let Some(sender) = PENDING_PICK.lock().ok().and_then(|mut p| p.take()) {
            // The receiver is gone if the screen that asked was left.
            let _ = sender.send(result);
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::LogErrorAndDefault>()
}

/// Runs `op` with the Activity, which `ndk-context` knows as the context.
fn with_activity<T>(
    op: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> Result<T, String>,
) -> Result<T, String> {
    with_env(|env| {
        let context_ptr = ndk_context::android_context().context() as jni::sys::jobject;
        if context_ptr.is_null() {
            return Err("Android context unavailable".to_string());
        }
        // SAFETY: ndk-context holds a valid global reference to the Activity
        // for the lifetime of the process.
        let activity = unsafe { JObject::from_raw(env, context_ptr) };
        op(env, &activity)
    })
}
