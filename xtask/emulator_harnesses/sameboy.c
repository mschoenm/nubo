// Runs a ROM in SameBoy (https://github.com/LIJI32/SameBoy) through its core
// library, with its own free boot ROMs, a DMG-B or CGB-E, its accurate
// high-pass filter, and turbo mode so it never waits for real time.
#include "harness.h"
#include <Core/gb.h>

static uint32_t pixels[256 * 224];
static GB_sample_t samples[HARNESS_SAMPLES_PER_BUFFER * 2];
static unsigned samples_made;
static unsigned long frames;

static void on_sample(GB_gameboy_t *gb, GB_sample_t *sample)
{
    (void)gb;
    samples[samples_made % (HARNESS_SAMPLES_PER_BUFFER * 2)] = *sample;
    samples_made++;
}

static void on_vblank(GB_gameboy_t *gb, GB_vblank_type_t type)
{
    (void)gb;
    (void)type;
    frames++;
}

static uint32_t encode_rgb(GB_gameboy_t *gb, uint8_t r, uint8_t g, uint8_t b)
{
    (void)gb;
    return (uint32_t)r << 16 | (uint32_t)g << 8 | b;
}

static void ignore_log(GB_gameboy_t *gb, const char *message, GB_log_attributes_t attributes)
{
    (void)gb;
    (void)message;
    (void)attributes;
}

// Runs SameBoy until the next 512 samples are ready and returns how many 8 MHz
// ticks that took.
__attribute__((noinline)) static uint64_t run_buffer(GB_gameboy_t *gb)
{
    uint64_t ticks = 0;
    while (samples_made < HARNESS_SAMPLES_PER_BUFFER) {
        ticks += GB_run(gb);
    }
    samples_made -= HARNESS_SAMPLES_PER_BUFFER;
    return ticks;
}

int main(int argc, char **argv)
{
    struct harness_args args;
    if (!harness_parse_args(argc, argv, &args)) return 1;

    GB_gameboy_t *gb = GB_init(GB_alloc(), args.cgb ? GB_MODEL_CGB_E : GB_MODEL_DMG_B);
    GB_set_log_callback(gb, ignore_log);
    if (GB_load_boot_rom(gb, harness_boot_rom(&args)) != 0) {
        fprintf(stderr, "could not load %s\n", harness_boot_rom(&args));
        return 1;
    }
    if (GB_load_rom(gb, args.rom) != 0) {
        fprintf(stderr, "could not load %s\n", args.rom);
        return 1;
    }
    GB_set_pixels_output(gb, pixels);
    GB_set_rgb_encode_callback(gb, encode_rgb);
    GB_set_vblank_callback(gb, on_vblank);
    GB_set_sample_rate(gb, HARNESS_SAMPLE_RATE);
    GB_apu_set_sample_callback(gb, on_sample);
    GB_set_highpass_filter_mode(gb, GB_HIGHPASS_ACCURATE);
    GB_set_rtc_mode(gb, GB_RTC_MODE_ACCURATE);
    GB_set_turbo_mode(gb, true, true);

    unsigned long buffers = harness_buffer_count(&args);
    uint64_t ticks = 0;
    double spent = 0;
    for (unsigned long buffer = 0; buffer < buffers; buffer++) {
        unsigned buttons = harness_buttons(buffer * harness_buffer_seconds());
        GB_set_key_state(gb, GB_KEY_A, buttons & HARNESS_A);
        GB_set_key_state(gb, GB_KEY_B, buttons & HARNESS_B);
        GB_set_key_state(gb, GB_KEY_START, buttons & HARNESS_START);
        GB_set_key_state(gb, GB_KEY_RIGHT, buttons & HARNESS_RIGHT);
        double started = harness_now();
        ticks += run_buffer(gb);
        spent += harness_now() - started;
    }
    harness_report(ticks / 8388608.0, spent, frames);

    static unsigned char rgb[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT * 3];
    for (unsigned i = 0; i < HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT; i++) {
        rgb[i * 3] = pixels[i] >> 16;
        rgb[i * 3 + 1] = pixels[i] >> 8;
        rgb[i * 3 + 2] = pixels[i];
    }
    harness_save_screenshot(&args, rgb);
    GB_free(gb);
    GB_dealloc(gb);
    return 0;
}
