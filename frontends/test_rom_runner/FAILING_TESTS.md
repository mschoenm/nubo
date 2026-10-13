# Failing test ROMs

`cargo test-roms` passes 6524 of the 6557 tests it runs. This file explains the 33 that fail: what each
test checks, what Nubo does instead, and why it isn't fixed yet or can't be. `cargo test-roms --verbose`
prints the same list with each test's symptom, and failing screenshot tests leave the emulator's
screen and a diff in `target/test-roms/failures/<model>/`.

Gambatte's tests and its behaviour are mentioned throughout. Gambatte is GPL-2.0-only, so Nubo uses
it only as a reference: its test ROMs, and what a local build of it does on them. None of its code is
ported.

| Group | Tests | Status |
|---|---|---|
| [Channel 1 duty timing across CGB speed switches](#channel-1-duty-timing-across-cgb-speed-switches) | 9 | Open |
| [DMG palette writes during mode 3](#dmg-palette-writes-during-mode-3) | 9 | References disagree |
| [Interrupts during a speed switch (AGE spsw-interrupts)](#interrupts-during-a-speed-switch) | 3 | Open, no reference emulator gets it right |
| [CGB object palette RAM at power-on](#cgb-object-palette-ram-at-power-on) | 3 | Depends on one console's power-on state |
| [GBMicrotest](#gbmicrotest) | 2 | The expected values are wrong |
| [Mealybug's tile data switch glitch on CGB-C](#tile-data-switch-glitch-on-cgb-c) | 2 | Open |
| [Mooneye's MGB madness test](#mooneyes-mgb-madness-test) | 2 | Open, unexplained by its author |
| [Tile data switch at line 0 in double speed](#tile-data-switch-at-line-0-in-double-speed) | 1 | Open |
| [Window toggled at WX 166](#window-toggled-at-wx-166) | 1 | Open |
| [AGE oam-write on DMG-C](#age-oam-write-on-dmg-c) | 1 | Depends on state its author flags as unexplained |

Tests are named as the runner prints them: the hardware model, then the ROM's path under
`target/test-roms/v7.0/`.

## Channel 1 duty timing across CGB speed switches

```
cgb-c gambatte/sound/ch1_duty0_pos6_to_pos7_timing_ds_6_cgb04c_outaudio1.gbc
cgb-c gambatte/speedchange/speedchange_ch1_duty0_pos6_to_pos7_timing_2_cgb04c_outaudio1.gbc
cgb-c gambatte/speedchange/speedchange_ch1_duty0_pos6_to_pos7_timing_nop_2_cgb04c_outaudio1.gbc
cgb-c gambatte/speedchange/speedchange_ch1_nr4init_duty0_pos6_to_pos7_timing_2_cgb04c_outaudio1.gbc
cgb-c gambatte/speedchange/speedchange4_ch1_duty0_pos6_to_pos7_timing_1_cgb04c_outaudio0.gbc
cgb-c gambatte/speedchange/speedchange4_ch1_duty0_pos6_to_pos7_timing_nop_1_cgb04c_outaudio0.gbc
cgb-c gambatte/speedchange/speedchange5_ch1_duty0_pos6_to_pos7_timing_1_cgb04c_outaudio0.gbc
cgb-c gambatte/speedchange/speedchange5_ch1_duty0_pos6_to_pos7_timing_nop_1_cgb04c_outaudio0.gbc
cgb-c gambatte/speedchange/speedchange5_nop_ch1_duty0_pos6_to_pos7_timing_1_cgb04c_outaudio0.gbc
```

**What they check.** Channel 1 plays a square wave with duty 0, where only the last of its eight
steps (step 7) is high. The test starts the channel, switches the CPU's speed one to five times
(`speedchange` once, `speedchange4` four times, and so on; `nr4init` starts the channel after the
switch, and the `sound/..._ds` tests run in double speed), and waits until the channel is about to
step from 6 to 7. It then keeps restarting the channel with a long period, which freezes it on the
step it had reached: step 7 makes a sound, step 6 is silent. Each `_1`/`_2` pair is one M-cycle
apart and sits on either side of that step, so the pair pins down, to one 2 MHz sound tick, where the
channel's steps fall relative to the CPU.

**What Nubo does.** Of the 36 `speedchange` tests in this family, the 28 others pass, including all
16 with two or three switches. Comparing channel 1's step counter in Nubo and in a traced Gambatte at
the first restart (the accuracy toolkit's `speedchange-deltas.txt`) gives:

| Speed switches | Nubo's next step compared with the tests' |
|---|---|
| 1 | 1 tick late |
| 2 or 3 | the same |
| 4 or 5 | 2 ticks early |

In `nr4init_2`, which starts the channel after the switch, Nubo is already one tick late when the
channel starts, so there the whole error is the delay of the first step after a switch, not ticks
lost during it. In `ds_6`, the start is one tick later than in `ds_2` (which passes); the reference's
first step lands on the same tick in both, while Nubo's moves by two.

**Why it isn't fixed.** No fixed offset per switch gives that pattern: the error changes by +1, −1,
0, −2 and 0 over the five switches. So the phase between the sound clock and the CPU after a switch
depends on more than the number of switches. Nubo's sound clock keeps the tick parity it got when the
APU was powered on, while STOP resets DIV; if the hardware's sound clock is derived from the same
counter, each switch can realign it. These were tried and rejected:

| Change | Result |
|---|---|
| Counting the M-cycle in which double speed starts as one and a half sound ticks, rounded up or down | Fixes the one-switch tests, breaks the two- and three-switch ones |
| A sound clock that follows real time (8 MHz) across switches, from any starting phase | 3 fewer |
| Freezing the sound clock together with the LCD during the switch | 5 more, 14 fewer |
| The double speed trigger delay measured in Gambatte | 3 more (`ds_2`, `ds_4`, `ds_6`), 8 fewer: SameSuite's `channel_1_freq_change_timing-cgb0BC` on CGB-0, CGB-B and CGB-C (real hardware) and 5 `speedchange` tests |

**Next step.** Realign the sound clock's tick parity from the system counter when STOP resets DIV,
then retry the measured double speed trigger delay,
which may only conflict with SameSuite because the parity isn't realigned yet. Start with
`nr4init_2`, which isolates the delay after a switch, and keep SameSuite's channel tests and every
`speedchange` test that passes now passing.

## DMG palette writes during mode 3

```
dmg gambatte/dmgpalette_during_m3/dmgpalette_during_m3_3.gb
dmg gambatte/dmgpalette_during_m3/dmgpalette_during_m3_4.gb
dmg gambatte/dmgpalette_during_m3/dmgpalette_during_m3_5.gb
dmg gambatte/dmgpalette_during_m3/dmgpalette_during_m3_scx1_4.gb
dmg gambatte/dmgpalette_during_m3/lycint_dmgpalette_during_m3_3.gb
dmg gambatte/dmgpalette_during_m3/lycint_dmgpalette_during_m3_4.gb
dmg gambatte/dmgpalette_during_m3/scx3/dmgpalette_during_m3_4.gb
dmg gambatte/dmgpalette_during_m3/scx3/dmgpalette_during_m3_5.gb
dmg age-test-roms/m3-bg-bgp/m3-bg-bgp.gb
```

**What they check.** On every line, Gambatte's tests write BGP (the DMG background palette) = 00 (all
white) as the LCD starts drawing and BGP = FF (all black) near the end of the line; each variant moves
the first write one M-cycle later, some with fine scrolling. AGE's test writes BGP = FF from a mode 2
interrupt and restores it after 4 to 12 NOPs, with a different SCX on every line. Both check which
pixel is the first to show the new palette.

**What Nubo does.** It follows SameBoy and Mealybug Tearoom's DMG tests: the write reaches the
pixel output two dots early, and for one dot the palette is the old and new values ORed together.
Writing 00 over FF therefore leaves one extra black pixel where these tests expect the new white one
(one pixel on the first line, one per line from then on). In AGE's test, pixel 0 is black on two lines
where the hardware shows white.

**Why it isn't fixed.** The references disagree, and they appear to come from the same kind of
hardware. Mealybug's `m3_bgp_change` and `m3_bgp_change_sprites` (references from a "DMG-blob"
console) and daid's `ppu_scanline_bgp` need the OR dot, and pass. Gambatte's tests were checked on a
DMG-CPU-08 board and AGE's on a DMG-CPU C, which its source lists as a DMG-CPU-08 blob board too, so
the difference can't be modelled as a hardware revision. Every variant tried breaks one side:

| Variant | Gained | Lost |
|---|---|---|
| The write lands one dot later | none | 14: 7 Gambatte tests, both Mealybug BGP tests, 4 other Mealybug palette and window tests, daid's |
| The write lands one dot earlier | none | 15 |
| AND instead of OR | none | 9: 7 Gambatte tests (only the line ends broke), both Mealybug BGP tests |
| The new value shows at once, no mixed dot | the 8 Gambatte tests | both Mealybug BGP tests; AGE's still fails |

Mealybug's test changes BGP between neighbouring values, where OR and the new value differ in
subtle ways, while Gambatte's only switch between 00 and FF. **Next step:** tabulate every BGP write
of the three suites (the dot it lands on, the pixel being output, the fetcher step, SCX & 7, the old
and new values, what the hardware shows) and look for what separates the OR cases from the "new
value" cases.

## Interrupts during a speed switch

```
cgb   age-test-roms/speed-switch/caution/spsw-interrupts-cgbE.gb
cgb-b age-test-roms/speed-switch/caution/spsw-interrupts-cgbBC.gb
cgb-c age-test-roms/speed-switch/caution/spsw-interrupts-cgbBC.gb
```

**What they check.** A speed switch (STOP with KEY1 armed) normally halts the CPU for about 0x20000
clock units. These tests switch with an interrupt already pending, which cuts that halt short, and
read DIV and TIMA from the interrupt handler after delays one M-cycle apart, for each of the five
interrupt sources and both directions. The ROMs warn that this "prematurely terminates" the halt and
that the author's CGB-E "became instable for a while" after running them; the BC version passes on
CGB-B and CGB-C hardware, the E version on CGB-E.

**What Nubo does.** The six rows where a timer interrupt fires during the switch are right. In the
rows with an interrupt pending before STOP, every DIV read is 01 where the shorter delay expects 00,
and on CGB-B/C every TIMA read is 02 where the shorter delay expects 01. So the handler runs at least
one M-cycle later than on the hardware. On CGB-E, TIMA is one too high after switching to double
speed and one too low after switching back.

**Why it isn't fixed.** There is no reference emulator to compare with: Gambatte reads exactly the
same values as Nubo (its first 70 timer and interrupt reads match Nubo's one for one), and SameBoy,
whose speed switch Nubo follows, hasn't been checked against this test yet. **Next step:** change the
delays in a copy of the ROM to measure the exact offset for DIV and for TIMA in each direction, then
model how a pending interrupt ends the switch's halt, while keeping AGE's other speed switch tests
and Gambatte's `speedchange` tests passing.

## CGB object palette RAM at power-on

```
cgb-c gambatte/scx_during_m3/scx_during_m3_spx2.gbc
cgb-c gambatte/scx_during_m3/scx_during_m3_spx2_ds.gbc
cgb-c gambatte/scx_during_m3/scx_attrib_during_m3_spx2_ds.gbc
```

**What they check.** SCX writes during mode 3 with an object on the left edge of the screen.

**Why they can't pass.** Only pixel x = 0 of lines 0 to 7 differs: Nubo draws black, the reference
shows the colour (33, 146, 108). That pixel is colour 2 of object palette 0, and these tests never
write the object palettes, so the colour is whatever the CGB's object palette RAM held at power-on.
Patching all SCX writes out of the test leaves the pixel unchanged, so timing plays no part. The
reference was made with Gambatte, which starts with object palette RAM holding values read from one
real console; the colour matches its entry (0x4261) exactly. Real consoles power on with different
contents, so passing would mean copying that one console's dump into Nubo, which would be fitting the
test rather than emulating the hardware (and Gambatte's licence doesn't allow copying it). The DMG
version of the test passes.

## GBMicrotest

```
dmg gbmicrotest/halt_op_dupe_delay.gb
dmg gbmicrotest/stat_write_glitch_l154_d.gb
```

Both tests expect a value the hardware can't produce; Gambatte reads the same values as Nubo.

- **halt_op_dupe_delay** resets DIV, runs HALT (two M-cycles), a NOP and 58 more NOPs, then expects
  DIV = 0x55. DIV counts once every 64 M-cycles, so about 61 M-cycles after a reset it reads 1, which
  is what Nubo reads. The suite's own `div_inc_timing_a` and `div_inc_timing_b` put that edge at 60
  and 61 NOPs, and both pass. The source still has a `; 57/58` tuning note next to the delay, so
  0x55 is a placeholder.
- **stat_write_glitch_l154_d** is a copy of `stat_write_glitch_l154_c` (the header comments are
  identical) without its `clear_if`. It waits a whole frame with the LCD on and then expects IF = E0,
  but that frame's VBlank sets bit 0, so IF reads E1, as in Nubo and Gambatte. The `_c` version, which
  clears IF first, passes.

## Tile data switch glitch on CGB-C

```
cgb-c mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change2.gb
cgb-c mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change2.gb
```

**What they check.** LCDC bit 4 (which tile data area the background or window uses) is toggled
three times per line during mode 3, with objects shifting the write by one T-cycle per row. When
the bit changes in the same T-cycle as a tile data read, the hardware fetches mixed data, and the
reference shows which. Only a CGB-C reference exists, so only CGB-C runs; the versions without "2" pass
on DMG, CGB-C, CGB-D and CGB-E.

**What Nubo does.** 93 pixels differ in 12 rows, two cells of the grid: Nubo draws a solid block or
the wrong bitplanes where the CGB-C shows mixed glyph data.

**Why it isn't fixed.** Mealybug's notes list three possible sources for the glitched byte when the
bit is set (the last object's high bitplane, the last tile's high bitplane from when the bit was last
cleared, or the read cut off at the end of the previous line) and say CGB-D differs from the other
CGBs when the bit is cleared. Nubo uses SameBoy's model, with no revision difference. Taking the data
from the end of the previous line, four different ways, changed none of the 93 pixels. **Next step:**
log which fetcher step and bitplane each write hits in the failing cells, and work out which of the
documented sources matches the reference pixels before changing the renderer.

## Mooneye's MGB madness test

```
mgb mooneye-test-suite/madness/mgb_oam_dma_halt_sprites.gb
mgb mooneye-test-suite-wilbertpol/madness/mgb_oam_dma_halt_sprites.gb
```

The same ROM in two copies of the suite. It starts an OAM DMA and runs HALT with interrupts
disabled, so the CPU never wakes and the DMA stays stuck in the middle of its copy. On an MGB this
shows an object built from mixed old and new OAM bytes: an "8" made of `(30 | 1A) & FC`, `40 | 1A`,
and so on. The author writes "Why & $FC? I have no idea", says it only happens when some OAM entry
falls in particular ranges, and verified it on the MGB only (the DMG and AGB show a different object,
the CGB none). Nubo stops the DMA while halted and keeps OAM away from the LCD, as SameBoy does, so
it draws only the background. Matching the reference would need an MGB-only rule taken from the
author's description rather than an understanding of the hardware, so it was left open.

## Tile data switch at line 0 in double speed

```
cgb-c gambatte/bgtiledata/bgtiledata_spx08_ds_4.gbc
```

In double speed, an interrupt handler switches LCDC to the other tile data area (where tile 0 is
black) and back during mode 3. Only line 0, pixels 24 to 31, differ: the reference is black and Nubo
shows the normal tile. Lines 1 to 143 match, and so does `bgtiledata_spx08_ds_3`, one M-cycle
earlier. Every CPU write lands on the same dot as in Gambatte, so the difference is inside the
renderer: line 0 starts differently after line 153, and Nubo's fetch of that tile on line 0 likely
comes a dot early relative to the write. **Next step:** compare the dots of that tile's data reads on
lines 0 and 1, and try starting line 0's rendering a dot later in double speed without moving its
STAT and VRAM timing.

## Window toggled at WX 166

```
dmg gambatte/window/on_screen/wxA6_weoff_at_xposA6.gbc
```

With WY = 16 and WX = 166, the test clears the window enable bit at dot 176 of every line and sets it
again in HBlank. The window's internal line counter then decides which window row is drawn. Nubo's
counter is one line ahead from line 16 on, so the window's 4-line bands start at lines 19, 23, 27...
instead of 20, 24, 28... (464 pixels). The test passes on CGB-C, and the writes land on the same dots
as in Gambatte. Nubo counts a line both when the window is re-enabled in HBlank and when the WX 166
start carries over to the next line; dropping either count (when the window was disabled at the end
of the line) fixes nothing and breaks one or two other WX 166 tests. **Next step:** trace the window
line counter in both emulators line by line and find which of the two counts the hardware skips.

## AGE oam-write on DMG-C

```
dmg age-test-roms/oam/oam-write-dmgC.gb
```

The test writes OAM at many points of a line and reads it back. 32 of its 40 rows match. The 8 that
differ are the third row of each SCX block, which the source marks with (*): "DMG-C: This line seems
to depend on when the LCD was last switched off. When the test is expanded to cover more than 2
frames, the complete line looks different. [...] TODO: we should look into this." Until the hardware
behaviour behind those rows is known, there is nothing to model.
