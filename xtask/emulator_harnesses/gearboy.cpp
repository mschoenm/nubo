// Runs a ROM in Gearboy (https://github.com/drhelius/Gearboy) with SameBoy's
// free boot ROMs, drawing in RGB565, the only kind of pixels it has.
//
// Gearboy's frontends run whole frames with RunToVBlank, which runs the CPU up
// to VBlank, takes the samples out of the APU (EndFrame) and draws the frame
// (RenderFrameBuffer). run_buffer makes the same calls (as Gearboy's libretro
// link cable mode does), but stops once the next 512 samples are ready, so the
// buttons change every 512 samples like in the other harnesses. Gearboy can't
// put a cartridge without Game Boy Color support in CGB mode, so `cgb` fails
// for those.
#include "harness.h"
#include "gearboy.h"

// Read by Gearboy's Log(); true keeps its messages off stdout.
bool g_mcp_stdio_mode = true;

static u16 pixels[SGB_SCREEN_WIDTH * SGB_SCREEN_HEIGHT];
static s16 samples[(HARNESS_SAMPLES_PER_BUFFER + AUDIO_BUFFER_SIZE) * 2];
static unsigned samples_ready;
static s16 frame_samples[AUDIO_BUFFER_SIZE];
static unsigned long frames;

static void hold(GearboyCore *core, Gameboy_Keys key, bool pressed)
{
    if (pressed)
        core->KeyPressed(key);
    else
        core->KeyReleased(key);
}

// Runs the CPU for the clock cycles the missing samples need, then takes the
// samples out of the APU, until 512 are ready, drawing every frame on the way.
__attribute__((noinline)) static void run_buffer(GearboyCore *core)
{
    while (samples_ready < HARNESS_SAMPLES_PER_BUFFER) {
        unsigned long long missing = HARNESS_SAMPLES_PER_BUFFER - samples_ready;
        unsigned cycles = (unsigned)((missing * GEARBOY_MASTER_CLOCK_RATE + HARNESS_SAMPLE_RATE - 1) /
                                     HARNESS_SAMPLE_RATE);
        unsigned elapsed = 0;
        while (elapsed < cycles) {
            unsigned int clocks;
            if (core->RunCycle(pixels, clocks)) {
                core->RenderFrameBuffer(pixels);
                frames++;
            }
            elapsed += clocks;
        }
        int count = 0;
        core->EndFrame(frame_samples, &count);
        memcpy(samples + samples_ready * 2, frame_samples, count * sizeof(s16));
        samples_ready += count / 2;
    }
    samples_ready -= HARNESS_SAMPLES_PER_BUFFER;
    memmove(samples, samples + HARNESS_SAMPLES_PER_BUFFER * 2, samples_ready * 2 * sizeof(s16));
}

int main(int argc, char **argv)
{
    harness_args args;
    if (!harness_parse_args(argc, argv, &args)) return 1;

    GearboyCore *core = new GearboyCore();
    core->Init(GB_PIXEL_RGB565);
    core->SetSGBEnabled(false);
    core->SetSoundSampleRate(HARNESS_SAMPLE_RATE);
    GB_Color white = {0xFF, 0xFF, 0xFF}, light = {0xAA, 0xAA, 0xAA}, dark = {0x55, 0x55, 0x55}, black = {0, 0, 0};
    core->SetDMGPalette(white, light, dark, black);

    Memory *memory = core->GetMemory();
    if (args.cgb) {
        memory->LoadBootromGBC(harness_boot_rom(&args));
        memory->EnableBootromGBC(true);
    }
    else {
        memory->LoadBootromDMG(harness_boot_rom(&args));
        memory->EnableBootromDMG(true);
    }
    if (!memory->IsBootromLoaded(args.cgb)) {
        fprintf(stderr, "could not load %s\n", harness_boot_rom(&args));
        return 1;
    }
    if (!core->LoadROM(args.rom, !args.cgb)) {
        fprintf(stderr, "could not load %s\n", args.rom);
        return 1;
    }
    if (core->IsCGB() != args.cgb) {
        fprintf(stderr, "Gearboy can't run a cartridge without Game Boy Color support in CGB mode\n");
        return 1;
    }

    unsigned long buffers = harness_buffer_count(&args);
    double spent = 0;
    for (unsigned long buffer = 0; buffer < buffers; buffer++) {
        unsigned buttons = harness_buttons(buffer * harness_buffer_seconds());
        hold(core, A_Key, buttons & HARNESS_A);
        hold(core, B_Key, buttons & HARNESS_B);
        hold(core, Start_Key, buttons & HARNESS_START);
        hold(core, Right_Key, buttons & HARNESS_RIGHT);
        double started = harness_now();
        run_buffer(core);
        spent += harness_now() - started;
    }
    harness_report(core->GetMasterClockCycles() / double(GEARBOY_MASTER_CLOCK_RATE), spent, frames);

    static unsigned char rgb[HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT * 3];
    for (unsigned i = 0; i < HARNESS_SCREEN_WIDTH * HARNESS_SCREEN_HEIGHT; i++) {
        unsigned red = pixels[i] >> 11 & 0x1F, green = pixels[i] >> 5 & 0x3F, blue = pixels[i] & 0x1F;
        rgb[i * 3] = red << 3 | red >> 2;
        rgb[i * 3 + 1] = green << 2 | green >> 4;
        rgb[i * 3 + 2] = blue << 3 | blue >> 2;
    }
    harness_save_screenshot(&args, rgb);
    delete core;
    return 0;
}
