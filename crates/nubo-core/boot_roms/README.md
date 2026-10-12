# Boot ROMs

The sources of Nubo's two boot ROMs. Their bytes are committed in
[`../src/bios.rs`](../src/bios.rs) as `DMG_BOOT` and `CGB_BOOT`, so building Nubo
needs nothing from this folder. You only need it to change a boot ROM.

| File | What it is |
| --- | --- |
| `nubo_dmg.asm` | The boot ROM of the DMG, MGB, SGB and SGB2 models, based on [Bootix](https://github.com/Hacktix/Bootix) (CC0-1.0) |
| `nubo_cgb.asm` | The boot ROM of the CGB and AGB models, based on [SameBoy](https://github.com/LIJI32/SameBoy)'s `cgb_boot.asm` at commit 6705401 (Expat License, see the notice in `../src/apu.rs`) |
| `sameboot.inc` | SameBoy's definitions for `nubo_cgb.asm`, from the same commit (Expat License) |
| `hardware.inc` | Names for the hardware registers, from [gbdev/hardware.inc](https://github.com/gbdev/hardware.inc) (CC0-1.0) |
| `gen_data.py` | Draws the Nubo name and cloud and works out the cloud's position in every frame, for both boot ROMs |
| `write_rom.py` | Writes a built boot ROM into `bios.rs` |
| `build.sh` | Builds both boot ROMs and writes them into `bios.rs` |

Each `.asm` file explains at its top what it shows, how it is laid out and how
its timeline goes.

## Building

You need [RGBDS](https://rgbds.gbdev.io) 1.0 (`brew install rgbds` on macOS) and
Python 3. Then run:

```bash
crates/nubo-core/boot_roms/build.sh
```

It generates the tiles and positions into `build/` (ignored by git), assembles
both boot ROMs, pads them to their size (0x900 bytes for the DMG, 0xC00 for the
CGB) and writes them into `bios.rs`. Afterwards run the core tests and the test
ROMs, as for any change to the emulator:

```bash
cargo test
```

```bash
cargo test-roms --verify-boot
```

To change the pictures, edit `GLYPHS` and `CLOUD` in `gen_data.py`; the cloud's
flight and bob are worked out there too. Timeline, colors and positions are
constants at the top of each `.asm` file.

## Keep the timing

The emulator's state after the boot ROM (DIV, the APU's frame sequencer,
channel 1 and where the PPU is in the frame) is calibrated in
`../src/address_bus/boot_rom.rs` against what the boot ROMs do. Both boot ROMs
therefore reproduce the timing of the ones they replaced, to the cycle, and
leave the same state behind. If a change moves any of these moments, the test
ROMs start failing (three of Gambatte's `ch2_init_env_counter_timing` tests are
the first to notice):

| Moment | DMG | CGB (CGB-C, a CGB game) |
| --- | --- | --- |
| The LCD switches on | 300 472 | 662 840 |
| The first note starts (`PlaySound` is entered) | 3 526 516 | 4 590 928 |
| The second note starts | 3 807 432 | 4 942 048 |
| `NuboIntro` rejoins SameBoy's `Preboot` (`$06E1`) | | 12 807 096 |
| The boot ROM hands over (the write to `$FF50`) | 7 318 460 | 13 089 000 |

The numbers are clock ticks (4 per M-cycle) since power-on. The constants under
"Delays" in each `.asm` file are tuned to hit them: the ones before switching the
LCD on, and the ones before and after waiting for the frames with a sound or the
hand-over. Waiting for a frame polls a flag every few cycles, so a delay before
the wait changes when the wait ends in steps that wrap around; try the values
one by one.

To measure them, add a temporary test like this one to
`../src/emulator/tests.rs`. It counts clock ticks by adding up how far the
system counter (the 16-bit counter whose upper byte is DIV) moves in every step,
which needs a temporary `probe_system_counter` method on `AddressBus` returning
`self.timers.system_counter()`:

```rust
#[test]
fn probe_boot_timing() {
    let model = Model::CgbC;
    let rom = rom(model, &header(0x80, 0, 0, 0x01), |_| 0);
    let mut emulator = Emulator::new();
    emulator.set_model(model);
    emulator.set_sample_rate(48_000);
    emulator.load_rom(&rom, empty_cartridge_effects()).unwrap();
    let mut ticks: u64 = 0;
    let mut last = emulator.address_bus().probe_system_counter();
    let mut lcd_on = false;
    while emulator.is_running_boot_rom() {
        // The CPU has already fetched the next opcode, so the program counter
        // is one past the instruction about to run.
        let pc = emulator.registers().program_counter;
        if !lcd_on && emulator.read_memory(0xFF40) & 0x80 != 0 {
            lcd_on = true;
            println!("LCD on at {ticks}");
        }
        if pc == 0x05D9 || pc == 0x06E2 {
            println!("{:04x} at {ticks}: {:?}", pc - 1, emulator.registers());
        }
        let before = ticks;
        emulator.step();
        if !emulator.is_running_boot_rom() {
            println!("hand-over at {before}");
            break;
        }
        let now = emulator.address_bus().probe_system_counter();
        ticks += u64::from(now.wrapping_sub(last));
        last = now;
    }
}
```

`PlaySound` is at `$05D8` in the CGB boot ROM; for the DMG one, look its address
up in `build/nubo_dmg.sym`. Run the test with
`cargo test -p nubo-core probe_boot_timing -- --nocapture`, once with the
current boot ROM and once with the changed one.

To check the end state too, print the whole emulator with `{emulator:#?}` at the
hand-over for both boot ROMs and compare the two. Do this with a real
cartridge as well as the test's made-up header: what the boot ROMs leave on the
stack can depend on the cartridge's logo. Only the boot ROM's own bytes,
a few leftovers inside the PPU from the cloud's sprites (which nothing reads
before a game shows sprites of its own) and tiny rounding differences in the
audio samples should differ.
