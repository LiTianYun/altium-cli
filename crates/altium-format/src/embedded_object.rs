//! Layer 4 parser for the embedded object envelope format.
//! Each entry in a Storage or sidecar block stream is a 0xD0-tagged envelope:
//! tag(1) + id_length(1) + id(N) + compressed_length(4) + zlib_data(M).
//! The 4-byte compressed_length field stores the size of the zlib-compressed
//! payload. The decompressed bytes are stored in `inner_data`.
//! `parse_embedded_object_stream` consumes the header block's params internally
//! so callers never receive a partially-consumed `ParameterCollection`.
use std::io::Write;

use altium_format_types::constants::file_headers::EMBEDDED_OBJECT_HEADER_RECORD;
use altium_format_types::constants::parsing::{BLOCK_SIZE_MASK, INSTRUCTION_BINARY};
use altium_format_types::constants::record_structure::{HEADER, RECORD, WEIGHT};
use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

use crate::binary_io::{BinaryReader, BinaryWriter};
use crate::block_stream::{Block, BlockFormat, write_binary_block, write_text_block};
use crate::param_collection::ParameterCollection;
use crate::{AltiumFormatError, Result, ResultExt};

#[derive(Debug)]
pub(crate) struct EmbeddedObject {
    pub(crate) id: String,
    pub(crate) inner_data: Vec<u8>,
}

// Parses a single 0xD0-tagged embedded object envelope from a binary block payload.
// The inner payload is zlib-decompressed before being stored in `inner_data`.
pub(crate) fn parse_embedded_object(data: &[u8]) -> Result<EmbeddedObject> {
    let mut reader = BinaryReader::new(data);
    let tag = reader
        .read_u8()
        .context("embedded object tag at offset 0")?;
    if tag != INSTRUCTION_BINARY {
        return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
            "expected {INSTRUCTION_BINARY:#04x} tag at offset 0, got {tag:#04x}"
        )));
    }
    let id_len = reader
        .read_u8()
        .context("embedded object identifier length at offset 1")? as usize;
    let id_bytes = reader
        .read_bytes(id_len)
        .context("embedded object identifier at offset 2")?;
    // Identifiers have a fixed single-byte encoding; BOM-looking bytes are text.
    let (id_cow, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(id_bytes);
    let id = id_cow.into_owned();
    let length_offset = reader.position();
    let compressed_size = reader.read_i32_le().with_context(|| {
        format!("embedded object '{id}' compressed length at offset {length_offset}")
    })?;
    let compressed_size = usize::try_from(compressed_size).map_err(|_| AltiumFormatError::InvalidEmbeddedObject(format!("embedded object '{id}' negative compressed length {compressed_size} at offset {length_offset}")))?;
    let payload_offset = reader.position();
    let compressed_bytes = reader.read_bytes(compressed_size).with_context(|| {
        format!("embedded object '{id}' compressed payload at offset {payload_offset}")
    })?;
    let inner_data = zlib_decompress(compressed_bytes).with_context(|| {
        format!("embedded object '{id}' zlib payload at offset {payload_offset}")
    })?;
    reader.assert_exhausted().with_context(|| {
        format!(
            "embedded object '{id}' envelope after offset {}",
            reader.position()
        )
    })?;
    Ok(EmbeddedObject { id, inner_data })
}

fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = Decompress::new(true);
    let mut out = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &data[before_in as usize..],
                &mut chunk,
                FlushDecompress::None,
            )
            .map_err(|e| {
                AltiumFormatError::InvalidEmbeddedObject(format!(
                    "zlib decompress failed at compressed offset {before_in}: {e}"
                ))
            })?;
        let produced = (decoder.total_out() - before_out) as usize;
        out.extend_from_slice(&chunk[..produced]);
        if status == Status::StreamEnd {
            if decoder.total_in() != data.len() as u64 {
                return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
                    "zlib stream ended at compressed offset {} with {} trailing bytes",
                    decoder.total_in(),
                    data.len() as u64 - decoder.total_in()
                )));
            }
            return Ok(out);
        }
        if decoder.total_in() == before_in && produced == 0 {
            return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
                "incomplete zlib stream at compressed offset {}: end marker/checksum not reached",
                decoder.total_in()
            )));
        }
    }
}

// Parses the Storage-style block stream: block 0 = header params, blocks 1..N = entries.
// Header params (RECORD, Weight) are consumed internally; callers receive only the entries.
pub(crate) fn parse_embedded_object_stream(blocks: &[Block]) -> Result<Vec<EmbeddedObject>> {
    if blocks.is_empty() {
        return Err(AltiumFormatError::InvalidEmbeddedObject(
            "empty block list for embedded object stream".to_owned(),
        ));
    }
    if blocks[0].format != BlockFormat::Text {
        return Err(AltiumFormatError::InvalidEmbeddedObject(
            "first block of embedded object stream must be text format".to_owned(),
        ));
    }
    let mut params = ParameterCollection::from_bytes(&blocks[0].data)
        .context("embedded object stream header block 0")?;
    // RECORD=0 is a stream-header sentinel, not a schematic object record.
    if let Some(record) = params
        .remove_optional::<i32>(RECORD)
        .context("embedded object stream header RECORD")?
    {
        if record != EMBEDDED_OBJECT_HEADER_RECORD {
            return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
                "embedded object stream header RECORD must be {EMBEDDED_OBJECT_HEADER_RECORD}, got {record}"
            )));
        }
    }
    // Header names vary across Storage and pin sidecars; retain them in diagnostics.
    let header_name = params
        .remove_optional::<String>(HEADER)
        .context("embedded object stream header HEADER")?;
    let stream_context = match header_name {
        Some(name) => format!("embedded object stream '{name}'"),
        None => "embedded object stream".to_owned(),
    };
    // Some legacy files omit Weight from Storage-style headers.
    let weight = params
        .remove_optional::<usize>(WEIGHT)
        .with_context(|| format!("{stream_context} header Weight"))?
        .unwrap_or(blocks.len() - 1);
    params
        .assert_exhausted()
        .with_context(|| format!("{stream_context} header block 0"))?;
    let entries: Result<Vec<EmbeddedObject>> = blocks[1..]
        .iter()
        .enumerate()
        .map(|(index, block)| {
            if block.format != BlockFormat::Binary {
                return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
                    "{stream_context} entry block {} must be binary format",
                    index + 1
                )));
            }
            parse_embedded_object(&block.data)
                .with_context(|| format!("{stream_context} entry block {}", index + 1))
        })
        .collect();
    let entries = entries?;
    if entries.len() != weight {
        return Err(AltiumFormatError::RecordCountMismatch {
            section: stream_context,
            expected: weight,
            actual: entries.len(),
        });
    }
    Ok(entries)
}

/// Compresses data using zlib (standard deflate with zlib header).
pub(crate) fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    // Altium's serializer uses Ionic.Zlib with default compression settings.
    // Use flate2 default level to match on-disk byte patterns for roundtrips.
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(|e| {
        AltiumFormatError::InvalidEmbeddedObject(format!("zlib compress failed: {e}"))
    })?;
    enc.finish().map_err(|e| {
        AltiumFormatError::InvalidEmbeddedObject(format!("zlib compress finish failed: {e}"))
    })
}

/// Serializes a single embedded object into the 0xD0-tagged envelope format.
/// The inner_data is zlib-compressed before being wrapped.
pub(crate) fn serialize_embedded_object(id: &str, inner_data: &[u8]) -> Result<Vec<u8>> {
    let (id_bytes, _, had_unmappable) = encoding_rs::WINDOWS_1252.encode(id);
    if had_unmappable {
        return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
            "embedded object '{id}' identifier is not representable in Windows-1252"
        )));
    }
    let id_len = u8::try_from(id_bytes.len()).map_err(|_| {
        AltiumFormatError::InvalidEmbeddedObject(format!(
            "embedded object '{id}' identifier is {} encoded bytes; maximum is {}",
            id_bytes.len(),
            u8::MAX
        ))
    })?;
    let compressed =
        zlib_compress(inner_data).with_context(|| format!("compressing embedded object '{id}'"))?;
    let compressed_len = checked_compressed_size(compressed.len())
        .with_context(|| format!("embedded object '{id}' compressed payload"))?;
    let mut w = BinaryWriter::new();
    w.write_u8(INSTRUCTION_BINARY);
    w.write_u8(id_len);
    w.write_bytes(&id_bytes);
    w.write_i32_le(compressed_len);
    w.write_bytes(&compressed);
    Ok(w.finish())
}

// AD26 SchDataSerializer.WriteBinary/ReadBinary use full signed 32-bit lengths.
fn checked_compressed_size(size: usize) -> Result<i32> {
    i32::try_from(size).map_err(|_| {
        AltiumFormatError::InvalidEmbeddedObject(format!(
            "compressed size {size} exceeds signed 32-bit length limit {}",
            i32::MAX
        ))
    })
}

// The shared block writer currently supports the standard 24-bit outer framing.
fn checked_block_size(size: usize) -> Result<i32> {
    if size > BLOCK_SIZE_MASK as usize {
        return Err(AltiumFormatError::InvalidEmbeddedObject(format!(
            "block size {size} exceeds the supported outer block length limit {BLOCK_SIZE_MASK}"
        )));
    }
    Ok(size as i32)
}

/// Serializes a complete embedded object stream: header text block + entry binary blocks.
/// The header block contains HEADER=<header_name>, Weight=<count>.
/// Each entry is a 0xD0-tagged envelope in a binary block.
pub(crate) fn serialize_embedded_object_stream(
    header_name: &str,
    entries: &[(String, Vec<u8>)],
) -> Result<Vec<u8>> {
    // Build header params (no RECORD=0 — original files omit it from sidecar/storage headers)
    let mut params = ParameterCollection::new();
    params.insert(HEADER, header_name.to_owned());
    params.insert(WEIGHT, entries.len().to_string());
    let header_bytes = params.to_bytes();

    checked_block_size(header_bytes.len())
        .with_context(|| format!("serializing embedded object stream '{header_name}' header"))?;
    let mut stream = write_text_block(&header_bytes);

    // Write each entry as a binary block containing a 0xD0 envelope
    for (index, (id, inner_data)) in entries.iter().enumerate() {
        let envelope = serialize_embedded_object(id, inner_data).with_context(|| {
            format!(
                "serializing embedded object stream '{header_name}' entry block {}",
                index + 1
            )
        })?;
        checked_block_size(envelope.len()).with_context(|| {
            format!(
                "serializing embedded object stream '{header_name}' entry block {} '{id}'",
                index + 1
            )
        })?;
        stream.extend_from_slice(&write_binary_block(&envelope));
    }

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_stream::parse_blocks;
    use altium_format_types::constants::parsing::INSTRUCTION_FILE_STREAM;
    use std::io::Read;

    fn make_envelope(id: &str, inner_data: &[u8]) -> Vec<u8> {
        let compressed = zlib_compress(inner_data).unwrap();
        let (id_bytes, _, _) = encoding_rs::WINDOWS_1252.encode(id);
        let mut w = BinaryWriter::new();
        w.write_u8(INSTRUCTION_BINARY);
        w.write_u8(id_bytes.len() as u8);
        w.write_bytes(&id_bytes);
        w.write_i32_le(compressed.len() as i32);
        w.write_bytes(&compressed);
        w.finish()
    }

    #[test]
    fn zlib_requires_complete_single_stream() {
        let compressed = zlib_compress(b"complete payload").unwrap();
        for end in 0..compressed.len() {
            assert!(
                zlib_decompress(&compressed[..end]).is_err(),
                "accepted truncated zlib stream ending at {end}"
            );
        }
        let mut trailing = compressed.clone();
        trailing.push(0);
        assert!(
            zlib_decompress(&trailing).is_err(),
            "accepted trailing byte"
        );
        let mut concatenated = compressed.clone();
        concatenated.extend_from_slice(&compressed);
        assert!(
            zlib_decompress(&concatenated).is_err(),
            "accepted second stream"
        );
    }

    #[test]
    fn compressed_lengths_are_signed_32_bit_not_masked_block_headers() {
        let mut data = make_envelope("x", b"payload");
        let offset = 3;
        let size = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        data[offset..offset + 4].copy_from_slice(&(size | (1 << 24)).to_le_bytes());
        let error = parse_embedded_object(&data).unwrap_err().to_string();
        assert!(
            error.contains("'x' compressed payload at offset 7"),
            "{error}"
        );
        data[offset..offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(
            parse_embedded_object(&data)
                .unwrap_err()
                .to_string()
                .contains("negative compressed length")
        );
        assert_eq!(
            checked_compressed_size(BLOCK_SIZE_MASK as usize + 1).unwrap(),
            BLOCK_SIZE_MASK as i32 + 1
        );
        assert!(checked_compressed_size(i32::MAX as usize + 1).is_err());
        assert!(checked_block_size(BLOCK_SIZE_MASK as usize + 1).is_err());
    }

    #[test]
    fn zlib_checks_checksum_and_multiple_output_chunks_with_resource_context() {
        let payload = vec![42; 100_000];
        let data = serialize_embedded_object("many", &payload).unwrap();
        assert_eq!(parse_embedded_object(&data).unwrap().inner_data, payload);
        assert!(
            parse_embedded_object(&serialize_embedded_object("empty", b"").unwrap())
                .unwrap()
                .inner_data
                .is_empty()
        );
        let mut corrupt = make_envelope("image", b"payload");
        *corrupt.last_mut().unwrap() ^= 1;
        let blocks = vec![
            Block {
                format: BlockFormat::Text,
                data: b"|HEADER=Test resources|Weight=1|\0".to_vec(),
            },
            Block {
                format: BlockFormat::Binary,
                data: corrupt,
            },
        ];
        let error = parse_embedded_object_stream(&blocks)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Test resources"), "{error}");
        assert!(error.contains("entry block 1"), "{error}");
        assert!(
            error.contains("'image' zlib payload at offset 11"),
            "{error}"
        );
    }

    #[test]
    fn identifier_serialization_is_lossless_and_length_checked() {
        assert!(serialize_embedded_object("image😀", b"x").is_err());
        assert!(serialize_embedded_object(&"a".repeat(256), b"x").is_err());
        let id = "é".repeat(255);
        let parsed = parse_embedded_object(&serialize_embedded_object(&id, b"x").unwrap()).unwrap();
        assert_eq!(parsed.id, id);
        for id in ["ï»¿name", "ÿþname", "þÿname"] {
            let data = serialize_embedded_object(id, b"x").unwrap();
            assert_eq!(
                parse_embedded_object(&data).unwrap().id,
                id,
                "BOM-looking Windows-1252 bytes are identifier text"
            );
        }
    }

    #[test]
    fn stream_rejects_wrong_entry_format_and_record_sentinel() {
        let header = Block {
            format: BlockFormat::Text,
            data: b"|RECORD=0|Weight=1|\0".to_vec(),
        };
        let wrong_entry = Block {
            format: BlockFormat::Text,
            data: make_envelope("x", b"x"),
        };
        assert!(parse_embedded_object_stream(&[header, wrong_entry]).is_err());
        let header = Block {
            format: BlockFormat::Text,
            data: b"|RECORD=1|Weight=0|\0".to_vec(),
        };
        assert!(parse_embedded_object_stream(&[header]).is_err());
    }

    #[test]
    fn parse_single_envelope_decompresses() {
        let inner = b"hello world";
        let data = make_envelope("comp1", inner);
        let obj = parse_embedded_object(&data).unwrap();
        assert_eq!(obj.id, "comp1");
        assert_eq!(obj.inner_data, inner);
    }

    #[test]
    fn parse_binary_inner_decompresses() {
        let inner = b"\x01\x02\x03";
        let data = make_envelope("item", inner);
        let obj = parse_embedded_object(&data).unwrap();
        assert_eq!(obj.id, "item");
        assert_eq!(obj.inner_data, inner);
    }

    #[test]
    fn wrong_tag_returns_error() {
        let mut data = make_envelope("x", b"");
        data[0] = INSTRUCTION_FILE_STREAM;
        let err = parse_embedded_object(&data).unwrap_err();
        assert!(matches!(err, AltiumFormatError::InvalidEmbeddedObject(_)));
    }

    #[test]
    fn truncated_data_returns_error() {
        let err = parse_embedded_object(&[INSTRUCTION_BINARY]).unwrap_err();
        assert!(err.to_string().contains("identifier length at offset 1"));
    }

    #[test]
    fn parse_stream_with_weight() {
        let header_data = b"|RECORD=0|Weight=2|\0";
        let header_block = Block {
            format: BlockFormat::Text,
            data: header_data.to_vec(),
        };
        let entry1 = make_envelope("e1", b"abc");
        let entry2 = make_envelope("e2", b"\x01\x02");
        let block1 = Block {
            format: BlockFormat::Binary,
            data: entry1,
        };
        let block2 = Block {
            format: BlockFormat::Binary,
            data: entry2,
        };
        let blocks = vec![header_block, block1, block2];
        let entries = parse_embedded_object_stream(&blocks).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "e1");
        assert_eq!(entries[0].inner_data, b"abc");
        assert_eq!(entries[1].id, "e2");
        assert_eq!(entries[1].inner_data, b"\x01\x02");
    }

    #[test]
    fn weight_mismatch_returns_error() {
        let header_data = b"|Weight=3|\0";
        let header_block = Block {
            format: BlockFormat::Text,
            data: header_data.to_vec(),
        };
        let entry = make_envelope("e1", b"x");
        let block1 = Block {
            format: BlockFormat::Binary,
            data: entry,
        };
        let blocks = vec![header_block, block1];
        let err = parse_embedded_object_stream(&blocks).unwrap_err();
        assert!(matches!(err, AltiumFormatError::RecordCountMismatch { .. }));
    }

    #[test]
    fn parse_stream_without_weight_uses_block_count() {
        let header_data = b"|HEADER=Icon storage|\0";
        let header_block = Block {
            format: BlockFormat::Text,
            data: header_data.to_vec(),
        };
        let mut blocks = vec![header_block];
        let entries = parse_embedded_object_stream(&blocks).unwrap();
        assert!(entries.is_empty());
        blocks.push(Block {
            format: BlockFormat::Binary,
            data: make_envelope("legacy", b"entry"),
        });
        let entries = parse_embedded_object_stream(&blocks).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "legacy");
        assert_eq!(entries[0].inner_data, b"entry");
    }

    #[test]
    fn empty_blocks_returns_error() {
        let err = parse_embedded_object_stream(&[]).unwrap_err();
        assert!(matches!(err, AltiumFormatError::InvalidEmbeddedObject(_)));
    }

    #[test]
    fn parse_windows_1252_id() {
        // Build an envelope with a Windows-1252 encoded ID containing
        // bytes that are invalid UTF-8: e-acute (0xE9), euro sign (0x80).
        let id_w1252: &[u8] = &[0x63, 0x61, 0x66, 0xE9, 0x80]; // "caf\xE9\x80"
        let inner = b"data";
        let compressed = zlib_compress(inner).unwrap();
        let mut w = BinaryWriter::new();
        w.write_u8(INSTRUCTION_BINARY);
        w.write_u8(id_w1252.len() as u8);
        w.write_bytes(id_w1252);
        w.write_i32_le(compressed.len() as i32);
        w.write_bytes(&compressed);
        let data = w.finish();

        let obj = parse_embedded_object(&data).unwrap();
        // Windows-1252 0xE9 = e-acute (U+00E9), 0x80 = euro sign (U+20AC)
        assert_eq!(obj.id, "caf\u{00E9}\u{20AC}");
        assert_eq!(obj.inner_data, inner);
    }

    // ── Serialization roundtrip tests ──────────────────────────────────

    #[test]
    fn serialize_embedded_object_roundtrips() {
        let inner = b"test data payload";
        let envelope = serialize_embedded_object("myid", inner).unwrap();
        let obj = parse_embedded_object(&envelope).unwrap();
        assert_eq!(obj.id, "myid");
        assert_eq!(obj.inner_data, inner);
    }

    #[test]
    fn serialize_embedded_object_stream_roundtrips() {
        let entries = vec![
            ("e1".to_owned(), b"abc".to_vec()),
            ("e2".to_owned(), b"\x01\x02\x03".to_vec()),
        ];
        let stream_bytes = serialize_embedded_object_stream("TestStream", &entries).unwrap();
        let blocks = parse_blocks(&stream_bytes).unwrap();
        let parsed = parse_embedded_object_stream(&blocks).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "e1");
        assert_eq!(parsed[0].inner_data, b"abc");
        assert_eq!(parsed[1].id, "e2");
        assert_eq!(parsed[1].inner_data, b"\x01\x02\x03");
    }

    #[test]
    fn zlib_compress_decompress_roundtrip() {
        let data = b"Hello, world! This is test data for zlib compression.";
        let compressed = zlib_compress(data).unwrap();
        assert!(compressed.len() > 0);
        let mut decoder = flate2::read::ZlibDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn serialize_empty_stream() {
        let entries: Vec<(String, Vec<u8>)> = vec![];
        let stream_bytes = serialize_embedded_object_stream("Empty", &entries).unwrap();
        let blocks = parse_blocks(&stream_bytes).unwrap();
        // Header block should have Weight=0
        assert_eq!(blocks.len(), 1); // only header, no entries
        let mut params = ParameterCollection::from_bytes(&blocks[0].data).unwrap();
        let weight: usize = params.remove_required("Weight").unwrap();
        assert_eq!(weight, 0);
    }
}
