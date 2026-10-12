#!/bin/zsh
# Builds Nubo's boot ROMs with RGBDS 1.0 and writes them into ../src/bios.rs
# as DMG_BOOT and CGB_BOOT. See README.md.
set -e
cd "${0:A:h}"
mkdir -p build
python3 -I gen_data.py build >/dev/null

# Assembles $1.asm and pads the result with zeros to $2 bytes
build() {
    rgbasm -Wno-obsolete -I build -o build/$1.o $1.asm
    rgblink -x -p 0 -n build/$1.sym -o build/$1.bin build/$1.o
    python3 -I -c "import sys; b = open(sys.argv[1], 'rb').read(); assert len(b) <= $2, len(b); open(sys.argv[1], 'wb').write(b.ljust($2, b'\0'))" build/$1.bin
}
build nubo_dmg 0x900
build nubo_cgb 0xC00

python3 -I write_rom.py ../src/bios.rs DMG_BOOT build/nubo_dmg.bin
python3 -I write_rom.py ../src/bios.rs CGB_BOOT build/nubo_cgb.bin
cargo fmt --all
