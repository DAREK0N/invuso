package dev.dioxus.main

import android.app.Activity
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Insets
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.KeyEvent
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.webkit.WebView
import java.lang.ref.WeakReference

// The generated wry sources reference `BuildConfig` from this package.
typealias BuildConfig = com.darekon.invuso.BuildConfig

/// Thin platform bridge (AGENTS.md 5, stage 4). It only does what neither
/// Dioxus nor the WebView can: edge-to-edge window chrome with the real
/// system-bar insets, and routing the Android back key into the Dioxus router.
/// No business logic, no state, no UI.
class MainActivity : WryActivity() {
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

    // Close an open sheet first, otherwise pop the router history. Ids are
    // rendered by BottomSheet and RouterBackTarget.
    private const val BACK_SCRIPT = """
        (function () {
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
