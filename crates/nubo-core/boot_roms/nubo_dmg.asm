; Nubo's boot ROM for the Game Boy models without Color hardware (DMG-0,
; DMG, MGB, SGB and SGB2).
;
; Based on Bootix by Hacktix (https://github.com/Hacktix/Bootix), released into
; the public domain under CC0-1.0.
;
; What it shows: the cartridge's Nintendo logo fades in with the Nubo name below
; it, while the Nubo cloud floats in from the left as sprites, settles next to
; the name, bobs, and blinks twice after the boot sound. Bootix shows the
; Nintendo logo wobbling into place instead.
;
; Why it is built this way: the emulator's post-boot state (DIV, the frame
; sequencer, channel 1 and the PPU position when the cartridge takes over) was
; calibrated against Bootix, and some games use what the boot ROM leaves in
; VRAM. So this boot ROM keeps Bootix's timing and end state exactly:
;  - everything up to switching the LCD on is Bootix's code, instruction for
;    instruction,
;  - both boot sounds start and the boot ROM hands over to the cartridge on
;    the same cycles as in Bootix,
;  - before handing over it erases the name, the cloud, its sprites and the
;    sprite palette and puts back what Bootix leaves on the stack, so VRAM,
;    OAM, HRAM and the registers end up as Bootix leaves them.
;
; Layout:
;   $0000-$00FF  Bootix's code up to switching the LCD on, then a jump to
;                NuboIntro; the hand-over to the cartridge at $00FC
;   $0100-$01FF  the cartridge header, which stays visible
;   $0200-$08FF  Nubo's code and data. Real DMGs have no boot ROM here; the
;                emulator maps it while the boot ROM runs, as the Game Boy
;                Color does with its own boot ROM.
;
; Timeline, in frames (vertical blanks) after the LCD is switched on; NuboIntro
; wakes up from HALT at the start of each one:
;   1-4      the name's and the cloud's tiles are copied to VRAM
;   7-19     the Nintendo logo and the name fade in; the cloud flies in during
;            7-45 and bobs from then on
;   46, 50   the two notes of the boot sound
;   58, 68   the cloud blinks
;   98-99    everything Nubo drew is erased
;   100      the boot ROM hands over to the cartridge
;
; The constants under "Delays" were found by tracing Bootix and this boot ROM in
; the emulator. If code that runs before one of the timed moments changes, they
; have to be measured again (see README.md).
;
; Build: see README.md in this folder.

DEF rIF EQU $FF0F
DEF rAUDENA EQU $FF26
DEF rLCDC EQU $FF40
DEF rBGP EQU $FF47
DEF rOBP0 EQU $FF48
DEF rIE EQU $FFFF
DEF AUDENA_ON EQU $80
DEF LCDCF_ON EQU $80
DEF LCDCF_BG8000 EQU $10
DEF LCDCF_OBJON EQU $02
DEF LCDCF_BGON EQU $01
DEF IEF_VBLANK EQU $01
DEF _OAMRAM EQU $FE00

; Nubo's timeline, in frames (vertical blanks) after the LCD is switched on
DEF LOAD_FRAMES EQU 4           ; frames 1-4 copy the tiles, 52 tile rows each
DEF ROWS_PER_FRAME EQU 52       ; (as much as fits in a vertical blank)
DEF FADE_1_FRAME EQU 7          ; the logo and the name turn light gray,
DEF FADE_2_FRAME EQU 13         ; dark gray
DEF FADE_3_FRAME EQU 19         ; and black, like Bootix's fade
DEF BLINK_1_FRAME EQU 58
DEF BLINK_2_FRAME EQU 68
DEF BLINK_FRAMES EQU 5          ; how long the eyes stay closed
DEF SOUND_1_FRAME EQU 46        ; the boot sound, early in these frames
DEF SOUND_2_FRAME EQU 50
DEF HIDE_FRAME EQU 98           ; frames 98 and 99 erase what Nubo drew

; Delays: after waking up in the frame of a sound or of the hand-over,
; Delay rounds of 4 M-cycles plus single M-cycles, so the sounds and the
; hand-over happen on the same cycle as in Bootix.
DEF SOUND_1_DELAY EQU 11
DEF SOUND_1_NOPS EQU 1
DEF SOUND_2_DELAY EQU 11
DEF SOUND_2_NOPS EQU 2
DEF EXIT_DELAY EQU 6
DEF EXIT_NOPS EQU 3

; Where things are. The Nintendo logo uses tiles 1-24 and the (R) tile 25,
; like in Bootix; Nubo's tiles follow.
DEF NAME_TILE EQU $1A           ; the name: 8x2 tiles
DEF NAME_TILES EQU 16
DEF CLOUD_TILE EQU NAME_TILE + NAME_TILES  ; the cloud: 3x3 sprites
DEF CLOUD_TILES EQU 9
DEF CLOSED_EYES_TILE EQU CLOUD_TILE + CLOUD_TILES
DEF EYES_SPRITE EQU 4           ; the middle sprite of the cloud's second row
DEF TILE_DATA EQU $8000 + NAME_TILE * 16
DEF TILE_DATA_SIZE EQU (NAME_TILES + CLOUD_TILES + 1) * 16
DEF NAME_MAP_ROW_1 EQU $9800 + 12 * 32 + 8
DEF NAME_MAP_ROW_2 EQU $9800 + 13 * 32 + 8

; Bootix leaves 4 bytes on the stack here, which Erase puts back: what
; LoadLogoNibble pushed last, which depends on the cartridge's logo, and the
; return address of Bootix's last call to PlaySound
DEF STACK_LEFTOVERS EQU $FFFA
DEF BOOTIX_PLAY_SOUND_RETURN EQU $008F

SECTION "Bootrom", ROM0[0]
    ; Initialize SP
    ld sp, $FFFE

    ; Clear VRAM
    ld hl, $9FFF
ClearVRAM:
    xor a
    ld [hld], a
    bit 7, h
    jr nz, ClearVRAM

    ; Initialize Audio
    ld c, $11
    ld hl, rAUDENA
    ld a, AUDENA_ON
    ld [hld], a
    ldh [c], a
    inc c
    ld a, $F3
    ld [hld], a
    ldh [c], a
    inc c
    ld a, $77
    ld [hld], a
    ldh [c], a

    ; Decode logo and load into VRAM
    ld de, $0104
    ld hl, $8010
DecodeLoop:
    ; Decode first 4 bits of byte
    ld a, [de]
    call LoadLogoNibble

    ; Decode second 4 bits of byte
    ld a, [de]
    swap a
    call LoadLogoNibble

    ; Loop if necessary
    inc de
    ld a, e
    cp $34
    jr nz, DecodeLoop

    ; Load Trademark Symbol into VRAM
    ld de, TrademarkLogo
    ld b, 8
TrademarkLoadLoop:
    ld a, [de]
    inc de
    ld [hli], a
    inc hl
    dec b
    jr nz, TrademarkLoadLoop

    ; Initialize tilemap (Row 1)
    ld hl, $9904
    ld bc, $010C
    call LogoMapInit
    ; Trademark Symbol
    ld a, $19
    ld [hl], a

    ; Initialize Tilemap (Row 2)
    ld hl, $9924
    ld c, $0C
    call LogoMapInit

    ; Initialize LCDC
    ld a, LCDCF_ON | LCDCF_BG8000 | LCDCF_BGON
    ldh [rLCDC], a

    jp NuboIntro

    ; Routine for loading logo tile numbers into VRAM
LogoMapInit:
    ld a, b
    ld [hli], a
    inc b
    dec c
    jr nz, LogoMapInit
    ret

    ; Routine for converting logo byte to VRAM data
LoadLogoNibble:
    ld b, a
    ld c, $04
    xor a
.bitShiftLoop
    push bc
    rl b
    rla
    pop bc
    rl b
    rla
    dec c
    jr nz, .bitShiftLoop
    ld [hli], a
    inc hl
    ld [hli], a
    inc hl
    ret

    ; Trademark Logo
TrademarkLogo:
    db $3C, $42, $B9, $A5, $B9, $A5, $42, $3C

    ; Mandatory Copyright Notice
    db "BOOTIX.DMG v1.2", 0

; Pad remaining space with NOP
ds $100-@-4, 0

EndBootrom:
    ; Disable Bootrom
    ld a, $01
    ldh [$FF50], a


SECTION "Nubo", ROM0[$0200]
; Runs Nubo's animation from the moment the LCD is on until the cartridge
; takes over, one frame at a time: MainLoop halts until the next vertical
; blank, starts a sound if this frame has one, and then lets Animate draw, all
; within the vertical blank. Bootix's timing is kept as described at the top.
NuboIntro:
    ; Keep what LoadLogoNibble left on the stack before Nubo's code uses it
    ldh a, [STACK_LEFTOVERS]
    ldh [hLogoLeftovers], a
    ldh a, [STACK_LEFTOVERS + 1]
    ldh [hLogoLeftovers + 1], a

    ; Hide the logo until it fades in, like Bootix
    xor a
    ldh [rBGP], a

    ; HALT then wakes up at every vertical blank. IME stays off, so no
    ; interrupt handler runs.
    ld a, IEF_VBLANK
    ldh [rIE], a

    xor a
    ldh [hFrame], a
    ; Where the tile copy in frames 1-4 reads and writes; DE and HL keep
    ; these between frames
    ld de, NuboTiles
    ld hl, TILE_DATA

MainLoop:
    ; Sleep until the next vertical blank: clear its flag in IF, and HALT
    ; wakes up as soon as the PPU sets it again
    xor a
    ldh [rIF], a
    halt

    ; The sounds have to start on exactly the same cycle as in Bootix, so
    ; this part takes equally long every frame.
    ldh a, [hFrame]
    inc a
    ldh [hFrame], a
    cp SOUND_1_FRAME
    jr z, .sound1
    cp SOUND_2_FRAME
    jr z, .sound2
    jr Animate

.sound1
    ld b, SOUND_1_DELAY
    call Delay
    REPT SOUND_1_NOPS
        nop
    ENDR
    ld a, $83
    call PlaySound
    jr Animate

.sound2
    ld b, SOUND_2_DELAY
    call Delay
    REPT SOUND_2_NOPS
        nop
    ENDR
    ld a, $C1
    call PlaySound
    ; fallthrough

; Everything else Nubo does in a frame, all within the vertical blank:
; copying the tiles in frames 1-4, then the fade, the cloud and the blink.
Animate:
    ldh a, [hFrame]
    cp LOAD_FRAMES + 1
    jr nc, .loaded

    ; Copy the next part of the tiles: one byte of each row goes to both
    ; bit planes, so the tiles use color 3. DE and HL carry on between frames.
    ld b, ROWS_PER_FRAME
.copy
    ld a, [de]
    inc de
    ld [hli], a
    ld [hli], a
    dec b
    jr nz, .copy

    ldh a, [hFrame]
    cp LOAD_FRAMES
    jp nz, MainLoop

    ; All tiles are loaded: show the name and set up the cloud's sprites
    push hl
    ld hl, NAME_MAP_ROW_1
    ld bc, (NAME_TILE << 8) | 8
    call LogoMapInit
    ld hl, NAME_MAP_ROW_2
    ld c, 8
    call LogoMapInit

    ld hl, _OAMRAM + 2
    ld a, CLOUD_TILE
    ld c, CLOUD_TILES
.sprites
    ld [hli], a                 ; tile
    inc l
    inc l
    inc l
    inc a
    dec c
    jr nz, .sprites
    pop hl

    ld a, $FC                   ; the sprites' color 3 is black
    ldh [rOBP0], a
    ld a, LCDCF_ON | LCDCF_BG8000 | LCDCF_OBJON | LCDCF_BGON
    ldh [rLCDC], a
    jp MainLoop

.loaded
    cp HIDE_FRAME
    jr nc, Erase

    ; Fade in the background like Bootix: light gray, dark gray, black
    ld b, $54
    cp FADE_1_FRAME
    jr z, .setBGP
    ld b, $A8
    cp FADE_2_FRAME
    jr z, .setBGP
    ld b, $FC
    cp FADE_3_FRAME
    jr nz, .moveCloud
.setBGP
    ld a, b
    ldh [rBGP], a

.moveCloud
    ; Place the 3x3 cloud sprites at this frame's position: CloudX and
    ; CloudY hold the OAM X and Y of the top left one for every frame
    ldh a, [hFrame]
    ld l, a
    ld h, HIGH(CloudX)
    ld d, [hl]
    ld h, HIGH(CloudY)
    ld e, [hl]
    ld hl, _OAMRAM
    ld c, 3
.row
    ld b, d
    REPT 3
        ld a, e
        ld [hli], a             ; Y
        ld a, b
        ld [hli], a             ; X
        add 8
        ld b, a
        inc l
        inc l
    ENDR
    ld a, e
    add 8
    ld e, a
    dec c
    jr nz, .row

    ; Blink twice: swap in the tile with closed eyes for a few frames
    ldh a, [hFrame]
    ld b, CLOSED_EYES_TILE
    sub BLINK_1_FRAME
    cp BLINK_FRAMES
    jr c, .setEyes
    sub BLINK_2_FRAME - BLINK_1_FRAME
    cp BLINK_FRAMES
    jr c, .setEyes
    ld b, CLOUD_TILE + EYES_SPRITE
.setEyes
    ld a, b
    ld [_OAMRAM + EYES_SPRITE * 4 + 2], a
    jp MainLoop

; Erases everything Nubo drew, over two frames, so the cartridge finds the
; same state as Bootix leaves behind.
Erase:
    jr nz, .lastFrame

    ; First frame: hide the cloud and the name, erase half of the tiles
    ld a, LCDCF_ON | LCDCF_BG8000 | LCDCF_BGON
    ldh [rLCDC], a
    xor a
    ld hl, NAME_MAP_ROW_1
    ld c, 1
    call Fill8
    ld hl, NAME_MAP_ROW_2
    ld c, 1
    call Fill8
    ld hl, TILE_DATA
    ld c, TILE_DATA_SIZE / 16
    call Fill8
    jp MainLoop

.lastFrame
    ; Second frame: the other half of the tiles, the sprites and registers
    xor a
    ld hl, TILE_DATA + TILE_DATA_SIZE / 2
    ld c, TILE_DATA_SIZE / 16
    call Fill8
    ld hl, _OAMRAM
    ld c, (CLOUD_TILES * 4 + 7) / 8
    call Fill8
    dec a
    ldh [rOBP0], a

    ; Leave the stack as Bootix does, and Nubo's variables at 0 like the
    ; HRAM Bootix never uses
    ldh a, [hLogoLeftovers]
    ldh [STACK_LEFTOVERS], a
    ldh a, [hLogoLeftovers + 1]
    ldh [STACK_LEFTOVERS + 1], a
    ld a, LOW(BOOTIX_PLAY_SOUND_RETURN)
    ldh [STACK_LEFTOVERS + 2], a
    ld a, HIGH(BOOTIX_PLAY_SOUND_RETURN)
    ldh [STACK_LEFTOVERS + 3], a
    xor a
    ld hl, hFrame
    ld c, hNuboEnd - hFrame
.clearVariables
    ld [hli], a
    dec c
    jr nz, .clearVariables

    ; Hand over to the cartridge at the start of the next vertical blank
    ldh [rIF], a
    halt
    xor a
    ldh [rIE], a
    ld b, EXIT_DELAY
.exitDelay
    dec b
    jr nz, .exitDelay
    REPT EXIT_NOPS
        nop
    ENDR
    jp EndBootrom

; Writes A to 8 * C bytes from HL on, quickly enough to erase everything
; within two vertical blanks.
Fill8:
    REPT 8
        ld [hli], a
    ENDR
    dec c
    jr nz, Fill8
    ret

; Waits 4 * B + 9 M-cycles, counting the call and the return.
Delay:
    dec b
    jr nz, Delay
    ret

; Bootix's routine for starting a sound on channel 1.
PlaySound:
    ld c, $13
    ldh [c], a
    inc c
    ld a, $87
    ldh [c], a
    ret

SECTION "NuboTiles", ROM0[$0600]
NuboTiles:
    INCLUDE "nubo_tiles.inc"

INCLUDE "nubo_dmg_tables.inc"

SECTION "Variables", HRAM[$FF80]
hFrame: ds 1                    ; frames since the LCD was switched on
hLogoLeftovers: ds 2            ; see STACK_LEFTOVERS
hNuboEnd:
