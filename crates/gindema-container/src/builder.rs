// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "builder.rs"
// ============================================================================

use gindema_common::SplitMix64;

use crate::crypto::seal;
use crate::error::ContainerError;
use crate::index::{self, Slot};
use crate::key::ContainerKey;

/// Magic bytes identifying a gindema-container archive.
pub(crate) const MAGIC: &[u8; 4] = b"GCTN";
/// The only archive format version this crate currently reads or writes.
pub(crate) const VERSION: u16 = 1;

/// Builds a gindema-container archive: add assets in whatever order the
/// caller wants to be able to retrieve them later, then [`build`](Self::build)
/// to seal everything — real assets and honeypot filler alike — behind one
/// key.
#[derive(Debug, Default)]
pub struct ContainerBuilder {
    assets: Vec<Vec<u8>>,
}

impl ContainerBuilder {
    /// Starts an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an asset and returns the order (0-based) to pass to
    /// [`crate::ContainerReader::read_asset`] later.
    ///
    /// # Errors
    /// [`ContainerError::EmptyAsset`] for zero-byte data.
    pub fn add_asset(&mut self, data: Vec<u8>) -> Result<u32, ContainerError> {
        if data.is_empty() {
            return Err(ContainerError::EmptyAsset);
        }
        let order = self.assets.len() as u32;
        self.assets.push(data);
        Ok(order)
    }

    /// Seals every added asset, plus `honeypot_count` decoy slots, into one
    /// archive under `key`.
    ///
    /// Real and honeypot slots are shuffled together (seeded by `seed`, so
    /// the same `(honeypot_count, seed)` pair always produces a
    /// byte-identical layout) before being written, so their on-disk order
    /// gives no hint which is which — only the encrypted index (always
    /// slot 0) says that, and reading it needs `key`. Honeypot filler
    /// length is drawn from the same range as this build's real asset
    /// sizes, so they don't stand out by size either; with no real assets
    /// at all, honeypots fall back to a fixed 64–1024 byte range.
    ///
    /// # Errors
    /// Propagates [`ContainerError::Compress`]/[`ContainerError::Encrypt`]
    /// from the underlying `dixscript` calls, though neither is expected
    /// in practice (see `crypto.rs`).
    pub fn build(
        &self,
        key: &ContainerKey,
        honeypot_count: u32,
        seed: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        let mut rng = SplitMix64::new(seed);

        let mut slots: Vec<Slot> = (0..self.assets.len() as u32)
            .map(|asset_order| Slot::Real { asset_order })
            .collect();
        slots.extend(std::iter::repeat_n(Slot::Honeypot, honeypot_count as usize));
        shuffle(&mut slots, &mut rng);

        let (min_len, max_len) = asset_size_range(&self.assets);

        let index_bytes = index::serialize(&slots);
        let index_blob = seal(key, &next_iv(&mut rng), &index_bytes)?;

        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        write_blob(&mut out, &index_blob);

        for slot in &slots {
            let blob = match slot {
                Slot::Real { asset_order } => {
                    seal(key, &next_iv(&mut rng), &self.assets[*asset_order as usize])?
                }
                Slot::Honeypot => {
                    let len = min_len + rng.next_below(max_len - min_len + 1);
                    let filler = rng.fill_bytes(len.max(1));
                    seal(key, &next_iv(&mut rng), &filler)?
                }
            };
            write_blob(&mut out, &blob);
        }

        Ok(out)
    }
}

fn shuffle(slots: &mut [Slot], rng: &mut SplitMix64) {
    for i in 0..slots.len() {
        let j = i + rng.next_below(slots.len() - i);
        slots.swap(i, j);
    }
}

fn asset_size_range(assets: &[Vec<u8>]) -> (usize, usize) {
    if assets.is_empty() {
        return (64, 1024);
    }
    let min = assets.iter().map(Vec::len).min().unwrap();
    let max = assets.iter().map(Vec::len).max().unwrap();
    (min, max.max(min + 1))
}

fn next_iv(rng: &mut SplitMix64) -> [u8; 12] {
    let bytes = rng.fill_bytes(12);
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&bytes);
    iv
}

fn write_blob(out: &mut Vec<u8>, blob: &[u8]) {
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    out.extend_from_slice(blob);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_asset_rejects_empty_data() {
        let mut b = ContainerBuilder::new();
        assert_eq!(b.add_asset(Vec::new()), Err(ContainerError::EmptyAsset));
    }

    #[test]
    fn add_asset_returns_sequential_orders() {
        let mut b = ContainerBuilder::new();
        assert_eq!(b.add_asset(vec![1]).unwrap(), 0);
        assert_eq!(b.add_asset(vec![2]).unwrap(), 1);
        assert_eq!(b.add_asset(vec![3]).unwrap(), 2);
    }

    #[test]
    fn same_seed_produces_a_byte_identical_archive() {
        let mut b = ContainerBuilder::new();
        b.add_asset(b"asset one".to_vec()).unwrap();
        b.add_asset(b"asset two, a bit longer".to_vec()).unwrap();
        let key = ContainerKey::generate();

        let a = b.build(&key, 3, 42).unwrap();
        let c = b.build(&key, 3, 42).unwrap();
        assert_eq!(a, c);
    }

    #[test]
    fn honeypots_strictly_increase_archive_size() {
        let mut b = ContainerBuilder::new();
        b.add_asset(b"an asset of some length to compare against".to_vec())
            .unwrap();
        let key = ContainerKey::generate();

        let none = b.build(&key, 0, 1).unwrap();
        let some = b.build(&key, 5, 1).unwrap();
        assert!(some.len() > none.len());
    }
}
