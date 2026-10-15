#ifndef SENA_H
#define SENA_H

#include <stdint.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*sena_native_messagebox_callback_t)(
    void *user_data,
    uint64_t request_id,
    int32_t kind,
    const char *title_utf8,
    const char *message_utf8);

void sena_free_c_string(char *ptr);
void sena_string_free(char *ptr);
char *sena_game_name_from_dir(const char *game_root_utf8);
char *sena_game_cover_path_from_dir(const char *game_root_utf8);
char *sena_game_cover_mime_from_dir(const char *game_root_utf8);

void *sena_host_create(
    const char *game_root_utf8,
    const char *nls_utf8,
    const char *lang_utf8,
    uint32_t width,
    uint32_t height);
int32_t sena_host_step(void *handle, uint32_t dt_ms);
void sena_host_resize(void *handle, uint32_t width, uint32_t height);
void sena_host_touch(void *handle, int32_t phase, double x, double y);
void sena_host_key(void *handle, const char *key_utf8, int32_t pressed);
const uint8_t *sena_host_frame_rgba(void *handle);
uint32_t sena_host_frame_width(void *handle);
uint32_t sena_host_frame_height(void *handle);
uint64_t sena_host_frame_generation(void *handle);
void sena_host_destroy(void *handle);

#if defined(__APPLE__) && TARGET_OS_IPHONE
void *sena_ios_create(
    void *ui_view,
    uint32_t surface_width,
    uint32_t surface_height,
    double native_scale_factor,
    const char *game_root_utf8,
    const char *nls_utf8,
    const char *lang_utf8);
void sena_ios_resize_viewport(
    void *handle,
    uint32_t surface_width,
    uint32_t surface_height,
    uint32_t viewport_x,
    uint32_t viewport_y,
    uint32_t viewport_width,
    uint32_t viewport_height);
void sena_ios_logical_size(void *handle, uint32_t *width_out, uint32_t *height_out);
void sena_ios_set_native_messagebox_callback(
    void *handle,
    sena_native_messagebox_callback_t callback,
    void *user_data);
void sena_ios_submit_messagebox_result(void *handle, uint64_t request_id, int64_t value);
int32_t sena_ios_step(void *handle, uint32_t dt_ms);
void sena_ios_resize(void *handle, uint32_t surface_width, uint32_t surface_height);
const uint8_t *sena_ios_frame_rgba(void *handle);
uint32_t sena_ios_frame_width(void *handle);
uint32_t sena_ios_frame_height(void *handle);
uint64_t sena_ios_frame_generation(void *handle);
void sena_ios_touch(void *handle, int32_t phase, double x_points, double y_points);
void sena_ios_destroy(void *handle);
#endif

#if defined(__ANDROID__)
void sena_android_init_context(void *java_vm_ptr, void *context_ptr);
void *sena_android_create(
    void *native_window_ptr,
    uint32_t surface_width_px,
    uint32_t surface_height_px,
    double native_scale_factor,
    const char *game_dir_utf8,
    const char *nls_utf8,
    const char *lang_utf8);
void sena_android_set_native_messagebox_callback(
    void *handle,
    sena_native_messagebox_callback_t callback,
    void *user_data);
void sena_android_submit_messagebox_result(void *handle, uint64_t request_id, int64_t value);
int32_t sena_android_step(void *handle, uint32_t dt_ms);
void sena_android_resize(void *handle, uint32_t surface_width_px, uint32_t surface_height_px);
void sena_android_set_surface(
    void *handle,
    void *native_window_ptr,
    uint32_t surface_width_px,
    uint32_t surface_height_px);
void sena_android_touch(void *handle, int32_t phase, double x_px, double y_px);
void sena_android_destroy(void *handle);
#endif

#if defined(__APPLE__) && TARGET_OS_MAC && !TARGET_OS_IPHONE
typedef struct SenaPumpHandle SenaPumpHandle;
SenaPumpHandle *sena_pump_create(const char *game_root_utf8);
SenaPumpHandle *sena_pump_create_nls(const char *game_root_utf8, const char *nls_utf8, const char *lang_utf8);
void sena_pump_set_native_messagebox_callback(
    SenaPumpHandle *handle,
    sena_native_messagebox_callback_t callback,
    void *user_data);
void sena_pump_submit_messagebox_result(SenaPumpHandle *handle, uint64_t request_id, int64_t value);
int32_t sena_pump_step(SenaPumpHandle *handle, uint32_t timeout_ms);
void sena_pump_destroy(SenaPumpHandle *handle);
int32_t sena_run_entry(const char *game_root_utf8);
int32_t sena_run_entry_nls(const char *game_root_utf8, const char *nls_utf8, const char *lang_utf8);
#endif

#ifdef __cplusplus
}
#endif

#endif
