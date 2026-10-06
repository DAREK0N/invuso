package dev.dioxus.main

import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Insets
import android.graphics.drawable.ColorDrawable
import android.icu.util.ULocale
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.MediaStore
import android.view.KeyEvent
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.view.translation.TranslationCapability
import android.view.translation.TranslationContext
import android.view.translation.TranslationManager
import android.view.translation.TranslationRequest
import android.view.translation.TranslationRequestValue
import android.view.translation.TranslationResponse
import android.view.translation.TranslationSpec
import android.webkit.WebView
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.RequiresApi
import java.io.File
import java.lang.ref.WeakReference

// The generated wry sources reference `BuildConfig` from this package.
typealias BuildConfig = com.darekon.invuso.BuildConfig

/// Thin platform bridge (AGENTS.md 5, stage 4). It only does what neither
/// Dioxus nor the WebView can: edge-to-edge window chrome with the real
/// system-bar insets, routing the Android back key into the Dioxus router,
/// opening the system camera or photo picker for receipt images, the
/// device's on-device translator (Java-only API) and the system share sheet
/// (the WebView has no `navigator.share`).
/// No business logic, no state beyond the running pick, no UI.
class MainActivity : WryActivity() {
    private val receiptImages = ReceiptImages(this)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        try {
            requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            InvusoChrome.configure(this)
        } catch (_: Throwable) {
        }
    }

    override fun onWebViewCreate(webView: WebView) {
        super.onWebViewCreate(webView)
        InvusoChrome.attachWebView(webView)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        InvusoChrome.pushToWebView()
    }

    /// Hardware key and back gesture alike. Intercepting here, ahead of the
    /// full-screen WebView, lets the Dioxus router own navigation. The WebView
    /// answers asynchronously; if it had nothing to close or pop, the
    /// platform default (leave the app) runs in the callback.
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_BACK) {
            if (event.action == KeyEvent.ACTION_UP) {
                InvusoChrome.dispatchBack { systemBack() }
            }
            return true
        }
        return super.dispatchKeyEvent(event)
    }

    /// Called from Rust (`platform/android.rs`): kind 0 = camera, 1 = photo
    /// picker. The image is copied to `dest`; the outcome arrives through
    /// `receiptImageResult`.
    fun requestReceiptImage(kind: Int, dest: String) {
        runOnUiThread { receiptImages.request(kind, dest) }
    }

    /// Called from Rust: whether the camera path works on this device.
    fun canTakeReceiptPhoto(): Boolean = receiptImages.canTakePhoto()

    /// Implemented in Rust. status 0 = saved, 1 = cancelled, 2 = failed.
    external fun receiptImageResult(status: Int, message: String?)

    /// Called from Rust (`platform/android.rs`): translates `texts` with the
    /// device's on-device translator; the outcome arrives through
    /// `translationResult` with the same `requestId`.
    fun translateTexts(requestId: Long, source: String, target: String, texts: Array<String>) {
        SystemTranslation.translate(this, requestId, source, target, texts)
    }

    /// Implemented in Rust. status 0 = done (one text per input), 1 = no
    /// engine for the pair, 2 = failed.
    external fun translationResult(requestId: Long, status: Int, texts: Array<String>?)

    /// Called from Rust (`platform/android.rs`): opens the system share sheet
    /// with plain text, e.g. a group's settlement for a messenger.
    fun shareText(text: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
        }
        runOnUiThread { startActivity(Intent.createChooser(send, null)) }
    }

    @Suppress("DEPRECATION")
    private fun systemBack() {
        try {
            super.onBackPressed()
        } catch (_: Throwable) {
            finish()
        }
    }
}

object InvusoChrome {
    private var activityRef: WeakReference<Activity>? = null
    private var webViewRef: WeakReference<WebView>? = null
    private var lastBackAt = 0L
    private const val BACK_DEBOUNCE_MS = 300L

    // Window background behind the transparent WebView: jet-black-950, so no
    // white flash appears before the first frame.
    private val WINDOW_BACKGROUND = Color.rgb(0x0c, 0x14, 0x18)

    // Close an open image viewer or sheet first, otherwise pop the router
    // history. Ids are rendered by ImageViewer, BottomSheet and
    // RouterBackTarget.
    private const val BACK_SCRIPT = """
        (function () {
            var viewer = document.getElementById('invuso-viewer-close');
            if (viewer) { viewer.click(); return 'viewer'; }
            var sheet = document.getElementById('invuso-sheet-backdrop');
            if (sheet) { sheet.click(); return 'sheet'; }
            var back = document.getElementById('invuso-router-back');
            if (!back || back.getAttribute('data-can-go-back') !== 'true') { return 'none'; }
            back.click();
            return 'router';
        })();
    """

    @Volatile private var insetTop = 0
    @Volatile private var insetRight = 0
    @Volatile private var insetBottom = 0
    @Volatile private var insetLeft = 0

    fun configure(activity: Activity) {
        activityRef = WeakReference(activity)
        val window = activity.window
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        // Android 10+ paints a scrim behind transparent bars unless contrast
        // enforcement is off; that scrim is what edge-to-edge should remove.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isStatusBarContrastEnforced = false
            window.isNavigationBarContrastEnforced = false
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val attrs = window.attributes
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            window.attributes = attrs
        }
        window.setBackgroundDrawable(ColorDrawable(WINDOW_BACKGROUND))
        applyEdgeToEdge(activity)
    }

    fun attachWebView(webView: WebView) {
        webViewRef = WeakReference(webView)
        webView.setBackgroundColor(Color.TRANSPARENT)
        // Android 12+ overscroll stretch drags the fixed BottomNav out of shape.
        webView.overScrollMode = View.OVER_SCROLL_NEVER
        // A page load wipes inline styles off <html>; re-push the cached
        // insets during the first seconds so they survive the initial load.
        val handler = Handler(Looper.getMainLooper())
        for (delay in longArrayOf(0L, 250L, 750L, 1500L, 3000L)) {
            handler.postDelayed({ pushToWebView() }, delay)
        }
    }

    fun dispatchBack(fallback: () -> Unit) {
        val now = SystemClock.uptimeMillis()
        if (now - lastBackAt < BACK_DEBOUNCE_MS) {
            return
        }
        lastBackAt = now
        val webView = webViewRef?.get()
        if (webView == null) {
            fallback()
            return
        }
        try {
            webView.evaluateJavascript(BACK_SCRIPT) { result ->
                if (result == null || result.contains("none")) {
                    fallback()
                }
            }
        } catch (_: Throwable) {
            fallback()
        }
    }

    /// Android WebView only fills `env(safe-area-inset-*)` from the display
    /// cutout, never from the status or navigation bar. Once the window is
    /// edge-to-edge, the real insets are mirrored into `--safe-area-*`.
    private fun applyEdgeToEdge(activity: Activity) {
        val decor = activity.window.decorView
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            activity.window.setDecorFitsSystemWindows(false)
        } else {
            @Suppress("DEPRECATION")
            decor.systemUiVisibility = decor.systemUiVisibility or
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        }
        decor.setOnApplyWindowInsetsListener { view, insets ->
            captureInsets(activity, insets)
            view.onApplyWindowInsets(withoutIme(insets))
        }
        decor.requestApplyInsets()
    }

    /// The page lifts itself above the keyboard via `--safe-area-bottom`; a
    /// WebView that also sees the IME inset would shrink a second time.
    private fun withoutIme(insets: WindowInsets): WindowInsets {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            return insets
        }
        return try {
            WindowInsets.Builder(insets).setInsets(WindowInsets.Type.ime(), Insets.NONE).build()
        } catch (_: Throwable) {
            insets
        }
    }

    /// WebView CSS pixels are density-independent, so physical insets are
    /// divided by the display density.
    private fun captureInsets(activity: Activity, insets: WindowInsets) {
        val density = activity.resources.displayMetrics.density.takeIf { it > 0f } ?: 1f
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val bars = insets.getInsets(
                    WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout(),
                )
                // Edge-to-edge windows no longer resize for the keyboard on
                // API 30+, so the IME height folds into the bottom inset.
                val ime = insets.getInsets(WindowInsets.Type.ime()).bottom
                insetTop = Math.round(bars.top / density)
                insetRight = Math.round(bars.right / density)
                insetBottom = Math.round(maxOf(bars.bottom, ime) / density)
                insetLeft = Math.round(bars.left / density)
            } else {
                @Suppress("DEPRECATION")
                run {
                    insetTop = Math.round(insets.systemWindowInsetTop / density)
                    insetRight = Math.round(insets.systemWindowInsetRight / density)
                    insetBottom = Math.round(insets.systemWindowInsetBottom / density)
                    insetLeft = Math.round(insets.systemWindowInsetLeft / density)
                }
            }
        } catch (_: Throwable) {
            return
        }
        pushToWebView()
    }

    fun pushToWebView() {
        val webView = webViewRef?.get() ?: return
        try {
            webView.evaluateJavascript(
                """
                (function () {
                    var root = document.documentElement;
                    if (!root) { return; }
                    root.style.setProperty('--safe-area-top', '${insetTop}px');
                    root.style.setProperty('--safe-area-right', '${insetRight}px');
                    root.style.setProperty('--safe-area-bottom', '${insetBottom}px');
                    root.style.setProperty('--safe-area-left', '${insetLeft}px');
                })();
                """.trimIndent(),
                null,
            )
        } catch (_: Throwable) {
        }
    }
}

/// Opens the system photo picker or camera app and copies the image to the
/// path Rust asked for. The camera needs a writable content URI; dx cannot
/// declare a FileProvider, so a MediaStore entry of this app takes the photo
/// and is deleted right after copying (or on cancel). Android 10+ needs no
/// permission for that. The entry cannot be pending (hidden): only its owner
/// may write to a pending entry, not the camera app.
class ReceiptImages(private val activity: MainActivity) {
    private var dest: String? = null
    private var captureUri: Uri? = null

    private val pickLauncher =
        activity.registerForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
            if (uri == null) finish(CANCELLED, null) else copy(uri, deleteAfter = false)
        }

    private val takeLauncher =
        activity.registerForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
            val uri = captureUri
            captureUri = null
            when {
                uri == null -> finish(CANCELLED, null)
                saved -> copy(uri, deleteAfter = true)
                else -> {
                    delete(uri)
                    finish(CANCELLED, null)
                }
            }
        }

    fun canTakePhoto(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
            activity.packageManager.hasSystemFeature("android.hardware.camera.any")

    fun request(kind: Int, path: String) {
        // A pick still running is answered as cancelled by Rust already.
        dest = path
        try {
            if (kind == CAMERA) {
                val uri = createCaptureUri() ?: return finish(FAILED, "camera unavailable")
                captureUri = uri
                takeLauncher.launch(uri)
            } else {
                pickLauncher.launch(
                    PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                )
            }
        } catch (error: Throwable) {
            captureUri?.let { delete(it) }
            captureUri = null
            finish(FAILED, error.message ?: error.javaClass.simpleName)
        }
    }

    private fun createCaptureUri(): Uri? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            return null
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, "invuso-receipt-${System.currentTimeMillis()}.jpg")
            put(MediaStore.MediaColumns.MIME_TYPE, "image/jpeg")
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Invuso")
        }
        return activity.contentResolver.insert(
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            values,
        )
    }

    private fun copy(uri: Uri, deleteAfter: Boolean) {
        val target = dest ?: return finish(FAILED, "no target")
        // Off the main thread: photos are several megabytes.
        Thread {
            try {
                val input = activity.contentResolver.openInputStream(uri)
                    ?: throw IllegalStateException("image unreadable")
                input.use { source ->
                    File(target).outputStream().use { sink -> source.copyTo(sink) }
                }
                if (deleteAfter) delete(uri)
                finish(SAVED, null)
            } catch (error: Throwable) {
                if (deleteAfter) delete(uri)
                finish(FAILED, error.message ?: error.javaClass.simpleName)
            }
        }.start()
    }

    private fun delete(uri: Uri) {
        try {
            activity.contentResolver.delete(uri, null, null)
        } catch (_: Throwable) {
        }
    }

    private fun finish(status: Int, message: String?) {
        dest = null
        try {
            activity.receiptImageResult(status, message)
        } catch (_: Throwable) {
        }
    }

    private companion object {
        const val CAMERA = 0
        const val SAVED = 0
        const val CANCELLED = 1
        const val FAILED = 2
    }
}

/// Android 12+ `TranslationManager`: used only if the device reports the
/// pair as installed on the device; everything else answers "unavailable",
/// so the app falls back to its own engine.
object SystemTranslation {
    private const val DONE = 0
    private const val UNAVAILABLE = 1
    private const val FAILED = 2

    fun translate(
        activity: MainActivity,
        id: Long,
        source: String,
        target: String,
        texts: Array<String>,
    ) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            return finish(activity, id, UNAVAILABLE, null)
        }
        // The capability query blocks on the system service.
        Thread {
            try {
                run(activity, id, source, target, texts)
            } catch (_: Throwable) {
                finish(activity, id, FAILED, null)
            }
        }.start()
    }

    @RequiresApi(Build.VERSION_CODES.S)
    private fun run(
        activity: MainActivity,
        id: Long,
        source: String,
        target: String,
        texts: Array<String>,
    ) {
        val manager = activity.getSystemService(TranslationManager::class.java)
            ?: return finish(activity, id, UNAVAILABLE, null)
        val format = TranslationSpec.DATA_FORMAT_TEXT
        val installed = manager.getOnDeviceTranslationCapabilities(format, format).any {
            it.state == TranslationCapability.STATE_ON_DEVICE &&
                it.sourceSpec.locale.language == source &&
                it.targetSpec.locale.language == target
        }
        if (!installed) {
            return finish(activity, id, UNAVAILABLE, null)
        }
        val context = TranslationContext.Builder(
            TranslationSpec(ULocale(source), format),
            TranslationSpec(ULocale(target), format),
        ).build()
        manager.createOnDeviceTranslator(context, { it.run() }) { translator ->
            if (translator == null) {
                finish(activity, id, UNAVAILABLE, null)
                return@createOnDeviceTranslator
            }
            val request = TranslationRequest.Builder()
                .setTranslationRequestValues(texts.map { TranslationRequestValue.forText(it) })
                .build()
            try {
                translator.translate(request, null, { it.run() }) { response ->
                    translator.destroy()
                    if (response.translationStatus != TranslationResponse.TRANSLATION_STATUS_SUCCESS) {
                        finish(activity, id, FAILED, null)
                    } else {
                        val values = response.translationResponseValues
                        val translated = Array(texts.size) { values.get(it)?.text?.toString() ?: "" }
                        finish(activity, id, DONE, translated)
                    }
                }
            } catch (_: Throwable) {
                translator.destroy()
                finish(activity, id, FAILED, null)
            }
        }
    }

    private fun finish(activity: MainActivity, id: Long, status: Int, texts: Array<String>?) {
        try {
            activity.translationResult(id, status, texts)
        } catch (_: Throwable) {
        }
    }
}
