// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "index.rs"
// ============================================================================

use crate::error::ContainerError;

/// One slot in an archive, in on-disk order. Stored only inside the
/// archive's encrypted index blob (slot 0) — the plain on-disk layout
/// outside that one decrypted blob gives no hint which slots are which,
/// or how many real assets exist versus how many are filler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Holds the asset added `asset_order`-th via
    /// [`crate::ContainerBuilder::add_asset`] (0-based).
    Real { asset_order: u32 },
    /// Random filler — decrypts successfully (it's sealed with the same
    /// real key as everything else) but to meaningless bytes.
    Honeypot,
}

pub(crate) fn serialize(slots: &[Slot]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(slots.len() as u32).to_le_bytes());
    for slot in slots {
        match slot {
            Slot::Honeypot => out.push(0),
            Slot::Real { asset_order } => {
                out.push(1);
                out.extend_from_slice(&asset_order.to_le_bytes());
            }
        }
    }
    out
}

pub(crate) fn deserialize(data: &[u8]) -> Result<Vec<Slot>, ContainerError> {
    let mut pos = 0usize;
    let mut take = |n: usize| -> Result<&[u8], ContainerError> {
        let end = pos.checked_add(n).ok_or(ContainerError::CorruptIndex)?;
        let slice = data.get(pos..end).ok_or(ContainerError::CorruptIndex)?;
        pos = end;
        Ok(slice)
    };

    let count_bytes = take(4)?;
    let count = u32::from_le_bytes(
        count_bytes
            .try_into()
            .expect("take(4) returns exactly 4 bytes"),
    );

    let mut slots = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let tag = take(1)?[0];
        let slot = match tag {
            0 => Slot::Honeypot,
            1 => {
                let order_bytes = take(4)?;
                let asset_order = u32::from_le_bytes(
                    order_bytes
                        .try_into()
                        .expect("take(4) returns exactly 4 bytes"),
                );
                Slot::Real { asset_order }
            }
            _ => return Err(ContainerError::CorruptIndex),
        };
        slots.push(slot);
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_mix_of_real_and_honeypot_slots() {
        let slots = vec![
            Slot::Honeypot,
            Slot::Real { asset_order: 0 },
            Slot::Real { asset_order: 1 },
            Slot::Honeypot,
            Slot::Honeypot,
            Slot::Real { asset_order: 2 },
        ];
        let bytes = serialize(&slots);
        let restored = deserialize(&bytes).unwrap();
        assert_eq!(slots, restored);
    }

    #[test]
    fn round_trips_an_empty_slot_list() {
        let bytes = serialize(&[]);
        assert_eq!(deserialize(&bytes).unwrap(), vec![]);
    }

    #[test]
    fn truncated_data_is_rejected_not_panicked() {
        let slots = vec![Slot::Real { asset_order: 7 }, Slot::Honeypot];
        let bytes = serialize(&slots);
        for cut in 0..bytes.len() {
            assert_eq!(
                deserialize(&bytes[..cut]),
                Err(ContainerError::CorruptIndex)
            );
        }
    }

    #[test]
    fn invalid_tag_is_rejected() {
        let mut bytes = serialize(&[Slot::Honeypot]);
        bytes[4] = 99; // the one slot's tag byte, right after the 4-byte count
        assert_eq!(deserialize(&bytes), Err(ContainerError::CorruptIndex));
    }
}
