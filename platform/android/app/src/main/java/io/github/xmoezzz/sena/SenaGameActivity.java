package io.github.xmoezzz.sena;

import android.os.Bundle;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Rect;
import android.util.DisplayMetrics;
import android.view.Choreographer;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.widget.Toast;
import android.app.AlertDialog;
import java.lang.ref.WeakReference;
import java.util.concurrent.ConcurrentHashMap;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;

/**
 * Android player activity using the same host-driven model as iOS:
 * - Java owns the main loop (Choreographer)
 * - Java owns the Surface lifecycle (SurfaceView)
 * - Rust is stepped via sena_android_* exported symbols
 */
public final class SenaGameActivity extends AppCompatActivity
        implements SurfaceHolder.Callback, Choreographer.FrameCallback, View.OnTouchListener {

    private static final ConcurrentHashMap<Long, WeakReference<SenaGameActivity>> sActivitiesByHandle = new ConcurrentHashMap<>();

    public static void onNativeMessagebox(long engineHandle, long requestId, int kind, String title, String message) {
        WeakReference<SenaGameActivity> ref = sActivitiesByHandle.get(engineHandle);
        SenaGameActivity activity = ref != null ? ref.get() : null;
        if (activity == null) {
            NativeSena.submitMessageboxResult(engineHandle, requestId, fallbackMessageboxValue(kind));
            return;
        }
        activity.runOnUiThread(() -> activity.showNativeMessagebox(engineHandle, requestId, kind, title, message));
    }

    private static long fallbackMessageboxValue(int kind) {
        switch (kind) {
            case 0: return 0; // OK
            case 1: return 1; // OK/CANCEL -> CANCEL
            case 2: return 1; // YES/NO -> NO
            case 3: return 2; // YES/NO/CANCEL -> CANCEL
            default: return 0;
        }
    }

    private SurfaceView surfaceView;

    private long handle = 0;
    private boolean running = false;
    private long lastFrameNs = 0;
    private long lastFrameGeneration = 0;
    private Bitmap frameBitmap = null;
    private int[] framePixels = null;

    /** Where the letterboxed frame lands inside the surface. */
    private final Rect contentRect = new Rect();
    private int surfaceWidth = 0;
    private int surfaceHeight = 0;
    private int reportedContentWidth = 0;
    private int reportedContentHeight = 0;

    private String gameRoot;
    private String nlsValue = NlsOption.SHIFT_JIS.value;
    private String langValue = LangOption.DEFAULT.value;

    @Override
    protected void onCreate(@Nullable Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        // Fullscreen immersive.
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        setContentView(R.layout.activity_player);

        surfaceView = findViewById(R.id.surface_view);
        LaunchParams p = readLaunchParams();
        if (p == null) {
            Toast.makeText(this, "Missing launch.json", Toast.LENGTH_LONG).show();
            finish();
            return;
        }
        gameRoot = p.gameRoot;
        nlsValue = p.nls;
        langValue = p.lang;

        surfaceView.getHolder().addCallback(this);
        surfaceView.setOnTouchListener(this);
        surfaceView.setFocusable(true);
        surfaceView.setFocusableInTouchMode(true);
        surfaceView.requestFocus();
        surfaceView.setKeepScreenOn(true);

        applyImmersive();
    }

    @Override
    protected void onResume() {
        super.onResume();
        applyImmersive();
        maybeStartFrameLoop();
    }

    @Override
    protected void onPause() {
        stopFrameLoop();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        stopFrameLoop();
        destroyEngine();
        super.onDestroy();
    }

    private void applyImmersive() {
        final View decor = getWindow().getDecorView();
        WindowInsetsControllerCompat c = ViewCompat.getWindowInsetsController(decor);
        if (c != null) {
            c.hide(WindowInsetsCompat.Type.systemBars());
            c.setSystemBarsBehavior(WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
        }
    }

    // ---- Surface lifecycle ----

    @Override
    public void surfaceCreated(@NonNull SurfaceHolder holder) {
        ensureEngine(holder);
        maybeStartFrameLoop();
    }

    @Override
    public void surfaceChanged(@NonNull SurfaceHolder holder, int format, int width, int height) {
        if (handle == 0) {
            ensureEngine(holder);
        } else {
            // SurfaceChanged fires frequently (format/size). Avoid recreating the WGPU surface here;
            // just re-letterbox the existing swapchain. Surface recreation is handled by
            // surfaceCreated/Destroyed.
            surfaceWidth = Math.max(1, width);
            surfaceHeight = Math.max(1, height);
            updateContentRect();
        }
    }

    @Override
    public void surfaceDestroyed(@NonNull SurfaceHolder holder) {
        // The ANativeWindow behind this Surface is about to become invalid.
        stopFrameLoop();
        destroyEngine();
        finish();
    }

    private void ensureEngine(@NonNull SurfaceHolder holder) {
        if (handle != 0) {
            return;
        }
        if (gameRoot == null || gameRoot.isEmpty()) {
            Toast.makeText(this, "Missing game root", Toast.LENGTH_LONG).show();
            finish();
            return;
        }

        DisplayMetrics dm = getResources().getDisplayMetrics();
        double scale = dm.density;

        // Must initialize ndk-context before the Rust engine initializes audio backends.
        NativeSena.initAndroidContext(getApplicationContext());

        int w = holder.getSurfaceFrame() != null ? holder.getSurfaceFrame().width() : surfaceView.getWidth();
        int h = holder.getSurfaceFrame() != null ? holder.getSurfaceFrame().height() : surfaceView.getHeight();
        if (w <= 0 || h <= 0) {
            w = Math.max(1, surfaceView.getWidth());
            h = Math.max(1, surfaceView.getHeight());
        }

        long hnd = NativeSena.create(holder.getSurface(), w, h, scale, gameRoot, nlsValue, langValue);
        if (hnd == 0) {
            Toast.makeText(this, "Failed to create engine", Toast.LENGTH_LONG).show();
            finish();
            return;
        }
        handle = hnd;
        sActivitiesByHandle.put(handle, new WeakReference<>(this));
        NativeSena.setNativeMessageboxCallback(handle);
        surfaceWidth = Math.max(1, w);
        surfaceHeight = Math.max(1, h);
        reportedContentWidth = 0;
        reportedContentHeight = 0;
        updateContentRect();
    }

    private void destroyEngine() {
        if (handle != 0) {
            sActivitiesByHandle.remove(handle);
            NativeSena.destroy(handle);
            handle = 0;
            reportedContentWidth = 0;
            reportedContentHeight = 0;
        }
    }

    // ---- Frame loop ----

    private void maybeStartFrameLoop() {
        if (!running && handle != 0) {
            running = true;
            lastFrameNs = 0;
            Choreographer.getInstance().postFrameCallback(this);
        }
    }

    private void stopFrameLoop() {
        if (running) {
            running = false;
            lastFrameNs = 0;
            Choreographer.getInstance().removeFrameCallback(this);
        }
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        if (!running || handle == 0) {
            return;
        }

        if (lastFrameNs == 0) {
            lastFrameNs = frameTimeNanos;
        }
        long dtNs = frameTimeNanos - lastFrameNs;
        lastFrameNs = frameTimeNanos;

        int dtMs = (int) (dtNs / 1_000_000L);
        if (dtMs < 0) dtMs = 0;
        if (dtMs > 250) dtMs = 250; // clamp (pause/background)

        int exit = NativeSena.step(handle, dtMs);
        if (exit != 0) {
            finish();
            return;
        }
        drawLatestFrame();

        Choreographer.getInstance().postFrameCallback(this);
    }

    /**
     * Fit the engine frame inside the surface without changing its aspect ratio,
     * and report the content size to the engine so the touch mapping matches what
     * is on screen. A phone that is wider than the game's 16:9 keeps black bars
     * on the sides instead of stretching the picture.
     */
    private void updateContentRect() {
        int frameWidth = handle != 0 ? NativeSena.frameWidth(handle) : 0;
        int frameHeight = handle != 0 ? NativeSena.frameHeight(handle) : 0;
        if (frameWidth <= 0 || frameHeight <= 0 || surfaceWidth <= 0 || surfaceHeight <= 0) {
            contentRect.set(0, 0, Math.max(1, surfaceWidth), Math.max(1, surfaceHeight));
            return;
        }
        double scale = Math.min((double) surfaceWidth / frameWidth, (double) surfaceHeight / frameHeight);
        int width = Math.max(1, (int) Math.round(frameWidth * scale));
        int height = Math.max(1, (int) Math.round(frameHeight * scale));
        int left = (surfaceWidth - width) / 2;
        int top = (surfaceHeight - height) / 2;
        contentRect.set(left, top, left + width, top + height);
        if (handle != 0 && (width != reportedContentWidth || height != reportedContentHeight)) {
            reportedContentWidth = width;
            reportedContentHeight = height;
            NativeSena.resize(handle, width, height);
        }
    }

    private void drawLatestFrame() {
        if (handle == 0 || surfaceView == null) {
            return;
        }
        long gen = NativeSena.frameGeneration(handle);
        if (gen == 0 || gen == lastFrameGeneration) {
            return;
        }
        int w = NativeSena.frameWidth(handle);
        int h = NativeSena.frameHeight(handle);
        byte[] rgba = NativeSena.frameRgba(handle);
        if (w <= 0 || h <= 0 || rgba == null || rgba.length != w * h * 4) {
            return;
        }
        if (frameBitmap == null || frameBitmap.getWidth() != w || frameBitmap.getHeight() != h) {
            frameBitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888);
            framePixels = new int[w * h];
        } else if (framePixels == null || framePixels.length != w * h) {
            framePixels = new int[w * h];
        }
        for (int i = 0, p = 0; p < framePixels.length; p++, i += 4) {
            int r = rgba[i] & 0xff;
            int g = rgba[i + 1] & 0xff;
            int b = rgba[i + 2] & 0xff;
            int a = rgba[i + 3] & 0xff;
            framePixels[p] = (a << 24) | (r << 16) | (g << 8) | b;
        }
        frameBitmap.setPixels(framePixels, 0, w, 0, 0, w, h);
        Canvas canvas = null;
        try {
            canvas = surfaceView.getHolder().lockCanvas();
            if (canvas == null) {
                return;
            }
            surfaceWidth = Math.max(1, canvas.getWidth());
            surfaceHeight = Math.max(1, canvas.getHeight());
            updateContentRect();
            canvas.drawColor(android.graphics.Color.BLACK);
            canvas.drawBitmap(frameBitmap, null, contentRect, null);
            lastFrameGeneration = gen;
        } finally {
            if (canvas != null) {
                surfaceView.getHolder().unlockCanvasAndPost(canvas);
            }
        }
    }

    // ---- Touch ----

    @Override
    public boolean onTouch(View v, MotionEvent e) {
        if (handle == 0 || e == null) {
            return false;
        }

        int action = e.getActionMasked();
        int phase;
        switch (action) {
            case MotionEvent.ACTION_DOWN:
                phase = 0;
                break;
            case MotionEvent.ACTION_MOVE:
                phase = 1;
                break;
            case MotionEvent.ACTION_UP:
                phase = 2;
                break;
            case MotionEvent.ACTION_CANCEL:
                phase = 3;
                break;
            default:
                return false;
        }

        // The frame is letterboxed inside the surface, so report the touch position
        // in the content rect the engine was told about.
        int contentWidth = Math.max(1, contentRect.width());
        int contentHeight = Math.max(1, contentRect.height());
        double x = Math.min(contentWidth - 1, Math.max(0, e.getX() - contentRect.left));
        double y = Math.min(contentHeight - 1, Math.max(0, e.getY() - contentRect.top));
        NativeSena.touch(handle, phase, x, y);
        return true;
    }


    private void showNativeMessagebox(long engineHandle, long requestId, int kind, String title, String message) {
        if (isFinishing() || isDestroyed()) {
            NativeSena.submitMessageboxResult(engineHandle, requestId, fallbackMessageboxValue(kind));
            return;
        }

        AlertDialog.Builder builder = new AlertDialog.Builder(this);
        builder.setTitle((title == null || title.isEmpty()) ? "Sena" : title);
        builder.setMessage(message == null ? "" : message);
        builder.setCancelable(true);

        switch (kind) {
            case 0: // OK
                builder.setPositiveButton("OK", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                builder.setOnCancelListener(dialog -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                break;
            case 1: // OK/CANCEL
                builder.setPositiveButton("OK", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                builder.setNegativeButton("Cancel", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 1));
                builder.setOnCancelListener(dialog -> NativeSena.submitMessageboxResult(engineHandle, requestId, 1));
                break;
            case 2: // YES/NO
                builder.setPositiveButton("Yes", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                builder.setNegativeButton("No", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 1));
                builder.setOnCancelListener(dialog -> NativeSena.submitMessageboxResult(engineHandle, requestId, 1));
                break;
            case 3: // YES/NO/CANCEL
                builder.setPositiveButton("Yes", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                builder.setNegativeButton("No", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 1));
                builder.setNeutralButton("Cancel", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 2));
                builder.setOnCancelListener(dialog -> NativeSena.submitMessageboxResult(engineHandle, requestId, 2));
                break;
            default:
                builder.setPositiveButton("OK", (dialog, which) -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                builder.setOnCancelListener(dialog -> NativeSena.submitMessageboxResult(engineHandle, requestId, 0));
                break;
        }

        AlertDialog dialog = builder.create();
        dialog.setOnDismissListener(null);
        dialog.show();
    }

    // ---- Launch contract ----

    private static final class LaunchParams {
        final String gameRoot;
        final String nls;
        final String lang;

        LaunchParams(String gameRoot, String nls, String lang) {
            this.gameRoot = gameRoot;
            this.nls = NlsOption.fromValue(nls).value;
            this.lang = LangOption.fromValue(lang).value;
        }
    }

    @Nullable
    private LaunchParams readLaunchParams() {
        try {
            File base = new File(getFilesDir(), "SenaLauncher");
            File f = new File(base, "launch.json");
            if (!f.isFile()) {
                return null;
            }
            byte[] data;
            try (FileInputStream in = new FileInputStream(f);
                 ByteArrayOutputStream out = new ByteArrayOutputStream()) {
                byte[] buf = new byte[8192];
                int n;
                while ((n = in.read(buf)) >= 0) {
                    if (n > 0) {
                        out.write(buf, 0, n);
                    }
                }
                data = out.toByteArray();
            }
            String s = new String(data, StandardCharsets.UTF_8);
            JSONObject o = new JSONObject(s);
            String root = o.optString("game_root_utf8", "");
            if (root == null || root.isEmpty()) {
                return null;
            }
            String nls = o.optString("nls_utf8", NlsOption.SHIFT_JIS.value);
            String lang = o.optString("lang_utf8", LangOption.DEFAULT.value);
            return new LaunchParams(root, nls, lang);
        } catch (Throwable t) {
            return null;
        }
    }
}
