# Writes a boot ROM binary into the array of a constant in bios.rs, updating
# the array's length.
# Usage: python3 write_rom.py <bios.rs> <constant> <rom.bin>
import re
import sys

path, name, binp = sys.argv[1:4]
src = open(path).read()
rom = open(binp, 'rb').read()
match = re.search(rf'pub const {name}: \[u8; \d+\] = \[', src)
start, end = match.end(), src.index('];', match.end())
src = (src[:match.start()] + f'pub const {name}: [u8; {len(rom)}] = [\n    '
       + ', '.join(map(str, rom)) + ',\n' + src[end:])
open(path, 'w').write(src)
