package com.manage_dan.app

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.view.Menu
import android.view.MenuItem
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
import androidx.appcompat.widget.Toolbar

class MainActivity : AppCompatActivity() {

    private lateinit var webView: WebView
    private val prefs by lazy { getSharedPreferences("manage_dan_prefs", Context.MODE_PRIVATE) }

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

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        setContentView(R.layout.activity_main)
        val toolbar = findViewById<Toolbar>(R.id.toolbar)
        setSupportActionBar(toolbar)
        supportActionBar?.title = "manage_dan"
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
                        </body></html>
                    """.trimIndent()
                    view.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
                }
            }
        }

        webView.webChromeClient = WebChromeClient()

        val savedUrl = prefs.getString("server_url", null)
        if (savedUrl == null) {
            promptForUrl(firstRun = true)
        } else {
            webView.loadUrl(savedUrl)
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
                webView.loadUrl(url)
            }
            .apply { if (!firstRun) setNegativeButton("Cancel", null) }
            .setCancelable(!firstRun)
            .show()
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.main_menu, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_reload -> { webView.reload(); true }
            R.id.action_settings -> { promptForUrl(); true }
            else -> super.onOptionsItemSelected(item)
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (webView.canGoBack()) webView.goBack()
        else super.onBackPressed()
    }
}
