// Runs a ROM in binjgb (https://github.com/binji/binjgb). binjgb can't run a
// boot ROM, so the game starts in the post-boot state binjgb sets up, and it
// can't put a cartridge without Game Boy Color support in CGB mode, so `cgb`
// fails for those. Its sound is unsigned 8-bit at 44,100 Hz.
#include "harness.h"
#include "emulator.h"

static u8 samples[(HARNESS_SAMPLES_PER_BUFFER * 2 + 256) * SOUND_OUTPUT_COUNT];
static unsigned samples_ready;
static unsigned long frames;

// Runs binjgb until 512 samples are ready. emulator_run_until returns at every
// new frame (drawn into binjgb's frame buffer by then) and whenever its audio
// buffer holds the 512 samples it was made with, which are copied out before
// the next call reuses the buffer.
__attribute__((noinline)) static void run_buffer(Emulator *emulator)
{
    AudioBuffer *audio = emulator_get_audio_buffer(emulator);
    while (samples_ready < HARNESS_SAMPLES_PER_BUFFER) {
        EmulatorEvent event = emulator_run_until(emulator, emulator_get_ticks(emulator) + CPU_TICKS_PER_SECOND);
        if (event & EMULATOR_EVENT_NEW_FRAME) frames++;
        if (event & EMULATOR_EVENT_AUDIO_BUFFER_FULL) {
            u32 made = audio_buffer_get_frames(audio);
            memcpy(samples + samples_ready * SOUND_OUTPUT_COUNT, audio->data, made * SOUND_OUTPUT_COUNT);
            samples_ready += made;
        }
    }
    samples_ready -= HARNESS_SAMPLES_PER_BUFFER;
    memmove(samples, samples + HARNESS_SAMPLES_PER_BUFFER * SOUND_OUTPUT_COUNT, samples_ready * SOUND_OUTPUT_COUNT);
}

int main(int argc, char **argv)
{
    struct harness_args args;
    if (!harness_parse_args(argc, argv, &args)) return 1;

    FileData rom;
    if (file_read_aligned(args.rom, MINIMUM_ROM_SIZE, &rom) != OK) return 1;
    u8 cgb_flag = rom.data[0x143];
    if (args.cgb && cgb_flag != 0x80 && cgb_flag != 0xC0) {
        fprintf(stderr, "binjgb can't run a cartridge without Game Boy Color support in CGB mode\n");
        return 1;
    }

    EmulatorInit init;
    memset(&init, 0, sizeof init);
    init.rom = rom;
    init.audio_frequency = HARNESS_SAMPLE_RATE;
    init.audio_frames = HARNESS_SAMPLES_PER_BUFFER;
    init.random_seed = 0xcabba6e5;
    init.force_dmg = !args.cgb;
    init.cgb_color_curve = CGB_COLOR_CURVE_NONE;
    Emulator *emulator = emulator_new(&init);
    if (!emulator) return 1;

    unsigned long buffers = harness_buffer_count(&args);
    Ticks start = emulator_get_ticks(emulator);
    double spent = 0;
    for (unsigned long buffer = 0; buffer < buffers; buffer++) {
        unsigned held = harness_buttons(buffer * harness_buffer_seconds());
        JoypadButtons buttons;
        memset(&buttons, 0, sizeof buttons);
        buttons.A = (held & HARNESS_A) != 0;
        buttons.B = (held & HARNESS_B) != 0;
        buttons.start = (held & HARNESS_START) != 0;
        buttons.right = (held & HARNESS_RIGHT) != 0;
        emulator_set_joypad_buttons(emulator, &buttons);
        double started = harness_now();
        run_buffer(emulator);
        spent += harness_now() - started;
    }
    harness_report((emulator_get_ticks(emulator) - start) / (double)CPU_TICKS_PER_SECOND, spent, frames);

    static unsigned char rgb[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT * 3];
    RGBA *pixels = *emulator_get_frame_buffer(emulator);
    for (unsigned i = 0; i < HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT; i++) {
        rgb[i * 3] = pixels[i];
        rgb[i * 3 + 1] = pixels[i] >> 8;
        rgb[i * 3 + 2] = pixels[i] >> 16;
    }
    harness_save_screenshot(&args, rgb);
    emulator_delete(emulator);
    return 0;
}
