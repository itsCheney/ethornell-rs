//! Files packed into a Windows executable with Enigma Virtual Box.
//!
//! Translation patches for BGI games are often a patched BGI.exe carrying
//! the translated scripts and images in its virtual file system. On Windows
//! the packed files appear in the executable's folder, where BGI reads a
//! loose file before any archive. `EvbOverlay` lists those files and reads
//! them straight from the executable, without extracting anything.
//!
//! Layout (the ".enigma1" section): "EVB\0" and 60 bytes, a main node, then
//! one node per folder or file in pre-order. A file's data follows the
//! table, in table order; compressed files are aPLib chunks.
use crate::ArchiveSource;
use ethornell_core::{EthornellError, Result};
use std::collections::HashMap;
use std::io::SeekFrom;
use std::sync::{Arc, Mutex};

const MAGIC: &[u8; 4] = b"EVB\0";
const NODE_FILE: u8 = 2;
const NODE_FOLDER: u8 = 3;
/// The executable's own folder.
const DEFAULT_FOLDER: &str = "%DEFAULT FOLDER%";
const MAX_FILES: usize = 100_000;

#[derive(Debug, Clone)]
pub struct EvbFile {
    /// Path below the executable's folder, '/' separated.
    pub name: String,
    pub size: u64,
    offset: u64,
    stored: u64,
}

#[derive(Debug)]
pub struct EvbOverlay {
    source: Mutex<Box<dyn ArchiveSource>>,
    files: Vec<EvbFile>,
    by_name: HashMap<String, usize>,
    max_size: u64,
    /// The last file read: BGI often asks for a file's size and then for
    /// the file, and some packed images are tens of megabytes.
    last: Mutex<Option<(usize, Arc<Vec<u8>>)>>,
}

fn error(message: impl Into<String>) -> EthornellError {
    EthornellError::Parse(format!("EVB: {}", message.into()))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_at(source: &mut dyn ArchiveSource, offset: u64, length: usize) -> Result<Vec<u8>> {
    source.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length];
    source.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// The file range of the ".enigma1" section, if the executable has one.
fn enigma_section(source: &mut dyn ArchiveSource) -> Result<Option<(u64, u64)>> {
    let length = source.byte_len()?;
    if length < 0x40 {
        return Ok(None);
    }
    let dos = read_at(source, 0, 0x40)?;
    if !dos.starts_with(b"MZ") {
        return Ok(None);
    }
    let pe = u32_at(&dos, 0x3C).unwrap_or(0) as u64;
    if pe.checked_add(24).is_none_or(|end| end > length) {
        return Ok(None);
    }
    let header = read_at(source, pe, 24)?;
    if !header.starts_with(b"PE\0\0") {
        return Ok(None);
    }
    let sections = u16_at(&header, 6).unwrap_or(0) as u64;
    let optional = u16_at(&header, 20).unwrap_or(0) as u64;
    let table = pe + 24 + optional;
    if table
        .checked_add(sections * 40)
        .is_none_or(|end| end > length)
    {
        return Ok(None);
    }
    let table = read_at(source, table, (sections * 40) as usize)?;
    for section in table.chunks_exact(40) {
        if section.starts_with(b".enigma1") {
            let size = u32_at(section, 16).unwrap_or(0) as u64;
            let start = u32_at(section, 20).unwrap_or(0) as u64;
            if start < length {
                return Ok(Some((start, size.min(length - start))));
            }
        }
    }
    Ok(None)
}

impl EvbOverlay {
    /// The packed files of `source`, or None when it is not an executable
    /// packed with Enigma Virtual Box. Files larger than `max_size` are left
    /// out.
    pub fn open(mut source: Box<dyn ArchiveSource>, max_size: u64) -> Result<Option<Self>> {
        let Some((start, length)) = enigma_section(source.as_mut())? else {
            return Ok(None);
        };
        // The file table sits at the start of the section; the data after it
        // is most of the section, so only look for the magic near the start.
        let probe = read_at(source.as_mut(), start, length.min(1 << 20) as usize)?;
        let Some(magic) = probe.windows(4).position(|w| w == MAGIC) else {
            return Ok(None);
        };
        let table_start = start + magic as u64;
        let table_len = (start + length - table_start).min(64 << 20) as usize;
        let table = read_at(source.as_mut(), table_start, table_len)?;
        let files = parse_table(&table, table_start)?;
        let files: Vec<EvbFile> = files
            .into_iter()
            .filter(|file| file.size <= max_size)
            .collect();
        let total = source.byte_len()?;
        if files.iter().any(|file| {
            file.offset
                .checked_add(file.stored)
                .is_none_or(|end| end > total)
        }) {
            return Err(error("file data beyond the executable"));
        }
        let by_name = files
            .iter()
            .enumerate()
            .rev()
            .map(|(index, file)| (crate::normalize_resource_name(&file.name), index))
            .collect();
        Ok(Some(Self {
            source: Mutex::new(source),
            files,
            by_name,
            max_size,
            last: Mutex::new(None),
        }))
    }

    pub fn files(&self) -> &[EvbFile] {
        &self.files
    }

    pub fn find(&self, name: &str) -> Option<&EvbFile> {
        self.by_name
            .get(&crate::normalize_resource_name(name))
            .map(|&index| &self.files[index])
    }

    pub fn read(&self, name: &str) -> Result<Arc<Vec<u8>>> {
        let index = *self
            .by_name
            .get(&crate::normalize_resource_name(name))
            .ok_or_else(|| error(format!("no packed file {name}")))?;
        if let Some((cached, bytes)) = &*self.last.lock().map_err(|_| error("cache poisoned"))?
            && *cached == index
        {
            return Ok(bytes.clone());
        }
        let file = &self.files[index];
        let stored = {
            let mut source = self.source.lock().map_err(|_| error("handle poisoned"))?;
            read_at(source.as_mut(), file.offset, file.stored as usize)?
        };
        let bytes = Arc::new(if file.stored == file.size {
            stored
        } else {
            unpack_chunks(&stored, file.size.min(self.max_size) as usize)
                .map_err(|e| error(format!("{}: {e}", file.name)))?
        });
        *self.last.lock().map_err(|_| error("cache poisoned"))? = Some((index, bytes.clone()));
        Ok(bytes)
    }
}

fn parse_table(table: &[u8], table_start: u64) -> Result<Vec<EvbFile>> {
    let short = || error("file table truncated");
    // Pack header (64) and main node (size, 8 unknown bytes, object count).
    let main_size = u32_at(table, 64).ok_or_else(short)? as u64;
    let top_level = u32_at(table, 76).ok_or_else(short)? as usize;
    // The data of the first file starts where the main node says the table
    // ends; the nodes themselves start one byte before the main node's end.
    let mut data = table_start + 80 + main_size - 12;
    let mut at = 79usize;
    let mut files = Vec::new();
    // (folder path, children left) for each open folder.
    let mut folders: Vec<(String, usize)> = vec![(String::new(), top_level)];
    while let Some((_, left)) = folders.last() {
        if *left == 0 {
            folders.pop();
            continue;
        }
        if let Some(last) = folders.last_mut() {
            last.1 -= 1;
        }
        let objects = u32_at(table, at + 12).ok_or_else(short)? as usize;
        at += 16;
        let mut units = Vec::new();
        loop {
            let unit = u16_at(table, at).ok_or_else(short)?;
            at += 2;
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        let kind = *table.get(at).ok_or_else(short)?;
        at += 1;
        let name = String::from_utf16(&units).map_err(|_| error("node name is not UTF-16"))?;
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
            return Err(error("invalid node name"));
        }
        let parent = folders.last().map(|(path, _)| path.as_str()).unwrap_or("");
        match kind {
            NODE_FILE => {
                let size = u32_at(table, at + 2).ok_or_else(short)? as u64;
                let stored = u32_at(table, at + 49).ok_or_else(short)? as u64;
                at += 53;
                files.push(EvbFile {
                    name: join(parent, &name),
                    size,
                    offset: data,
                    stored,
                });
                data += stored;
                if files.len() > MAX_FILES {
                    return Err(error("too many files"));
                }
            }
            NODE_FOLDER => {
                at += 25;
                let path = if folders.len() == 1 && name == DEFAULT_FOLDER {
                    String::new()
                } else {
                    join(parent, &name)
                };
                if folders.len() > 64 {
                    return Err(error("folders nested too deep"));
                }
                folders.push((path, objects));
            }
            _ => return Err(error(format!("unknown node type {kind}"))),
        }
    }
    Ok(files)
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// A compressed file: a chunk table (its byte size and 4 unknown bytes,
/// then 12 bytes per chunk whose first word is the chunk's packed size),
/// then the aPLib-packed chunks.
fn unpack_chunks(stored: &[u8], size: usize) -> std::result::Result<Vec<u8>, &'static str> {
    let table_size = u32_at(stored, 0).ok_or("chunk table truncated")? as usize;
    let table = stored.get(8..table_size).ok_or("chunk table truncated")?;
    let mut data = stored.get(table_size..).ok_or("chunk table truncated")?;
    let mut out = Vec::with_capacity(size);
    for entry in table.chunks(12) {
        let packed = u32_at(entry, 0).ok_or("chunk table truncated")? as usize;
        let chunk = data.get(..packed).ok_or("chunk beyond the file")?;
        data = &data[packed..];
        aplib_depack(chunk, &mut out, size)?;
    }
    if out.len() != size {
        return Err("unpacked size mismatch");
    }
    Ok(out)
}

struct Bits<'a> {
    input: &'a [u8],
    at: usize,
    tag: u8,
    left: u8,
}

impl Bits<'_> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.input.get(self.at)?;
        self.at += 1;
        Some(byte)
    }
    fn bit(&mut self) -> Option<u32> {
        if self.left == 0 {
            self.tag = self.byte()?;
            self.left = 8;
        }
        self.left -= 1;
        let bit = (self.tag >> 7) as u32;
        self.tag <<= 1;
        Some(bit)
    }
    fn gamma(&mut self) -> Option<usize> {
        let mut value = 1usize;
        loop {
            value = value.checked_mul(2)? + self.bit()? as usize;
            if self.bit()? == 0 {
                return Some(value);
            }
        }
    }
}

/// Appends one aPLib-packed block (optionally with its "AP32" header) to
/// `out`, which may not grow past `limit`. Back-references reach into what
/// earlier blocks unpacked.
fn aplib_depack(
    packed: &[u8],
    out: &mut Vec<u8>,
    limit: usize,
) -> std::result::Result<(), &'static str> {
    let mut input = packed;
    if packed.starts_with(b"AP32") && packed.len() >= 24 {
        let header = u32_at(packed, 4).unwrap_or(0) as usize;
        let length = u32_at(packed, 8).unwrap_or(0) as usize;
        input = packed
            .get(header..header.saturating_add(length))
            .ok_or("aPLib header out of range")?;
    }
    let block_start = out.len();
    let mut bits = Bits {
        input,
        at: 0,
        tag: 0,
        left: 0,
    };
    let copy = |out: &mut Vec<u8>, offset: usize, length: usize| {
        // Each chunk is packed on its own.
        if offset == 0 || offset > out.len() - block_start {
            return Err("aPLib offset out of range");
        }
        if out.len() + length > limit {
            return Err("aPLib output larger than the file");
        }
        for _ in 0..length {
            out.push(out[out.len() - offset]);
        }
        Ok(())
    };
    let push = |out: &mut Vec<u8>, byte: u8| {
        if out.len() >= limit {
            return Err("aPLib output larger than the file");
        }
        out.push(byte);
        Ok(())
    };
    let Some(first) = bits.byte() else {
        return Ok(());
    };
    push(out, first)?;
    let mut last_offset = 0usize;
    let mut after_match = false;
    let truncated = "aPLib data truncated";
    loop {
        if bits.bit().ok_or(truncated)? == 0 {
            push(out, bits.byte().ok_or(truncated)?)?;
            after_match = false;
            continue;
        }
        if bits.bit().ok_or(truncated)? == 0 {
            let high = bits.gamma().ok_or(truncated)?;
            if !after_match && high == 2 {
                let length = bits.gamma().ok_or(truncated)?;
                copy(out, last_offset, length)?;
            } else {
                let high = high - if after_match { 2 } else { 3 };
                let offset = (high << 8) + bits.byte().ok_or(truncated)? as usize;
                let mut length = bits.gamma().ok_or(truncated)?;
                if offset >= 32000 {
                    length += 1;
                }
                if offset >= 1280 {
                    length += 1;
                }
                if offset < 128 {
                    length += 2;
                }
                copy(out, offset, length)?;
                last_offset = offset;
            }
            after_match = true;
        } else if bits.bit().ok_or(truncated)? == 0 {
            let byte = bits.byte().ok_or(truncated)? as usize;
            let (offset, length) = (byte >> 1, 2 + (byte & 1));
            if offset == 0 {
                return Ok(());
            }
            copy(out, offset, length)?;
            last_offset = offset;
            after_match = true;
        } else {
            let mut offset = 0usize;
            for _ in 0..4 {
                offset = (offset << 1) + bits.bit().ok_or(truncated)? as usize;
            }
            if offset == 0 {
                push(out, 0)?;
            } else {
                copy(out, offset, 1)?;
            }
            after_match = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes aPLib bits most significant first, with the tag byte placed
    /// ahead of the literal bytes it covers, as the packer does.
    #[derive(Default)]
    struct Writer {
        out: Vec<u8>,
        tag_at: Option<usize>,
        used: u8,
    }
    impl Writer {
        fn bit(&mut self, bit: u8) {
            if self.tag_at.is_none() || self.used == 8 {
                self.tag_at = Some(self.out.len());
                self.out.push(0);
                self.used = 0;
            }
            let at = self.tag_at.unwrap();
            self.out[at] |= bit << (7 - self.used);
            self.used += 1;
        }
        fn gamma(&mut self, value: usize) {
            let bits = usize::BITS - value.leading_zeros();
            for index in (0..bits - 1).rev() {
                self.bit(((value >> index) & 1) as u8);
                self.bit(u8::from(index != 0));
            }
        }
    }

    #[test]
    fn aplib_literals_matches_and_end() {
        let mut w = Writer::default();
        w.out.push(b'a');
        // literal 'b'
        w.bit(0);
        w.out.push(b'b');
        // short match: offset 2, length 2 -> "ab"
        w.bit(1);
        w.bit(1);
        w.bit(0);
        w.out.push(2 << 1);
        // repeat last offset (gamma 2 right after a literal is required, so
        // put a literal first): literal 'c', then rep-match length 2 -> "bc"
        // would need offset 2 from "...abc": out is "ababc" -> "bc"
        w.bit(0);
        w.out.push(b'c');
        w.bit(1);
        w.bit(0);
        w.gamma(2);
        w.gamma(2);
        // 4-bit offset match of 1 byte: offset 1 -> 'c'
        w.bit(1);
        w.bit(1);
        w.bit(1);
        for bit in [0, 0, 0, 1] {
            w.bit(bit);
        }
        // a zero byte
        w.bit(1);
        w.bit(1);
        w.bit(1);
        for _ in 0..4 {
            w.bit(0);
        }
        // end marker
        w.bit(1);
        w.bit(1);
        w.bit(0);
        w.out.push(0);
        let mut out = Vec::new();
        aplib_depack(&w.out, &mut out, 64).unwrap();
        assert_eq!(out, b"ababcbcc\0");
    }

    #[test]
    fn rejects_offsets_before_the_output() {
        let mut out = Vec::new();
        // 'a', then a short match at offset 5.
        assert!(aplib_depack(&[b'a', 0b1100_0000, 5 << 1], &mut out, 64).is_err());
    }

    fn node(name: &str, kind: u8, objects: u32) -> Vec<u8> {
        let mut bytes = vec![0; 12];
        bytes.extend(objects.to_le_bytes());
        for unit in name.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend([0, 0, kind]);
        bytes
    }

    fn file_node(name: &str, size: u32, stored: u32) -> Vec<u8> {
        let mut bytes = node(name, NODE_FILE, 0);
        let mut extra = vec![0u8; 53];
        extra[2..6].copy_from_slice(&size.to_le_bytes());
        extra[49..53].copy_from_slice(&stored.to_le_bytes());
        bytes.extend(extra);
        bytes
    }

    /// An image with a .enigma1 section holding `ipl._bp` and `Sub/x.txt`.
    fn packed_exe() -> Vec<u8> {
        let mut table = MAGIC.to_vec();
        table.resize(64, 0);
        let mut nodes = node(DEFAULT_FOLDER, NODE_FOLDER, 2);
        nodes.extend([0; 25]);
        nodes.extend(file_node("ipl._bp", 5, 5));
        nodes.extend(node("Sub", NODE_FOLDER, 1));
        nodes.extend([0; 25]);
        nodes.extend(file_node("x.txt", 2, 2));
        // Main node: its size is counted from its own 4th byte (see parse).
        let main_size = (nodes.len() - 1 + 12) as u32;
        table.extend(main_size.to_le_bytes());
        table.extend([0; 8]);
        table.extend(1u32.to_le_bytes());
        table.extend(&nodes[1..]);
        table.extend(b"hellohi");
        let section_at = 0x200usize;
        let mut exe = vec![0u8; section_at];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        exe[0x80..0x84].copy_from_slice(b"PE\0\0");
        exe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        exe[0x94..0x96].copy_from_slice(&0u16.to_le_bytes());
        let section = 0x80 + 24;
        exe[section..section + 8].copy_from_slice(b".enigma1");
        exe[section + 16..section + 20].copy_from_slice(&(table.len() as u32).to_le_bytes());
        exe[section + 20..section + 24].copy_from_slice(&(section_at as u32).to_le_bytes());
        exe.extend(table);
        exe
    }

    #[test]
    fn lists_and_reads_packed_files() {
        let exe = packed_exe();
        // The tree has one top-level folder; the main node counts it.
        let overlay = EvbOverlay::open(Box::new(std::io::Cursor::new(exe)), 1 << 20)
            .unwrap()
            .unwrap();
        let names: Vec<_> = overlay.files().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ipl._bp", "Sub/x.txt"]);
        assert_eq!(overlay.read("IPL._BP").unwrap().as_slice(), b"hello");
        assert_eq!(overlay.read("sub\\X.TXT").unwrap().as_slice(), b"hi");
        assert!(overlay.find("missing").is_none());
    }

    #[test]
    fn packed_files_win_over_archive_entries() {
        let mut arc = b"BURIKO ARC20".to_vec();
        arc.extend(2u32.to_le_bytes());
        for (name, offset, size) in [("ipl._bp", 0u32, 3u32), ("title._bp", 3, 3)] {
            let mut field = [0u8; 0x60];
            field[..name.len()].copy_from_slice(name.as_bytes());
            arc.extend(field);
            arc.extend(offset.to_le_bytes());
            arc.extend(size.to_le_bytes());
            arc.extend([0; 24]);
        }
        arc.extend(b"oldttl");
        let root = std::path::PathBuf::from("/game");
        let mut manager = crate::ResourceManager::from_authorized_sources_with_limits(
            root.clone(),
            vec![(root.join("system.arc"), Box::new(std::io::Cursor::new(arc)))],
            crate::ResourceBudget::default(),
        )
        .unwrap();
        assert_eq!(
            manager
                .read_decoded_from_archive("system.arc", "ipl._bp")
                .unwrap(),
            b"old"
        );
        let plain = Box::new(std::io::Cursor::new(b"MZ".to_vec()));
        assert!(
            !manager
                .set_launcher_overlay(root.join("x.exe"), plain)
                .unwrap()
        );
        let exe = Box::new(std::io::Cursor::new(packed_exe()));
        assert!(
            manager
                .set_launcher_overlay(root.join("BGI-CHS.exe"), exe)
                .unwrap()
        );
        assert_eq!(
            manager
                .read_decoded_from_archive("system.arc", "ipl._bp")
                .unwrap(),
            b"hello"
        );
        let entry = manager.find_in_archive("system.arc", "IPL._BP").unwrap();
        assert_eq!(entry.archive_path, root.join("BGI-CHS.exe"));
        assert_eq!(manager.read_by_entry_decoded(&entry).unwrap(), b"hello");
        assert_eq!(manager.decoded_size_by_entry(&entry).unwrap(), 5);
        assert_eq!(
            manager.find("ipl._bp").unwrap().archive_path,
            root.join("BGI-CHS.exe")
        );
        // Names the launcher doesn't carry still come from the archives.
        assert_eq!(
            manager
                .read_decoded_from_archive("system.arc", "title._bp")
                .unwrap(),
            b"ttl"
        );
    }

    #[test]
    fn plain_files_are_not_overlays() {
        let source = Box::new(std::io::Cursor::new(b"PackFile    \0\0\0\0".to_vec()));
        assert!(EvbOverlay::open(source, 1 << 20).unwrap().is_none());
    }
}
