//! The two flash images and their framing. Nothing here talks to the dock.
//!
//! No firmware is shipped with this tool. The images are rebuilt from the updater that Cable Matters
//! publishes for a dock on the same platform, and every step is pinned by SHA-256, so the bytes
//! written are exactly the ones that were tested.

use sha2::{Digest, Sha256};

/// Length of the power controller's flash image.
pub const IMAGE_LEN: usize = 0x29284;
/// The flash programs one page of this size at a time.
pub const PAGE: usize = 256;
/// Data bytes in a full frame. A shorter frame makes the chip program what it has buffered.
pub const FRAME_DATA: usize = 59;
/// The chip's 16-bit sum over the whole image, for the original and for the patched image.
pub const ORIGINAL_SUM: u16 = 0x7943;
pub const PATCHED_SUM: u16 = 0x78c9;

const VENDOR_SHA256: &str = "8c42ec0dcbf74236f98ddcd06c19df302402cb2e7a479c1f2f30db25e364bdf8";
const ORIGINAL_SHA256: &str = "ec99db11b12383463f21cb1f312f58a4904f84f41563ca66a34d4b56f66712ed";
const PATCHED_SHA256: &str = "64b1919d3b242ae5b26447afe35ac6c623cebdddbbdefa0cbc7b452255d5ca8d";

/// Offset, the bytes expected there, the bytes to put there.
type Edit = (usize, &'static [u8], &'static [u8]);

// The i-tec dock's image is the vendor image with AutomaticIDRequest set in both copies of the
// configuration, plus each copy's checksum.
const VENDOR_TO_ORIGINAL: [Edit; 4] = [
    (0x2804, &[0x89, 0x1c, 0x26, 0x4d], &[0xbc, 0x9d, 0xd1, 0x03]),
    (0x290f, &[0x70], &[0x71]),
    (0x16804, &[0x89, 0x1c, 0x26, 0x4d], &[0xbc, 0x9d, 0xd1, 0x03]),
    (0x1690f, &[0x70], &[0x71]),
];

// Try.SRC off (bit 2 of 0x28F9) in the low copy only, plus its checksum. The high copy is left as
// it was: the controller falls back to it if it ever rejects the low one.
const ORIGINAL_TO_PATCHED: [Edit; 2] = [
    (0x2804, &[0xbc, 0x9d, 0xd1, 0x03], &[0x1e, 0x27, 0xad, 0xc5]),
    (0x28f9, &[0x0c], &[0x08]),
];

/// The image an unmodified dock holds, and the same image with Try.SRC switched off.
pub struct Images {
    pub original: Vec<u8>,
    pub patched: Vec<u8>,
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn edited(image: &[u8], edits: &[Edit], want_sha256: &str, name: &str) -> Result<Vec<u8>, String> {
    let mut out = image.to_vec();
    for &(at, before, after) in edits {
        let slot = out.get_mut(at..at + before.len()).ok_or("image too short")?;
        if *slot != *before {
            return Err(format!("unexpected bytes at {at:#x} while building the {name} image"));
        }
        slot.copy_from_slice(after);
    }
    if sha256_hex(&out) != want_sha256 {
        return Err(format!("the {name} image does not have the expected SHA-256"));
    }
    Ok(out)
}

/// Finds the vendor's power controller image in `file` and derives both images. The file may be the
/// vendor's whole updater or just the image.
pub fn from_updater(file: &[u8]) -> Result<Images, String> {
    // The image starts with its region pointer (0x2000) and has the boot header magic there.
    let vendor = (0..file.len().saturating_sub(IMAGE_LEN - 1))
        .filter(|&o| file[o..o + 4] == [0x00, 0x20, 0x00, 0x00] && file[o + 0x2000..o + 0x2004] == [0x01, 0x00, 0xe0, 0xac])
        .map(|o| &file[o..o + IMAGE_LEN])
        .find(|candidate| sha256_hex(candidate) == VENDOR_SHA256)
        .ok_or("this file does not contain the expected power controller image (it is in DUD15X0D1_Update_E108.exe)")?;
    let original = edited(vendor, &VENDOR_TO_ORIGINAL, ORIGINAL_SHA256, "original")?;
    let patched = edited(&original, &ORIGINAL_TO_PATCHED, PATCHED_SHA256, "patched")?;
    Ok(Images { original, patched })
}

/// The image filled up with blank (FF) bytes to whole pages.
pub fn padded(image: &[u8]) -> Vec<u8> {
    let mut out = image.to_vec();
    out.resize(image.len().div_ceil(PAGE) * PAGE, 0xff);
    out
}

/// One data frame: report ID, byte count, sequence number, data, trailer.
pub fn data_frame(seq: u16, chunk: &[u8]) -> [u8; 64] {
    debug_assert!(chunk.len() <= FRAME_DATA && (1..0xff00).contains(&seq));
    let mut f = [0u8; 64];
    f[0] = 0x02;
    f[1] = chunk.len() as u8;
    f[2..4].copy_from_slice(&seq.to_le_bytes());
    f[4..4 + chunk.len()].copy_from_slice(chunk);
    f[63] = 0x5a;
    f
}

/// Every page as 59+59+59+59+20 bytes, the vendor tool's split: the short last frame makes the
/// chip program exactly one page.
pub fn frames(padded: &[u8]) -> Vec<[u8; 64]> {
    let chunks = padded.chunks(PAGE).flat_map(|page| page.chunks(FRAME_DATA));
    (1u16..).zip(chunks).map(|(seq, chunk)| data_frame(seq, chunk)).collect()
}

/// What the chip's checksum command returns for the first `n` bytes if the flash holds `image`
/// followed by blank space.
pub fn sum16(image: &[u8], n: usize) -> u16 {
    let held: usize = image.iter().take(n).map(|&b| usize::from(b)).sum();
    ((held + 0xff * n.saturating_sub(image.len())) & 0xffff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_should_split_a_page_into_59_59_59_59_20() {
        let sizes: Vec<u8> = frames(&[0u8; PAGE]).iter().map(|f| f[1]).collect();
        assert_eq!(sizes, [59, 59, 59, 59, 20]);
    }

    #[test]
    fn frames_should_count_the_sequence_from_one_across_pages() {
        let seqs: Vec<u16> = frames(&[0u8; 2 * PAGE]).iter().map(|f| u16::from_le_bytes([f[2], f[3]])).collect();
        assert_eq!(seqs, (1..=10).collect::<Vec<u16>>());
    }

    #[test]
    fn data_frame_should_carry_id_count_sequence_data_and_trailer() {
        let f = data_frame(0x0102, &[0xaa, 0xbb]);
        assert_eq!((&f[..6], f[63]), (&[0x02, 2, 0x02, 0x01, 0xaa, 0xbb][..], 0x5a));
    }

    #[test]
    fn padded_should_fill_the_last_page_with_ff() {
        let p = padded(&[1u8; 300]);
        assert_eq!((p.len(), p[299], p[300], p[511]), (512, 1, 0xff, 0xff));
    }

    #[test]
    fn sum16_should_count_blank_space_past_the_image() {
        assert_eq!(sum16(&[1, 2, 3], 5), 6 + 2 * 0xff);
    }

    #[test]
    fn from_updater_should_reject_a_file_without_the_image() {
        assert!(from_updater(&[0u8; 0x40000]).is_err());
    }

    #[test]
    #[cfg(feature = "embedded-image")]
    fn built_in_image_should_give_both_pinned_images() {
        let sums = crate::images(None).map(|i| (sum16(&i.original, IMAGE_LEN), sum16(&i.patched, IMAGE_LEN)));
        assert_eq!(sums, Ok((ORIGINAL_SUM, PATCHED_SUM)));
    }

    /// `DOCK_IMAGE` is a file holding only the vendor's power controller image.
    #[test]
    #[ignore = "needs the vendor image, which is not part of this repository"]
    fn from_updater_should_accept_a_file_that_holds_only_the_image() {
        let file = std::fs::read(std::env::var("DOCK_IMAGE").unwrap_or_default()).unwrap_or_default();
        assert!(file.len() == IMAGE_LEN && from_updater(&file).is_ok());
    }

    /// Needs the real vendor file: `DOCK_UPDATER=/path/to/DUD15X0D1_Update_E108.exe cargo test -- --ignored`
    #[test]
    #[ignore = "needs the vendor updater file, which is not part of this repository"]
    fn from_updater_should_build_both_pinned_images_from_the_vendor_file() {
        let path = std::env::var("DOCK_UPDATER").unwrap_or_default();
        let file = std::fs::read(path).unwrap_or_default();
        let images = from_updater(&file).map(|i| (sum16(&i.original, IMAGE_LEN), sum16(&i.patched, IMAGE_LEN)));
        assert_eq!(images, Ok((ORIGINAL_SUM, PATCHED_SUM)));
    }

    /// `DOCK_FRAMES` is the frame stream of the first (Swift) tool for the patched image, the one
    /// that was written to a real dock: `tiwrite frames itec-ti-flash-patched-trysrc0.bin <out>`.
    #[test]
    #[ignore = "needs the vendor updater file and the first tool's frame stream"]
    fn frames_should_equal_the_stream_that_was_written_to_a_real_dock() {
        let file = std::fs::read(std::env::var("DOCK_UPDATER").unwrap_or_default()).unwrap_or_default();
        let ours = from_updater(&file).map(|i| frames(&padded(&i.patched)).concat());
        let theirs = std::fs::read(std::env::var("DOCK_FRAMES").unwrap_or_default()).unwrap_or_default();
        assert!(ours.is_ok_and(|ours| !ours.is_empty() && ours == theirs));
    }
}
