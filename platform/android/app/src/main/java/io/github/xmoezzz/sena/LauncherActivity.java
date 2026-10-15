package io.github.xmoezzz.sena;

import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.widget.Button;
import android.widget.ArrayAdapter;
import android.widget.Spinner;
import android.widget.Toast;

import androidx.activity.result.ActivityResultLauncher;
import androidx.activity.result.contract.ActivityResultContracts;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.appcompat.app.AlertDialog;
import androidx.recyclerview.widget.GridLayoutManager;
import androidx.recyclerview.widget.RecyclerView;

import java.io.File;
import java.util.List;

/**
 * Minimal launcher UI:
 * - Shows imported games as a scrollable grid.
 * - Import: select a directory (SAF), then record a direct path in no-copy mode.
 * - Run: taps a tile -> writes launch.json -> starts GameActivity.
 */
public final class LauncherActivity extends AppCompatActivity implements GameAdapter.Listener {

    private GameLibrary library;
    private GameAdapter adapter;
    private Spinner nlsSpinner;
    private Spinner langSpinner;

    private final ActivityResultLauncher<Uri> openTreeLauncher =
            registerForActivityResult(new ActivityResultContracts.OpenDocumentTree(), this::onImportTreeSelected);

    @Override
    protected void onCreate(@Nullable Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        setContentView(R.layout.activity_launcher);

        library = new GameLibrary(this);

        RecyclerView rv = findViewById(R.id.game_grid);
        rv.setLayoutManager(new GridLayoutManager(this, computeColumnCount()));
        adapter = new GameAdapter(this);
        rv.setAdapter(adapter);

        Button importBtn = findViewById(R.id.btn_import);
        importBtn.setOnClickListener(v -> startImportFlow());

        nlsSpinner = findViewById(R.id.spinner_nls);
        ArrayAdapter<String> nlsAdapter = new ArrayAdapter<>(this, android.R.layout.simple_spinner_item, NlsOption.labels());
        nlsAdapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        nlsSpinner.setAdapter(nlsAdapter);
        nlsSpinner.setSelection(0);

        langSpinner = findViewById(R.id.spinner_lang);
        ArrayAdapter<String> langAdapter = new ArrayAdapter<>(this, android.R.layout.simple_spinner_item, LangOption.labels());
        langAdapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        langSpinner.setAdapter(langAdapter);
        langSpinner.setSelection(LangOption.indexOfValue(LangOption.DEFAULT.value));

        refresh();
    }

    private void refresh() {
        List<GameEntry> entries = library.load();
        adapter.setItems(entries);
    }

    private int computeColumnCount() {
        float density = getResources().getDisplayMetrics().density;
        int widthPx = getResources().getDisplayMetrics().widthPixels;
        int minTilePx = Math.max(1, (int) (170f * density));
        return Math.max(1, Math.min(4, widthPx / minTilePx));
    }

    private void startImportFlow() {
        if (!hasAllFilesAccess()) {
            showAllFilesAccessPrompt();
            return;
        }
        openTreeLauncher.launch(null);
    }

    private boolean hasAllFilesAccess() {
        // MANAGE_EXTERNAL_STORAGE applies on Android 11+.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        return true;
    }

    private void showAllFilesAccessPrompt() {
        new AlertDialog.Builder(this)
                .setTitle("Grant Storage Access")
                .setMessage("No-copy import requires 'All files access'. Please grant it, then import again.")
                .setPositiveButton("Open Settings", (d, w) -> {
                    try {
                        Intent i = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
                        i.setData(Uri.parse("package:" + getPackageName()));
                        startActivity(i);
                    } catch (Throwable t) {
                        // Fallback to generic all-files access settings.
                        Intent i = new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION);
                        startActivity(i);
                    }
                })
                .setNegativeButton("Cancel", null)
                .show();
    }

    private void onImportTreeSelected(@Nullable Uri treeUri) {
        if (treeUri == null) {
            return;
        }

        // Persist permission so we can re-import / retry if needed.
        final int flags = Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
        try {
            getContentResolver().takePersistableUriPermission(treeUri, flags);
        } catch (Throwable ignored) {
            // Some providers may not support persistable perms; direct-path import may still work.
        }

        Toast.makeText(this, "Importing...", Toast.LENGTH_SHORT).show();

        new Thread(() -> {
            try {
                GameLibrary.ImportedGameDraft draft = library.importFromTreeUri(treeUri, selectedNlsValue(), selectedLangValue());
                runOnUiThread(() -> addImportedGame(draft));
            } catch (Throwable t) {
                library.cleanupPartialImport();
                runOnUiThread(() -> Toast.makeText(this, "Import failed: " + t.getMessage(), Toast.LENGTH_LONG).show());
            }
        }, "sena-import").start();
    }

    private void addImportedGame(GameLibrary.ImportedGameDraft draft) {
        if (draft == null) return;
        try {
            GameEntry e = library.addImportedGame(draft);
            Toast.makeText(this, "Imported: " + e.title, Toast.LENGTH_SHORT).show();
            refresh();
        } catch (Throwable t) {
            library.cleanupPartialImport();
            Toast.makeText(this, "Import failed: " + t.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    @Override
    public void onGameClicked(GameEntry e) {
        try {
            // Write launch config that native code can read on startup.
            LaunchConfig.write(this, e.rootPath, e.nls, e.lang);

            Intent it = new Intent(this, SenaGameActivity.class);
            startActivity(it);
        } catch (Throwable t) {
            Toast.makeText(this, "Failed to start: " + t.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    @Override
    public void onGameLongPressed(GameEntry e) {
        if (e == null) return;

        final LangOption[] langs = LangOption.values();
        String[] items = new String[3 + langs.length + 1];
        items[0] = "Use ShiftJIS";
        items[1] = "Use GBK";
        items[2] = "Use UTF-8";
        for (int i = 0; i < langs.length; i++) {
            items[3 + i] = "Language: " + langs[i].label;
        }
        items[3 + langs.length] = "Remove";

        new AlertDialog.Builder(this)
                .setTitle(e.title)
                .setItems(items, (d, which) -> {
                    try {
                        if (which >= 0 && which < 3) {
                            library.updateNls(e, NlsOption.values()[which].value);
                            refresh();
                        } else if (which >= 3 && which < 3 + langs.length) {
                            library.updateLang(e, langs[which - 3].value);
                            refresh();
                        } else {
                            Toast.makeText(this, "Use the launcher list controls to remove games.", Toast.LENGTH_SHORT).show();
                        }
                    } catch (Throwable t) {
                        Toast.makeText(this, "Update failed: " + t.getMessage(), Toast.LENGTH_LONG).show();
                    }
                })
                .setNegativeButton("Cancel", null)
                .show();
    }

    private String selectedNlsValue() {
        int pos = nlsSpinner != null ? nlsSpinner.getSelectedItemPosition() : 0;
        NlsOption[] options = NlsOption.values();
        if (pos < 0 || pos >= options.length) pos = 0;
        return options[pos].value;
    }

    private String selectedLangValue() {
        int pos = langSpinner != null ? langSpinner.getSelectedItemPosition() : 0;
        LangOption[] options = LangOption.values();
        if (pos < 0 || pos >= options.length) pos = 0;
        return options[pos].value;
    }
}
