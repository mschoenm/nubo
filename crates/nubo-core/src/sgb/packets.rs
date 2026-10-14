/*
    The packet protocol (which JOYP writes start, send and end a packet, and which commands are
    accepted) is ported from SameBoy's sgb.c (https://github.com/LIJI32/SameBoy, Expat License,
    Copyright (c) 2015-2026 Lior Halphon; see the notice in apu.rs).
*/

//! How a game sends commands to the Super Game Boy: one bit at a time,
//! through the joypad register JOYP (0xFF00).
//!
//! The two group select bits of JOYP (bit 4 for the directions and bit 5 for
//! the action buttons) are wired to the Super Nintendo, which watches them:
//!
//! - Writing both low (0x00) and then both high (0x30) starts a packet.
//! - Each bit follows as one line pulled low and then both high again: 0x10
//!   then 0x30 sends a 1, 0x20 then 0x30 sends a 0, lowest bit of each byte
//!   first.
//! - After 16 bytes (128 bits) comes one more pulse, the stop bit, which ends
//!   the packet. Its value isn't checked (CasualPokePlayer's `sgb-ext-test`).
//!
//! A command is one to seven packets: the low three bits of its first byte say
//! how many, and the other five bits which command it is.

use nubo_serializable_derive::Serializable;

/// The bytes in one packet.
pub(super) const PACKET_SIZE: usize = 16;
/// The most packets a command can have.
const MAX_PACKETS: usize = 7;
/// The most bytes a command can have.
pub(super) const MAX_COMMAND_SIZE: usize = PACKET_SIZE * MAX_PACKETS;
/// The bits in one packet.
const PACKET_BITS: u16 = (PACKET_SIZE * 8) as u16;

/// Collects the bits of a command as the game sends them.
#[derive(Clone, Debug, Serializable)]
pub(super) struct PacketReceiver {
    /// The command received so far, zero where no bit has arrived yet.
    command: [u8; MAX_COMMAND_SIZE],
    /// How many bits of `command` have arrived.
    bits_received: u16,
    /// Set by the pulse that starts a packet (0x00 then 0x30); a bit only
    /// counts while it is.
    ready_for_bit: bool,
    /// Set once a whole packet has arrived, when the next pulse is its stop
    /// bit.
    ready_for_stop: bool,
}

impl PacketReceiver {
    pub(super) fn new() -> Self {
        PacketReceiver {
            command: [0; MAX_COMMAND_SIZE],
            bits_received: 0,
            ready_for_bit: false,
            ready_for_stop: false,
        }
    }

    /// How many bits the command being received has: as many packets as its
    /// first byte says, and at least one. The packets in which the boot ROM
    /// sends the cartridge header (first byte 0b1111xxx1) are always one
    /// packet long.
    fn command_bits(&self) -> u16 {
        let packets = if is_header_packet(self.command[0]) {
            1
        } else {
            u16::from(self.command[0] & 0x07).max(1)
        };
        packets * PACKET_BITS
    }

    /// Sees a write to JOYP's group select bits, `previous_select` being what
    /// they were before. Returns the command that write finished, if any.
    pub(super) fn receive(
        &mut self,
        previous_select: u8,
        value: u8,
    ) -> Option<[u8; MAX_COMMAND_SIZE]> {
        let previous = (previous_select >> 4) & 0x03;
        match (value >> 4) & 0x03 {
            0x03 => match previous {
                0x00 => self.ready_for_bit = true,
                0x01 | 0x02 if self.ready_for_bit => {
                    if self.ready_for_stop {
                        let finished = self.bits_received == self.command_bits();
                        let command = finished.then_some(self.command);
                        if finished {
                            self.clear_command();
                        }
                        self.ready_for_bit = false;
                        self.ready_for_stop = false;
                        return command;
                    }
                    self.receive_bit(previous == 0x01);
                }
                _ => {}
            },
            // Both lines low: the start of a packet. A command whose packets
            // didn't all arrive, or one that just ended, starts over.
            0x00 => {
                self.ready_for_bit = false;
                if !self.bits_received.is_multiple_of(PACKET_BITS)
                    || self.bits_received == 0
                    || self.ready_for_stop
                {
                    self.clear_command();
                    self.ready_for_stop = false;
                }
            }
            // One line low: the first half of a bit, which counts once both
            // lines are high again.
            _ => {}
        }
        None
    }

    /// Stores one bit of the command, as long as it fits.
    fn receive_bit(&mut self, bit: bool) {
        let index = usize::from(self.bits_received);
        if index >= MAX_COMMAND_SIZE * 8 {
            return;
        }
        if bit {
            self.command[index / 8] |= 1 << (index % 8);
        }
        self.bits_received += 1;
        if self.bits_received.is_multiple_of(PACKET_BITS) {
            self.ready_for_stop = true;
        }
    }

    /// Forgets the command received so far.
    fn clear_command(&mut self) {
        self.command = [0; MAX_COMMAND_SIZE];
        self.bits_received = 0;
    }
}

/// Whether a command whose first byte is `first_byte` is one of the packets in
/// which the boot ROM sends the cartridge header to the Super Nintendo. Nubo's
/// boot ROM doesn't send them (the Super Game Boy reads the header itself when
/// the boot ROM exits, see `Sgb::start`), and once the game runs the Super
/// Game Boy ignores them.
pub(super) fn is_header_packet(first_byte: u8) -> bool {
    first_byte & 0xF1 == 0xF1
}
