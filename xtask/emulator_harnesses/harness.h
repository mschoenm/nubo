// What the harnesses of `cargo xtask compare-emulators` share. Each harness
// runs a ROM in another Game Boy emulator the way nubo-benchmark runs it in
// Nubo, so their speeds can be compared: 44,100 Hz stereo audio taken in
// buffers of 512 samples, nubo-benchmark's scripted buttons decided once per
// buffer from the emulated time, every frame rendered, and only the time spent
// in `run_buffer` counted. `run_buffer` is the function in each harness that
// runs the emulator until its next buffer is ready; it is never inlined, so
// callgrind can count the instructions inside it by name.
//
// Usage: <harness> <rom> <dmg|cgb> <seconds> <boot ROM folder> [screenshot.ppm]
//
// The boot ROM folder holds SameBoy's free dmg_boot.bin and cgb_boot.bin;
// harnesses whose emulator can't run them ignore it. A harness prints one line
// starting with "speed", which the xtask reads.
#ifndef HARNESS_H
#define HARNESS_H

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

#define HARNESS_SAMPLE_RATE 44100
#define HARNESS_SAMPLES_PER_BUFFER 512
#define HARNESS_SCREEN_WIDTH 160
#define HARNESS_SCREEN_HEIGHT 144

enum {
    HARNESS_A = 1,
    HARNESS_B = 2,
    HARNESS_START = 4,
    HARNESS_RIGHT = 8,
};

struct harness_args {
    const char *rom;
    // "dmg" or "cgb".
    const char *model;
    bool cgb;
    double seconds;
    const char *boot_roms;
    // NULL when no screenshot is wanted.
    const char *screenshot;
};

static inline bool harness_parse_args(int argc, char **argv, struct harness_args *args)
{
    if (argc != 5 && argc != 6) {
        fprintf(stderr, "usage: %s <rom> <dmg|cgb> <seconds> <boot ROM folder> [screenshot.ppm]\n", argv[0]);
        return false;
    }
    args->rom = argv[1];
    args->model = argv[2];
    if (strcmp(args->model, "dmg") != 0 && strcmp(args->model, "cgb") != 0) {
        fprintf(stderr, "the model must be dmg or cgb, not %s\n", args->model);
        return false;
    }
    args->cgb = strcmp(args->model, "cgb") == 0;
    char *end;
    args->seconds = strtod(argv[3], &end);
    if (end == argv[3] || *end != '\0' || !(args->seconds > 0)) {
        fprintf(stderr, "the seconds must be a positive number, not %s\n", argv[3]);
        return false;
    }
    args->boot_roms = argv[4];
    args->screenshot = argc == 6 ? argv[5] : NULL;
    return true;
}

// The path of the boot ROM for the model, in a buffer the next call reuses.
static inline const char *harness_boot_rom(const struct harness_args *args)
{
    static char path[4096];
    snprintf(path, sizeof path, "%s/%s_boot.bin", args->boot_roms, args->model);
    return path;
}

static inline double harness_buffer_seconds(void)
{
    return (double)HARNESS_SAMPLES_PER_BUFFER / HARNESS_SAMPLE_RATE;
}

// How many buffers make up the run, rounded up like nubo-benchmark does.
static inline unsigned long harness_buffer_count(const struct harness_args *args)
{
    return (unsigned long)ceil(args->seconds / harness_buffer_seconds());
}

// The buttons nubo-benchmark's `scripted_keys` holds at `seconds` of emulated
// time: Start a few times to get past title screens and menus, then running
// right while jumping.
static inline unsigned harness_buttons(double seconds)
{
    unsigned buttons = 0;
    if (seconds < 10.0) {
        if (seconds >= 4.0 && fmod(seconds, 2.0) < 0.15) buttons |= HARNESS_START;
    }
    else {
        buttons |= HARNESS_RIGHT;
        if (fmod(seconds, 1.0) < 0.4) buttons |= HARNESS_A;
        if (fmod(seconds, 3.0) < 1.5) buttons |= HARNESS_B;
    }
    return buttons;
}

static inline double harness_now(void)
{
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return now.tv_sec + now.tv_nsec / 1e9;
}

static inline void harness_report(double emulated, double spent, unsigned long frames)
{
    printf("speed %.3fx  emulated %.4f s  spent %.4f s  frames %lu\n", emulated / spent, emulated, spent,
           frames);
}

// Writes the last frame as a binary PPM, given the red, green and blue bytes
// of each of its 160x144 pixels.
static inline void harness_save_screenshot(const struct harness_args *args, const unsigned char *rgb)
{
    if (!args->screenshot) return;
    FILE *file = fopen(args->screenshot, "wb");
    if (!file) {
        fprintf(stderr, "could not write %s\n", args->screenshot);
        return;
    }
    fprintf(file, "P6\n%d %d\n255\n", HARNESS_SCREEN_WIDTH, HARNESS_SCREEN_HEIGHT);
    fwrite(rgb, 3, HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT, file);
    fclose(file);
}

#endif
