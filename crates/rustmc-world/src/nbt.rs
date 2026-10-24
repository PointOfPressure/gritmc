//! NBT reader and writer — the serialization under every chunk on disk.
//! Reference: `net.minecraft.nbt.NbtIo` / `TagTypes` semantics.
//!
//! **This header said "read-only for now, the writer arrives with wave-1
//! world saving" while [`write_named_root`] sat 240 lines below it.** The
//! same stale sentence was in `anvil.rs`; between them they were enough for
//! a survey of this repo to report chunk support as read-only. Both are
//! corrected.
//!
//! Compounds parse into BTreeMap: deterministic iteration everywhere a tag
//! map is walked (ORACLE.md §6 — hash-order must never become behavior).

use std::collections::BTreeMap;
use std::io::{self, Read};

#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Nbt>),
    Compound(BTreeMap<String, Nbt>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(m) => m.get(key),
            _ => None,
        }
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Nbt::Byte(v) => Some(*v as i32),
            Nbt::Short(v) => Some(*v as i32),
            Nbt::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Nbt]> {
        match self {
            Nbt::List(l) => Some(l),
            _ => None,
        }
    }
    /// `TAG_Byte_Array`. The light nibble arrays are written as one.
    pub fn as_byte_array(&self) -> Option<&[i8]> {
        match self {
            Nbt::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// `TAG_Int_Array`. Structure piece bounding boxes are written as one.
    pub fn as_i32_array(&self) -> Option<&[i32]> {
        match self {
            Nbt::IntArray(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_long_array(&self) -> Option<&[i64]> {
        match self {
            Nbt::LongArray(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_compound(&self) -> Option<&BTreeMap<String, Nbt>> {
        match self {
            Nbt::Compound(m) => Some(m),
            _ => None,
        }
    }
}

/// Guard against corrupt length prefixes allocating unbounded memory.
const MAX_LEN: i32 = 1 << 26;

/// Read a disk-format NBT blob: named root compound (name almost always "").
pub fn read_named_root(r: &mut impl Read) -> io::Result<(String, Nbt)> {
    let tag = read_u8(r)?;
    if tag != 10 {
        return Err(io::Error::other(format!(
            "root tag {tag}, expected compound (10)"
        )));
    }
    let name = read_utf(r)?;
    Ok((name, read_payload(r, 10, 0)?))
}

/// Read a **network**-format NBT value: a bare tag with no root name.
///
/// The wire form dropped the root name years ago, so `read_named_root` reads
/// two bytes too many on anything that came off a socket — and those two
/// bytes are usually `00 00`, an empty name, which makes the mistake decode
/// successfully on a compound and then fail somewhere later. They are
/// separate functions for that reason.
///
/// `TAG_End` (0) is a valid network value meaning "absent", and is `None`
/// rather than a variant of [`Nbt`] — the enum has no `End` and giving it one
/// would put an unrepresentable case into every match that handles real data.
///
/// # Errors
/// On a malformed tag, or nesting deeper than 64.
pub fn read_network_tag(r: &mut impl Read) -> io::Result<Option<Nbt>> {
    let tag = read_u8(r)?;
    if tag == 0 {
        return Ok(None);
    }
    read_payload(r, tag, 0).map(Some)
}

fn read_u8(r: &mut impl Read) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_utf(r: &mut impl Read) -> io::Result<String> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    let len = u16::from_be_bytes(b) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    // Java modified-UTF8 differs from UTF-8 only for NUL + surrogates; chunk
    // data in practice is plain. Replace on the rare mismatch, never fail.
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn checked_len(len: i32, what: &str) -> io::Result<usize> {
    if !(0..=MAX_LEN).contains(&len) {
        return Err(io::Error::other(format!(
            "{what} length {len} out of range"
        )));
    }
    Ok(len as usize)
}

fn read_payload(r: &mut impl Read, tag: u8, depth: u32) -> io::Result<Nbt> {
    if depth > 64 {
        return Err(io::Error::other("NBT nesting deeper than 64"));
    }
    macro_rules! be {
        ($t:ty) => {{
            let mut b = [0u8; std::mem::size_of::<$t>()];
            r.read_exact(&mut b)?;
            <$t>::from_be_bytes(b)
        }};
    }
    Ok(match tag {
        1 => Nbt::Byte(be!(i8)),
        2 => Nbt::Short(be!(i16)),
        3 => Nbt::Int(be!(i32)),
        4 => Nbt::Long(be!(i64)),
        5 => Nbt::Float(be!(f32)),
        6 => Nbt::Double(be!(f64)),
        7 => {
            let n = checked_len(be!(i32), "byte array")?;
            let mut buf = vec![0u8; n];
            r.read_exact(&mut buf)?;
            Nbt::ByteArray(buf.into_iter().map(|b| b as i8).collect())
        }
        8 => Nbt::String(read_utf(r)?),
        9 => {
            let elem = read_u8(r)?;
            let n = checked_len(be!(i32), "list")?;
            if elem == 0 && n > 0 {
                return Err(io::Error::other("non-empty list of TAG_End"));
            }
            let mut items = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                items.push(read_payload(r, elem, depth + 1)?);
            }
            Nbt::List(items)
        }
        10 => {
            let mut map = BTreeMap::new();
            loop {
                let t = read_u8(r)?;
                if t == 0 {
                    break;
                }
                let name = read_utf(r)?;
                map.insert(name, read_payload(r, t, depth + 1)?);
            }
            Nbt::Compound(map)
        }
        11 => {
            let n = checked_len(be!(i32), "int array")?;
            let mut items = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                items.push(be!(i32));
            }
            Nbt::IntArray(items)
        }
        12 => {
            let n = checked_len(be!(i32), "long array")?;
            let mut items = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                items.push(be!(i64));
            }
            Nbt::LongArray(items)
        }
        t => return Err(io::Error::other(format!("unknown NBT tag {t}"))),
    })
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

use std::io::Write;

impl Nbt {
    /// The tag byte this payload is written under.
    ///
    /// Kept next to the reader's match so the two cannot drift: a wrong tag
    /// id here produces a file vanilla rejects at load, which is at least
    /// loud — but a *swapped* pair (Int/Float are both 4 bytes) produces one
    /// it accepts and misreads.
    #[must_use]
    pub const fn tag(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(_) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }
}

/// Write a named root tag — the shape `NbtIo.write` produces.
///
/// Vanilla's root is always a compound with an empty name; the parameter
/// exists because the format allows otherwise and the reader returns it.
pub fn write_named_root(w: &mut impl Write, name: &str, value: &Nbt) -> io::Result<()> {
    w.write_all(&[value.tag()])?;
    write_utf(w, name)?;
    write_payload(w, value)
}

/// The order `CompoundTag.write` emits a compound's keys in.
///
/// # This is a byte-identity question, not a tidiness one
///
/// NBT compounds are unordered, so sorted keys are a *legal* encoding of the
/// same tag and this writer emitted them for as long as it has existed. What
/// that cost is exact: any oracle check whose projection digests a packet body
/// carrying a multi-key compound compares these bytes against vanilla's, and
/// `chat`'s projection digests the **whole** body — the chat arm of
/// `trace::decode` consumes zero bytes — so for
/// `110-server-answers-a-command` byte-identical NBT is not a nicety, it *is*
/// the check. There is no partial credit.
///
/// Vanilla's order is neither arbitrary nor insertion-dependent for the tags
/// this server writes. `CompoundTag` wraps a `java.util.HashMap` and `write`
/// iterates `keySet()`, so the order is bucket-major: a pure function of
/// `String.hashCode` — which the JLS specifies, unlike most of what
/// [`rustmc_data::java_hash_set`] reproduces — and the table capacity.
///
/// # The limit, stated rather than left to be found
///
/// Bucket order is insertion-*minor*: two keys that land in the same bucket
/// come out in the order they went in, and this inserts in `BTreeMap` order
/// rather than in whatever order vanilla's codec built the tag. So the
/// reproduction is **exact when the keys occupy distinct buckets** and is a
/// legal-but-possibly-different encoding when they do not — which is no worse
/// than what it replaced.
///
/// Everything this server writes today is in the first case, and that is
/// asserted rather than assumed:
/// `tests::the_list_reply_compound_writes_with_before_translate` pins the one
/// the corpus reads, insertion-order-independently. (Not an intra-doc link: it
/// is a `cfg(test)` item and rustdoc cannot see it.)
///
/// Returns `None` when the table refuses to order itself — a bin long enough
/// to treeify whose spread hashes tie, which
/// [`rustmc_data::java_hash_set`] declines rather than guesses at. The caller
/// falls back to sorted order, which is what it did before and is still legal.
fn java_key_order(map: &BTreeMap<String, Nbt>) -> Option<Vec<String>> {
    let mut set = rustmc_data::java_hash_set::JavaHashSet::new();
    for key in insertion_order(map) {
        set.insert(key.clone()).ok()?;
    }
    let ordered = set.to_vec();
    // A key lost between the two would silently drop a tag. The map is the
    // authority on how many there are.
    (ordered.len() == map.len()).then_some(ordered)
}

/// The order vanilla's codec puts a text component's keys into its
/// `CompoundTag`, which is the order two keys sharing a `HashMap` bucket come
/// out in.
///
/// `ComponentSerialization.createCodec` is a record of the content's own map
/// codec, then `extra`, then `Style.Serializer.MAP_CODEC`, whose fields are
/// read off its constant pool in declaration order: `color`, `shadow_color`,
/// `bold`, `italic`, `underlined`, `strikethrough`, `obfuscated`,
/// `click_event`, `hover_event`, `insertion`, `font`. Content keys come first:
/// `text`; `translate`, `fallback`, `with`; and the single-key contents.
///
/// It matters because `text`, `bold` and `strikethrough` share bucket 11 of a
/// sixteen-slot table: a bold text component is `text` then `bold` on
/// vanilla's wire, and was `bold` then `text` here (scenario 134, measured
/// 2026-10-01 in `cmdexec2-rec1`).
const COMPONENT_KEY_ORDER: [&str; 21] = [
    "text",
    "translate",
    "fallback",
    "with",
    "keybind",
    "score",
    "selector",
    "separator",
    "nbt",
    "object",
    "extra",
    "color",
    "shadow_color",
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
    "click_event",
    "hover_event",
    "insertion",
];

/// Keys in the order they would have been inserted: a text component's
/// ([`COMPONENT_KEY_ORDER`], then `font` and anything else in `BTreeMap`
/// order) when the compound carries a component content key, and `BTreeMap`
/// order otherwise — which is what every other compound got before.
fn insertion_order(map: &BTreeMap<String, Nbt>) -> Vec<&String> {
    const CONTENT: [&str; 7] = [
        "text",
        "translate",
        "keybind",
        "score",
        "selector",
        "nbt",
        "object",
    ];
    let mut keys: Vec<&String> = map.keys().collect();
    if CONTENT.iter().any(|k| map.contains_key(*k)) {
        let rank = |k: &str| {
            COMPONENT_KEY_ORDER
                .iter()
                .position(|x| *x == k)
                .unwrap_or(COMPONENT_KEY_ORDER.len())
        };
        keys.sort_by_key(|k| rank(k));
    }
    keys
}

fn write_payload(w: &mut impl Write, value: &Nbt) -> io::Result<()> {
    match value {
        Nbt::Byte(v) => w.write_all(&v.to_be_bytes()),
        Nbt::Short(v) => w.write_all(&v.to_be_bytes()),
        Nbt::Int(v) => w.write_all(&v.to_be_bytes()),
        Nbt::Long(v) => w.write_all(&v.to_be_bytes()),
        Nbt::Float(v) => w.write_all(&v.to_be_bytes()),
        Nbt::Double(v) => w.write_all(&v.to_be_bytes()),
        Nbt::ByteArray(v) => {
            w.write_all(&(i32::try_from(v.len()).map_err(too_long)?).to_be_bytes())?;
            let bytes: Vec<u8> = v.iter().map(|b| *b as u8).collect();
            w.write_all(&bytes)
        }
        Nbt::String(v) => write_utf(w, v),
        Nbt::List(items) => {
            // An empty list is written as TAG_End-typed, which is what the
            // reader's `elem == 0 && n > 0` check is the mirror of. Writing a
            // non-zero element type with length 0 is legal but not what
            // vanilla emits, and round-tripping through vanilla would change
            // the bytes.
            let elem = items.first().map_or(0, Nbt::tag);
            w.write_all(&[elem])?;
            w.write_all(&(i32::try_from(items.len()).map_err(too_long)?).to_be_bytes())?;
            for item in items {
                if item.tag() != elem {
                    return Err(io::Error::other(
                        "heterogeneous NBT list — every element must share one tag",
                    ));
                }
                write_payload(w, item)?;
            }
            Ok(())
        }
        Nbt::Compound(map) => {
            // **`HashMap` bucket order, not sorted order** — see
            // [`java_key_order`]. `CompoundTag.write` iterates `keySet()` of a
            // `java.util.HashMap`, so anything that compares these bytes
            // against vanilla's compares that order.
            match java_key_order(map) {
                Some(keys) => {
                    for key in keys {
                        let Some(entry) = map.get(&key) else { continue };
                        w.write_all(&[entry.tag()])?;
                        write_utf(w, &key)?;
                        write_payload(w, entry)?;
                    }
                }
                None => {
                    for (key, entry) in map {
                        w.write_all(&[entry.tag()])?;
                        write_utf(w, key)?;
                        write_payload(w, entry)?;
                    }
                }
            }
            w.write_all(&[0])
        }
        Nbt::IntArray(v) => {
            w.write_all(&(i32::try_from(v.len()).map_err(too_long)?).to_be_bytes())?;
            for item in v {
                w.write_all(&item.to_be_bytes())?;
            }
            Ok(())
        }
        Nbt::LongArray(v) => {
            w.write_all(&(i32::try_from(v.len()).map_err(too_long)?).to_be_bytes())?;
            for item in v {
                w.write_all(&item.to_be_bytes())?;
            }
            Ok(())
        }
    }
}

fn too_long(_: std::num::TryFromIntError) -> io::Error {
    io::Error::other("NBT array longer than i32::MAX")
}

/// Modified UTF-8, length-prefixed with an unsigned short.
///
/// "Modified" matters for exactly two inputs: a NUL byte is encoded as the
/// two-byte sequence C0 80 rather than 00, and characters outside the BMP
/// are written as a surrogate pair of three-byte sequences rather than one
/// four-byte sequence. Block and biome ids are ASCII, so neither arises in
/// practice — but silently writing standard UTF-8 for a name that did
/// contain one would produce a file vanilla misreads rather than rejects.
fn write_utf(w: &mut impl Write, s: &str) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let c = ch as u32;
        if c == 0 {
            bytes.extend_from_slice(&[0xC0, 0x80]);
        } else if c < 0x80 {
            bytes.push(c as u8);
        } else if c < 0x800 {
            bytes.push(0xC0 | (c >> 6) as u8);
            bytes.push(0x80 | (c & 0x3F) as u8);
        } else if c < 0x1_0000 {
            bytes.push(0xE0 | (c >> 12) as u8);
            bytes.push(0x80 | ((c >> 6) & 0x3F) as u8);
            bytes.push(0x80 | (c & 0x3F) as u8);
        } else {
            // Surrogate pair, each half as its own three-byte sequence.
            let v = c - 0x1_0000;
            let high = 0xD800 + (v >> 10);
            let low = 0xDC00 + (v & 0x3FF);
            for half in [high, low] {
                bytes.push(0xE0 | (half >> 12) as u8);
                bytes.push(0x80 | ((half >> 6) & 0x3F) as u8);
                bytes.push(0x80 | (half & 0x3F) as u8);
            }
        }
    }
    let len = u16::try_from(bytes.len())
        .map_err(|_| io::Error::other("NBT string longer than 65535 bytes"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&bytes)
}

// ---------------------------------------------------------------------------
// Compressed files — `level.dat`, `data/<ns>/<id>.dat`, `players/data/<uuid>.dat`
// ---------------------------------------------------------------------------

/// Read a gzip-compressed named root, the form `NbtIo.readCompressed` reads.
///
/// Every NBT file vanilla writes *outside* a region file is gzip, not the
/// zlib/deflate that `RegionFile` frames its chunks with: `NbtIo.write` wraps
/// a `GZIPOutputStream`, and `NbtIo.readCompressed` opens the file through
/// `FastBufferedInputStream` -> `GZIPInputStream`. Reading one with the region
/// decoder gets `invalid stored block lengths` rather than a wrong answer, so
/// this is loud when it is wrong — but they are separate functions because
/// nothing about the two call sites says which is which.
///
/// # Errors
/// On a truncated or non-gzip file, or malformed NBT inside it.
pub fn read_compressed(bytes: &[u8]) -> io::Result<(String, Nbt)> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
    read_named_root(&mut out.as_slice())
}

/// Write a gzip-compressed named root, the form `NbtIo.write(tag, file)`
/// produces.
///
/// **The gzip container is not byte-reproducible against vanilla and no check
/// here should ask it to be.** Java's `Deflater` and `flate2`'s miniz emit
/// different (both valid) DEFLATE streams for the same input, and gzip's
/// header carries an mtime field. What *is* comparable, and what
/// [`encode_named_root`] exists for, is the NBT payload underneath.
///
/// # Errors
/// If the encoder cannot flush.
pub fn write_compressed(name: &str, value: &Nbt) -> io::Result<Vec<u8>> {
    let plain = encode_named_root(name, value)?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&plain)?;
    // `finish()` rather than a drop for the reason `anvil::encode_record`
    // gives: `flate2`'s `Drop` discards the result of its final flush, so a
    // failure would leave a truncated file and no error.
    encoder.finish()
}

/// The uncompressed bytes of a named root — the unit any comparison against
/// vanilla is taken in.
///
/// # Errors
/// If a string or array is too long for the format.
pub fn encode_named_root(name: &str, value: &Nbt) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    write_named_root(&mut out, name, value)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-assembled blob: {"": {num: Int 7, name: "mc", longs: [L;1,2]}}
    #[test]
    fn parses_handcrafted_compound() {
        let mut b: Vec<u8> = vec![10, 0, 0]; // compound, root name ""
        b.extend([3]); // Int tag
        b.extend([0, 3]);
        b.extend(*b"num");
        b.extend(7i32.to_be_bytes());
        b.extend([8]); // String tag
        b.extend([0, 4]);
        b.extend(*b"name");
        b.extend([0, 2]);
        b.extend(*b"mc");
        b.extend([12]); // LongArray tag
        b.extend([0, 5]);
        b.extend(*b"longs");
        b.extend(2i32.to_be_bytes());
        b.extend(1i64.to_be_bytes());
        b.extend(2i64.to_be_bytes());
        b.extend([0]); // TAG_End

        let (root_name, root) = read_named_root(&mut b.as_slice()).unwrap();
        assert_eq!(root_name, "");
        assert_eq!(root.get("num").unwrap().as_i32(), Some(7));
        assert_eq!(root.get("name").unwrap().as_str(), Some("mc"));
        assert_eq!(
            root.get("longs").unwrap().as_long_array(),
            Some(&[1i64, 2][..])
        );
    }

    /// **The compound the corpus reads, pinned to vanilla's own byte order.**
    ///
    /// `/list` answers with `Component.translatable("commands.list.players",
    /// …)`, which serialises to a two-key compound. Read straight out of a
    /// recording of real vanilla on 2026-08-04, that compound carries **`with`
    /// before `translate`** — not alphabetical, and stable across runs.
    /// `rustmc_data::java_hash_set` predicts it from `String.hashCode` without
    /// being told it, and this asserts the writer actually emits it.
    ///
    /// The second half is what makes it a claim about *buckets* rather than a
    /// lucky ordering: the same two keys built the other way round produce the
    /// same bytes, so no chain is two long and vanilla's own insertion order —
    /// which we do not know and cannot know — cannot enter.
    #[test]
    fn the_list_reply_compound_writes_with_before_translate() {
        let compound = |first: (&str, Nbt), second: (&str, Nbt)| -> Vec<u8> {
            let mut m = BTreeMap::new();
            m.insert(first.0.to_string(), first.1);
            m.insert(second.0.to_string(), second.1);
            let mut out = Vec::new();
            write_named_root(&mut out, "", &Nbt::Compound(m)).expect("buf");
            out
        };
        let bytes = compound(
            ("translate", Nbt::String("commands.list.players".into())),
            ("with", Nbt::List(vec![Nbt::String("oracle".into())])),
        );
        // tag 10, name length 0, then the first key: tag 9 (list), u16 4,
        // "with". Sorted order would put `translate` here.
        assert_eq!(&bytes[..3], &[10, 0, 0], "an unnamed root compound");
        assert_eq!(bytes[3], 9, "the first entry is the `with` list");
        assert_eq!(&bytes[4..6], &[0, 4]);
        assert_eq!(&bytes[6..10], b"with");

        let reversed = compound(
            ("with", Nbt::List(vec![Nbt::String("oracle".into())])),
            ("translate", Nbt::String("commands.list.players".into())),
        );
        assert_eq!(
            bytes, reversed,
            "the two keys are in different buckets, so insertion order cannot show"
        );
    }

    /// `text` and `bold` share bucket 11. Vanilla's component codec inserts
    /// the content before the style, so a bold text component is `text` then
    /// `bold` — the reverse of sorted order — and a compound with no content
    /// key keeps the sorted order it always had.
    #[test]
    fn a_bold_text_component_writes_text_before_bold() {
        let mut m = BTreeMap::new();
        m.insert("bold".to_string(), Nbt::Byte(1));
        m.insert("text".to_string(), Nbt::String("hi".into()));
        m.insert("color".to_string(), Nbt::String("gold".into()));
        assert_eq!(
            java_key_order(&m).expect("orders"),
            vec!["color".to_string(), "text".to_string(), "bold".to_string()]
        );
        let mut plain = BTreeMap::new();
        plain.insert("bold".to_string(), Nbt::Byte(1));
        plain.insert("strikethrough".to_string(), Nbt::Byte(1));
        assert_eq!(
            java_key_order(&plain).expect("orders"),
            vec!["bold".to_string(), "strikethrough".to_string()]
        );
    }

    /// A single-key compound cannot be reordered, and most of what this server
    /// writes is one. Here so that a future change to [`java_key_order`] that
    /// broke the trivial case would be caught by something cheaper to read
    /// than a corpus run.
    #[test]
    fn a_one_key_compound_is_unaffected_by_the_ordering() {
        let mut m = BTreeMap::new();
        m.insert(
            "translate".to_string(),
            Nbt::String("container.chest".into()),
        );
        let mut out = Vec::new();
        write_named_root(&mut out, "", &Nbt::Compound(m)).expect("buf");
        assert_eq!(out[3], 8, "tag byte of the only entry");
        assert_eq!(&out[6..15], b"translate");
    }

    /// Whatever order it writes in, the reader gets the same tag back.
    #[test]
    fn a_reordered_compound_still_round_trips() {
        let mut m = BTreeMap::new();
        m.insert("translate".to_string(), Nbt::String("x".into()));
        m.insert("with".to_string(), Nbt::Int(3));
        m.insert("insertion".to_string(), Nbt::Byte(1));
        let tag = Nbt::Compound(m.clone());
        let mut out = Vec::new();
        write_named_root(&mut out, "", &tag).expect("buf");
        let (name, back) = read_named_root(&mut out.as_slice()).expect("round trip");
        assert_eq!(name, "");
        assert_eq!(back.as_compound(), Some(&m));
    }

    #[test]
    fn rejects_junk() {
        assert!(read_named_root(&mut [42u8, 0, 0].as_slice()).is_err());
        // Truncated compound (no TAG_End) → clean EOF error, not a hang.
        assert!(read_named_root(&mut [10u8, 0, 0, 3, 0, 1].as_slice()).is_err());
    }
}
