# Kept save states

Every later version of Nubo must load these save states, so that an update
never costs a player their save states. Each folder is a set of states made by
Nubo on the day it is named after, with `layout.txt`: every field that
version's save states store with how its value is stored (such as
`ppu.lcdc: u8`), and every variant each enum value can be stored as. A new
set is only made when that list changes: `cargo test --workspace` and `cargo
save-states` (see `tools/nubo-save-state-check`) fail when the current version
stores other fields or variants than the newest set lists, and `cargo
save-states --record` then adds a set. Never change or remove a set once it is
made.

The states are of the test cartridges `tools/nubo-save-state-check` builds,
one for each kind of memory bank controller, and of three free homebrew games,
20 emulated seconds in, which `cargo xtask benchmark-roms` downloads. The
states hold the games' graphics and memory as the games left them:

- [Tobu Tobu Girl Deluxe](https://tangramgames.itch.io/tobu-tobu-girl-deluxe)
  by Tangram Games (`tobudx.gb`): code under the MIT License, graphics under
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
- [µCity](https://github.com/AntonioND/ucity) (`ucity.gbc`) and
  [Geometrix](https://github.com/AntonioND/geometrix) (`geometrix.gbc`) by
  Antonio Niño Díaz, under the GNU General Public License, version 3 or later.
