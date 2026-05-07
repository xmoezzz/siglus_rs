package com.chino.siglus;

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
        String name = safeTrim(NativeSiglus.gameNameFromDir(rootPath));
        if (name == null || name.isEmpty()) {
            name = fallbackName(root);
        }
        String coverPath = safeTrim(NativeSiglus.gameCoverPathFromDir(rootPath));
        String coverMime = safeTrim(NativeSiglus.gameCoverMimeFromDir(rootPath));
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
        return "Siglus";
    }
}
