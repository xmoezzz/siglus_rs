#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <switch.h>
#include <switch/nvidia/address_space.h>
#include <switch/nvidia/map.h>

#include "gpu.h"

// libnx's default heap takes all the memory the process may use: the Rust
// VM, the decoded images and the movie frames, and deko3d's memory blocks,
// which it allocates from the same heap.

enum {
    // A 20 ms buffer and six queued buffers tolerate decode/GPU stalls of up
    // to roughly 120 ms without audren underflowing. The old 4 × 10 ms queue
    // was audible as periodic crackle whenever Ryujinx stalled a frame.
    AudioBufferFrames = 960,
    AudioBufferCount = 6,
    // audrv memory pools must begin and end on page boundaries. Each active
    // buffer still submits only AudioBufferFrames; this tail makes every slot
    // exactly one 4 KiB page, for a 24 KiB pool.
    AudioBufferStorageFrames = 1024,
};
static AudioDriver audio_driver;
static AudioDriverWaveBuf audio_wavebufs[AudioBufferCount];
static int16_t audio_buffers[AudioBufferCount][AudioBufferStorageFrames * 2]
    __attribute__((aligned(0x1000)));
static bool audio_renderer_initialized;
static bool audio_driver_initialized;

/* Rust owns the existing SiglusHost/SceneVm.  libnx owns the process and
 * presentation lifetime; no desktop event loop or WGPU object is involved. */
extern void* siglus_switch_engine_create(const char* project_dir, uint32_t width, uint32_t height);
extern bool siglus_switch_engine_step(void* host, uint32_t dt_ms);
extern void siglus_switch_engine_gamepad(void* host, uint8_t button, bool down);
extern void siglus_switch_engine_touch(void* host, int32_t phase, double x, double y);
extern void siglus_switch_engine_destroy(void* host);
extern void siglus_switch_audio_render_i16(int16_t* dst, size_t frames);

/* A packaged game is self-contained: prefer its RomFS payload.  Keeping the
 * SD-card location as a fallback also preserves the small-NRO deployment
 * workflow for developers and for games that are too large for one file. */
static const char* const RomfsGameRoot = "romfs:/game";
static const char* const SdmcGameRoot = "sdmc:/switch/siglus_rs/game";

/* Keep startup diagnostics usable even when Rust's stdio implementation is
 * unavailable on a particular Horizon loader/emulator. */
void siglus_switch_log_message(const char* message) {
    FILE* const file = fopen("sdmc:/switch/siglus_rs/siglus_switch.log", "ab");
    if (file == NULL) return;
    fputs(message, file);
    fclose(file);
}

static void log_startup_result(const char* operation, Result result) {
    char message[96];
    snprintf(message, sizeof(message), "siglus_switch: %s rc=0x%08x\n",
             operation, (unsigned int) result);
    fputs(message, stderr);
    siglus_switch_log_message(message);
}

/* Preserve the Horizon Result that deko3d otherwise compresses to
 * DkResult_Fail.  The wrappers are link-time only; all calls still go to the
 * standard libnx implementation. */
extern Result __real_nvMapCreate(NvMap* map, void* cpu_address, u32 size,
                                 u32 alignment, NvKind kind, bool cached);
extern Result __real_nvAddressSpaceMap(NvAddressSpace* address_space, u32 handle,
                                       bool cached, NvKind kind, iova_t* output);

Result __wrap_nvMapCreate(NvMap* map, void* cpu_address, u32 size, u32 alignment,
                          NvKind kind, bool cached) {
    Result rc = __real_nvMapCreate(map, cpu_address, size, alignment, kind, cached);
    /* Every GPU memory block maps; only failures are worth the SD write. */
    if (R_FAILED(rc)) log_startup_result("nvMapCreate", rc);
    return rc;
}

Result __wrap_nvAddressSpaceMap(NvAddressSpace* address_space, u32 handle,
                                bool cached, NvKind kind, iova_t* output) {
    Result rc = __real_nvAddressSpaceMap(address_space, handle, cached, kind, output);
    if (R_FAILED(rc)) log_startup_result("nvAddressSpaceMap", rc);
    return rc;
}

static const char* select_game_root(void) {
    FILE* const scene_package = fopen("romfs:/game/Scene.pck", "rb");
    if (scene_package != NULL) {
        fclose(scene_package);
        return RomfsGameRoot;
    }
    return SdmcGameRoot;
}

void siglus_switch_random_fill(void* buffer, size_t length) {
    siglus_switch_log_message("siglus_switch: random-fill begin\n");
    randomGet(buffer, length);
    siglus_switch_log_message("siglus_switch: random-fill complete\n");
}

/* Rust's newlib std build uses these Unix-shaped hooks.  Horizon has neither
 * Linux getrandom(2) nor POSIX sysconf(3), so map them to the equivalent
 * libnx primitives in the native shell. */
long getrandom(void* buffer, size_t length, unsigned int flags) {
    (void) flags;
    siglus_switch_random_fill(buffer, length);
    return (long) length;
}

long sysconf(int name) {
    (void) name;
    return 4096;
}

static void initialize_audio(void) {
    static const AudioRendererConfig config = {
        .output_rate = AudioRendererOutputRate_48kHz,
        // audrv allocates one driver channel per configured voice slot. The
        // Kira bridge submits one stereo voice, so it needs two channels even
        // though we only use voice ID 0.
        .num_voices = 2,
        .num_effects = 0,
        .num_sinks = 1,
        .num_mix_objs = 1,
        .num_mix_buffers = 2,
    };
    const Result audren_rc = audrenInitialize(&config);
    if (R_FAILED(audren_rc)) {
        log_startup_result("audrenInitialize", audren_rc);
        return;
    }
    audio_renderer_initialized = true;
    const Result audrv_rc = audrvCreate(&audio_driver, &config, 2);
    if (R_FAILED(audrv_rc)) {
        log_startup_result("audrvCreate", audrv_rc);
        return;
    }
    audio_driver_initialized = true;
    armDCacheFlush(audio_buffers, sizeof(audio_buffers));
    const int pool = audrvMemPoolAdd(&audio_driver, audio_buffers, sizeof(audio_buffers));
    if (pool < 0) {
        siglus_switch_log_message("siglus_switch: audrvMemPoolAdd failed\n");
        return;
    }
    if (!audrvMemPoolAttach(&audio_driver, pool)) {
        siglus_switch_log_message("siglus_switch: audrvMemPoolAttach failed\n");
        return;
    }
    static const u8 sink_channels[] = { 0, 1 };
    audrvDeviceSinkAdd(&audio_driver, AUDREN_DEFAULT_DEVICE_NAME, 2, sink_channels);
    // Commit the memory pool and output sink before creating a voice. libnx's
    // driver does not expose the attached pool to voice setup until this update.
    const Result initial_update_rc = audrvUpdate(&audio_driver);
    if (R_FAILED(initial_update_rc)) {
        log_startup_result("audrvUpdate(pool)", initial_update_rc);
        return;
    }
    const Result start_rc = audrenStartAudioRenderer();
    if (R_FAILED(start_rc)) {
        log_startup_result("audrenStartAudioRenderer", start_rc);
        return;
    }
    if (!audrvVoiceInit(&audio_driver, 0, 2, PcmFormat_Int16, 48000)) {
        siglus_switch_log_message("siglus_switch: audrvVoiceInit failed\n");
        return;
    }
    audrvVoiceSetDestinationMix(&audio_driver, 0, AUDREN_FINAL_MIX_ID);
    audrvVoiceSetMixFactor(&audio_driver, 0, 1.0f, 0, 0);
    audrvVoiceSetMixFactor(&audio_driver, 0, 1.0f, 1, 1);
    audrvVoiceStart(&audio_driver, 0);
    const Result voice_update_rc = audrvUpdate(&audio_driver);
    if (R_FAILED(voice_update_rc)) {
        log_startup_result("audrvUpdate(voice)", voice_update_rc);
        return;
    }
    siglus_switch_log_message("siglus_switch: audio-ready\n");
}

static void pump_audio(void) {
    if (!audio_driver_initialized) return;
    for (unsigned i = 0; i < AudioBufferCount; ++i) {
        AudioDriverWaveBuf* wavebuf = &audio_wavebufs[i];
        if (wavebuf->state != AudioDriverWaveBufState_Free &&
            wavebuf->state != AudioDriverWaveBufState_Done) continue;
        siglus_switch_audio_render_i16(audio_buffers[i], AudioBufferFrames);
        armDCacheFlush(audio_buffers[i], sizeof(audio_buffers[i]));
        wavebuf->data_raw = audio_buffers[i];
        wavebuf->size = (size_t) AudioBufferFrames * 2 * sizeof(int16_t);
        wavebuf->start_sample_offset = 0;
        wavebuf->end_sample_offset = AudioBufferFrames;
        wavebuf->is_looping = false;
        audrvVoiceAddWaveBuf(&audio_driver, 0, wavebuf);
    }
    audrvUpdate(&audio_driver);
}

static void exit_audio(void) {
    if (audio_driver_initialized) audrvClose(&audio_driver);
    if (audio_renderer_initialized) audrenExit();
}

int main(void) {
    /* libnx's default __appInit has already called fsInitialize() and
     * fsdevMountSdmc() before main. Mount the NRO payload only after that
     * startup path has completed, and never hide a mount failure. */
    Result rc = romfsMountSelf("romfs");
    log_startup_result("romfsMountSelf", rc);
    if (R_FAILED(rc)) {
        return 1;
    }

    siglus_gpu_init();
    initialize_audio();

    siglus_switch_log_message("siglus_switch: engine-create begin\n");
    void* engine = siglus_switch_engine_create(select_game_root(),
                                               siglus_gpu_display_width(),
                                               siglus_gpu_display_height());
    if (engine == NULL) {
        // A missing/corrupt GameData tree must not look like a hung black
        // screen. Rust has already reported the resource/startup error; leave
        // the applet cleanly so homebrew launchers can surface the failure.
        exit_audio();
        siglus_gpu_exit();
        romfsExit();
        return 1;
    }
    siglus_switch_log_message("siglus_switch: engine-create complete\n");

    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    PadState pad;
    padInitializeDefault(&pad);
    bool touch_active = false;
    double last_touch_x = 0.0;
    double last_touch_y = 0.0;
    bool first_frame = true;
    unsigned frame_count = 0;
    uint64_t period_start = armGetSystemTick();
    siglus_switch_log_message("siglus_switch: main-loop enter\n");
    while (appletMainLoop()) {
        padUpdate(&pad);
        const uint64_t buttons_down = padGetButtonsDown(&pad);
        const uint64_t buttons_up = padGetButtonsUp(&pad);
        if (engine != NULL) {
            for (uint8_t button = 0; button < 32; ++button) {
                const uint64_t bit = UINT64_C(1) << button;
                if (buttons_down & bit) siglus_switch_engine_gamepad(engine, button, true);
                if (buttons_up & bit) siglus_switch_engine_gamepad(engine, button, false);
            }
        }
        HidTouchScreenState touch_state;
        const size_t touch_state_count = hidGetTouchScreenStates(&touch_state, 1);
        const bool has_touch = touch_state_count != 0 && touch_state.count > 0;
        if (engine != NULL && has_touch) {
            const HidTouchState* touch = &touch_state.touches[0];
            last_touch_x = (double) touch->x;
            last_touch_y = (double) touch->y;
            siglus_switch_engine_touch(engine, touch_active ? 1 : 0,
                                       last_touch_x, last_touch_y);
        } else if (engine != NULL && touch_active) {
            // Button activation requires mouse-down and mouse-up to hit the
            // same object. Keep the final sampled touch position on release;
            // sending (0, 0) here made taps behave like drags off the button.
            siglus_switch_engine_touch(engine, 2, last_touch_x, last_touch_y);
        }
        touch_active = has_touch;
        if (first_frame) siglus_switch_log_message("siglus_switch: first-frame step begin\n");
        if (engine != NULL && siglus_switch_engine_step(engine, 16)) {
            break;
        }
        if (first_frame) siglus_switch_log_message("siglus_switch: first-frame step complete\n");
        /* The engine step rendered and presented the frame. */
        pump_audio();
        first_frame = false;
        if (++frame_count % 600 == 0) {
            const uint64_t now = armGetSystemTick();
            char message[96];
            snprintf(message, sizeof(message), "siglus_switch: frame %u, %.1f fps\n", frame_count,
                     600.0 * armGetSystemTickFreq() / (double) (now - period_start));
            siglus_switch_log_message(message);
            period_start = now;
        }
    }

    siglus_switch_engine_destroy(engine);

    exit_audio();
    siglus_gpu_exit();
    romfsExit();
    return 0;
}
