package com.chino.siglus;

import android.content.Context;
import android.view.Surface;

/**
 * JNI bridge to the Siglus Rust host-driven Android API.
 */
public final class NativeSiglus {
    static {
        System.loadLibrary("siglus");
        System.loadLibrary("siglus_jni");
    }

    private NativeSiglus() {}

    private static boolean sAndroidContextInited = false;

    public static synchronized void initAndroidContext(Context ctx) {
        if (sAndroidContextInited) {
            return;
        }
        if (ctx == null) {
            return;
        }
        nativeInitAndroidContext(ctx.getApplicationContext());
        sAndroidContextInited = true;
    }

    private static native void nativeInitAndroidContext(Context appContext);

    public static native String gameNameFromDir(String gameDirUtf8);
    public static native String gameCoverPathFromDir(String gameDirUtf8);
    public static native String gameCoverMimeFromDir(String gameDirUtf8);

    public static native long create(
            Surface surface,
            int widthPx,
            int heightPx,
            double nativeScaleFactor,
            String gameDirUtf8,
            String nlsUtf8
    );

    public static native void setNativeMessageboxCallback(long handle);
    public static native void submitMessageboxResult(long handle, long requestId, long value);

    public static native int step(long handle, int dtMs);
    public static native void resize(long handle, int widthPx, int heightPx);
    public static native void setSurface(long handle, Surface surface, int widthPx, int heightPx);
    public static native void touch(long handle, int phase, double xPx, double yPx);
    public static native void destroy(long handle);
}
