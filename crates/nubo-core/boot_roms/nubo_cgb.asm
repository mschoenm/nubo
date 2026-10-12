; Nubo's boot ROM for the Game Boy Color models.
;
; Based on SameBoy's CGB boot ROM: https://github.com/LIJI32/SameBoy,
; BootROMs/cgb_boot.asm at commit 6705401 (Expat License, Copyright (c)
; 2015-2026 Lior Halphon; see the notice in ../src/apu.rs).
;
; What it shows: the cartridge's Nintendo logo where the DMG boot ROM shows it,
; fading in the same way, with the Nubo name below it. The Nubo cloud floats in
; from the left as sprites; when it reaches the name, a rainbow sweeps over the
; letters and leaves them in Nubo's accent purple. The cloud bobs, blinks twice
; after the boot sound, and the picture stays until the game starts (SameBoy
; fades to white instead).
;
; Why it is built this way: before this, Nubo used SameBoy's boot ROM with
; another logo, and the emulator's post-boot state (DIV, the frame sequencer,
; channel 1 and the PPU position when the cartridge takes over) was calibrated
; against it. This boot ROM therefore keeps that version's timing to the cycle:
;  - everything up to switching the LCD on takes exactly as long,
;  - both boot sounds start on the same cycles,
;  - NuboIntro rejoins SameBoy's Preboot after where its fade ended, on the
;    same cycle and with the same registers, stack, HRAM and palettes.
; Everything after that is SameBoy's code at its original addresses, so the
; return addresses it leaves on the stack match too and games find exactly the
; same state.
;
; Layout:
;   $0000-$00FF  SameBoy's start-up code, which now calls NuboSetup and jumps
;                to NuboIntro once the LCD is on
;   $0100-$01FF  the cartridge header, which stays visible
;   $0200-$08FF  SameBoy's data and routines at their original addresses. The
;                space its logo, the logo's decompressor, its intro animation
;                and its fade used is left empty (pad_to).
;   $0900-$0BFF  Nubo's code and data. Real CGBs have no boot ROM here; the
;                emulator maps it while the boot ROM runs.
;
; Timeline, in frames (vertical blanks) after the LCD is switched on:
;   1-6      the cloud's tiles and the name are in place, still invisible
;   7-19     the Nintendo logo fades in; the cloud flies in during 7-45
;   33-61    the rainbow sweep over the name, once the cloud has arrived
;   56, 61   the two notes of the boot sound
;   68, 78   the cloud blinks
;   110-172  the hold, where SameBoy fades to white; the cloud goes at 171
;   172      the screen turns white; NuboIntro rejoins Preboot at frame 173,
;            which hands over to the cartridge a few frames later
; The player can still pick a DMG palette with the D-pad until frame 109.
;
; The constants under "Delays" were found by tracing the previous version and
; this one in the emulator. If code that runs before one of the timed moments
; changes, they have to be measured again (see README.md).
;
; Build: see README.md in this folder.


include "sameboot.inc"
include "nubo_cgb_constants.inc"

; Waits the given number of M-cycles, in as few bytes as it can. PUSH AF and
; POP AF take 7 M-cycles and change nothing but the stack below SP.
MACRO wait_cycles ; M-cycles
    REPT (\1) / 7
        push af
        pop af
    ENDR
    REPT (\1) % 7
        nop
    ENDR
ENDM

; Leaves the rest of the space up to `address` empty, so the next part of
; SameBoy's boot ROM stays at the address it had (see the top of this file).
MACRO pad_to ; address
    ASSERT @ <= \1, "the code before \1 has grown past it"
    ds \1 - @, 0
ENDM

; Nubo's timeline, in frames (vertical blanks) after the LCD is switched on
DEF FADE_1_FRAME EQU 7          ; the Nintendo logo turns light gray,
DEF FADE_2_FRAME EQU 13         ; dark gray
DEF FADE_3_FRAME EQU 19         ; and black, as in the DMG boot ROM
DEF FLIGHT_START EQU 7          ; the cloud flies in during frames 7-45
DEF FLIGHT_END EQU 46           ; and bobs from frame 46 on
DEF SWEEP_FRAME EQU 33          ; the cloud has reached the name
DEF SOUND_1_FRAME EQU 56        ; the boot sound, at the end of these frames
DEF SOUND_2_FRAME EQU 61
DEF BLINK_1_FRAME EQU 68
DEF BLINK_2_FRAME EQU 78
DEF BLINK_FRAMES EQU 5          ; how long the eyes stay closed
DEF HOLD_FRAMES EQU 63          ; frames 110-172, where SameBoy fades to white
DEF BOB_FRAMES EQU 40           ; the length of one bob

; NuboIntro's phases. The player can pick a DMG palette with the D-pad until
; the hold, like in SameBoy.
DEF PHASE_INTRO EQU 0           ; until the second sound
DEF PHASE_WAIT EQU 1            ; SameBoy's wait loop (hWaitLoopCounter)
DEF PHASE_HOLD EQU 2            ; where SameBoy fades

; Delays that keep the previous version's timing (see the top of this file):
; LCD_ON_DELAY rounds of 7 M-cycles plus LCD_ON_CYCLES before switching the LCD
; on, and M-cycles before and after waiting for the frames that end with a
; sound or with rejoining Preboot.
DEF LCD_ON_DELAY EQU 1810
DEF LCD_ON_CYCLES EQU 4
DEF SOUND_1_PRE_NOPS EQU 5
DEF SOUND_2_PRE_NOPS EQU 2
DEF SOUND_POST_CYCLES EQU 8
DEF FINISH_PRE_NOPS EQU 5
DEF FINISH_POST_NOPS EQU 3

; Colors (15-bit BGR, 5 bits per channel)
DEF ACCENT EQU 17 | 15 << 5 | 30 << 10  ; Nubo's accent purple, #8B7CF6
DEF LIGHT_GRAY EQU 21 * $0421
DEF DARK_GRAY EQU 10 * $0421
DEF BLACK EQU 0

; Where things are. Nubo's tiles are in VRAM bank 1 where SameBoy's logo was;
; the Nintendo logo's tiles are in bank 0, loaded below for DMG compatibility.
DEF ATTR_BANK1 EQU OAMF_BANK1   ; the same bit picks the tile bank in BG attributes
DEF NAME_TILE EQU $08           ; the name: 8x2 tiles
DEF NAME_COLUMNS EQU 8
DEF CLOUD_TILE EQU NAME_TILE + 2 * NAME_COLUMNS  ; the cloud: 3x3 sprites
DEF CLOUD_TILES EQU 9
DEF CLOSED_EYES_TILE EQU CLOUD_TILE + CLOUD_TILES
DEF EYES_SPRITE EQU 4           ; the middle sprite of the cloud's second row
DEF LOGO_COLUMNS EQU 12
DEF TRADEMARK_TILE EQU $19
DEF LOGO_MAP_ROW_1 EQU _SCRN0 + 8 * SCRN_VX_B + 4
DEF LOGO_MAP_ROW_2 EQU _SCRN0 + 9 * SCRN_VX_B + 4
DEF NAME_MAP_ROW_1 EQU _SCRN0 + 12 * SCRN_VX_B + 8
DEF NAME_MAP_ROW_2 EQU _SCRN0 + 13 * SCRN_VX_B + 8  ; = ROW_1 with bit 5 set
DEF STACK_LEFTOVERS EQU $FFF4   ; see StackLeftovers
DEF STACK_LEFTOVERS_SIZE EQU 10

SECTION "BootCode", ROM0[$0000]
Start:
; Init stack pointer
    ld sp, $FFFE

; Clear memory VRAM
    call ClearMemoryVRAM

; Clear OAM
    ld h, HIGH(_OAMRAM)
    ld c, sizeof_OAM_ATTRS * OAM_COUNT
.clearOAMLoop
    ldi [hl], a
    dec c
    jr nz, .clearOAMLoop

IF !DEF(CGB0)
; Init waveform
    ld c, 16
    ld hl, _AUD3WAVERAM
.waveformLoop
    ldi [hl], a
    cpl
    dec c
    jr nz, .waveformLoop
ENDC

; Clear chosen input palette
    ldh [hInputPalette], a
; Clear title checksum
    ldh [hTitleChecksum], a

; Init Audio
    ld a, AUDENA_ON
    ldh [rNR52], a
    assert AUDENA_ON == AUDLEN_DUTY_50
    ldh [rNR11], a
    ld a, $F3
    ldh [rNR12], a ; Envelope $F, decreasing, sweep $3
    ldh [rNR51], a ; Channels 1+2+3+4 left, channels 1+2 right
    ld a, $77
    ldh [rNR50], a ; Volume $7, left and right

; Init BG palette
    ld a, %11_11_11_00
    ldh [rBGP], a

; Load logo from ROM.
; A nibble represents a 4-pixels line, 2 bytes represent a 4x4 tile, scaled to 8x8.
; Tiles are ordered left to right, top to bottom.
; These tiles are not used, but are required for DMG compatibility. This is done
; by the original CGB Boot ROM as well.
    ld de, NintendoLogo
    ld hl, _VRAM + $10 ; This is where we load the tiles in VRAM

.loadLogoLoop
    ld a, [de] ; Read 2 rows
    ld b, a
    call DoubleBitsAndWriteRowTwice
    inc de
    ld a, e
    cp LOW(NintendoLogoEnd)
    jr nz, .loadLogoLoop
    call ReadTrademarkSymbol

; Clear the second VRAM bank
    ld a, 1
    ldh [rVBK], a
    call ClearMemoryVRAM
    call NuboSetup

    ; Switch the LCD on on the same cycle as the previous version, so that
    ; everything after it happens at the same time as well
    ld bc, LCD_ON_DELAY
.lcdOnDelay
    dec bc
    ld a, b
    or c
    jr nz, .lcdOnDelay
    wait_cycles LCD_ON_CYCLES

    ; Turn on LCD
    ld a, LCDCF_ON | LCDCF_BLK01 | LCDCF_BGON
    ldh [rLCDC], a
    jp NuboIntro                ; never returns: it rejoins Preboot

    ; SameBoy's tile map and palette setup was here
    pad_to $00D8
; NuboIntro rejoins Preboot with this as its return address, where the
; previous version's `call Preboot` returned to.
ReturnFromPreboot:
    jr BootGame

HDMAData:
MACRO hdma_data ; source, destination, length
    db HIGH(\1), LOW(\1)
    db HIGH(\2), LOW(\2)
    db (\3)
ENDM
    hdma_data _RAMBANK, _SCRN0 + 5 * SCRN_VX_B + 0, 18
    hdma_data _RAMBANK, _VRAM, 64
    ASSERT HDMAData == $00DA, "HDMAData moved; the stack leftovers would differ"



SECTION "BootGame", ROM0[$00FE]
BootGame:
    ldh [rBANK], a ; unmap boot ROM

SECTION "BootData", ROM0[$0200]
; Game Palettes Data
TitleChecksums:
    db $00 ; Default
    db $88 ; ALLEY WAY
    db $16 ; YAKUMAN
    db $36 ; BASEBALL, (Game and Watch 2)
    db $D1 ; TENNIS
    db $DB ; TETRIS
    db $F2 ; QIX
    db $3C ; DR.MARIO
    db $8C ; RADARMISSION
    db $92 ; F1RACE
    db $3D ; YOSSY NO TAMAGO
    db $5C ;
    db $58 ; X
    db $C9 ; MARIOLAND2
    db $3E ; YOSSY NO COOKIE
    db $70 ; ZELDA
    db $1D ;
    db $59 ;
    db $69 ; TETRIS FLASH
    db $19 ; DONKEY KONG
    db $35 ; MARIO'S PICROSS
    db $A8 ;
    db $14 ; POKEMON RED, (GAMEBOYCAMERA G)
    db $AA ; POKEMON GREEN
    db $75 ; PICROSS 2
    db $95 ; YOSSY NO PANEPON
    db $99 ; KIRAKIRA KIDS
    db $34 ; GAMEBOY GALLERY
    db $6F ; POCKETCAMERA
    db $15 ;
    db $FF ; BALLOON KID
    db $97 ; KINGOFTHEZOO
    db $4B ; DMG FOOTBALL
    db $90 ; WORLD CUP
    db $17 ; OTHELLO
    db $10 ; SUPER RC PRO-AM
    db $39 ; DYNABLASTER
    db $F7 ; BOY AND BLOB GB2
    db $F6 ; MEGAMAN
    db $A2 ; STAR WARS-NOA
    db $49 ;
    db $4E ; WAVERACE
    db $43 ;
    db $68 ; LOLO2
    db $E0 ; YOSHI'S COOKIE
    db $8B ; MYSTIC QUEST
    db $F0 ;
    db $CE ; TOPRANKINGTENNIS
    db $0C ; MANSELL
    db $29 ; MEGAMAN3
    db $E8 ; SPACE INVADERS
    db $B7 ; GAME&WATCH
    db $86 ; DONKEYKONGLAND95
    db $9A ; ASTEROIDS/MISCMD
    db $52 ; STREET FIGHTER 2
    db $01 ; DEFENDER/JOUST
    db $9D ; KILLERINSTINCT95
    db $71 ; TETRIS BLAST
    db $9C ; PINOCCHIO
    db $BD ;
    db $5D ; BA.TOSHINDEN
    db $6D ; NETTOU KOF 95
    db $67 ;
    db $3F ; TETRIS PLUS
    db $6B ; DONKEYKONGLAND 3
; For these games, the 4th letter is taken into account
FirstChecksumWithDuplicate:
    ; Let's play hangman!
    db $B3 ; ???[B]????????
    db $46 ; SUP[E]R MARIOLAND
    db $28 ; GOL[F]
    db $A5 ; SOL[A]RSTRIKER
    db $C6 ; GBW[A]RS
    db $D3 ; KAE[R]UNOTAMENI
    db $27 ; ???[B]????????
    db $61 ; POK[E]MON BLUE
    db $18 ; DON[K]EYKONGLAND
    db $66 ; GAM[E]BOY GALLERY2
    db $6A ; DON[K]EYKONGLAND 2
    db $BF ; KID[ ]ICARUS
    db $0D ; TET[R]IS2
    db $F4 ; ???[-]????????
    db $B3 ; MOG[U]RANYA
    db $46 ; ???[R]????????
    db $28 ; GAL[A]GA&GALAXIAN
    db $A5 ; BT2[R]AGNAROKWORLD
    db $C6 ; KEN[ ]GRIFFEY JR
    db $D3 ; ???[I]????????
    db $27 ; MAG[N]ETIC SOCCER
    db $61 ; VEG[A]S STAKES
    db $18 ; ???[I]????????
    db $66 ; MIL[L]I/CENTI/PEDE
    db $6A ; MAR[I]O & YOSHI
    db $BF ; SOC[C]ER
    db $0D ; POK[E]BOM
    db $F4 ; G&W[ ]GALLERY
    db $B3 ; TET[R]IS ATTACK
ChecksumsEnd:

PalettePerChecksum:
MACRO palette_index ; palette[, flags]
    IF _NARG == 1
        db (\1)
    ELSE
        db (\1) | (\2) ; flag $80 means game requires DMG boot tilemap
    ENDC
ENDM
    palette_index  0 ; Default Palette
    palette_index  4 ; ALLEY WAY
    palette_index  5 ; YAKUMAN
    palette_index 35 ; BASEBALL, (Game and Watch 2)
    palette_index 34 ; TENNIS
    palette_index  3 ; TETRIS
    palette_index 31 ; QIX
    palette_index 15 ; DR.MARIO
    palette_index 10 ; RADARMISSION
    palette_index  5 ; F1RACE
    palette_index 19 ; YOSSY NO TAMAGO
    palette_index 36 ;
    palette_index  7, $80 ; X
    palette_index 37 ; MARIOLAND2
    palette_index 30 ; YOSSY NO COOKIE
    palette_index 44 ; ZELDA
    palette_index 21 ;
    palette_index 32 ;
    palette_index 31 ; TETRIS FLASH
    palette_index 20 ; DONKEY KONG
    palette_index  5 ; MARIO'S PICROSS
    palette_index 33 ;
    palette_index 13 ; POKEMON RED, (GAMEBOYCAMERA G)
    palette_index 14 ; POKEMON GREEN
    palette_index  5 ; PICROSS 2
    palette_index 29 ; YOSSY NO PANEPON
    palette_index  5 ; KIRAKIRA KIDS
    palette_index 18 ; GAMEBOY GALLERY
    palette_index  9 ; POCKETCAMERA
    palette_index  3 ;
    palette_index  2 ; BALLOON KID
    palette_index 26 ; KINGOFTHEZOO
    palette_index 25 ; DMG FOOTBALL
    palette_index 25 ; WORLD CUP
    palette_index 41 ; OTHELLO
    palette_index 42 ; SUPER RC PRO-AM
    palette_index 26 ; DYNABLASTER
    palette_index 45 ; BOY AND BLOB GB2
    palette_index 42 ; MEGAMAN
    palette_index 45 ; STAR WARS-NOA
    palette_index 36 ;
    palette_index 38 ; WAVERACE
    palette_index 26, $80 ;
    palette_index 42 ; LOLO2
    palette_index 30 ; YOSHI'S COOKIE
    palette_index 41 ; MYSTIC QUEST
    palette_index 34 ;
    palette_index 34 ; TOPRANKINGTENNIS
    palette_index  5 ; MANSELL
    palette_index 42 ; MEGAMAN3
    palette_index  6 ; SPACE INVADERS
    palette_index  5 ; GAME&WATCH
    palette_index 33 ; DONKEYKONGLAND95
    palette_index 25 ; ASTEROIDS/MISCMD
    palette_index 42 ; STREET FIGHTER 2
    palette_index 42 ; DEFENDER/JOUST
    palette_index 40 ; KILLERINSTINCT95
    palette_index  2 ; TETRIS BLAST
    palette_index 16 ; PINOCCHIO
    palette_index 25 ;
    palette_index 42 ; BA.TOSHINDEN
    palette_index 42 ; NETTOU KOF 95
    palette_index  5 ;
    palette_index  0 ; TETRIS PLUS
    palette_index 39 ; DONKEYKONGLAND 3
    palette_index 36 ;
    palette_index 22 ; SUPER MARIOLAND
    palette_index 25 ; GOLF
    palette_index  6 ; SOLARSTRIKER
    palette_index 32 ; GBWARS
    palette_index 12 ; KAERUNOTAMENI
    palette_index 36 ;
    palette_index 11 ; POKEMON BLUE
    palette_index 39 ; DONKEYKONGLAND
    palette_index 18 ; GAMEBOY GALLERY2
    palette_index 39 ; DONKEYKONGLAND 2
    palette_index 24 ; KID ICARUS
    palette_index 31 ; TETRIS2
    palette_index 50 ;
    palette_index 17 ; MOGURANYA
    palette_index 46 ;
    palette_index  6 ; GALAGA&GALAXIAN
    palette_index 27 ; BT2RAGNAROKWORLD
    palette_index  0 ; KEN GRIFFEY JR
    palette_index 47 ;
    palette_index 41 ; MAGNETIC SOCCER
    palette_index 41 ; VEGAS STAKES
    palette_index  0 ;
    palette_index  0 ; MILLI/CENTI/PEDE
    palette_index 19 ; MARIO & YOSHI
    palette_index 34 ; SOCCER
    palette_index 23 ; POKEBOM
    palette_index 18 ; G&W GALLERY
    palette_index 29 ; TETRIS ATTACK

Dups4thLetterArray:
    db "BEFAARBEKEK R-URAR INAILICE R"

; We assume the last three arrays fit in the same $100 byte page!

PaletteCombinations:
MACRO palette_comb ; Obj0, Obj1, Bg
    db (\1) * 8, (\2) * 8, (\3) *8
ENDM
MACRO raw_palette_comb ; Obj0, Obj1, Bg
    db (\1) * 2, (\2) * 2, (\3) * 2
ENDM
    palette_comb  4,  4, 29 ;  0, Right + A
    palette_comb 18, 18, 18 ;  1, Right
    palette_comb 20, 20, 20 ;  2
    palette_comb 24, 24, 24 ;  3, Down + A
    palette_comb  9,  9,  9 ;  4
    palette_comb  0,  0,  0 ;  5, Up
    palette_comb 27, 27, 27 ;  6, Right + B
    palette_comb  5,  5,  5 ;  7, Left + B
    palette_comb 12, 12, 12 ;  8, Down
    palette_comb 26, 26, 26 ;  9
    palette_comb 16,  8,  8 ; 10
    palette_comb  4, 28, 28 ; 11
    palette_comb  4,  2,  2 ; 12
    palette_comb  3,  4,  4 ; 13
    palette_comb  4, 29, 29 ; 14
    palette_comb 28,  4, 28 ; 15
    palette_comb  2, 17,  2 ; 16
    palette_comb 16, 16,  8 ; 17
    palette_comb  4,  4,  7 ; 18
    palette_comb  4,  4, 18 ; 19
    palette_comb  4,  4, 20 ; 20
    palette_comb 19, 19,  9 ; 21
    raw_palette_comb 4 * 4 - 1, 4 * 4 - 1, 11 * 4 ; 22
    palette_comb 17, 17,  2 ; 23
    palette_comb  4,  4,  2 ; 24
    palette_comb  4,  4,  3 ; 25
    palette_comb 28, 28,  0 ; 26
    palette_comb  3,  3,  0 ; 27
    palette_comb  0,  0,  1 ; 28, Up + B
    palette_comb 18, 22, 18 ; 29
    palette_comb 20, 22, 20 ; 30
    palette_comb 24, 22, 24 ; 31
    palette_comb 16, 22,  8 ; 32
    palette_comb 17,  4, 13 ; 33
    raw_palette_comb 28 * 4 - 1, 0 * 4, 14 * 4 ; 34
    raw_palette_comb 28 * 4 - 1, 4 * 4, 15 * 4 ; 35
    raw_palette_comb 19 * 4, 23 * 4 - 1, 9 * 4 ; 36
    palette_comb 16, 28, 10 ; 37
    palette_comb  4, 23, 28 ; 38
    palette_comb 17, 22,  2 ; 39
    palette_comb  4,  0,  2 ; 40, Left + A
    palette_comb  4, 28,  3 ; 41
    palette_comb 28,  3,  0 ; 42
    palette_comb  3, 28,  4 ; 43, Up + A
    palette_comb 21, 28,  4 ; 44
    palette_comb  3, 28,  0 ; 45
    palette_comb 25,  3, 28 ; 46
    palette_comb  0, 28,  8 ; 47
    palette_comb  4,  3, 28 ; 48, Left
    palette_comb 28,  3,  6 ; 49, Down + B
    palette_comb  4, 28, 29 ; 50
    ; SameBoy "Exclusives"
    palette_comb 30, 30, 30 ; 51, Right + A + B, CGA
    palette_comb 31, 31, 31 ; 52, Left + A + B, DMG LCD
    palette_comb 28,  4,  1 ; 53, Up + A + B
    palette_comb  0,  0,  2 ; 54, Down + A + B

Palettes:
    dw $7FFF, $32BF, $00D0, $0000 ;  0
    dw $639F, $4279, $15B0, $04CB ;  1
    dw $7FFF, $6E31, $454A, $0000 ;  2
    dw $7FFF, $1BEF, $0200, $0000 ;  3
    dw $7FFF, $421F, $1CF2, $0000 ;  4
    dw $7FFF, $5294, $294A, $0000 ;  5
    dw $7FFF, $03FF, $012F, $0000 ;  6
    dw $7FFF, $03EF, $01D6, $0000 ;  7
    dw $7FFF, $42B5, $3DC8, $0000 ;  8
    dw $7E74, $03FF, $0180, $0000 ;  9
    dw $67FF, $77AC, $1A13, $2D6B ; 10
    dw $7ED6, $4BFF, $2175, $0000 ; 11
    dw $53FF, $4A5F, $7E52, $0000 ; 12
    dw $4FFF, $7ED2, $3A4C, $1CE0 ; 13
    dw $03ED, $7FFF, $255F, $0000 ; 14
    dw $036A, $021F, $03FF, $7FFF ; 15
    dw $7FFF, $01DF, $0112, $0000 ; 16
    dw $231F, $035F, $00F2, $0009 ; 17
    dw $7FFF, $03EA, $011F, $0000 ; 18
    dw $299F, $001A, $000C, $0000 ; 19
    dw $7FFF, $027F, $001F, $0000 ; 20
    dw $7FFF, $03E0, $0206, $0120 ; 21
    dw $7FFF, $7EEB, $001F, $7C00 ; 22
    dw $7FFF, $3FFF, $7E00, $001F ; 23
    dw $7FFF, $03FF, $001F, $0000 ; 24
    dw $03FF, $001F, $000C, $0000 ; 25
    dw $7FFF, $033F, $0193, $0000 ; 26
    dw $0000, $4200, $037F, $7FFF ; 27
    dw $7FFF, $7E8C, $7C00, $0000 ; 28
    dw $7FFF, $1BEF, $6180, $0000 ; 29
    ; SameBoy "Exclusives"
    dw $7FFF, $7FEA, $7D5F, $0000 ; 30, CGA 1
    dw $4778, $3290, $1D87, $0861 ; 31, DMG LCD

KeyCombinationPalettes:
MACRO palette_comb_id ; PaletteCombinations ID
    db (\1) * 3
ENDM
    palette_comb_id  1 ;  1, Right
    palette_comb_id 48 ;  2, Left
    palette_comb_id  5 ;  3, Up
    palette_comb_id  8 ;  4, Down
    palette_comb_id  0 ;  5, Right + A
    palette_comb_id 40 ;  6, Left + A
    palette_comb_id 43 ;  7, Up + A
    palette_comb_id  3 ;  8, Down + A
    palette_comb_id  6 ;  9, Right + B
    palette_comb_id  7 ; 10, Left + B
    palette_comb_id 28 ; 11, Up + B
    palette_comb_id 49 ; 12, Down + B
    ; SameBoy "Exclusives"
    palette_comb_id 51 ; 13, Right + A + B
    palette_comb_id 52 ; 14, Left + A + B
    palette_comb_id 53 ; 15, Up + A + B
    palette_comb_id 54 ; 16, Down + A + B

TrademarkSymbol:
    pusho
    opt b.X
    db %..XXXX..
    db %.X....X.
    db %X.XXX..X
    db %X.X..X.X
    db %X.XXX..X
    db %X.X..X.X
    db %.X....X.
    db %..XXXX..
    popo
TrademarkSymbolEnd:

    ; SameBoy's logo was here
    pad_to $059A


AnimationColors:
    dw $7FFF ; White
    dw $774F ; Cyan
    dw $7C1F ; Magenta
    dw $039F ; Yellow
    dw $017D ; Orange
    dw $241D ; Red
    dw $6D38 ; Purple
    dw ACCENT ; Nubo's accent purple

AnimationColorsEnd:

; Helper Functions
DoubleBitsAndWriteRowTwice:
    call .twice
.twice
; Double the most significant 4 bits, b is shifted by 4
    ld a, 4
    ld c, 0
.doubleCurrentBit
    sla b
    push af
    rl c
    pop af
    rl c
    dec a
    jr nz, .doubleCurrentBit
    ld a, c
; Write as two rows
    ldi [hl], a
    inc hl
    ldi [hl], a
    inc hl
    ret

WaitFrame:
    push hl
    ld hl, rIF
    res 0, [hl]
.wait
    bit 0, [hl]
    jr z, .wait
    pop hl
    ret

WaitBFrames:
    call GetInputPaletteIndex
    call WaitFrame
    dec b
    jr nz, WaitBFrames
    ret

PlaySound:
    ldh [rNR13], a
    ld a, $87
    ldh [rNR14], a
    ret

ClearMemoryVRAM:
    ld hl, _VRAM
; Clear from HL to HL | 0x2000
ClearMemoryPage:
    xor a
    ldi [hl], a
    bit 5, h
    jr z, ClearMemoryPage
    ret

    ; SameBoy's logo decompressor was here
    pad_to $0667

ReadTrademarkSymbol:
    ld de, TrademarkSymbol
    ld c, TrademarkSymbolEnd - TrademarkSymbol
.loadTrademarkSymbolLoop:
    ld a, [de]
    inc de
    ldi [hl], a
    inc hl
    dec c
    jr nz, .loadTrademarkSymbolLoop
    ret

    ; SameBoy's intro animation and the fade at the start of Preboot were here
    pad_to $06E1

; The end of SameBoy's Preboot, after its fade; NuboIntro rejoins it here.
Preboot:
    ld a, 2
    ldh [rSVBK], a
    ; Clear RAM Bank 2 (Like the original boot ROM)
    ld hl, _RAMBANK
    call ClearMemoryPage
    inc a
    call ClearVRAMViaHDMA
    call _ClearVRAMViaHDMA
    call ClearVRAMViaHDMA ; A = $40, so it's bank 0
    xor a
    ldh [rSVBK], a
    cpl
    ldh [rJOYP], a

    ; Final values for CGB mode
    ld d, a
    ld e, c
    ld l, $0D

    ld a, [CGBFlag]
    bit 7, a
    call z, EmulateDMG
    bit 7, a

    ldh [rKEY0], a ; write CGB compatibility byte, CGB mode
    ldh a, [hTitleChecksum]
    ld b, a

    jr z, .skipDMGForCGBCheck
    ldh a, [hInputPalette]
    and a
    jr nz, .emulateDMGForCGBGame
.skipDMGForCGBCheck
IF DEF(AGB)
    ; Set registers to match the original AGB-CGB boot
    ; AF = $1100, C = 0
    xor a
    ld c, a
    add a, BOOTUP_A_CGB
    ld h, c
    ; B is set to BOOTUP_B_AGB (1) after ret
ELSE
    ; Set registers to match the original CGB boot
    ; AF = $1180, C = 0
    xor a
    ld c, a
    ld a, BOOTUP_A_CGB
    ld h, c
    ; B is set to the title checksum (BOOTUP_B_CGB, 0)
ENDC
    ret

.emulateDMGForCGBGame
    call EmulateDMG
    ldh [rKEY0], a ; write $04, DMG emulation mode
    ld a, $1
    ret

GetKeyComboPalette:
    ld hl, KeyCombinationPalettes - 1 ; Return value is 1-based, 0 means nothing down
    ld c, a
    ld b, 0
    add hl, bc
    ld a, [hl]
    ret

EmulateDMG:
    ld a, 1
    ldh [rOPRI], a ; DMG Emulation sprite priority
    call GetPaletteIndex
    bit 7, a
    call nz, LoadDMGTilemap
    res 7, a
    ld b, a
    add b
    add b
    ld b, a
    ldh a, [hInputPalette]
    and a
    jr z, .nothingDown
    call GetKeyComboPalette
    jr .paletteFromKeys
.nothingDown
    ld a, b
.paletteFromKeys
    call WaitFrame
    call LoadPalettesFromIndex
    ld a, 4
    ; Set the final values for DMG mode
    ld de, 8
    ld l, $7C
    ret

GetPaletteIndex:
    ld hl, OldLicenseeCode
    ld a, [hl]
    cp $33
    jr z, .newLicensee
    dec a ; 1 = Nintendo
    jr nz, .notNintendo
    jr .doChecksum
.newLicensee
    ld l, LOW(NewLicenseeCode)
    ld a, [hli]
    cp "0"
    jr nz, .notNintendo
    ld a, [hl]
    cp "1"
    jr nz, .notNintendo

.doChecksum
    ld l, LOW(Title)
    ld c, 16
    xor a
.checksumLoop
    add [hl]
    inc l
    dec c
    jr nz, .checksumLoop
    ldh [hTitleChecksum], a
    ld b, a

    ; c = 0
    ld hl, TitleChecksums

.searchLoop
    ld a, l
    sub LOW(ChecksumsEnd) ; use sub to zero out a
    ret z
    ld a, [hli]
    cp b
    jr nz, .searchLoop

    ; We might have a match, Do duplicate/4th letter check
    ld a, l
    sub FirstChecksumWithDuplicate - TitleChecksums + 1
    jr c, .match ; Does not have a duplicate, must be a match!
    ; Has a duplicate; check 4th letter
    push hl
    ld a, l
    add Dups4thLetterArray - FirstChecksumWithDuplicate - 1 ; -1 since hl was incremented
    ld l, a
    ld a, [hl]
    pop hl
    ld c, a
    ld a, [Title + 3] ; Get 4th letter
    cp c
    jr nz, .searchLoop ; Not a match, continue

.match
    ld a, l
    add PalettePerChecksum - TitleChecksums - 1; -1 since hl was incremented
    ld l, a
    ld a, b
    ldh [hTitleChecksum], a
    ld a, [hl]
    ret

.notNintendo
    xor a
    ret

; optimizations in callers rely on this returning with b = 0
GetPaletteCombo:
    ld hl, PaletteCombinations
    ld b, 0
    ld c, a
    add hl, bc
    ret

LoadPalettesFromIndex: ; a = index of combination
    call GetPaletteCombo

    ; Obj Palettes
    ld e, 0
.loadObjPalette
    ld a, [hli]
    push hl
    ld hl, Palettes
    ; b is already 0
    ld c, a
    add hl, bc
    ld d, 4 * 2
    ld c, LOW(rOBPI)
    call LoadPalettes
    pop hl
    bit OAMB_BANK1, e
    jr nz, .loadBGPalette
    ld e, OAMF_BANK1
    jr .loadObjPalette
.loadBGPalette
    ;BG Palette
    ld c, [hl]
    ; b is already 0
    ld hl, Palettes
    add hl, bc
    ld d, 8
    jr LoadBGPalettes

LoadPalettesFromHRAM:
    ld hl, hBgPalettes
    ld d, hBgPalettesEnd - hBgPalettes
LoadBGPalettes:
    ld e, 0
    ld c, LOW(rBGPI)
LoadPalettes:
    ld a, $80
    or e
    ldh [c], a
    inc c
.loop
    ld a, [hli]
    ldh [c], a
    dec d
    jr nz, .loop
    ret

ClearVRAMViaHDMA:
    ldh [rVBK], a
    ld hl, HDMAData
_ClearVRAMViaHDMA:
    call WaitFrame ; Wait for vblank
    ld c, LOW(rHDMA1)
    ld b, 5
.loop
    ld a, [hli]
    ldh [c], a
    inc c
    dec b
    jr nz, .loop
    ret

; clobbers AF and HL
GetInputPaletteIndex:
    ld a, P1F_GET_DPAD
    ldh [rJOYP], a
    ldh a, [rJOYP]
    cpl
    and $F
    ret z ; No direction keys pressed, no palette

    ld l, 0
.directionLoop
    inc l
    rra
    jr nc, .directionLoop

    ; c = 1: Right, 2: Left, 3: Up, 4: Down

    ld a, P1F_GET_BTN
    ldh [rJOYP], a
    ldh a, [rJOYP]
    cpl
    rla
    rla
    and $C
    add l
    ld l, a
    ldh a, [hInputPalette]
    cp l
    ret z ; No change, don't load
    ld a, l
    ldh [hInputPalette], a
    ; Slide into change Animation Palette

ChangeAnimationPalette:
    push bc
    push de
    call GetKeyComboPalette
    call GetPaletteCombo
    inc l
    inc l
    ld c, [hl]
    ld hl, Palettes + 1
    add hl, bc
    ld a, [hld]
    cp $7F ; Is white color?
    jr nz, .isWhite
    inc hl
    inc hl
.isWhite
    push af
    ld a, [hli]

    push hl
    ld hl, hBgPalettes ; First color, all palettes
    call ReplaceColorInAllPalettes
    ld l, LOW(hBgPalettes + 2)  ; Second color, all palettes
    call ReplaceColorInAllPalettes
    pop hl
    ldh [hBgPalettes + 6], a ; Fourth color, first palette

    ld a, [hli]
    push hl
    ld hl, hBgPalettes + 1 ; First color, all palettes
    call ReplaceColorInAllPalettes
    ld l, LOW(hBgPalettes + 3) ; Second color, all palettes
    call ReplaceColorInAllPalettes
    pop hl
    ldh [hBgPalettes + 7], a ; Fourth color, first palette

    pop af
    jr z, .isNotWhite
    inc hl
    inc hl
.isNotWhite
    ; Mixing code by ISSOtm
    ldh a, [hBgPalettes + 7 * 8 + 2]
    and ~$21
    ld b, a
    ld a, [hli]
    and ~$21
    add a, b
    ld b, a
    ld a, [hBgPalettes + 7 * 8 + 3]
    res 2, a ; and ~$04, but not touching carry
    ld c, [hl]
    res 2, c ; and ~$04, but not touching carry
    adc a, c
    rra ; Carry sort of "extends" the accumulator, we're bringing that bit back home
    ld [hBgPalettes + 7 * 8 + 3], a
    ld a, b
    rra
    ld [hBgPalettes + 7 * 8 + 2], a
    dec l

    ld a, [hli]
    ldh [hBgPalettes + 7 * 8 + 6], a ; Fourth color, 7th palette
    ld a, [hli]
    ldh [hBgPalettes + 7 * 8 + 7], a ; Fourth color, 7th palette

    ld a, [hli]
    ldh [hBgPalettes + 4], a ; Third color, first palette
    ld a, [hli]
    ldh [hBgPalettes + 5], a ; Third color, first palette


    call WaitFrame
    call LoadPalettesFromHRAM
    ; Delay the wait loop while the user is selecting a palette
    ld a, 48
    ldh [hWaitLoopCounter], a
    pop de
    pop bc
    ret

ReplaceColorInAllPalettes:
    ld de, 8
    ld c, e
.loop
    ld [hl], a
    add hl, de
    dec c
    jr nz, .loop
    ret

LoadDMGTilemap:
    push af
    call WaitFrame
    ld a, $19                           ; Trademark symbol tile ID
    ld [_SCRN0 + 8 * SCRN_VX_B + 16], a ; ... put in the superscript position
    ld hl, _SCRN0 + 9 * SCRN_VX_B + 15  ; Bottom right corner of the logo
    ld c, 12                            ; Tiles in a logo row
.tilemapLoop
    dec a
    jr z, .tilemapDone
    ldd [hl], a
    dec c
    jr nz, .tilemapLoop
    ld l, $0F ; Jump to top row
    jr .tilemapLoop
.tilemapDone
    pop af
    ret

BootEnd:
IF BootEnd > $0900
    FAIL "BootROM overflowed: {BootEnd}"
ENDC
    ds $100 + $800 - @ ; Ensure that the ROM is padded up to standard size.

; Nubo's part of the boot ROM, where real CGBs have none (see the top)
SECTION "Nubo", ROM0[$0900]

; Loads Nubo's tiles and draws the first frame while the LCD is still off: the
; Nintendo logo where the DMG boot ROM shows it, the name below it (white for
; now), the background palettes and the cloud's sprites. Expects VRAM bank 1.
NuboSetup:
    ; Nubo's tiles go to VRAM bank 1, in the high bit plane only, so their
    ; pixels use color 2. Each byte holds four rows at half size, which
    ; DoubleBitsAndWriteRowTwice doubles in both directions, the same way the
    ; Nintendo logo is loaded.
    ld de, NuboTileNibbles
    ld hl, _VRAM + NAME_TILE * 16 + 1
.tiles
    ld a, [de]
    ld b, a
    call DoubleBitsAndWriteRowTwice
    inc de
    ld a, e
    cp LOW(NuboTileNibblesEnd)
    jr nz, .tiles

    ; The name's attributes: tiles from bank 1, palette 0, which keeps it
    ; white until the sweep reaches it
    ld a, ATTR_BANK1
    ld hl, NAME_MAP_ROW_1
    ld c, NAME_COLUMNS
    call FillRow
    ld l, LOW(NAME_MAP_ROW_2)
    ld c, NAME_COLUMNS
    call FillRow

    ; The tile numbers, in bank 0
    xor a
    ldh [rVBK], a
    ld hl, LOGO_MAP_ROW_1
    lb bc, 1, LOGO_COLUMNS
    call MapRow
    ld [hl], TRADEMARK_TILE
    ld l, LOW(LOGO_MAP_ROW_2)
    ld c, LOGO_COLUMNS
    call MapRow
    ld hl, NAME_MAP_ROW_1
    lb bc, NAME_TILE, NAME_COLUMNS
    call MapRow
    ld l, LOW(NAME_MAP_ROW_2)
    ld c, NAME_COLUMNS
    call MapRow

    ; Background palette n: white, the Nintendo logo's color (white until it
    ; fades in, and only palette 0 shows it), animation color n, black
    ld de, AnimationColors
    ld hl, hBgPalettes
.palettes
    ld b, 2
.white
    ld a, $FF
    ldi [hl], a
    ld a, $7F
    ldi [hl], a
    dec b
    jr nz, .white
    ld a, [de]
    inc de
    ldi [hl], a
    ld a, [de]
    inc de
    ldi [hl], a
    xor a
    ldi [hl], a
    ldi [hl], a
    ld a, l
    cp LOW(hBgPalettesEnd)
    jr nz, .palettes
    call LoadPalettesFromHRAM
    ; fallthrough

; Sets up the cloud's 3x3 sprites: tiles from VRAM bank 1 and OBJ palette 0,
; whose color 2 is the accent purple. They stay out of sight (Y = 0) until the
; cloud flies in.
SetupSprites:
    ld a, OBPIF_AUTOINC | 2 * 2
    ldh [rOBPI], a
    ld a, LOW(ACCENT)
    ldh [rOBPD], a
    ld a, HIGH(ACCENT)
    ldh [rOBPD], a
    ld a, CLOUD_START_X
    ldh [hCloudX], a
    ld hl, _OAMRAM + OAMA_TILEID
    ld a, CLOUD_TILE
.sprites
    ldi [hl], a
    ld [hl], OAMF_BANK1
    inc l
    inc l
    inc l
    inc a
    cp CLOUD_TILE + CLOUD_TILES
    jr nz, .sprites
    ret

; Runs Nubo's animation from the moment the LCD is on until the cartridge
; takes over, one frame at a time, with the drawing done in the vertical blank.
; It keeps the previous version's timing (see the top of this file): the input
; palette is polled before the same frames, the sounds start on the same
; cycles and Finish rejoins Preboot on the same cycle.
NuboIntro:
    ld a, 1                     ; VRAM bank 1: tile map writes set attributes
    ldh [rVBK], a
    ld a, LCDCF_ON | LCDCF_BLK01 | LCDCF_OBJON | LCDCF_BGON
    ldh [rLCDC], a
.frame
    ; Like SameBoy's WaitBFrames and wait loop, let the player pick a DMG
    ; palette with the D-pad (this may also wait a frame)
    ldh a, [hPhase]
    cp PHASE_HOLD
    call c, GetInputPaletteIndex

    ldh a, [hFrame]
    cp SOUND_1_FRAME - 1
    jr z, .sound1
    cp SOUND_2_FRAME - 1
    jr z, .sound2
    call WaitFrame

.nextFrame
    ld hl, hFrame
    inc [hl]
    ldh a, [hPhase]
    cp PHASE_HOLD
    jr nz, .animate
    ld hl, hHold
    dec [hl]
    jp z, Finish                ; frame 172: end
    ld a, [hl]
    dec a
    jp z, HideCloud             ; frame 171: remove the cloud
.animate
    call AnimateName
    call AnimateCloud
    ldh a, [hPhase]
    cp PHASE_WAIT
    jr nz, .frame
    ld hl, hWaitLoopCounter
    dec [hl]
    jr nz, .frame
    ld a, PHASE_HOLD
    ldh [hPhase], a
    ld a, HOLD_FRAMES
    ldh [hHold], a
    jr .frame

    ; The frames that end with a sound: wait for the vertical blank with the
    ; same timing as SameBoy's WaitBFrames, then start the sound
.sound1
    REPT SOUND_1_PRE_NOPS
        nop
    ENDR
    ld b, $83
    jr .sound
.sound2
    REPT SOUND_2_PRE_NOPS
        nop
    ENDR
    ld b, $C1
.sound
    call WaitFrame
    wait_cycles SOUND_POST_CYCLES
    ld a, b
    call PlaySound
    ld a, b
    cp $C1
    jr nz, .nextFrame
    ; After the second sound, wait like SameBoy's wait loop. It waits 48
    ; frames after this one, and this frame is counted too.
    ld a, PHASE_WAIT
    ldh [hPhase], a
    ld a, 48 + 1
    ldh [hWaitLoopCounter], a
    jr .nextFrame

; Moves the Nintendo logo and the name on by one frame, in the vertical blank.
AnimateName:
    ; Fade the Nintendo logo in like the DMG boot ROM does: light gray, dark
    ; gray, then black (LogoFade). It is color 1 of background palette 0, and
    ; is kept in hBgPalettes too, where SameBoy's palette code reads it.
    ldh a, [hFrame]
    ld c, a
    ld hl, LogoFade
.fadeStep
    ld a, [hli]
    ld e, [hl]
    inc hl
    ld d, [hl]
    inc hl
    cp c
    jr z, .logoColor
    cp FADE_3_FRAME
    jr nz, .fadeStep
    jr .sweep
.logoColor
    ld hl, hBgPalettes + 1 * 2
    ld [hl], e
    inc l
    ld [hl], d
    ld a, BGPIF_AUTOINC | 1 * 2
    ldh [rBGPI], a
    ld a, e
    ldh [rBGPD], a
    ld a, d
    ldh [rBGPD], a

.sweep
    ; The rainbow sweep, from the frame the cloud arrives: every two frames
    ; each of the name's columns moves on to its next palette (the next
    ; animation color), each column one step behind the one to its left,
    ; until all of them show palette 7, the accent purple
    ldh a, [hFrame]
    sub SWEEP_FRAME
    ret c
    srl a
    inc a                       ; the step: 1, 1, 2, 2, 3, ...
    cp NAME_COLUMNS + 8
    ret nc                      ; every column has reached palette 7
    ld b, a
    ld hl, NAME_MAP_ROW_1
.column
    ; this column's palette: the step minus the column number, kept in 0-7
    ld a, b
    bit 7, a
    jr z, .started
    xor a
.started
    cp 8
    jr c, .palette
    ld a, 7
.palette
    or ATTR_BANK1
    ld [hl], a
    set 5, l                    ; the same column in the second row
    ld [hl], a
    res 5, l
    inc l
    dec b
    ld a, l
    cp LOW(NAME_MAP_ROW_1 + NAME_COLUMNS)
    jr nz, .column
    ret

; Moves the cloud on by one frame, in the vertical blank: it flies in, then
; bobs, and it blinks twice.
AnimateCloud:
    ldh a, [hFrame]
    sub FLIGHT_START
    ret c                       ; not on screen yet
    cp FLIGHT_END - FLIGHT_START
    jr nc, .bob
    ; flying in: CloudFlight says how far it moves right (high nibble) and
    ; its height (low nibble, 8 = at rest)
    ld e, a
    ld d, 0
    ld hl, CloudFlight
    add hl, de
    ld a, [hl]
    and $0F
    add CLOUD_REST_Y - 8
    ld c, a
    ld a, [hl]
    swap a
    and $0F
    ld hl, hCloudX
    add [hl]
    ld [hl], a
    ld b, a
    jr .place
.bob
    ; bobbing: X stays, Y follows a cycle of BOB_FRAMES frames
    ld hl, hBob
    ld a, [hl]
    inc a
    cp BOB_FRAMES
    jr c, .bobIndex
    xor a
.bobIndex
    ld [hl], a
    ld e, a
    ld d, 0
    ld hl, CloudBobY
    add hl, de
    ld c, [hl]
    ld b, CLOUD_REST_X
.place
    ; the 3x3 sprites, the top left one at OAM X = B and OAM Y = C
    ld hl, _OAMRAM
    ld e, 3
.row
    push bc
    ld d, 3
.column
    ld a, c
    ldi [hl], a                 ; Y
    ld a, b
    ldi [hl], a                 ; X
    add 8
    ld b, a
    inc l                       ; skip the tile and attributes
    inc l
    dec d
    jr nz, .column
    pop bc
    ld a, c
    add 8
    ld c, a
    dec e
    jr nz, .row

    ; Blink twice: the middle sprite of the second row shows closed eyes
    ldh a, [hFrame]
    ld b, CLOSED_EYES_TILE
    sub BLINK_1_FRAME
    cp BLINK_FRAMES
    jr c, .eyes
    sub BLINK_2_FRAME - BLINK_1_FRAME
    cp BLINK_FRAMES
    jr c, .eyes
    ld b, CLOUD_TILE + EYES_SPRITE
.eyes
    ld a, b
    ld [_OAMRAM + EYES_SPRITE * sizeof_OAM_ATTRS + OAMA_TILEID], a
    ret

; Removes the cloud a frame before the end: sprites off, and OAM and the
; sprite palette back to how the previous version left them (all zero).
HideCloud:
    ld a, LCDCF_ON | LCDCF_BLK01 | LCDCF_BGON
    ldh [rLCDC], a
    xor a
    ld hl, _OAMRAM
    ld c, CLOUD_TILES * sizeof_OAM_ATTRS
    call FillRow
    ld a, OBPIF_AUTOINC | 2 * 2
    ldh [rOBPI], a
    xor a
    ldh [rOBPD], a
    ldh [rOBPD], a
    ldh [rOBPI], a
    jp NuboIntro.frame

; Ends the animation where the previous version's fade ended: turns every
; background palette white as that fade left them, puts the stack back the way
; it was, waits for the vertical blank and rejoins Preboot on the same cycle
; with the same registers.
Finish:
    ; Nubo's variables back to 0, as the previous version never used them
    xor a
    ld hl, hFrame
    ld c, hNuboEnd - hFrame
    call FillRow

    ; White: $FFFF for color 0 and $7FFF for the others
    ld hl, hBgPalettes
.white
    ld a, $FF
    ldi [hl], a
    ldi [hl], a
    ld b, 3
.color
    ldi [hl], a
    ld [hl], $7F
    inc l
    dec b
    jr nz, .color
    ld a, l
    cp LOW(hBgPalettesEnd)
    jr nz, .white
    call LoadPalettesFromHRAM

    ; The stack, with Preboot's return address on top
    ld de, StackLeftovers
    ld hl, STACK_LEFTOVERS
.leftovers
    ld a, [de]
    inc de
    ldi [hl], a
    ld a, l
    cp LOW(STACK_LEFTOVERS + STACK_LEFTOVERS_SIZE)
    jr nz, .leftovers
    ld sp, $FFFC

    ; The registers Preboot had after its last WaitFrame: A = $7F, B = 0,
    ; C = LOW(rBGPD), DE = 0, HL = hBgPalettesEnd, Z and N set
    ld de, 0
    ld bc, $0100 | LOW(rBGPD)   ; B = 1 for the DEC below
    ld a, $7F
    and a                       ; carry clear
    ; Wait for the vertical blank like WaitFrame does, without the stack
    ld hl, rIF
    res 0, [hl]
    REPT FINISH_PRE_NOPS
        nop
    ENDR
.wait
    bit 0, [hl]
    jr z, .wait
    ld l, LOW(hBgPalettesEnd)
    REPT FINISH_POST_NOPS
        nop
    ENDR
    dec b
    jp Preboot

; Writes the tile numbers B, B + 1, ... to C bytes from HL on.
MapRow:
    ld a, b
    ldi [hl], a
    inc b
    dec c
    jr nz, MapRow
    ret

; Writes A to C bytes from HL on.
FillRow:
    ldi [hl], a
    dec c
    jr nz, FillRow
    ret

; The tiles and the cloud's movement, generated by gen_data.py
    INCLUDE "nubo_cgb_data.inc"

; The frames the Nintendo logo changes color on, and its colors
LogoFade:
    db FADE_1_FRAME
    dw LIGHT_GRAY
    db FADE_2_FRAME
    dw DARK_GRAY
    db FADE_3_FRAME
    dw BLACK

; What the previous version had on the stack at $FFF4-$FFFD when it reached
; the end of its fade. Finish puts it back so HRAM ends up the same.
StackLeftovers:
    dw $05EC                    ; a return address in its logo decompressor
    dw $9920                    ; HL pushed by WaitFrame during its intro
    dw hBgPalettesEnd           ; HL pushed by its last WaitFrame
    dw $06DE                    ; where its last WaitFrame returned to
    dw ReturnFromPreboot        ; Preboot's return address
    ASSERT @ - StackLeftovers == STACK_LEFTOVERS_SIZE

NuboEnd:
    ASSERT NuboEnd <= $0C00, "Nubo's part overflows $0BFF: {NuboEnd}"

SECTION "HRAM", HRAM[_HRAM]
hTitleChecksum:
    ds 1
hBgPalettes:
    ds 8 * 4 * 2
hBgPalettesEnd:
hInputPalette:
    ds 1
hWaitLoopCounter:
    ds 1
; Nubo's variables, back to 0 when Finish rejoins Preboot
hFrame:                         ; frames since the LCD was switched on
    ds 1
hPhase:                         ; PHASE_INTRO, PHASE_WAIT or PHASE_HOLD
    ds 1
hHold:                          ; frames of the hold left
    ds 1
hBob:                           ; where the cloud is in its bob
    ds 1
hCloudX:                        ; the cloud's OAM X while it flies in
    ds 1
hNuboEnd:
