package com.manage_dan.app

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.EditText
import android.widget.FrameLayout
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {

    private lateinit var webView: WebView
    private val prefs by lazy { getSharedPreferences("manage_dan_prefs", Context.MODE_PRIVATE) }

    // URL to load on the next onResume — set by deep-link intents so that
    // webView.loadUrl() is always called once the WebView is fully active.
    private var pendingUrl: String? = null

    /** Exposed to JavaScript as `window.AndroidVibrator`. */
    private inner class VibrationBridge {
        @JavascriptInterface
        fun vibrate(duration: Long) {
            val ms = duration.coerceIn(1L, 500L)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val mgr = getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager
                mgr.defaultVibrator.vibrate(
                    VibrationEffect.createOneShot(ms, VibrationEffect.DEFAULT_AMPLITUDE)
                )
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                @Suppress("DEPRECATION")
                (getSystemService(Context.VIBRATOR_SERVICE) as Vibrator).vibrate(
                    VibrationEffect.createOneShot(ms, VibrationEffect.DEFAULT_AMPLITUDE)
                )
            } else {
                @Suppress("DEPRECATION")
                (getSystemService(Context.VIBRATOR_SERVICE) as Vibrator).vibrate(ms)
            }
        }
    }

    /**
     * Exposed to JavaScript as `window.AndroidBridge`. Pull-to-reload on the
     * dashboard calls [reload]; the "cannot reach server" page's
     * "Change Server URL" button calls [changeServerUrl] — the only two
     * places a URL change or reload can be triggered from the page itself.
     */
    private inner class AndroidBridge {
        @JavascriptInterface
        fun reload() {
            runOnUiThread { webView.reload() }
        }

        @JavascriptInterface
        fun changeServerUrl() {
            runOnUiThread { promptForUrl() }
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        setContentView(R.layout.activity_main)
        webView = findViewById(R.id.webview)

        webView.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            loadWithOverviewMode = true
            useWideViewPort = true
            setSupportZoom(false)
            builtInZoomControls = false
            displayZoomControls = false
            // Allow mixed content for local HTTP servers
            @Suppress("DEPRECATION")
            mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_ALWAYS_ALLOW
        }

        // Prevent WebView's own overscroll glow/stretch from conflicting with
        // the page's internal scrollable panes (groups, quick-add, item list).
        webView.overScrollMode = View.OVER_SCROLL_NEVER
        webView.isVerticalScrollBarEnabled = false
        webView.isHorizontalScrollBarEnabled = false

        // Expose native vibration so navigator.vibrate() works inside the page.
        webView.addJavascriptInterface(VibrationBridge(), "AndroidVibrator")
        // Expose reload/change-URL so the page can trigger them (pull-to-reload,
        // the "cannot reach server" page's Change Server URL button).
        webView.addJavascriptInterface(AndroidBridge(), "AndroidBridge")

        webView.webViewClient = object : WebViewClient() {
            override fun onPageFinished(view: WebView, url: String) {
                view.evaluateJavascript(
                    """
                    (function() {
                        // 1. Route navigator.vibrate() through the Android bridge.
                        //    WebView ignores it by default even with VIBRATE permission.
                        if (typeof window.AndroidVibrator !== 'undefined') {
                            Object.defineProperty(navigator, 'vibrate', {
                                value: function(pattern) {
                                    var ms = Array.isArray(pattern) ? pattern[0]
                                           : (typeof pattern === 'number' ? pattern : 0);
                                    if (ms > 0) window.AndroidVibrator.vibrate(ms);
                                    return true;
                                },
                                configurable: true,
                                writable: true
                            });
                        }

                        // 2. Left swipe (not on an item row) navigates back to the
                        //    group/list sidebar, mirroring the header back button.
                        var _sx = 0, _sy = 0, _onItem = false;
                        document.addEventListener('touchstart', function(e) {
                            _sx = e.touches[0].clientX;
                            _sy = e.touches[0].clientY;
                            _onItem = !!e.target.closest('.item-row');
                        }, { passive: true });
                        document.addEventListener('touchend', function(e) {
                            if (_onItem) return;
                            var dx = e.changedTouches[0].clientX - _sx;
                            var dy = e.changedTouches[0].clientY - _sy;
                            // Left swipe: negative dx, more horizontal than vertical,
                            // minimum 60 px travel.
                            if (dx < -60 && Math.abs(dx) > Math.abs(dy) * 1.5) {
                                var main = document.getElementById('main');
                                if (main && main.classList.contains('show-panel')) {
                                    if (typeof goBack === 'function') goBack();
                                }
                            }
                        }, { passive: true });

                        // 3. Swipe down on the dashboard (Home view), starting from
                        //    the top of the page, reloads the app. The indicator banner
                        //    stays up a beat, then the screen blanks momentarily before
                        //    the actual reload fires, so the refresh reads as deliberate
                        //    rather than an abrupt jump cut.
                        var _reloading = false;
                        function triggerReload() {
                            if (_reloading) return;
                            _reloading = true;

                            if (!document.getElementById('android-reload-style')) {
                                var style = document.createElement('style');
                                style.id = 'android-reload-style';
                                style.textContent = '@keyframes android-spin { to { transform: rotate(360deg); } }';
                                document.head.appendChild(style);
                            }
                            var el = document.createElement('div');
                            el.id = 'android-reload-indicator';
                            el.style.cssText = 'position:fixed;top:0;left:0;right:0;display:flex;' +
                                'align-items:center;justify-content:center;gap:8px;padding:10px;' +
                                'background:#1a1d27;color:#e4e8f0;font-family:sans-serif;font-size:0.9rem;' +
                                'z-index:999999;transform:translateY(-100%);transition:transform 0.2s ease-out;';
                            el.innerHTML = '<span style="display:inline-block;width:14px;height:14px;' +
                                'border:2px solid #7a82a0;border-top-color:#5b8dee;border-radius:50%;' +
                                'animation:android-spin 0.6s linear infinite"></span><span>Reloading…</span>';
                            document.body.appendChild(el);
                            requestAnimationFrame(function() { el.style.transform = 'translateY(0)'; });

                            // Let the banner register with the user, then blank the
                            // screen (indicator stays on top of the blank layer),
                            // then actually reload.
                            setTimeout(function() {
                                var blank = document.createElement('div');
                                blank.id = 'android-reload-blank';
                                blank.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;' +
                                    'background:#0f1117;z-index:999998;';
                                document.body.appendChild(blank);
                                setTimeout(function() {
                                    window.AndroidBridge.reload();
                                }, 400);
                            }, 900);
                        }
                        var _dsx = 0, _dsy = 0, _dOk = false;
                        document.addEventListener('touchstart', function(e) {
                            var home = document.getElementById('home-overlay');
                            var atTop = (document.scrollingElement || document.documentElement).scrollTop <= 0;
                            _dOk = !!(home && home.classList.contains('active') && atTop);
                            _dsx = e.touches[0].clientX;
                            _dsy = e.touches[0].clientY;
                        }, { passive: true });
                        document.addEventListener('touchend', function(e) {
                            if (!_dOk) return;
                            var dx = e.changedTouches[0].clientX - _dsx;
                            var dy = e.changedTouches[0].clientY - _dsy;
                            if (dy > 80 && dy > Math.abs(dx) * 1.5 && typeof window.AndroidBridge !== 'undefined') {
                                triggerReload();
                            }
                        }, { passive: true });
                    })();
                    """.trimIndent(), null
                )
            }

            override fun onReceivedError(
                view: WebView, request: WebResourceRequest, error: WebResourceError
            ) {
                // Only show error for the main frame, not sub-resources
                if (request.isForMainFrame) {
                    val html = """
                        <html><body style="background:#0f1117;color:#e4e8f0;font-family:sans-serif;
                            display:flex;flex-direction:column;align-items:center;justify-content:center;
                            height:100vh;margin:0;gap:16px;text-align:center;padding:32px;">
                          <div style="font-size:3rem">⚠️</div>
                          <div style="font-size:1.3rem;font-weight:600">Cannot reach server</div>
                          <div style="color:#7a82a0">${error.description}</div>
                          <button onclick="location.reload()"
                            style="margin-top:8px;background:#5b8dee;color:#fff;border:none;
                                   border-radius:10px;padding:14px 28px;font-size:1rem;cursor:pointer">
                            Retry
                          </button>
                          <button onclick="AndroidBridge.changeServerUrl()"
                            style="background:transparent;color:#7a82a0;border:1px solid #7a82a0;
                                   border-radius:10px;padding:14px 28px;font-size:1rem;cursor:pointer">
                            Change Server URL
                          </button>
                        </body></html>
                    """.trimIndent()
                    view.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
                }
            }
        }

        webView.webChromeClient = object : WebChromeClient() {
            override fun onJsPrompt(
                view: WebView,
                url: String?,
                message: String?,
                defaultValue: String?,
                result: android.webkit.JsPromptResult
            ): Boolean {
                val input = EditText(this@MainActivity).apply {
                    setText(defaultValue ?: "")
                    setSelection(text.length)
                    setTextColor(Color.WHITE)
                    setPadding(48, 32, 48, 32)
                }
                val container = FrameLayout(this@MainActivity).apply { addView(input) }
                AlertDialog.Builder(this@MainActivity)
                    .setMessage(message)
                    .setView(container)
                    .setPositiveButton("OK") { _, _ -> result.confirm(input.text.toString()) }
                    .setNegativeButton("Cancel") { _, _ -> result.cancel() }
                    .setOnCancelListener { result.cancel() }
                    .show()
                return true
            }
        }

        val savedUrl = prefs.getString("server_url", null)
        if (savedUrl == null) {
            promptForUrl(firstRun = true)
        } else {
            // Queue whichever URL should open — deep link wins over default.
            pendingUrl = deepLinkUrl(intent) ?: savedUrl
        }
    }

    // webView.loadUrl() must be called once the WebView is fully active.
    // onResume is the correct place: it runs after onNewIntent and after the
    // activity window is visible, so the load always takes effect.
    override fun onResume() {
        super.onResume()
        pendingUrl?.let {
            webView.loadUrl(it)
            pendingUrl = null
        }
    }

    // Called when the activity is already running (singleTop) and a new intent arrives.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Store the URL; onResume fires next and loads it.
        deepLinkUrl(intent)?.let { pendingUrl = it }
    }

    /**
     * Extracts a recognised manage-dan:// deep-link URL from the intent and
     * maps it to the corresponding page on the configured server.
     * Returns null if the intent is not a recognised deep link.
     */
    private fun deepLinkUrl(intent: Intent): String? {
        val uri: Uri = intent.data ?: return null
        if (uri.scheme != "manage-dan") return null

        val serverUrl = prefs.getString("server_url", null) ?: return null

        return when (uri.host) {
            "todo" -> {
                val taskId = uri.pathSegments.firstOrNull() ?: return null
                taskId.toLongOrNull() ?: return null  // validate it's a number
                "$serverUrl/todo/$taskId"
            }
            "notes" -> {
                val uuid = uri.pathSegments.firstOrNull() ?: return null
                if (uuid.isBlank()) return null
                "$serverUrl/notes/$uuid"
            }
            else -> null
        }
    }

    private fun promptForUrl(firstRun: Boolean = false) {
        val currentUrl = prefs.getString("server_url", "http://192.168.1.")
        val input = EditText(this).apply {
            setText(currentUrl)
            setSelection(text.length)
            setTextColor(Color.WHITE)
            hint = "http://192.168.1.x"
            setPadding(48, 32, 48, 32)
        }
        val container = FrameLayout(this).apply {
            addView(input)
        }

        AlertDialog.Builder(this)
            .setTitle(getString(R.string.dialog_title))
            .setMessage(getString(R.string.dialog_message))
            .setView(container)
            .setPositiveButton(getString(R.string.dialog_connect)) { _, _ ->
                val url = input.text.toString().trim().trimEnd('/')
                prefs.edit().putString("server_url", url).apply()
                // Load deep-link target if one was pending, otherwise the main page.
                webView.loadUrl(deepLinkUrl(intent) ?: url)
            }
            .apply { if (!firstRun) setNegativeButton("Cancel", null) }
            .setCancelable(!firstRun)
            .show()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (webView.canGoBack()) webView.goBack()
        else super.onBackPressed()
    }
}
