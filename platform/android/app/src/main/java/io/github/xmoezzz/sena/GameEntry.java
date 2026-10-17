package io.github.xmoezzz.sena;

import androidx.annotation.Nullable;

public final class GameEntry {
    public final String id;
    public final String title;
    public final String rootPath;
    public final long addedAtEpochMs;
    @Nullable public final String coverPath;
    public final String nls;
    public final String lang;

    public GameEntry(String id, String title, String rootPath, long addedAtEpochMs, @Nullable String coverPath, String nls, String lang) {
        this.id = id;
        this.title = title;
        this.rootPath = rootPath;
        this.addedAtEpochMs = addedAtEpochMs;
        this.coverPath = coverPath;
        this.nls = NlsOption.fromValue(nls).value;
        this.lang = LangOption.fromValue(lang).value;
    }
}
