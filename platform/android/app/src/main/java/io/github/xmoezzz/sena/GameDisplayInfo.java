package io.github.xmoezzz.sena;

import androidx.annotation.Nullable;

import java.io.File;

public final class GameDisplayInfo {
    public final String name;
    @Nullable public final String coverPath;
    @Nullable public final String coverMime;

    private GameDisplayInfo(String name, @Nullable String coverPath, @Nullable String coverMime) {
        this.name = name;
        this.coverPath = coverPath;
        this.coverMime = coverMime;
    }

    public static GameDisplayInfo fromGameRoot(File root) {
        String rootPath = root != null ? root.getAbsolutePath() : "";
        String name = null;
        String coverPath = null;
        String coverMime = null;
        try {
            name = safeTrim(NativeSena.gameNameFromDir(rootPath));
            coverPath = safeTrim(NativeSena.gameCoverPathFromDir(rootPath));
            coverMime = safeTrim(NativeSena.gameCoverMimeFromDir(rootPath));
        } catch (Throwable t) {
            // The engine library may be absent for this ABI, in which case the
            // whole native bridge is unavailable. A tile still works without it:
            // it falls back to the folder name and a generated cover.
            name = null;
            coverPath = null;
            coverMime = null;
        }
        if (name == null || name.isEmpty()) {
            name = fallbackName(root);
        }
        if (coverPath == null || coverPath.isEmpty()) {
            coverPath = null;
            coverMime = null;
        }
        return new GameDisplayInfo(name, coverPath, coverMime);
    }

    private static String safeTrim(@Nullable String s) {
        if (s == null) return null;
        String t = s.trim();
        return t.isEmpty() ? null : t;
    }

    private static String fallbackName(@Nullable File root) {
        if (root != null) {
            String n = root.getName();
            if (n != null && !n.trim().isEmpty()) {
                return n.trim();
            }
        }
        return "Sena";
    }
}
