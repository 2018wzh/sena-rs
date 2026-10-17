#include <jni.h>
#include <android/native_window_jni.h>
#include <android/log.h>
#include <dlfcn.h>

#include <cstdint>
#include <climits>
#include <mutex>
#include <string>
#include <unordered_map>

#define LOG_TAG "sena_jni"
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGW(...) __android_log_print(ANDROID_LOG_WARN, LOG_TAG, __VA_ARGS__)
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)

using messagebox_callback_t = void (*)(void* user_data,
                                       uint64_t request_id,
                                       int32_t kind,
                                       const char* title_utf8,
                                       const char* message_utf8);

using create_fn_t = void* (*)(void* native_window_ptr,
                              uint32_t w_px,
                              uint32_t h_px,
                              double scale,
                              const char* game_dir_utf8,
                              const char* nls_utf8,
                              const char* lang_utf8);
using set_messagebox_callback_fn_t = void (*)(void* handle, messagebox_callback_t callback, void* user_data);
using submit_messagebox_result_fn_t = void (*)(void* handle, uint64_t request_id, int64_t value);
using step_fn_t = int32_t (*)(void* handle, uint32_t dt_ms);
using resize_fn_t = void (*)(void* handle, uint32_t w_px, uint32_t h_px);
using set_surface_fn_t = void (*)(void* handle, void* native_window_ptr, uint32_t w_px, uint32_t h_px);
using touch_fn_t = void (*)(void* handle, int32_t phase, double x_px, double y_px);
using destroy_fn_t = void (*)(void* handle);
using frame_rgba_fn_t = const uint8_t* (*)(void* handle);
using frame_size_fn_t = uint32_t (*)(void* handle);
using frame_generation_fn_t = uint64_t (*)(void* handle);
using init_context_fn_t = void (*)(void* java_vm_ptr, void* app_context_global_ref);
using string_free_fn_t = void (*)(char* ptr);
using string_from_dir_fn_t = char* (*)(const char* game_dir_utf8);

struct Api {
    create_fn_t create = nullptr;
    set_messagebox_callback_fn_t set_messagebox_callback = nullptr;
    submit_messagebox_result_fn_t submit_messagebox_result = nullptr;
    step_fn_t step = nullptr;
    resize_fn_t resize = nullptr;
    set_surface_fn_t set_surface = nullptr;
    touch_fn_t touch = nullptr;
    destroy_fn_t destroy = nullptr;
    frame_rgba_fn_t frame_rgba = nullptr;
    frame_size_fn_t frame_width = nullptr;
    frame_size_fn_t frame_height = nullptr;
    frame_generation_fn_t frame_generation = nullptr;
    init_context_fn_t init_context = nullptr;
    string_free_fn_t string_free = nullptr;
    string_from_dir_fn_t game_name_from_dir = nullptr;
    string_from_dir_fn_t game_cover_path_from_dir = nullptr;
    string_from_dir_fn_t game_cover_mime_from_dir = nullptr;
};

static Api g_api;
static std::once_flag g_api_once;
static void* g_lib_handle = nullptr;

static std::once_flag g_ctx_once;
static JavaVM* g_java_vm = nullptr;
static jobject g_app_ctx = nullptr;

static std::mutex g_win_mu;
static std::unordered_map<jlong, ANativeWindow*> g_windows;

static jclass g_activity_class = nullptr;
static jmethodID g_activity_on_messagebox = nullptr;

static void* load_symbol(const char* sym) {
    void* p = dlsym(g_lib_handle, sym);
    if (!p) {
        LOGE("dlsym failed: %s (%s)", sym, dlerror());
    }
    return p;
}

static void load_api_or_log() {
    std::call_once(g_api_once, []() {
        g_lib_handle = dlopen("libsena.so", RTLD_NOW);
        if (!g_lib_handle) {
            LOGE("dlopen libsena.so failed: %s", dlerror());
            return;
        }

        g_api.create = reinterpret_cast<create_fn_t>(load_symbol("sena_android_create"));
        g_api.set_messagebox_callback = reinterpret_cast<set_messagebox_callback_fn_t>(load_symbol("sena_android_set_native_messagebox_callback"));
        g_api.submit_messagebox_result = reinterpret_cast<submit_messagebox_result_fn_t>(load_symbol("sena_android_submit_messagebox_result"));
        g_api.step = reinterpret_cast<step_fn_t>(load_symbol("sena_android_step"));
        g_api.resize = reinterpret_cast<resize_fn_t>(load_symbol("sena_android_resize"));
        g_api.set_surface = reinterpret_cast<set_surface_fn_t>(load_symbol("sena_android_set_surface"));
        g_api.touch = reinterpret_cast<touch_fn_t>(load_symbol("sena_android_touch"));
        g_api.destroy = reinterpret_cast<destroy_fn_t>(load_symbol("sena_android_destroy"));
        g_api.frame_rgba = reinterpret_cast<frame_rgba_fn_t>(load_symbol("sena_android_frame_rgba"));
        g_api.frame_width = reinterpret_cast<frame_size_fn_t>(load_symbol("sena_android_frame_width"));
        g_api.frame_height = reinterpret_cast<frame_size_fn_t>(load_symbol("sena_android_frame_height"));
        g_api.frame_generation = reinterpret_cast<frame_generation_fn_t>(load_symbol("sena_android_frame_generation"));
        g_api.init_context = reinterpret_cast<init_context_fn_t>(load_symbol("sena_android_init_context"));
        g_api.string_free = reinterpret_cast<string_free_fn_t>(load_symbol("sena_string_free"));
        g_api.game_name_from_dir = reinterpret_cast<string_from_dir_fn_t>(load_symbol("sena_game_name_from_dir"));
        g_api.game_cover_path_from_dir = reinterpret_cast<string_from_dir_fn_t>(load_symbol("sena_game_cover_path_from_dir"));
        g_api.game_cover_mime_from_dir = reinterpret_cast<string_from_dir_fn_t>(load_symbol("sena_game_cover_mime_from_dir"));

        if (g_api.create && g_api.step && g_api.resize && g_api.set_surface && g_api.touch && g_api.destroy) {
            LOGI("sena_android_* symbols resolved");
        } else {
            LOGE("missing one or more required sena_android_* symbols");
        }
        if (!g_api.init_context) {
            LOGW("sena_android_init_context is missing; Android audio backends may fail");
        }
        if (!g_api.set_messagebox_callback || !g_api.submit_messagebox_result) {
            LOGW("native messagebox callback symbols are missing");
        }
    });
}

static JNIEnv* attach_current_thread(bool* did_attach) {
    if (did_attach) {
        *did_attach = false;
    }
    if (!g_java_vm) {
        return nullptr;
    }
    JNIEnv* env = nullptr;
    jint status = g_java_vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6);
    if (status == JNI_OK) {
        return env;
    }
    if (status == JNI_EDETACHED) {
        if (g_java_vm->AttachCurrentThread(&env, nullptr) == JNI_OK) {
            if (did_attach) {
                *did_attach = true;
            }
            return env;
        }
    }
    return nullptr;
}

static void detach_current_thread_if_needed(bool did_attach) {
    if (did_attach && g_java_vm) {
        g_java_vm->DetachCurrentThread();
    }
}

static void release_window_locked(jlong handle_key) {
    auto it = g_windows.find(handle_key);
    if (it != g_windows.end()) {
        if (it->second) {
            ANativeWindow_release(it->second);
        }
        g_windows.erase(it);
    }
}

static const char* get_utf8_or_null(JNIEnv* env, jstring s) {
    if (!s) return nullptr;
    return env->GetStringUTFChars(s, nullptr);
}

static void release_utf8(JNIEnv* env, jstring s, const char* p) {
    if (s && p) {
        env->ReleaseStringUTFChars(s, p);
    }
}

static jstring take_sena_string(JNIEnv* env, char* p) {
    if (!p) {
        return nullptr;
    }
    jstring out = env->NewStringUTF(p);
    if (g_api.string_free) {
        g_api.string_free(p);
    }
    return out;
}

static jstring string_from_dir(JNIEnv* env, jstring game_dir_utf8, string_from_dir_fn_t fn) {
    if (!fn) {
        return nullptr;
    }
    const char* game_dir = get_utf8_or_null(env, game_dir_utf8);
    char* p = fn(game_dir);
    release_utf8(env, game_dir_utf8, game_dir);
    return take_sena_string(env, p);
}

static int64_t fallback_messagebox_value(int32_t kind) {
    switch (kind) {
        case 0: return 0; // OK
        case 1: return 1; // OK/CANCEL -> CANCEL
        case 2: return 1; // YES/NO -> NO
        case 3: return 2; // YES/NO/CANCEL -> CANCEL
        default: return 0;
    }
}

static void sena_messagebox_callback(void* user_data,
                                       uint64_t request_id,
                                       int32_t kind,
                                       const char* title_utf8,
                                       const char* message_utf8) {
    auto handle_key = reinterpret_cast<jlong>(user_data);
    bool did_attach = false;
    JNIEnv* env = attach_current_thread(&did_attach);
    if (!env || !g_activity_class || !g_activity_on_messagebox) {
        if (g_api.submit_messagebox_result && handle_key != 0) {
            g_api.submit_messagebox_result(reinterpret_cast<void*>(handle_key), request_id, fallback_messagebox_value(kind));
        }
        detach_current_thread_if_needed(did_attach);
        return;
    }

    jstring title = env->NewStringUTF(title_utf8 ? title_utf8 : "Sena");
    jstring message = env->NewStringUTF(message_utf8 ? message_utf8 : "");
    env->CallStaticVoidMethod(g_activity_class,
                              g_activity_on_messagebox,
                              handle_key,
                              static_cast<jlong>(request_id),
                              static_cast<jint>(kind),
                              title,
                              message);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        if (g_api.submit_messagebox_result && handle_key != 0) {
            g_api.submit_messagebox_result(reinterpret_cast<void*>(handle_key), request_id, fallback_messagebox_value(kind));
        }
    }
    if (title) env->DeleteLocalRef(title);
    if (message) env->DeleteLocalRef(message);
    detach_current_thread_if_needed(did_attach);
}

extern "C" JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void*) {
    g_java_vm = vm;
    JNIEnv* env = nullptr;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK || !env) {
        return JNI_ERR;
    }
    jclass local = env->FindClass("io/github/xmoezzz/sena/SenaGameActivity");
    if (local) {
        g_activity_class = reinterpret_cast<jclass>(env->NewGlobalRef(local));
        env->DeleteLocalRef(local);
        if (g_activity_class) {
            g_activity_on_messagebox = env->GetStaticMethodID(
                    g_activity_class,
                    "onNativeMessagebox",
                    "(JJILjava/lang/String;Ljava/lang/String;)V");
        }
    }
    if (!g_activity_class || !g_activity_on_messagebox) {
        LOGW("failed to resolve SenaGameActivity.onNativeMessagebox");
    }
    return JNI_VERSION_1_6;
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_nativeInitAndroidContext(JNIEnv* env, jclass, jobject app_context) {
    load_api_or_log();
    if (!g_api.init_context) {
        LOGE("nativeInitAndroidContext: sena_android_init_context is null");
        return;
    }
    if (!app_context) {
        LOGE("nativeInitAndroidContext: app_context is null");
        return;
    }

    std::call_once(g_ctx_once, [env, app_context]() {
        JavaVM* vm = nullptr;
        if (env->GetJavaVM(&vm) != JNI_OK || !vm) {
            LOGE("nativeInitAndroidContext: GetJavaVM failed");
            return;
        }
        jobject gref = env->NewGlobalRef(app_context);
        if (!gref) {
            LOGE("nativeInitAndroidContext: NewGlobalRef failed");
            return;
        }
        g_java_vm = vm;
        g_app_ctx = gref;
        g_api.init_context(reinterpret_cast<void*>(vm), reinterpret_cast<void*>(gref));
        LOGI("nativeInitAndroidContext: ndk-context initialized");
    });
}

extern "C" JNIEXPORT jstring JNICALL
Java_io_github_xmoezzz_sena_NativeSena_gameNameFromDir(JNIEnv* env, jclass, jstring game_dir_utf8) {
    load_api_or_log();
    return string_from_dir(env, game_dir_utf8, g_api.game_name_from_dir);
}

extern "C" JNIEXPORT jstring JNICALL
Java_io_github_xmoezzz_sena_NativeSena_gameCoverPathFromDir(JNIEnv* env, jclass, jstring game_dir_utf8) {
    load_api_or_log();
    return string_from_dir(env, game_dir_utf8, g_api.game_cover_path_from_dir);
}

extern "C" JNIEXPORT jstring JNICALL
Java_io_github_xmoezzz_sena_NativeSena_gameCoverMimeFromDir(JNIEnv* env, jclass, jstring game_dir_utf8) {
    load_api_or_log();
    return string_from_dir(env, game_dir_utf8, g_api.game_cover_mime_from_dir);
}

extern "C" JNIEXPORT jlong JNICALL
Java_io_github_xmoezzz_sena_NativeSena_create(JNIEnv* env, jclass,
                                        jobject surface,
                                        jint width_px,
                                        jint height_px,
                                        jdouble scale,
                                        jstring game_dir_utf8,
                                        jstring nls_utf8,
                                        jstring lang_utf8) {
    load_api_or_log();
    if (!g_api.create) {
        return 0;
    }
    if (!surface) {
        LOGE("create: surface is null");
        return 0;
    }

    ANativeWindow* win = ANativeWindow_fromSurface(env, surface);
    if (!win) {
        LOGE("ANativeWindow_fromSurface returned null");
        return 0;
    }

    const char* game_dir = get_utf8_or_null(env, game_dir_utf8);
    const char* nls = get_utf8_or_null(env, nls_utf8);
    const char* lang = get_utf8_or_null(env, lang_utf8);

    void* handle = g_api.create(reinterpret_cast<void*>(win),
                                static_cast<uint32_t>(width_px),
                                static_cast<uint32_t>(height_px),
                                static_cast<double>(scale),
                                game_dir,
                                nls,
                                lang);

    release_utf8(env, game_dir_utf8, game_dir);
    release_utf8(env, nls_utf8, nls);
    release_utf8(env, lang_utf8, lang);

    if (!handle) {
        ANativeWindow_release(win);
        LOGE("sena_android_create returned null");
        return 0;
    }

    jlong key = reinterpret_cast<jlong>(handle);
    {
        std::lock_guard<std::mutex> lk(g_win_mu);
        release_window_locked(key);
        g_windows.emplace(key, win);
    }

    if (g_api.set_messagebox_callback) {
        g_api.set_messagebox_callback(handle, sena_messagebox_callback, reinterpret_cast<void*>(key));
    }

    return key;
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_setNativeMessageboxCallback(JNIEnv*, jclass, jlong handle) {
    load_api_or_log();
    if (g_api.set_messagebox_callback && handle != 0) {
        g_api.set_messagebox_callback(reinterpret_cast<void*>(handle), sena_messagebox_callback, reinterpret_cast<void*>(handle));
    }
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_submitMessageboxResult(JNIEnv*, jclass,
                                                        jlong handle,
                                                        jlong request_id,
                                                        jlong value) {
    load_api_or_log();
    if (g_api.submit_messagebox_result && handle != 0) {
        g_api.submit_messagebox_result(reinterpret_cast<void*>(handle), static_cast<uint64_t>(request_id), static_cast<int64_t>(value));
    }
}

extern "C" JNIEXPORT jint JNICALL
Java_io_github_xmoezzz_sena_NativeSena_step(JNIEnv*, jclass, jlong handle, jint dt_ms) {
    load_api_or_log();
    if (!g_api.step || handle == 0) {
        return 1;
    }
    return static_cast<jint>(g_api.step(reinterpret_cast<void*>(handle), static_cast<uint32_t>(dt_ms)));
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_io_github_xmoezzz_sena_NativeSena_frameRgba(JNIEnv* env, jclass, jlong handle) {
    load_api_or_log();
    if (!g_api.frame_rgba || !g_api.frame_width || !g_api.frame_height || handle == 0) {
        return nullptr;
    }
    uint32_t w = g_api.frame_width(reinterpret_cast<void*>(handle));
    uint32_t h = g_api.frame_height(reinterpret_cast<void*>(handle));
    const uint8_t* ptr = g_api.frame_rgba(reinterpret_cast<void*>(handle));
    if (!ptr || w == 0 || h == 0) {
        return nullptr;
    }
    uint64_t len64 = static_cast<uint64_t>(w) * static_cast<uint64_t>(h) * 4ull;
    if (len64 > static_cast<uint64_t>(INT32_MAX)) {
        LOGE("frameRgba too large: %ux%u", w, h);
        return nullptr;
    }
    jbyteArray out = env->NewByteArray(static_cast<jsize>(len64));
    if (!out) {
        return nullptr;
    }
    env->SetByteArrayRegion(out, 0, static_cast<jsize>(len64), reinterpret_cast<const jbyte*>(ptr));
    return out;
}

extern "C" JNIEXPORT jint JNICALL
Java_io_github_xmoezzz_sena_NativeSena_frameWidth(JNIEnv*, jclass, jlong handle) {
    load_api_or_log();
    if (!g_api.frame_width || handle == 0) {
        return 0;
    }
    return static_cast<jint>(g_api.frame_width(reinterpret_cast<void*>(handle)));
}

extern "C" JNIEXPORT jint JNICALL
Java_io_github_xmoezzz_sena_NativeSena_frameHeight(JNIEnv*, jclass, jlong handle) {
    load_api_or_log();
    if (!g_api.frame_height || handle == 0) {
        return 0;
    }
    return static_cast<jint>(g_api.frame_height(reinterpret_cast<void*>(handle)));
}

extern "C" JNIEXPORT jlong JNICALL
Java_io_github_xmoezzz_sena_NativeSena_frameGeneration(JNIEnv*, jclass, jlong handle) {
    load_api_or_log();
    if (!g_api.frame_generation || handle == 0) {
        return 0;
    }
    return static_cast<jlong>(g_api.frame_generation(reinterpret_cast<void*>(handle)));
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_resize(JNIEnv*, jclass, jlong handle, jint width_px, jint height_px) {
    load_api_or_log();
    if (!g_api.resize || handle == 0) {
        return;
    }
    g_api.resize(reinterpret_cast<void*>(handle), static_cast<uint32_t>(width_px), static_cast<uint32_t>(height_px));
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_setSurface(JNIEnv* env, jclass,
                                            jlong handle,
                                            jobject surface,
                                            jint width_px,
                                            jint height_px) {
    load_api_or_log();
    if (!g_api.set_surface || handle == 0) {
        return;
    }
    if (!surface) {
        LOGW("setSurface: surface is null (ignored)");
        return;
    }

    ANativeWindow* win = ANativeWindow_fromSurface(env, surface);
    if (!win) {
        LOGE("setSurface: ANativeWindow_fromSurface returned null");
        return;
    }

    g_api.set_surface(reinterpret_cast<void*>(handle), reinterpret_cast<void*>(win),
                      static_cast<uint32_t>(width_px), static_cast<uint32_t>(height_px));

    {
        std::lock_guard<std::mutex> lk(g_win_mu);
        release_window_locked(handle);
        g_windows.emplace(handle, win);
    }
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_touch(JNIEnv*, jclass, jlong handle, jint phase, jdouble x_px, jdouble y_px) {
    load_api_or_log();
    if (!g_api.touch || handle == 0) {
        return;
    }
    g_api.touch(reinterpret_cast<void*>(handle), static_cast<int32_t>(phase),
                static_cast<double>(x_px), static_cast<double>(y_px));
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_xmoezzz_sena_NativeSena_destroy(JNIEnv*, jclass, jlong handle) {
    load_api_or_log();
    if (!g_api.destroy || handle == 0) {
        return;
    }
    if (g_api.set_messagebox_callback) {
        g_api.set_messagebox_callback(reinterpret_cast<void*>(handle), nullptr, nullptr);
    }
    g_api.destroy(reinterpret_cast<void*>(handle));
    {
        std::lock_guard<std::mutex> lk(g_win_mu);
        release_window_locked(handle);
    }
}
