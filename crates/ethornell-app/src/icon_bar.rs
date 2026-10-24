//! Save, load, quick save and quick load for a host menu, for games whose
//! commands live only on their own script-drawn icon bar. The host clicks
//! the icon, so the game runs its usual flow (screens, confirmations).
//!
//! Only an exact, known icon bar program is recognised: the icon positions
//! and the globals that enable each icon belong to that script.

use ethornell_archive::ResourceManager;

pub const SAVE: u32 = 1;
pub const LOAD: u32 = 2;
pub const QUICK_SAVE: u32 = 3;
pub const QUICK_LOAD: u32 = 4;

/// Wonderful Everyday HD (sysprg.arc iconmngr._bp): the bar at (286, 902)
/// in a 1280x960 screen, 113x40 icons 118 px apart, SAVE, LOAD, QSAVE and
/// QLOAD first.
const WONDERFUL_EVERYDAY_HD: (usize, u64) = (17712, 0xcca8_0ccb_6b4b_e2fd);
const USER_GLOBALS: usize = 86032;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IconBar;

impl IconBar {
    pub(crate) fn detect(manager: &ResourceManager) -> Option<Self> {
        let program = manager
            .read_decoded_from_archive("sysprg.arc", "iconmngr._bp")
            .ok()?;
        let fingerprint = program.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
        (program.len(), fingerprint)
            .eq(&WONDERFUL_EVERYDAY_HD)
            .then_some(Self)
    }

    /// The commands whose icon is enabled, as bits `1 << (command - 1)`.
    /// iconmngr._bp enables SAVE and QSAVE with user flag +348 outside the
    /// system screens (1048) and 1324, LOAD with +344, QLOAD only after a
    /// quick save exists (1476).
    pub(crate) fn enabled(self, memory: &[u8]) -> u32 {
        let word = |address: usize| {
            memory
                .get(address..address + 4)
                .map_or(0, |bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        };
        let idle = word(1048) == 0;
        let save = word(USER_GLOBALS + 348) != 0 && idle && word(1324) == 0;
        let load = word(USER_GLOBALS + 344) != 0 && idle;
        let mut bits = 0;
        for (command, on) in [
            (SAVE, save),
            (LOAD, load),
            (QUICK_SAVE, save),
            (QUICK_LOAD, load && word(1476) != 0),
        ] {
            if on {
                bits |= 1 << (command - 1);
            }
        }
        bits
    }

    /// The centre of a command's icon, in game pixels.
    pub(crate) fn icon(self, command: u32) -> Option<(f32, f32)> {
        matches!(command, SAVE..=QUICK_LOAD)
            .then(|| (286.0 + 56.0 + 118.0 * (command - 1) as f32, 922.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_the_bar_and_flags_gate_them() {
        let bar = IconBar;
        assert_eq!(bar.icon(SAVE), Some((342.0, 922.0)));
        assert_eq!(bar.icon(QUICK_LOAD), Some((696.0, 922.0)));
        assert_eq!(bar.icon(5), None);

        let mut memory = vec![0u8; USER_GLOBALS + 352];
        assert_eq!(bar.enabled(&memory), 0);
        let set = |memory: &mut [u8], address: usize| {
            memory[address..address + 4].copy_from_slice(&1u32.to_le_bytes())
        };
        set(&mut memory, USER_GLOBALS + 348);
        set(&mut memory, USER_GLOBALS + 344);
        assert_eq!(bar.enabled(&memory), 0b0111);
        set(&mut memory, 1476);
        assert_eq!(bar.enabled(&memory), 0b1111);
        set(&mut memory, 1324);
        assert_eq!(bar.enabled(&memory), 0b1010);
        set(&mut memory, 1048);
        assert_eq!(bar.enabled(&memory), 0);
    }
}
