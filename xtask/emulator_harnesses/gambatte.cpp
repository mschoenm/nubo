// Runs a ROM in Gambatte (gambatte-core, https://github.com/pokemon-speedrunning/gambatte-core)
// with SameBoy's free boot ROMs. Gambatte makes 2,097,152 samples a second,
// which its own "Fast" resampler (the cheapest it has) turns into 44,100,
// inside the timed function like the sound the other emulators make. The ROM
// is loaded from memory, so Gambatte neither reads nor writes a save file next
// to it.
#include "harness.h"
#include <gambatte.h>
#include <resample/resampler.h>
#include <resample/resamplerinfo.h>
#include <vector>

static const long GAMBATTE_SAMPLE_RATE = 2097152;
static const size_t MOST_SAMPLES_PER_RUN = 35112 + 2064;

static gambatte::uint_least32_t pixels[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT];
static gambatte::uint_least32_t gambatte_samples[MOST_SAMPLES_PER_RUN];
static short samples[8192 * 2];
static unsigned samples_ready;
static unsigned long long gambatte_samples_made;
static unsigned long frames;
static unsigned held_buttons;

// The bits of Gambatte's button mask (its InputGetter::Button, which
// gambatte-core leaves commented out).
enum { GAMBATTE_A = 0x01, GAMBATTE_B = 0x02, GAMBATTE_START = 0x08, GAMBATTE_RIGHT = 0x10 };

static unsigned held(void *)
{
    unsigned buttons = 0;
    if (held_buttons & HARNESS_A) buttons |= GAMBATTE_A;
    if (held_buttons & HARNESS_B) buttons |= GAMBATTE_B;
    if (held_buttons & HARNESS_START) buttons |= GAMBATTE_START;
    if (held_buttons & HARNESS_RIGHT) buttons |= GAMBATTE_RIGHT;
    return buttons;
}

// Runs Gambatte until the resampler has the next 512 samples.
__attribute__((noinline)) static void run_buffer(gambatte::GB &gb, Resampler &resampler)
{
    while (samples_ready < HARNESS_SAMPLES_PER_BUFFER) {
        size_t made = 35112;
        if (gb.runFor(pixels, HARNESS_SCREEN_WIDTH, gambatte_samples, made) >= 0) frames++;
        gambatte_samples_made += made;
        samples_ready += resampler.resample(samples + samples_ready * 2,
                                            reinterpret_cast<short const *>(gambatte_samples), made);
    }
    samples_ready -= HARNESS_SAMPLES_PER_BUFFER;
    memmove(samples, samples + HARNESS_SAMPLES_PER_BUFFER * 2, samples_ready * 2 * sizeof(short));
}

int main(int argc, char **argv)
{
    harness_args args;
    if (!harness_parse_args(argc, argv, &args)) return 1;

    FILE *file = fopen(args.rom, "rb");
    if (!file) {
        fprintf(stderr, "could not open %s\n", args.rom);
        return 1;
    }
    std::vector<char> rom;
    char chunk[65536];
    for (size_t read; (read = fread(chunk, 1, sizeof chunk, file)) > 0;) {
        rom.insert(rom.end(), chunk, chunk + read);
    }
    fclose(file);

    gambatte::GB gb;
    if (gb.loadBios(harness_boot_rom(&args), args.cgb ? 0x900 : 0x100, 0) != 0) {
        fprintf(stderr, "could not load %s\n", harness_boot_rom(&args));
        return 1;
    }
    unsigned flags = gambatte::GB::READONLY_SAV | (args.cgb ? gambatte::GB::CGB_MODE : 0);
    if (gb.load(rom.data(), rom.size(), flags) != 0) {
        fprintf(stderr, "could not load %s\n", args.rom);
        return 1;
    }
    gb.setInputGetter(held, 0);
    gb.setTimeMode(true);
    Resampler *resampler = ResamplerInfo::get(0).create(GAMBATTE_SAMPLE_RATE, HARNESS_SAMPLE_RATE,
                                                        MOST_SAMPLES_PER_RUN);

    unsigned long buffers = harness_buffer_count(&args);
    double spent = 0;
    for (unsigned long buffer = 0; buffer < buffers; buffer++) {
        held_buttons = harness_buttons(buffer * harness_buffer_seconds());
        double started = harness_now();
        run_buffer(gb, *resampler);
        spent += harness_now() - started;
    }
    harness_report(double(gambatte_samples_made) / GAMBATTE_SAMPLE_RATE, spent, frames);

    static unsigned char rgb[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT * 3];
    for (unsigned i = 0; i < HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT; i++) {
        rgb[i * 3] = pixels[i] >> 16;
        rgb[i * 3 + 1] = pixels[i] >> 8;
        rgb[i * 3 + 2] = pixels[i];
    }
    harness_save_screenshot(&args, rgb);
    delete resampler;
    return 0;
}
