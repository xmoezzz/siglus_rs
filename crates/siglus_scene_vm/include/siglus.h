#ifndef SIGLUS_H
#define SIGLUS_H

#include <stdint.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*siglus_native_messagebox_callback_t)(
    void *user_data,
    uint64_t request_id,
    int32_t kind,
    const char *title_utf8,
    const char *message_utf8);

void siglus_string_free(char *ptr);
char *siglus_game_name_from_dir(const char *game_root_utf8);
char *siglus_game_cover_path_from_dir(const char *game_root_utf8);
char *siglus_game_cover_mime_from_dir(const char *game_root_utf8);

#if defined(__APPLE__) && TARGET_OS_IPHONE
void *siglus_ios_create(
    void *ui_view,
    uint32_t surface_width,
    uint32_t surface_height,
    double native_scale_factor,
    const char *game_root_utf8,
    const char *nls_utf8);
void siglus_ios_set_native_messagebox_callback(
    void *handle,
    siglus_native_messagebox_callback_t callback,
    void *user_data);
void siglus_ios_submit_messagebox_result(void *handle, uint64_t request_id, int64_t value);
int32_t siglus_ios_step(void *handle, uint32_t dt_ms);
void siglus_ios_resize(void *handle, uint32_t surface_width, uint32_t surface_height);
void siglus_ios_touch(void *handle, int32_t phase, double x_points, double y_points);
void siglus_ios_text_input(void *handle, const char *text_utf8);
void siglus_ios_key_down(void *handle, int32_t key_code);
void siglus_ios_key_up(void *handle, int32_t key_code);
void siglus_ios_destroy(void *handle);
#endif

#if defined(__ANDROID__)
void siglus_android_init_context(void *java_vm_ptr, void *context_ptr);
void *siglus_android_create(
    void *native_window_ptr,
    uint32_t surface_width_px,
    uint32_t surface_height_px,
    double native_scale_factor,
    const char *game_dir_utf8,
    const char *nls_utf8);
void siglus_android_set_native_messagebox_callback(
    void *handle,
    siglus_native_messagebox_callback_t callback,
    void *user_data);
void siglus_android_submit_messagebox_result(void *handle, uint64_t request_id, int64_t value);
int32_t siglus_android_step(void *handle, uint32_t dt_ms);
void siglus_android_resize(void *handle, uint32_t surface_width_px, uint32_t surface_height_px);
void siglus_android_set_surface(
    void *handle,
    void *native_window_ptr,
    uint32_t surface_width_px,
    uint32_t surface_height_px);
void siglus_android_touch(void *handle, int32_t phase, double x_px, double y_px);
void siglus_android_text_input(void *handle, const char *text_utf8);
void siglus_android_key_down(void *handle, int32_t key_code);
void siglus_android_key_up(void *handle, int32_t key_code);
void siglus_android_destroy(void *handle);
#endif

#if defined(__APPLE__) && TARGET_OS_MAC && !TARGET_OS_IPHONE
typedef struct SiglusPumpHandle SiglusPumpHandle;
SiglusPumpHandle *siglus_pump_create(const char *game_root_utf8, const char *nls_utf8);
void siglus_pump_set_native_messagebox_callback(
    SiglusPumpHandle *handle,
    siglus_native_messagebox_callback_t callback,
    void *user_data);
void siglus_pump_submit_messagebox_result(SiglusPumpHandle *handle, uint64_t request_id, int64_t value);
void siglus_pump_text_input(SiglusPumpHandle *handle, const char *text_utf8);
void siglus_pump_key_down(SiglusPumpHandle *handle, int32_t key_code);
void siglus_pump_key_up(SiglusPumpHandle *handle, int32_t key_code);
int32_t siglus_pump_step(SiglusPumpHandle *handle, uint32_t timeout_ms);
void siglus_pump_destroy(SiglusPumpHandle *handle);
int32_t siglus_run_entry(const char *game_root_utf8, const char *nls_utf8);
#endif

#ifdef __cplusplus
}
#endif

#endif
