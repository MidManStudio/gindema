// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "reader.rs"
// ============================================================================

use crate::builder::{MAGIC, VERSION};
use crate::crypto::open;
use crate::error::ContainerError;
use crate::index::{self, Slot};
use crate::key::ContainerKey;

/// Reads a gindema-container archive. [`ContainerReader::open`] decrypts
/// only the small index blob (slot 0) — real asset data stays encrypted
/// on disk until [`ContainerReader::read_asset`] is called for it, which
/// decrypts exactly that one slot and nothing else.
pub struct ContainerReader<'a> {
    data: &'a [u8],
    key: ContainerKey,
    /// Byte range of each slot's length-prefixed blob within `data`
    /// (prefix excluded), in on-disk order — found by walking length
    /// prefixes once at `open` time, without decrypting anything but slot
    /// 0.
    slot_ranges: Vec<(usize, usize)>,
    slots: Vec<Slot>,
}

impl<'a> ContainerReader<'a> {
    /// Opens an archive: validates the header, decrypts and parses the
    /// index (slot 0), and locates every other slot's byte range without
    /// decrypting them yet.
    ///
    /// # Errors
    /// [`ContainerError::InvalidMagic`]/[`ContainerError::UnsupportedVersion`]
    /// for a file that isn't a gindema-container archive this crate can
    /// read. [`ContainerError::Decrypt`] if `key` is wrong (surfaces here,
    /// from the index blob, rather than only once a specific asset is
    /// requested). [`ContainerError::UnexpectedEof`]/
    /// [`ContainerError::CorruptIndex`] for a truncated or corrupted file.
    pub fn open(data: &'a [u8], key: ContainerKey) -> Result<Self, ContainerError> {
        let mut pos = 0usize;
        let mut take = |n: usize| -> Result<&'a [u8], ContainerError> {
            let end = pos.checked_add(n).ok_or(ContainerError::UnexpectedEof)?;
            let slice = data.get(pos..end).ok_or(ContainerError::UnexpectedEof)?;
            pos = end;
            Ok(slice)
        };

        if take(MAGIC.len())? != MAGIC {
            return Err(ContainerError::InvalidMagic);
        }
        let version = u16::from_le_bytes(
            take(2)?
                .try_into()
                .expect("take(2) returns exactly 2 bytes"),
        );
        if version != VERSION {
            return Err(ContainerError::UnsupportedVersion(version));
        }

        let (index_start, index_len) = read_blob_header(data, &mut pos)?;
        let index_bytes = open(&key, &data[index_start..index_start + index_len])?;
        let slots = index::deserialize(&index_bytes)?;

        let mut slot_ranges = Vec::with_capacity(slots.len());
        for _ in &slots {
            let (start, len) = read_blob_header(data, &mut pos)?;
            slot_ranges.push((start, len));
        }

        Ok(Self {
            data,
            key,
            slot_ranges,
            slots,
        })
    }

    /// How many real assets this archive holds (honeypot slots don't
    /// count).
    pub fn asset_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| matches!(s, Slot::Real { .. }))
            .count()
    }

    /// Decrypts and decompresses exactly the one slot holding asset
    /// `asset_order` — no other slot in the archive is touched.
    ///
    /// # Errors
    /// [`ContainerError::InvalidAssetId`] if no slot holds that order.
    /// [`ContainerError::Decrypt`]/[`ContainerError::Decompress`] for a
    /// corrupted archive (the key is already known good, having
    /// successfully opened the index in [`ContainerReader::open`]).
    pub fn read_asset(&self, asset_order: u32) -> Result<Vec<u8>, ContainerError> {
        let slot_index = self
            .slots
            .iter()
            .position(|s| matches!(s, Slot::Real { asset_order: o } if *o == asset_order))
            .ok_or(ContainerError::InvalidAssetId(asset_order))?;
        let (start, len) = self.slot_ranges[slot_index];
        open(&self.key, &self.data[start..start + len])
    }
}

/// Reads one `[u32 length][bytes]` blob header at `*pos`, advances `*pos`
/// past the bytes, and returns the bytes' own `(start, len)` range.
fn read_blob_header(data: &[u8], pos: &mut usize) -> Result<(usize, usize), ContainerError> {
    let len_end = pos.checked_add(4).ok_or(ContainerError::UnexpectedEof)?;
    let len_bytes = data
        .get(*pos..len_end)
        .ok_or(ContainerError::UnexpectedEof)?;
    let len = u32::from_le_bytes(
        len_bytes
            .try_into()
            .expect("checked above to be exactly 4 bytes"),
    ) as usize;

    let start = len_end;
    let end = start
        .checked_add(len)
        .ok_or(ContainerError::UnexpectedEof)?;
    if end > data.len() {
        return Err(ContainerError::UnexpectedEof);
    }
    *pos = end;
    Ok((start, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ContainerBuilder;

    #[test]
    fn opening_with_the_wrong_key_fails_at_the_index() {
        let mut b = ContainerBuilder::new();
        b.add_asset(b"an asset".to_vec()).unwrap();
        let key = ContainerKey::generate();
        let archive = b.build(&key, 2, 1).unwrap();

        let wrong_key = ContainerKey::generate();
        assert!(matches!(
            ContainerReader::open(&archive, wrong_key),
            Err(ContainerError::Decrypt(_))
        ));
    }

    #[test]
    fn wrong_magic_and_version_are_rejected() {
        let mut b = ContainerBuilder::new();
        b.add_asset(b"x".to_vec()).unwrap();
        let key = ContainerKey::generate();
        let mut archive = b.build(&key, 0, 1).unwrap();

        let mut bad_magic = archive.clone();
        bad_magic[0] = b'X';
        assert!(matches!(
            ContainerReader::open(&bad_magic, key.clone()),
            Err(ContainerError::InvalidMagic)
        ));

        archive[4] = 0xFF;
        archive[5] = 0xFF;
        assert!(matches!(
            ContainerReader::open(&archive, key),
            Err(ContainerError::UnsupportedVersion(0xFFFF))
        ));
    }

    #[test]
    fn invalid_asset_id_is_reported() {
        let mut b = ContainerBuilder::new();
        b.add_asset(b"only asset".to_vec()).unwrap();
        let key = ContainerKey::generate();
        let archive = b.build(&key, 0, 1).unwrap();
        let reader = ContainerReader::open(&archive, key).unwrap();
        assert_eq!(reader.read_asset(5), Err(ContainerError::InvalidAssetId(5)));
    }
}
