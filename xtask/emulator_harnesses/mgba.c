// Runs a ROM in mGBA's Game Boy core (https://github.com/mgba-emu/mgba) with
// its software renderer, the model forced to DMG or CGB.
//
// mGBA only runs boot ROMs whose checksum matches a known dump, so it refuses
// SameBoy's free ones and starts the game in its own post-boot state. Its core
// makes 131,072 samples a second; like mGBA's frontends, the harness turns
// them into 44,100 with mGBA's resampler, inside the timed function, using the
// cheap cosine interpolator rather than the frontends' default windowed sinc,
// which takes more time than the emulation itself.
#include "harness.h"
#include <mgba-util/audio-buffer.h>
#include <mgba-util/audio-resampler.h>
#include <mgba-util/vfs.h>
#include <mgba/core/config.h>
#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/core/timing.h>
#include <stdarg.h>

// The bits of the GB core's key mask (enum GBKey).
enum { MGBA_A = 1 << 0, MGBA_B = 1 << 1, MGBA_START = 1 << 3, MGBA_RIGHT = 1 << 4 };

static mColor pixels[256 * 224];
static int16_t samples[HARNESS_SAMPLES_PER_BUFFER * 2];
static unsigned long frames;

static void on_frame_ended(void *context)
{
    (void)context;
    frames++;
}

// Prints warnings and errors only, like the SDL frontend's default, so game
// errors such as writes to unusable memory aren't formatted in the timed loop.
static void on_log(struct mLogger *logger, int category, enum mLogLevel level, const char *format, va_list args)
{
    (void)logger;
    (void)level;
    fprintf(stderr, "%s: ", mLogCategoryName(category));
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
}

// Runs the core until the resampler has 512 samples at 44,100 Hz and reads
// them out, as a frontend playing the sound would.
__attribute__((noinline)) static void run_buffer(struct mCore *core, struct mAudioResampler *resampler,
                                                 struct mAudioBuffer *output)
{
    struct mAudioBuffer *source = core->getAudioBuffer(core);
    double source_step = resampler->sourceRate / resampler->destRate;
    size_t ready = mAudioBufferAvailable(output);
    while (ready < HARNESS_SAMPLES_PER_BUFFER) {
        // The resampler makes an output sample while its timestamp plus its
        // high water mark is below the source samples available.
        double needed = resampler->timestamp + (HARNESS_SAMPLES_PER_BUFFER - ready - 1) * source_step +
                        resampler->highWaterMark;
        while ((double)mAudioBufferAvailable(source) <= needed) {
            core->runLoop(core);
        }
        mAudioResamplerProcess(resampler);
        ready = mAudioBufferAvailable(output);
    }
    mAudioBufferRead(output, samples, HARNESS_SAMPLES_PER_BUFFER);
}

int main(int argc, char **argv)
{
    struct harness_args args;
    if (!harness_parse_args(argc, argv, &args)) return 1;

    static struct mLogFilter log_filter;
    mLogFilterInit(&log_filter);
    log_filter.defaultLevels = mLOG_WARN | mLOG_ERROR | mLOG_FATAL;
    static struct mLogger logger = {.log = on_log, .filter = &log_filter};
    mLogSetDefaultLogger(&logger);

    struct mCore *core = mCoreFind(args.rom);
    if (!core) {
        fprintf(stderr, "mGBA has no core for %s\n", args.rom);
        return 1;
    }
    mCoreInitConfig(core, NULL);
    if (!core->init(core)) return 1;
    core->setVideoBuffer(core, pixels, 256);

    static const char *const model_keys[] = {"gb.model", "sgb.model", "cgb.model", "cgb.hybridModel", "cgb.sgbModel"};
    for (size_t i = 0; i < sizeof model_keys / sizeof model_keys[0]; i++) {
        mCoreConfigSetDefaultValue(&core->config, model_keys[i], args.cgb ? "CGB" : "DMG");
    }
    struct mCoreOptions options = {.useBios = false, .volume = 0x100};
    mCoreConfigLoadDefaults(&core->config, &options);
    mCoreLoadForeignConfig(core, &core->config);

    struct VFile *rom = VFileOpen(args.rom, O_RDONLY);
    if (!rom || !core->loadROM(core, rom)) {
        fprintf(stderr, "could not load %s\n", args.rom);
        return 1;
    }
    struct mCoreCallbacks callbacks = {.videoFrameEnded = on_frame_ended};
    core->addCoreCallbacks(core, &callbacks);
    core->reset(core);

    struct mAudioBuffer output;
    mAudioBufferInit(&output, HARNESS_SAMPLES_PER_BUFFER, 2);
    struct mAudioResampler resampler;
    mAudioResamplerInit(&resampler, mINTERPOLATOR_COSINE);
    mAudioResamplerSetDestination(&resampler, &output, HARNESS_SAMPLE_RATE);
    mAudioResamplerSetSource(&resampler, core->getAudioBuffer(core), core->audioSampleRate(core), true);

    unsigned long buffers = harness_buffer_count(&args);
    uint64_t start = mTimingGlobalTime(core->timing);
    unsigned long start_frames = frames;
    double spent = 0;
    for (unsigned long buffer = 0; buffer < buffers; buffer++) {
        unsigned buttons = harness_buttons(buffer * harness_buffer_seconds());
        uint32_t keys = 0;
        if (buttons & HARNESS_A) keys |= MGBA_A;
        if (buttons & HARNESS_B) keys |= MGBA_B;
        if (buttons & HARNESS_START) keys |= MGBA_START;
        if (buttons & HARNESS_RIGHT) keys |= MGBA_RIGHT;
        core->setKeys(core, keys);
        double started = harness_now();
        run_buffer(core, &resampler, &output);
        spent += harness_now() - started;
    }
    double emulated = (mTimingGlobalTime(core->timing) - start) / (double)core->timingFrequency(core);
    harness_report(emulated, spent, frames - start_frames);

    static unsigned char rgb[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT * 3];
    for (unsigned y = 0; y < HARNESS_SCREEN_HEIGHT; y++) {
        for (unsigned x = 0; x < HARNESS_SCREEN_WIDTH; x++) {
            uint32_t pixel = pixels[y * 256 + x];
            unsigned char *out = rgb + (y * HARNESS_SCREEN_WIDTH + x) * 3;
            out[0] = pixel;
            out[1] = pixel >> 8;
            out[2] = pixel >> 16;
        }
    }
    harness_save_screenshot(&args, rgb);

    mAudioResamplerDeinit(&resampler);
    mAudioBufferDeinit(&output);
    mCoreConfigDeinit(&core->config);
    core->deinit(core);
    return 0;
}
