//! Layer 3 of the 5-layer parsing stack: block-stream framing.
//! Each Altium stream is a sequence of length-prefixed blocks.
//! The 4-byte header encodes payload size (bits 0-23) and format (bits 24-31):
//! 0x00 = text (pipe-delimited parameters), 0x01 = binary (packed struct).
//! Binary D0 envelopes can also occupy 16..32 MiB when their checked internal
//! envelope length equals the full header (AD26 GetNextLine special case).
//! Unknown flag values remain hard errors.
use altium_format_types::constants::parsing::{
    BLOCK_FLAG_BINARY, BLOCK_FLAG_SHIFT, BLOCK_FLAG_TEXT, BLOCK_SIZE_MASK, INSTRUCTION_BINARY,
};

use crate::{AltiumFormatError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockFormat {
    Text,
    Binary,
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub(crate) format: BlockFormat,
    pub(crate) data: Vec<u8>,
}

// Parses all blocks from `stream_data` eagerly, returning an error on the first bad header.
pub(crate) fn parse_blocks(stream_data: &[u8]) -> Result<Vec<Block>> {
    let mut iter = BlockIter::new(stream_data);
    let mut blocks = Vec::new();
    for result in &mut iter {
        blocks.push(result?);
    }
    Ok(blocks)
}

// Returns a lazy iterator over blocks; use when processing a stream incrementally.
pub(crate) fn iter_blocks(stream_data: &[u8]) -> BlockIter<'_> {
    BlockIter::new(stream_data)
}

/// Encodes a payload as a text-format block: 4-byte header (flags=0x00) + payload bytes.
pub(crate) fn write_text_block(payload: &[u8]) -> Vec<u8> {
    let header: i32 = (BLOCK_FLAG_TEXT as i32) << BLOCK_FLAG_SHIFT
        | (payload.len() as i32 & BLOCK_SIZE_MASK as i32);
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Encodes a payload as a binary-format block: 4-byte header (flags=0x01) + payload bytes.
pub(crate) fn write_binary_block(payload: &[u8]) -> Vec<u8> {
    let header: i32 = (BLOCK_FLAG_BINARY as i32) << BLOCK_FLAG_SHIFT
        | (payload.len() as i32 & BLOCK_SIZE_MASK as i32);
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub(crate) struct BlockIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BlockIter<'a> {
    // Wraps a byte slice for lazy block parsing starting at position 0.
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl BlockIter<'_> {
    fn read_next(&mut self) -> Result<Block> {
        let header_offset = self.pos;
        let remaining = &self.data[header_offset..];
        let header_bytes: [u8; 4] = remaining
            .get(..4)
            .ok_or_else(|| AltiumFormatError::InvalidBlockHeader {
                offset: header_offset,
                detail: format!("truncated header: only {} bytes remain", remaining.len()),
            })?
            .try_into()
            .expect("four-byte slice");
        let header = u32::from_le_bytes(header_bytes);
        let flags = (header >> BLOCK_FLAG_SHIFT) as u8;
        let format = match flags {
            BLOCK_FLAG_TEXT => BlockFormat::Text,
            BLOCK_FLAG_BINARY => BlockFormat::Binary,
            other => {
                return Err(AltiumFormatError::InvalidBlockHeader {
                    offset: header_offset,
                    detail: format!("unknown flags byte {other:#04x}"),
                });
            }
        };
        let payload = &remaining[4..];
        let mut size = (header & BLOCK_SIZE_MASK) as usize;
        // AD26 SchDataSerializerParam.GetNextLine retains the full header only
        // when mode==1 and a complete D0 envelope prefix proves that length.
        // Otherwise preserve standard framing: the next frame of an empty binary
        // block can itself start with byte D0. Envelope semantics are checked by
        // embedded_object, which receives every byte of the framed payload.
        if format == BlockFormat::Binary && payload.first() == Some(&INSTRUCTION_BINARY) {
            let candidate_size = (|| -> Option<usize> {
                let name_len = usize::from(*payload.get(1)?);
                let compressed_offset = 2 + name_len;
                let prefix_size = compressed_offset + 4;
                let bytes = payload.get(compressed_offset..prefix_size)?;
                let compressed_size = i32::from_le_bytes(bytes.try_into().ok()?);
                usize::try_from(compressed_size)
                    .ok()?
                    .checked_add(prefix_size)
            })();
            if candidate_size == Some(header as usize) {
                size = header as usize;
            }
        }
        let data = payload
            .get(..size)
            .ok_or_else(|| AltiumFormatError::InvalidBlockHeader {
                offset: header_offset,
                detail: format!(
                    "payload size {size} extends past stream end ({} bytes remain after header)",
                    payload.len()
                ),
            })?
            .to_vec();
        // get(..size) above proves this addition stays within the source slice.
        self.pos = header_offset + 4 + size;
        Ok(Block { format, data })
    }
}

impl Iterator for BlockIter<'_> {
    type Item = Result<Block>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }
        let result = self.read_next();
        if result.is_err() {
            // An invalid frame has no trustworthy next offset. Yield the error
            // once and terminate rather than repeatedly returning that frame.
            self.pos = self.data.len();
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed_d0(envelope_size: usize) -> Vec<u8> {
        let name = b"image";
        let prefix_size = 2 + name.len() + 4;
        let compressed_size = i32::try_from(envelope_size - prefix_size).unwrap();
        let header = u32::try_from(envelope_size).unwrap()
            | (u32::from(BLOCK_FLAG_BINARY) << BLOCK_FLAG_SHIFT);
        let mut stream = header.to_le_bytes().to_vec();
        stream.push(INSTRUCTION_BINARY);
        stream.push(name.len() as u8);
        stream.extend_from_slice(name);
        stream.extend_from_slice(&compressed_size.to_le_bytes());
        stream.resize(4 + envelope_size, 0x5a);
        stream
    }

    #[test]
    fn empty_binary_before_text_with_d0_length_byte_preserves_both_blocks() {
        let mut stream = write_binary_block(&[]);
        let text = vec![b'x'; usize::from(INSTRUCTION_BINARY)];
        stream.extend(write_text_block(&text));
        let blocks = parse_blocks(&stream).expect("legal empty binary then 208-byte text");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].format, BlockFormat::Binary);
        assert!(blocks[0].data.is_empty());
        assert_eq!(blocks[1].format, BlockFormat::Text);
        assert_eq!(blocks[1].data, text);
    }

    #[test]
    fn d0_framing_preserves_standard_and_oversized_boundaries() {
        for size in [
            32,
            BLOCK_SIZE_MASK as usize,
            BLOCK_SIZE_MASK as usize + 1,
            BLOCK_SIZE_MASK as usize + 137,
        ] {
            let mut stream = framed_d0(size);
            stream.extend(write_text_block(b"following block"));
            let mut blocks = iter_blocks(&stream);
            let first = blocks.next().expect("first frame").expect("valid D0 frame");
            assert_eq!(first.format, BlockFormat::Binary);
            assert_eq!(first.data.len(), size);
            assert_eq!(first.data[0], INSTRUCTION_BINARY);
            assert!(first.data[11..].iter().all(|byte| *byte == 0x5a));
            let next = blocks.next().expect("following frame").expect("text frame");
            assert_eq!(next.data, b"following block");
            assert_eq!(next.format, BlockFormat::Text);
            assert!(blocks.next().is_none());
        }
    }

    #[test]
    fn malformed_d0_does_not_promote_standard_framing_and_envelope_still_rejects() {
        let mut mismatch = framed_d0(32);
        mismatch[11..15].copy_from_slice(&22i32.to_le_bytes());
        let mut negative = framed_d0(32);
        negative[11..15].copy_from_slice(&(-1i32).to_le_bytes());
        let mut oversized_truncated = framed_d0(32);
        let declared = BLOCK_SIZE_MASK + 100;
        oversized_truncated[..4].copy_from_slice(&declared.to_le_bytes());
        oversized_truncated[11..15].copy_from_slice(&(declared as i32 - 11).to_le_bytes());
        let mut signed_max = framed_d0(32);
        signed_max[11..15].copy_from_slice(&i32::MAX.to_le_bytes());
        let missing_name = make_block_bytes(&[INSTRUCTION_BINARY], BLOCK_FLAG_BINARY);
        let missing_length = make_block_bytes(&[INSTRUCTION_BINARY, 8, b'a'], BLOCK_FLAG_BINARY);
        for stream in [mismatch, negative, signed_max, missing_name, missing_length] {
            let blocks = parse_blocks(&stream).expect("standard-sized frame is preserved");
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].data, stream[4..], "no envelope bytes discarded");
            assert!(
                crate::embedded_object::parse_embedded_object(&blocks[0].data).is_err(),
                "malformed envelope must still fail its own parser"
            );
        }
        // A matching full-header candidate commits to that oversized frame,
        // whose missing payload must fail instead of using its low24 size.
        let mut blocks = iter_blocks(&oversized_truncated);
        assert!(matches!(
            blocks.next().unwrap(),
            Err(AltiumFormatError::InvalidBlockHeader { offset: 0, .. })
        ));
        assert!(blocks.next().is_none());
    }

    #[test]
    fn d0_signature_does_not_authorize_unknown_flags_or_text_extension() {
        let mut stream = framed_d0(32);
        let invalid_header = (32u32 | (3u32 << BLOCK_FLAG_SHIFT)).to_le_bytes();
        stream[..4].copy_from_slice(&invalid_header);
        let error = parse_blocks(&stream).expect_err("unknown flag remains invalid");
        assert!(error.to_string().contains("unknown flags"), "{error}");
        let text = make_block_bytes(&[INSTRUCTION_BINARY], BLOCK_FLAG_TEXT);
        assert_eq!(parse_blocks(&text).unwrap()[0].format, BlockFormat::Text);
    }

    #[test]
    fn iterator_reports_following_offset_once_after_valid_oversized_frame() {
        let size = BLOCK_SIZE_MASK as usize + 1;
        let mut stream = framed_d0(size);
        stream.extend_from_slice(&[1, 2, 3]);
        let mut blocks = iter_blocks(&stream);
        assert_eq!(blocks.next().unwrap().unwrap().data.len(), size);
        let error = blocks.next().unwrap().unwrap_err();
        assert!(
            matches!(error, AltiumFormatError::InvalidBlockHeader { offset, .. } if offset == size + 4)
        );
        assert!(blocks.next().is_none());
        assert!(blocks.next().is_none());
        for stream in [vec![1, 2, 3], 100u32.to_le_bytes().to_vec()] {
            let mut blocks = iter_blocks(&stream);
            assert!(blocks.next().unwrap().is_err());
            assert!(blocks.next().is_none());
        }
    }

    fn make_block_bytes(payload: &[u8], flags: u8) -> Vec<u8> {
        let size = payload.len() as i32;
        let header = size | ((flags as i32) << 24);
        let mut bytes = header.to_le_bytes().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn empty_stream_produces_empty_vec() {
        let blocks = parse_blocks(&[]).unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn single_text_block() {
        let payload = b"hello";
        let data = make_block_bytes(payload, BLOCK_FLAG_TEXT);
        let blocks = parse_blocks(&data).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].format, BlockFormat::Text);
        assert_eq!(blocks[0].data, payload);
    }

    #[test]
    fn single_binary_block() {
        let payload = b"\x01\x02\x03";
        let data = make_block_bytes(payload, BLOCK_FLAG_BINARY);
        let blocks = parse_blocks(&data).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].format, BlockFormat::Binary);
        assert_eq!(blocks[0].data, payload);
    }

    #[test]
    fn multiple_blocks() {
        let mut data = make_block_bytes(b"text", BLOCK_FLAG_TEXT);
        data.extend_from_slice(&make_block_bytes(b"\xAA\xBB", BLOCK_FLAG_BINARY));
        let blocks = parse_blocks(&data).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].format, BlockFormat::Text);
        assert_eq!(blocks[0].data, b"text");
        assert_eq!(blocks[1].format, BlockFormat::Binary);
        assert_eq!(blocks[1].data, b"\xAA\xBB");
    }

    #[test]
    fn truncated_header_returns_error() {
        let result = parse_blocks(&[0x01, 0x02, 0x03]);
        assert!(matches!(
            result,
            Err(AltiumFormatError::InvalidBlockHeader { .. })
        ));
    }

    #[test]
    fn payload_past_end_returns_error() {
        // Header says 100 bytes but only 2 bytes follow
        let header = 100i32.to_le_bytes();
        let mut data = header.to_vec();
        data.extend_from_slice(&[0xAA, 0xBB]);
        let result = parse_blocks(&data);
        assert!(matches!(
            result,
            Err(AltiumFormatError::InvalidBlockHeader { .. })
        ));
    }

    #[test]
    fn unknown_flags_returns_error() {
        let data = make_block_bytes(b"x", 0x02);
        let result = parse_blocks(&data);
        assert!(matches!(
            result,
            Err(AltiumFormatError::InvalidBlockHeader { .. })
        ));
    }

    #[test]
    fn write_text_block_roundtrips() {
        let payload = b"|RECORD=1|KEY=VALUE|\0";
        let block_bytes = write_text_block(payload);
        let blocks = parse_blocks(&block_bytes).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].format, BlockFormat::Text);
        assert_eq!(blocks[0].data, payload);
    }

    #[test]
    fn write_binary_block_roundtrips() {
        let payload = b"\x02\x00\x00\x00\x01\x00";
        let block_bytes = write_binary_block(payload);
        let blocks = parse_blocks(&block_bytes).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].format, BlockFormat::Binary);
        assert_eq!(blocks[0].data, payload);
    }

    #[test]
    fn write_multiple_blocks_roundtrip() {
        let mut stream = write_text_block(b"|RECORD=0|\0");
        stream.extend_from_slice(&write_binary_block(b"\x02\x03"));
        stream.extend_from_slice(&write_text_block(b"|KEY=val|\0"));
        let blocks = parse_blocks(&stream).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].format, BlockFormat::Text);
        assert_eq!(blocks[1].format, BlockFormat::Binary);
        assert_eq!(blocks[2].format, BlockFormat::Text);
    }

    #[test]
    fn write_empty_payload_roundtrips() {
        let block_bytes = write_text_block(b"");
        let blocks = parse_blocks(&block_bytes).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].data.is_empty());
    }

    #[test]
    fn iter_blocks_matches_parse_blocks() {
        let mut data = make_block_bytes(b"abc", BLOCK_FLAG_TEXT);
        data.extend_from_slice(&make_block_bytes(b"\x01\x02", BLOCK_FLAG_BINARY));
        let parsed: Vec<Block> = parse_blocks(&data).unwrap();
        let iterated: Vec<Block> = iter_blocks(&data)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(parsed.len(), iterated.len());
        for (p, i) in parsed.iter().zip(iterated.iter()) {
            assert_eq!(p.format, i.format);
            assert_eq!(p.data, i.data);
        }
    }
}
