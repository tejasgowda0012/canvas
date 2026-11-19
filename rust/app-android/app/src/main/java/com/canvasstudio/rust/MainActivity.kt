package com.canvasstudio.rust

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.PixelFormat
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.HapticFeedbackConstants
import android.view.KeyEvent
import android.view.SurfaceHolder
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import java.io.File
import java.util.TimeZone

/**
 * Android API host. Rust owns every visible screen, control, dialog and board.
 * The only Android text widget is an invisible IME connection for the Rust field.
 */
class MainActivity : Activity(), SurfaceHolder.Callback {
    private val main = Handler(Looper.getMainLooper())
    private val handwriting by lazy { Handwriting(applicationContext) }
    private val updater by lazy { Updater(this) { message -> notifyRust(message) } }
    @Volatile private var handle = 0L
    private var destroying = false
    private lateinit var surface: BoardSurface
    private lateinit var input: EditText
    private var settingText = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = FrameLayout(this).apply {
            setBackgroundColor(Color.rgb(246, 245, 250))
            setOnApplyWindowInsetsListener { view, insets ->
                @Suppress("DEPRECATION")
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                insets
            }
        }
        surface = BoardSurface(this).apply {
            holder.setFormat(PixelFormat.RGBA_8888)
            holder.addCallback(this@MainActivity)
            isFocusable = true
            isFocusableInTouchMode = true
        }
        root.addView(surface, FrameLayout.LayoutParams(-1, -1))
        input = EditText(this).apply {
            setTextColor(Color.TRANSPARENT)
            setBackgroundColor(Color.TRANSPARENT)
            isCursorVisible = false
            setPadding(0, 0, 0, 0)
            setTextIsSelectable(false)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                    val h = handle
                    if (!settingText && h != 0L) RustApp.textInput(h, s?.toString() ?: "", false)
                }
                override fun afterTextChanged(s: Editable?) {}
            })
            setOnEditorActionListener { _, actionId, event ->
                val done = actionId == EditorInfo.IME_ACTION_DONE ||
                    (event?.action == KeyEvent.ACTION_DOWN && event.keyCode == KeyEvent.KEYCODE_ENTER && maxLines == 1)
                if (done && handle != 0L) RustApp.textInput(handle, text.toString(), true)
                done
            }
        }
        root.addView(input, FrameLayout.LayoutParams(1, 1))
        setContentView(root)
        surface.requestFocus()
        val version = packageManager.getPackageInfo(packageName, 0).versionName ?: "0"
        handle = RustApp.create(this,
            File(filesDir, "boards").apply { mkdirs() }.absolutePath,
            File(filesDir, "settings.json").absolutePath,
            version, TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000)
        surface.handle = handle
        // A Surface may have been created while the engine was starting.
        if (surface.holder.surface.isValid && surface.width > 0 && surface.height > 0) {
            val frame = surface.holder.surfaceFrame
            RustApp.surface(handle, surface.holder.surface, frame.width(), frame.height(), density())
        }
        onNewIntent(intent)
    }

    private fun density(): Double {
        val dp = resources.displayMetrics.density
        val shortestDp = resources.configuration.smallestScreenWidthDp.toFloat()
        // Wall panels need larger physical controls than handheld tablets.
        val panelScale = when { shortestDp >= 1000f -> 1.4; shortestDp >= 800f -> 1.2; else -> 1.0 }
        return dp * surface.scale.toDouble() * panelScale
    }

    override fun surfaceCreated(holder: SurfaceHolder) {}
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val h = handle
        if (h != 0L) RustApp.surface(h, holder.surface, width, height, density())
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        val h = handle
        if (h != 0L) RustApp.surface(h, null, 0, 0, 1.0)
    }

    override fun onPause() {
        val h = handle
        if (h != 0L) RustApp.pause(h)
        super.onPause()
    }

    override fun onDestroy() {
        destroying = true
        val h = handle
        handle = 0L
        surface.handle = 0L
        main.removeCallbacksAndMessages(null)
        handwriting.close()
        if (h != 0L) RustApp.destroy(h)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.action == Updater.ACTION) updater.onResult(intent)
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() { if (handle != 0L) RustApp.back(handle) else finish() }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (handle != 0L && keyCode in intArrayOf(KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN,
                KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT, KeyEvent.KEYCODE_DPAD_CENTER)) {
            RustApp.key(handle, keyCode)
            return true
        }
        if (handle != 0L && !input.hasFocus() && keyCode in intArrayOf(KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER)) {
            RustApp.key(handle, keyCode)
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    // These callbacks are invoked from the Rust render thread. No UI is drawn here.
    private fun onMain(block: () -> Unit) { main.post { if (!destroying) block() } }
    fun haptic() { onMain { surface.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK) } }

    fun showKeyboard(text: String, multiline: Boolean) { onMain {
        if (!destroying) {
            settingText = true
            input.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or
                (if (multiline) InputType.TYPE_TEXT_FLAG_MULTI_LINE else 0)
            input.setSingleLine(!multiline)
            input.imeOptions = if (multiline) EditorInfo.IME_ACTION_NONE else EditorInfo.IME_ACTION_DONE
            input.setText(text)
            input.setSelection(input.text.length)
            settingText = false
            input.requestFocus()
            (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
        }
    } }

    fun hideKeyboard() { onMain {
        if (!destroying) {
            (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(input.windowToken, 0)
            input.clearFocus()
            surface.requestFocus()
        }
    } }

    fun prepareHandwriting(lang: String) { onMain { handwriting.prepare(lang) } }

    fun readInk(token: Int, lang: String, data: FloatArray) { onMain {
        if (!destroying) handwriting.read(lang, data) { text ->
            val h = handle
            if (h != 0L) RustApp.inkResult(h, token, text)
        }
    } }

    fun checkUpdates(manual: Boolean) { onMain {
        if (!destroying) updater.check(manual) { version, url, failed ->
            val h = handle
            if (h != 0L) RustApp.updateResult(h, version, url, failed, manual)
        }
    } }

    fun installUpdate(url: String) { onMain { updater.download(url) } }
    fun setFullResolution(on: Boolean) { onMain { surface.maxSide = if (on) 0 else BoardSurface.MAX_SIDE } }
    fun finishApp() { onMain { finish() } }

    /** Opens a web page (Google sign-in, links) in the browser. */
    fun openUrl(url: String) { onMain {
        try {
            startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url)))
        } catch (e: Exception) {
            RustApp.toast(handle, "No browser is installed. Use a code to sign in instead.")
        }
    } }

    fun copyText(text: String) { onMain {
        val clip = getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        clip.setPrimaryClip(android.content.ClipData.newPlainText("Inkboard", text))
    } }

    /** Saves an exported board to Pictures/Inkboard (app pictures before Android 10). */
    fun savePng(title: String, png: ByteArray) {
        Thread {
            val safe = title.replace(Regex("[^A-Za-z0-9 _-]"), "").trim().ifEmpty { "Board" }
            val name = "$safe ${System.currentTimeMillis() / 1000}.png"
            val message = try {
                if (android.os.Build.VERSION.SDK_INT >= 29) {
                    val values = android.content.ContentValues().apply {
                        put(android.provider.MediaStore.Images.Media.DISPLAY_NAME, name)
                        put(android.provider.MediaStore.Images.Media.MIME_TYPE, "image/png")
                        put(android.provider.MediaStore.Images.Media.RELATIVE_PATH, "Pictures/Inkboard")
                    }
                    val uri = contentResolver.insert(android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
                        ?: throw java.io.IOException("no media store")
                    contentResolver.openOutputStream(uri)?.use { it.write(png) } ?: throw java.io.IOException("no stream")
                    "Saved to Pictures/Inkboard"
                } else {
                    val dir = getExternalFilesDir(android.os.Environment.DIRECTORY_PICTURES) ?: filesDir
                    File(dir, name).writeBytes(png)
                    "Saved to ${dir.path}"
                }
            } catch (e: Exception) {
                "Could not save the picture"
            }
            main.post { notifyRust(message) }
        }.start()
    }
    private fun notifyRust(message: String) { val h = handle; if (h != 0L) RustApp.toast(h, message) }

    /** Opens the system file picker for board files (for example ones copied from Canvas Studio). */
    fun pickBoards() {
        onMain {
            val pick = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("application/json", "text/plain", "application/octet-stream"))
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            }
            try {
                @Suppress("DEPRECATION")
                startActivityForResult(pick, PICK_BOARDS)
            } catch (e: Exception) {
                notifyRust("No file picker on this device")
            }
        }
    }

    private var pickedFile: Pair<Int, Int>? = null

    fun pickFile(token: Int, kind: Int) {
        onMain {
            val h = handle
            if (h == 0L) return@onMain
            if (pickedFile != null) {
                RustApp.fileResult(h, token, "", "", null, "A file picker is already open")
                return@onMain
            }
            pickedFile = token to kind
            try {
                @Suppress("DEPRECATION")
                startActivityForResult(ContentImport.intent(kind), PICK_FILE)
            } catch (e: Exception) {
                pickedFile = null
                RustApp.fileResult(h, token, "", "", null, "No file picker on this device")
            }
        }
    }

    private fun filePicked(uri: android.net.Uri?) {
        val (token, kind) = pickedFile ?: return
        pickedFile = null
        val h = handle
        if (h == 0L) return
        if (uri == null) {
            RustApp.fileResult(h, token, "", "", null, null)
            return
        }
        Thread {
            try {
                val file = ContentImport.read(this, uri, kind)
                onMain {
                    if (handle == h) RustApp.fileResult(h, token, file.name, file.mime, file.bytes, null)
                }
            } catch (e: Exception) {
                val error = if (e is java.io.IOException) e.message ?: "Could not read the file" else "Could not read the file"
                onMain { if (handle == h) RustApp.fileResult(h, token, "", "", null, error) }
            }
        }.start()
    }

    @Deprecated("Platform API")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == PICK_FILE) {
            filePicked(if (resultCode == RESULT_OK) data?.data else null)
            return
        }
        if (requestCode != PICK_BOARDS || resultCode != RESULT_OK || data == null) return
        val uris = ArrayList<android.net.Uri>()
        data.clipData?.let { clip -> for (i in 0 until clip.itemCount) uris.add(clip.getItemAt(i).uri) }
        if (uris.isEmpty()) data.data?.let { uris.add(it) }
        if (uris.isEmpty()) return
        Thread {
            val files = org.json.JSONArray()
            for (uri in uris) {
                try {
                    contentResolver.openInputStream(uri)?.use { input ->
                        val bytes = input.readBytes()
                        files.put(if (bytes.size <= MAX_BOARD_BYTES) String(bytes, Charsets.UTF_8) else "")
                    }
                } catch (e: Exception) {
                    files.put("")
                }
            }
            val text = files.toString()
            main.post { val h = handle; if (h != 0L) RustApp.importBoards(h, text) }
        }.start()
    }

    private companion object {
        const val PICK_BOARDS = 41
        const val PICK_FILE = 42
        const val MAX_BOARD_BYTES = 32 * 1024 * 1024
    }
}
