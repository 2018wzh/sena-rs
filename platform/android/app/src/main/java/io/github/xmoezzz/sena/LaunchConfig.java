package io.github.xmoezzz.sena;

import android.content.Context;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

/**
 * A tiny launch contract between Java wrapper and native code.
 *
 * Native side should read <filesDir>/SenaLauncher/launch.json at startup and call:
 *   sena_run_entry(game_root_utf8)
 */
public final class LaunchConfig {

    private LaunchConfig() {}

    public static void write(Context ctx, String gameRootUtf8, String nlsUtf8, String langUtf8) throws Exception {
        File base = new File(ctx.getFilesDir(), "SenaLauncher");
        //noinspection ResultOfMethodCallIgnored
        base.mkdirs();

        File f = new File(base, "launch.json");
        JSONObject o = new JSONObject();
        o.put("game_root_utf8", gameRootUtf8);
        o.put("nls_utf8", NlsOption.fromValue(nlsUtf8).value);
        o.put("lang_utf8", LangOption.fromValue(langUtf8).value);

        byte[] data = o.toString(2).getBytes(StandardCharsets.UTF_8);
        try (FileOutputStream out = new FileOutputStream(f, false)) {
            out.write(data);
        }
    }
}
