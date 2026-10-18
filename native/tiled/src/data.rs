//! Tile layer and chunk payloads: csv, base64 (plain, zlib, gzip, zstd) and the deprecated XML
//! `<tile gid="..."/>` list, decoded to gids.
//!
//! A gid is a `u32`: the tile's global id in the low 28 bits, Tiled's flip flags in the high
//! four ([`Gid`]). Payloads stay undecoded in the model ([`Payload`]); [`decode`] runs when a
//! layer or chunk is wanted.

use crate::model::{Chunk, Layer, Payload};
use crate::{Error, ErrorKind};
use base64::Engine as _;
use std::borrow::Cow;
use std::io::Read;

/// Tiled's horizontal flip flag.
pub const FLIPPED_HORIZONTALLY: u32 = 0x8000_0000;
/// Tiled's vertical flip flag.
pub const FLIPPED_VERTICALLY: u32 = 0x4000_0000;
/// Tiled's anti-diagonal flip flag (with the others: rotation by 90° steps).
pub const FLIPPED_DIAGONALLY: u32 = 0x2000_0000;
/// Tiled's 120° rotation flag of hexagonal maps (masked off, as the GDScript runtime does).
pub const ROTATED_HEXAGONAL_120: u32 = 0x1000_0000;
/// The bits that hold the global tile id.
pub const GID_MASK: u32 = 0x0FFF_FFFF;

/// A cell's value: global tile id plus flip flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Gid(pub u32);

impl Gid {
    /// The global tile id without flags (0: empty cell).
    pub fn id(self) -> u32 {
        self.0 & GID_MASK
    }
    /// True for an empty cell.
    pub fn is_empty(self) -> bool {
        self.id() == 0
    }
    /// Flipped horizontally.
    pub fn flip_h(self) -> bool {
        self.0 & FLIPPED_HORIZONTALLY != 0
    }
    /// Flipped vertically.
    pub fn flip_v(self) -> bool {
        self.0 & FLIPPED_VERTICALLY != 0
    }
    /// Flipped anti-diagonally (transposed).
    pub fn flip_d(self) -> bool {
        self.0 & FLIPPED_DIAGONALLY != 0
    }
}

/// Decodes a payload into gids.
///
/// `encoding` is `csv` (also when `None`) or `base64`; `compression` is `zlib`, `gzip`,
/// `zstd`, empty or `None`. `cells` is the expected number of cells (width × height), used to
/// size the buffers and to cut an overlong payload, as Godot's `decompress(cells * 4)` does.
pub fn decode<'a>(
    payload: &'a Payload,
    encoding: Option<&str>,
    compression: Option<&str>,
    cells: usize,
) -> Result<Cow<'a, [u32]>, Error> {
    match payload {
        Payload::Gids(g) => Ok(Cow::Borrowed(g)),
        Payload::Text(t) => match encoding.unwrap_or("csv") {
            "csv" => Ok(Cow::Owned(decode_csv(t, cells))),
            "base64" => decode_base64(t, compression.unwrap_or(""), cells).map(Cow::Owned),
            other => Err(Error::new(
                ErrorKind::Data,
                format!("Encoding '{other}' is not supported."),
            )),
        },
    }
}

impl Layer {
    /// The cells of a finite tile layer (`data`), decoded; `None` when it has none.
    pub fn decode(&self) -> Option<Result<Cow<'_, [u32]>, Error>> {
        let cells =
            self.width.unwrap_or(0).max(0) as usize * self.height.unwrap_or(0).max(0) as usize;
        let data = self.data.as_ref()?;
        Some(decode(
            data,
            self.encoding.as_deref(),
            self.compression.as_deref(),
            cells,
        ))
    }
}

impl Chunk {
    /// The chunk's cells, decoded with its layer's encoding and compression.
    pub fn decode<'a>(&'a self, layer: &Layer) -> Option<Result<Cow<'a, [u32]>, Error>> {
        let cells =
            self.width.unwrap_or(0).max(0) as usize * self.height.unwrap_or(0).max(0) as usize;
        let data = self.data.as_ref()?;
        Some(decode(
            data,
            layer.encoding.as_deref(),
            layer.compression.as_deref(),
            cells,
        ))
    }
}

/// Csv text to gids: `,`-separated numbers, white space around them ignored, an empty field 0
/// (the GDScript runtime's `split_floats(",")`), each value's low 32 bits.
pub fn decode_csv(text: &str, cells: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(cells);
    for field in text.split(',') {
        out.push(csv_field(field) as u32);
    }
    out
}

/// Csv text as the GDScript dictionary holds it: integers (`PackedInt64Array` of the floats).
pub(crate) fn csv_values(text: &str) -> Vec<i64> {
    text.split(',').map(csv_field).collect()
}

fn csv_field(field: &str) -> i64 {
    let f = field.trim();
    if !f.is_empty() && f.len() <= 18 && f.bytes().all(|c| c.is_ascii_digit()) {
        // The fast path: a plain non-negative integer.
        return f.bytes().fold(0i64, |a, c| a * 10 + i64::from(c - b'0'));
    }
    crate::godot_str::to_float(f) as i64
}

/// Base64 text (white space anywhere ignored), decompressed, as little-endian `u32` gids.
pub fn decode_base64(text: &str, compression: &str, cells: usize) -> Result<Vec<u32>, Error> {
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let raw = if text.bytes().any(|c| c.is_ascii_whitespace()) {
        let compact: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
        engine.decode(compact)
    } else {
        engine.decode(text)
    }
    .map_err(|e| Error::new(ErrorKind::Data, format!("Invalid base64 layer data: {e}.")))?;
    let bytes = decompress(&raw, compression, cells * 4)?;
    let n = (bytes.len() / 4).min(if cells > 0 { cells } else { usize::MAX });
    Ok(bytes[..n * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

fn decompress<'a>(
    raw: &'a [u8],
    compression: &str,
    size_hint: usize,
) -> Result<Cow<'a, [u8]>, Error> {
    let fail = |e: std::io::Error| {
        Error::new(
            ErrorKind::Data,
            format!("Decompressing '{compression}' layer data failed: {e}."),
        )
    };
    let mut out = Vec::with_capacity(size_hint);
    match compression {
        "" => return Ok(Cow::Borrowed(raw)),
        "zlib" => {
            flate2::read::ZlibDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(fail)?;
        }
        "gzip" => {
            flate2::read::MultiGzDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(fail)?;
        }
        #[cfg(feature = "zstd")]
        "zstd" => {
            zstd::stream::read::Decoder::with_buffer(raw)
                .map_err(fail)?
                .read_to_end(&mut out)
                .map_err(fail)?;
        }
        other => {
            return Err(Error::new(
                ErrorKind::Data,
                format!("Decompression for type '{other}' not yet implemented."),
            ))
        }
    }
    Ok(Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_like_split_floats() {
        assert_eq!(decode_csv(" 1,\n 2 ,3 , ,4", 5), vec![1, 2, 3, 0, 4]);
        assert_eq!(decode_csv("2147483650", 1), vec![2_147_483_650]);
        assert_eq!(csv_values("1,2.7,-1"), vec![1, 2, -1]);
    }

    #[test]
    fn base64_plain_with_white_space() {
        // 1, 2 as little-endian u32
        assert_eq!(
            decode_base64("AQAAAA\n  IAAAA=", "", 2).unwrap(),
            vec![1, 2]
        );
        assert!(decode_base64("AQAAAA==", "lz4", 1).is_err());
    }
}
