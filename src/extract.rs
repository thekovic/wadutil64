use arrayvec::ArrayVec;
use itertools::Itertools;
use nom::error::{context, VerboseError};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
};

use crate::{
    convert_error, gfx, invalid_data,
    sound::{SampleData, SoundData},
    Compression, FlatEntry, FlatWad, LumpType, WadEntry,
};

#[derive(clap::Args)]
pub struct Args {
    /// WAD or ROM file to extract
    input: PathBuf,
    /// Directory to output WAD data into [default: DOOM64]
    #[arg(short, long)]
    outdir: Option<PathBuf>,
    /// Extract only the first matched file to this path (ignores OUTDIR)
    #[arg(long)]
    outfile: Option<PathBuf>,
    /// Glob patterns to include entry names
    #[arg(short, long)]
    include: Vec<String>,
    /// Don't extract lumps to subfolders
    #[arg(short, long, default_value_t = false)]
    flat: bool,
    /// Keep lumps in raw N64 format
    #[arg(long, default_value_t = false)]
    raw: bool,
    /// Do not decompress WAD data
    #[arg(long, default_value_t = false)]
    no_decompress: bool,
    /// Optional WDD file to read when extracting IWAD [default: DOOM64.WDD]
    #[arg(long)]
    wdd: Option<PathBuf>,
    /// Optional WMD file to read when extracting IWAD [default: DOOM64.WMD]
    #[arg(long)]
    wmd: Option<PathBuf>,
    /// Optional WSD file to read when extracting IWAD [default: DOOM64.WSD]
    #[arg(long)]
    wsd: Option<PathBuf>,
    /// Optional DLS file to read when extracting remaster IWAD [default: DOOMSND.DLS]
    #[arg(long)]
    dls: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Region {
    US,
    EU,
    JP,
}

const ROMNAME: &[u8; 0x14] = b"Doom64              ";
const ROMNAME_JP: &[u8; 0x14] = b"DOOM64              ";

struct RomData<'a> {
    name: &'a [u8; 0x14],
    sha256: [u8; 32],
    wad_offset: u32,
    wad_size: u32,
    wmd_offset: u32,
    wmd_size: u32,
    wsd_offset: u32,
    wsd_size: u32,
    wdd_offset: u32,
    wdd_size: u32,
}

impl<'a> RomData<'a> {
    fn new(region: Region, revision: u8) -> io::Result<Self> {
        match (region, revision) {
            (Region::US, 0) => Ok(ROMDATA_US),
            (Region::US, 1) => Ok(ROMDATA_US_1),
            (Region::EU, 0) => Ok(ROMDATA_EU),
            (Region::JP, 0) => Ok(ROMDATA_JP),
            _ => Err(invalid_data(format_args!(
                "Unknown ROM: Region {region:?} Rev {revision}"
            ))),
        }
    }
}

const ROMDATA_US: RomData<'static> = RomData {
    name: ROMNAME,
    sha256: hex_literal::hex!("d3404a7e8ca9d20ba034651932e67aa90c6c475c5f4738f222cd1e3056df935f"),
    wad_offset: 0x63D10,
    wad_size: 0x5D18B0,
    wmd_offset: 0x6355C0,
    wmd_size: 0xB9E0,
    wsd_offset: 0x640FA0,
    wsd_size: 0x14300,
    wdd_offset: 0x6552A0,
    wdd_size: 0x1716C4,
};
const ROMDATA_US_1: RomData<'static> = RomData {
    name: ROMNAME,
    sha256: hex_literal::hex!("c28eaac9a8a8cc1d30c1b50fbb04622c2ddeb9b14ddcecc6edbaad4a6d067f3f"),
    wad_offset: 0x63DC0,
    wad_size: 0x5D301C,
    wmd_offset: 0x636DE0,
    wmd_size: 0xB9E0,
    wsd_offset: 0x6427C0,
    wsd_size: 0x14300,
    wdd_offset: 0x656AC0,
    wdd_size: 0x1716C4,
};
const ROMDATA_EU: RomData<'static> = RomData {
    name: ROMNAME,
    sha256: hex_literal::hex!("e8460f2fa7e55172a296a1e30354cbb868be924a454ff883d1a6601c66b9610f"),
    wad_offset: 0x63F60,
    wad_size: 0x5D6CDC,
    wmd_offset: 0x63AC40,
    wmd_size: 0xB9E0,
    wsd_offset: 0x646620,
    wsd_size: 0x14300,
    wdd_offset: 0x65A920,
    wdd_size: 0x1716C4,
};
const ROMDATA_JP: RomData<'static> = RomData {
    name: ROMNAME_JP,
    sha256: hex_literal::hex!("19ad4130f8b259f24761d5c873e2ce468315cc5f7bce07e7f44db21241cef4a9"),
    wad_offset: 0x64580,
    wad_size: 0x5D8478,
    wmd_offset: 0x63CA00,
    wmd_size: 0xB9E0,
    wsd_offset: 0x6483E0,
    wsd_size: 0x14300,
    wdd_offset: 0x65C6E0,
    wdd_size: 0x1716C4,
};
const ROMDATA_PROTO: RomData<'static> = RomData {
    name: ROMNAME,
    sha256: hex_literal::hex!("4b3931c14d548fedf98fcd28681ec45695dec415d39fd4a6d58a877e1c6dd2a2"),
    wad_offset: 0x5A640,
    wad_size: 0x64B7B0,
    wmd_offset: 0x6A5E00,
    wmd_size: 0xBA00,
    wsd_offset: 0x6B1800,
    wsd_size: 0x14300,
    wdd_offset: 0x6C5B00,
    wdd_size: 0x1716C4,
};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum WadType {
    N64,
    N64Map,
    N64Prototype,
    Remaster,
    RemasterMap,
}

impl WadType {
    #[inline]
    pub fn is_map(&self) -> bool {
        matches!(self, Self::N64Map | Self::RemasterMap)
    }
    #[inline]
    pub fn is_remaster(&self) -> bool {
        matches!(self, Self::Remaster | Self::RemasterMap)
    }
    #[inline]
    pub fn is_prototype(&self) -> bool {
        matches!(self, Self::N64Prototype)
    }
}

fn is_map_lump(name: &[u8]) -> bool {
    [
        b"THINGS".as_slice(),
        b"LINEDEFS",
        b"SIDEDEFS",
        b"VERTEXES",
        b"SEGS",
        b"SSECTORS",
        b"NODES",
        b"SECTORS",
        b"REJECT",
        b"BLOCKMAP",
        b"LEAFS",
        b"LIGHTS",
        b"MACROS",
    ]
    .contains(&name)
}

impl FlatWad {
    pub fn parse<'a>(
        wad: &'a [u8],
        wad_type: WadType,
        decompress: bool,
        filters: &crate::FileFilters,
    ) -> nom::IResult<&'a [u8], Self, VerboseError<&'a [u8]>> {
        use nom::branch::alt;
        use nom::bytes::complete::{tag, take};
        use nom::number::complete::le_u32;
        use LumpType::*;

        let (count, table_offset) = context("WAD Header", |data| {
            let (data, _) = alt((tag("PWAD"), tag("IWAD")))(data)?;
            let (data, count) = le_u32(data)?;
            let (data, offset) = le_u32(data)?;
            Ok((data, (count, offset as usize)))
        })(wad)?
        .1;
        let mut table = &wad[table_offset..];
        let mut cur_map = None::<(ArrayVec<u8, 8>, Self)>;
        let mut entries = Vec::with_capacity(count as usize);
        let mut base_typ = Unknown;
        let mut blanktex_count = 0;
        let mut parsed_table = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let (t, offset) = le_u32(table)?;
            let (t, size) = le_u32(t)?;
            let (t, name) = take(8usize)(t)?;
            table = t;
            parsed_table.push((offset, size, name));
        }
        for (i, (offset, size, name)) in parsed_table.iter().copied().enumerate() {
            let name = name.split(|b| *b == b'\0').next().unwrap();
            let mut name = ArrayVec::try_from(name).unwrap();
            let mut compressed = false;
            if let Some(first) = name.first().copied() {
                if first & 0x80 != 0 {
                    compressed = true;
                    name[0] = first & !0x80;
                }
            }
            let n = name.as_slice();
            let mut typ = base_typ;
            if n == b"?" {
                blanktex_count += 1;
                if base_typ == Texture && blanktex_count == 2 {
                    typ = Flat;
                    base_typ = Flat;
                }
            } else if n == b"S_START" {
                typ = Marker;
                base_typ = Sprite;
            } else if n == b"T_START" {
                blanktex_count = 0;
                typ = Marker;
                base_typ = Texture;
            } else if wad_type.is_remaster() && n == b"DS_START" {
                typ = Marker;
                base_typ = Sample;
            } else if wad_type.is_remaster() && n == b"DM_START" {
                typ = Marker;
                base_typ = Sequence;
            } else if n == b"S_END"
                || n == b"T_END"
                || (wad_type.is_remaster() && n == b"DS_END")
                || (wad_type.is_remaster() && n == b"DM_END")
            {
                typ = Marker;
                base_typ = Unknown;
            } else if n == b"ENDOFWAD" {
                typ = Marker;
            }
            let mut cmap = None;
            if typ == Sprite && n.starts_with(b"PAL") {
                typ = Palette;
            } else if typ == Unknown {
                if wad_type.is_prototype() && is_map_lump(n) {
                    typ = MapLump;
                    cmap = cur_map.as_mut();
                } else {
                    if let Some((map_name, cur_map)) = cur_map.take() {
                        let mut entry = FlatEntry {
                            name: crate::EntryName(map_name),
                            entry: WadEntry {
                                typ: Map,
                                compression: Compression::None,
                                data: Vec::new(),
                            },
                        };
                        cur_map.write(&mut entry.entry.data, false).unwrap();
                        entries.push(entry);
                    }
                    if wad_type.is_map() {
                        if is_map_lump(n) {
                            typ = MapLump;
                        }
                    } else if n.starts_with(b"MAP") {
                        typ = Map;
                        if wad_type.is_prototype() {
                            cur_map = Some((name.clone(), FlatWad::default()));
                            cmap = cur_map.as_mut();
                        }
                    } else if n.starts_with(b"DEMO")
                        || (wad_type.is_prototype() && n == b"TITLELMP")
                    {
                        typ = Demo;
                    } else if n == b"SFONT"
                        || (!wad_type.is_prototype() && n == b"STATUS")
                        || n.starts_with(b"JPMSG")
                    {
                        typ = HudGraphic;
                    } else if n.starts_with(b"MOUNT") || n.starts_with(b"SPACE") {
                        typ = Sky;
                    } else if n == b"FIRE" {
                        typ = Fire;
                    } else if n == b"CLOUD" {
                        typ = Cloud;
                    } else {
                        typ = Graphic;
                    }
                }
            }
            let decompress = if filters.is_empty() {
                decompress
            } else {
                filters.matches(&String::from_utf8_lossy(n))
            };
            let mut compression = match (typ, compressed) {
                (Map | Demo | Texture | Flat, true) => Compression::Huffman(size as usize),
                (_, true) => Compression::Lzss(size as usize),
                (_, false) => Compression::None,
            };
            if decompress || filters.is_empty() {
                log::debug!(
                    "Loading {} as {typ:?} with compression {}",
                    String::from_utf8_lossy(&name),
                    compression.name()
                );
            }
            let data = if size > 0 {
                let start = offset as usize;
                let compressed_size = parsed_table
                    .get(i + 1)
                    .map(|e| e.0 as usize - offset as usize)
                    .unwrap_or_else(|| table_offset - offset as usize);
                match (compression, wad_type.is_prototype(), decompress) {
                    (Compression::None, _, _) => wad[start..start + size as usize].to_owned(),
                    (_, _, false) => wad[start..start + compressed_size].to_owned(),
                    (Compression::Huffman(_), true, true) => {
                        compression = Compression::Lzss(size as usize);
                        wad[start..start + compressed_size].to_owned()
                    }
                    (Compression::Lzss(_), _, true) => {
                        compression = Compression::None;
                        context("Jag Decompression", |d| {
                            crate::compression::decode_jaguar(d, size as usize)
                        })(&wad[start..start + compressed_size])?
                        .1
                    }
                    (Compression::Huffman(_), false, true) => {
                        compression = Compression::None;
                        context("D64 Decompression", |d| {
                            crate::compression::decode_d64(d, size as usize)
                        })(&wad[start..start + compressed_size])?
                        .1
                    }
                }
            } else {
                Vec::new()
            };
            let entry = FlatEntry {
                name: crate::EntryName(name),
                entry: WadEntry {
                    typ,
                    compression,
                    data,
                },
            };
            if let Some((_, map_wad)) = cmap {
                map_wad.entries.push(entry);
            } else {
                entries.push(entry);
            }
        }
        Ok((table, Self { entries }))
    }
    pub fn extract_one(
        &self,
        index: usize,
        palettes: &mut PaletteCache,
        raw: bool,
    ) -> io::Result<Cow<[u8]>> {
        use LumpType::*;
        let FlatEntry { name, entry } = &self.entries[index];
        if raw {
            return Ok(Cow::Borrowed(entry.data.as_slice()));
        }
        let entry = entry.decompress().map_err(|e| {
            invalid_data(format!(
                "Failed to decompress entry `{}`: {}",
                name.display(),
                e,
            ))
        })?;
        Ok(match entry.typ {
            Palette => {
                let data = entry.data.get(8..).ok_or_else(|| {
                    invalid_data(format_args!("palette lump {} too small", name.display()))
                })?;
                let colors = (data.len() / 2).min(256);
                let mut palette = vec![0; colors * 3];
                gfx::palette_16_to_rgb(&data[..colors * 2], &mut palette);
                Cow::Owned(palette)
            }
            Graphic | Fire | Cloud => {
                context("Graphic", |d| gfx::Graphic::parse(d, entry.typ))(&entry.data)
                    .map_err(|e| {
                        invalid_data(format!(
                            "Failed to parse graphic `{}`: {}",
                            name.display(),
                            convert_error(entry.data.as_slice(), e),
                        ))
                    })
                    .and_then(|r| r.1.write_png().map_err(invalid_data))
                    .map(Cow::Owned)?
            }
            Texture | Flat => context("Texture", gfx::Texture::parse)(&entry.data)
                .map_err(|e| {
                    invalid_data(format!(
                        "Failed to parse texture `{}`: {}",
                        name.display(),
                        convert_error(entry.data.as_slice(), e),
                    ))
                })
                .and_then(|r| r.1.write_png().map_err(invalid_data))
                .map(Cow::Owned)?,
            HudGraphic | Sky => context("HudGraphic/Sky", gfx::Sprite::parse)(&entry.data)
                .map_err(|e| {
                    invalid_data(format!(
                        "Failed to parse HUD graphic `{}`: {}",
                        name.display(),
                        convert_error(entry.data.as_slice(), e),
                    ))
                })
                .and_then(|r| r.1.write_png(None).map_err(invalid_data))
                .map(Cow::Owned)?,
            Sprite => {
                let sprite = context("Sprite", gfx::Sprite::parse)(&entry.data)
                    .map_err(|e| {
                        invalid_data(format!(
                            "Failed to parse sprite `{}`: {}",
                            name.display(),
                            convert_error(entry.data.as_slice(), e),
                        ))
                    })?
                    .1;
                let palette = if let gfx::SpritePalette::Offset(offset) = &sprite.palette {
                    use std::collections::btree_map::Entry;

                    let palindex = index
                        .checked_sub(*offset as usize)
                        .ok_or_else(|| invalid_data("palette offset out of range"))?;
                    palettes.sprite_to_palette.insert(index, palindex);

                    match palettes.cache.entry(palindex) {
                        Entry::Vacant(e) => {
                            let palentry = self
                                .entries
                                .get(palindex)
                                .ok_or_else(|| invalid_data("palette offset out of range"))?;
                            match palentry.entry.typ {
                                Palette => {
                                    let entry = palentry.entry.decompress()?;
                                    let data = entry.data.get(8..).ok_or_else(|| {
                                        invalid_data(format_args!(
                                            "palette lump {} too small",
                                            palentry.name.display()
                                        ))
                                    })?;
                                    let colors = (data.len() / 2).min(256);
                                    let mut palette = vec![gfx::RGBA::default(); colors];
                                    gfx::palette_16_to_rgba(&data[..colors * 2], &mut palette);
                                    Some(e.insert(palette).as_slice())
                                }
                                Sprite => {
                                    let sprentry = palentry.entry.decompress()?;
                                    let pspr = gfx::Sprite::parse(&sprentry.data)
                                        .map_err(|e| {
                                            invalid_data(convert_error(sprentry.data.as_slice(), e))
                                        })?
                                        .1;
                                    let palette = match &pspr.palette {
                                        gfx::SpritePalette::Rgb8(palette) => palette.to_vec(),
                                        gfx::SpritePalette::Rgb4(palette) => palette.to_vec(),
                                        _ => {
                                            return Err(invalid_data(format_args!(
                                                "sprite {} does not contain a palette",
                                                palentry.name.display()
                                            )))
                                        }
                                    };
                                    Some(e.insert(palette).as_slice())
                                }
                                _ => {
                                    return Err(invalid_data(format_args!(
                                        "lump {} is not a palette or sprite",
                                        palentry.name.display()
                                    )));
                                }
                            }
                        }
                        Entry::Occupied(e) => Some(e.into_mut().as_slice()),
                    }
                } else {
                    None
                };
                sprite
                    .write_png(palette)
                    .map_err(invalid_data)
                    .map(Cow::Owned)?
            }
            _ => match entry {
                Cow::Borrowed(entry) => Cow::Borrowed(entry.data.as_slice()),
                Cow::Owned(entry) => Cow::Owned(entry.data),
            },
        })
    }
}

#[inline]
fn read_rom_data(data: &[u8], offset: u32, size: u32) -> &[u8] {
    &data[offset as usize..(offset + size) as usize]
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ReadFlags: u32 {
        const IWAD = 0b00000001;
        const SOUND = 0b00000010;
        const DECOMPRESS = 0b00000100;
    }
}

#[derive(Debug, Default)]
pub struct ReadPaths {
    pub filters: crate::FileFilters,
    pub wdd: Option<PathBuf>,
    pub wmd: Option<PathBuf>,
    pub wsd: Option<PathBuf>,
    pub dls: Option<PathBuf>,
}

pub fn read_rom_or_iwad(
    path: impl AsRef<Path>,
    flags: ReadFlags,
    paths: &ReadPaths,
) -> io::Result<(Option<FlatWad>, Option<SoundData>)> {
    let path = path.as_ref();
    let decompress = flags.contains(ReadFlags::DECOMPRESS);
    log::info!("Reading `{}`", path.display());
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 64];
    file.read_exact(header.as_mut_slice())?;
    let mut wad = None;
    let mut snd = None;
    let mut wad_type = WadType::N64;
    if &header[..4] == b"PWAD" || &header[..4] == b"IWAD" {
        let mut data = Vec::new();
        file.seek(io::SeekFrom::Start(0))?;
        file.read_to_end(&mut data)?;
        if &header[..4] == b"IWAD"
            && data.len() == crate::remaster::REMASTER_WAD_SIZE
            && <sha2::Sha256 as sha2::Digest>::digest(&data).as_slice()
                == crate::remaster::REMASTER_WAD_HASH
        {
            wad_type = WadType::Remaster;
        }
        if flags.contains(ReadFlags::IWAD) {
            wad = Some(data);
        }
        if &header[..4] == b"IWAD" && !wad_type.is_remaster() && flags.contains(ReadFlags::SOUND) {
            fn read_sound_data(
                filename: &Option<PathBuf>,
                path: &Path,
                ext: &str,
            ) -> std::io::Result<Vec<u8>> {
                let filename = filename
                    .as_ref()
                    .map(|p| Cow::Borrowed(p.as_path()))
                    .unwrap_or_else(|| Cow::Owned(path.with_extension(ext)));
                log::info!("Reading `{}`", filename.display());
                std::fs::read(filename)
            }
            let wmd = read_sound_data(&paths.wmd, path, "WMD")?;
            let wsd = read_sound_data(&paths.wsd, path, "WSD")?;
            let wdd = read_sound_data(&paths.wdd, path, "WDD")?;
            snd = Some(crate::sound::extract_sound(&wmd, &wsd, &wdd, decompress)?);
        }
    } else {
        let end = file.seek(io::SeekFrom::End(0))?;
        if end != 0x800000 && end != 0x1000000 {
            return Err(invalid_data(format_args!(
                "Invalid ROM size {}, expected exactly 8 MiB or 16MiB",
                end
            )));
        }
        let region = match header[0x3e] {
            0x45 => Region::US,
            0x4A => Region::JP,
            0x50 => Region::EU,
            r => return Err(invalid_data(format_args!("Unknown region 0x{:02x}", r))),
        };
        let revision = header[0x3f];
        let data = if region == Region::US && end == 0x1000000 {
            wad_type = WadType::N64Prototype;
            ROMDATA_PROTO
        } else {
            RomData::new(region, revision)?
        };
        let name = &header[0x20..0x34];
        if name != data.name {
            return Err(invalid_data(format_args!("Unknown ROM Name: {:?}", name)));
        }
        file.seek(io::SeekFrom::Start(0))?;
        let mut rom = Vec::new();
        file.read_to_end(&mut rom)?;
        let digest = <sha2::Sha256 as sha2::Digest>::digest(&rom);
        if digest.as_slice() != data.sha256 {
            return Err(invalid_data(format_args!(
                "Bad sha256 hash {:x}, expected {:x}",
                digest.iter().format(""),
                data.sha256.iter().format(""),
            )));
        }
        if flags.contains(ReadFlags::IWAD) {
            wad = Some(read_rom_data(&rom, data.wad_offset, data.wad_size).to_vec());
        }
        if flags.contains(ReadFlags::SOUND) {
            let wmd = read_rom_data(&rom, data.wmd_offset, data.wmd_size);
            let wsd = read_rom_data(&rom, data.wsd_offset, data.wsd_size);
            let wdd = read_rom_data(&rom, data.wdd_offset, data.wdd_size);
            snd = Some(crate::sound::extract_sound(wmd, wsd, wdd, decompress)?);
        }
        log::info!(
            "Loaded ROM `{}`: {region:?} v1.{revision}",
            path.file_name()
                .map(|p| Path::new(p).display())
                .unwrap_or_else(|| Path::new("(Unknown)").display())
        );
    }
    let wad = if let Some(wad) = wad {
        if wad_type.is_remaster() {
            if flags.contains(ReadFlags::SOUND) {
                snd = Some(SoundData::default());
            }
            let wad = crate::remaster::read_wad(&wad, snd.as_mut(), &paths.filters)?;
            if let Some(snd) = snd.as_mut() {
                let dls = paths
                    .dls
                    .as_ref()
                    .map(|p| Cow::Borrowed(p.as_path()))
                    .unwrap_or_else(|| {
                        let p = Path::new(crate::remaster::REMASTER_DLS_NAME);
                        if let Some(parent) = path.parent() {
                            Cow::Owned(parent.join(p))
                        } else {
                            Cow::Borrowed(p)
                        }
                    });
                log::info!("Reading `{}`", dls.display());
                let dls = std::fs::read(dls)?;
                if dls.len() == crate::remaster::REMASTER_DLS_SIZE {
                    let digest = <sha2::Sha256 as sha2::Digest>::digest(&dls);
                    if digest.as_slice() == crate::remaster::REMASTER_DLS_HASH {
                        crate::remaster::read_dls(&dls, snd)?;
                    } else {
                        return Err(invalid_data(format_args!(
                            "Remaster DLS: Bad sha256 hash {}, expected {}",
                            digest.iter().format(""),
                            crate::remaster::REMASTER_DLS_HASH.iter().format(""),
                        )));
                    }
                } else {
                    return Err(invalid_data(format_args!(
                        "Remaster DLS: Bad size {}, expected {}",
                        dls.len(),
                        crate::remaster::REMASTER_DLS_SIZE,
                    )));
                }
            }
            Some(wad)
        } else {
            Some(
                FlatWad::parse(&wad, wad_type, decompress, &paths.filters)
                    .map(|w| w.1)
                    .map_err(|e| {
                        invalid_data(format!(
                            "Failed reading {}: {}",
                            path.display(),
                            convert_error(wad.as_slice(), e)
                        ))
                    })?,
            )
        }
    } else {
        None
    };
    Ok((wad, snd))
}

#[derive(Debug, Default)]
pub struct PaletteCache {
    pub cache: BTreeMap<usize, Vec<gfx::RGBA>>,
    pub sprite_to_palette: BTreeMap<usize, usize>,
}

pub fn extract(mut args: Args) -> io::Result<()> {
    use LumpType::*;

    args.outdir = Some(args.outdir.unwrap_or_else(|| PathBuf::from("DOOM64")));
    let outdir = args.outdir.as_deref().unwrap();

    let paths = ReadPaths {
        filters: crate::FileFilters {
            includes: args.include.clone(),
            excludes: Vec::new(),
        },
        wdd: args.wdd.clone(),
        wmd: args.wmd.clone(),
        wsd: args.wsd.clone(),
        dls: args.dls.clone(),
    };
    let mut flags = ReadFlags::IWAD | ReadFlags::SOUND;
    if !args.no_decompress {
        flags |= ReadFlags::DECOMPRESS;
    }
    let (wad, snd) = read_rom_or_iwad(&args.input, flags, &paths)?;
    let wad = wad.unwrap();

    let mut palettes = PaletteCache::default();
    if args.outfile.is_none() {
        std::fs::create_dir_all(outdir).unwrap();
    }
    fn ext_for(typ: LumpType) -> &'static str {
        match typ {
            Sprite | Texture | Flat | Graphic | HudGraphic | Sky | Fire | Cloud => "PNG",
            Palette => "PAL",
            Map => "WAD",
            Unknown | Demo => "LMP",
            Marker | Sample | SoundFont | Sequence | MapLump => unreachable!(),
        }
    }
    fn subdir_for(typ: LumpType) -> Option<&'static str> {
        Some(match typ {
            Unknown => return None,
            Sprite => "SPRITES",
            Palette => "PALETTES",
            Texture => "TEXTURES",
            Flat => "FLATS",
            Graphic => "GRAPHICS",
            HudGraphic => "HUD",
            Sky | Fire | Cloud => "SKIES",
            Map => "MAPS",
            Demo => "DEMOS",
            Marker | Sample | SoundFont | Sequence | MapLump => unreachable!(),
        })
    }
    for (index, FlatEntry { name, entry }) in wad.entries.iter().enumerate() {
        if entry.typ == Marker {
            continue;
        }
        if let Some(mut file) = args.try_create_file(
            || subdir_for(entry.typ),
            || name.display(),
            || ext_for(entry.typ),
            entry.compression.is_compressed().then_some("LMPZ"),
        )? {
            let data = wad.extract_one(index, &mut palettes, args.raw)?;
            file.write_all(&data).unwrap();
        }
        if args.outfile.is_some() {
            break;
        }
    }
    if let Some(snd) = snd {
        if !snd.instruments.is_empty() {
            if let Some(mut file) = args.try_create_file(
                || Some("MUSIC"),
                || Cow::Borrowed("DOOMSND"),
                || "SF2",
                Some("WMD"),
            )? {
                if args.raw {
                    snd.write_wmd(&mut file).unwrap();
                } else {
                    snd.write_sf2(&mut file).unwrap();
                }
            }
        }
        for (index, seq) in &snd.sequences {
            match seq {
                crate::sound::Sequence::MusicSeq(seq) => {
                    if let Some(mut file) = args.try_create_file(
                        || Some("MUSIC"),
                        || Cow::Owned(format!("MUS_{index:03}")),
                        || "MID",
                        Some("SEQ"),
                    )? {
                        if args.raw {
                            seq.write_raw(&mut file).unwrap();
                        } else {
                            seq.write_midi(&snd, &mut file).unwrap();
                        }
                    }
                }
                crate::sound::Sequence::Effect(sample) => {
                    if let Some(mut file) = args.try_create_file(
                        || Some("SOUNDS"),
                        || Cow::Owned(format!("SFX_{index:03}")),
                        || "WAV",
                        Some(match &sample.info.samples {
                            SampleData::Raw(_) => "LMP",
                            SampleData::Adpcm { .. } => "VAD",
                        }),
                    )? {
                        if args.raw {
                            match &sample.info.samples {
                                SampleData::Raw(samples) => {
                                    for s in samples.iter().copied() {
                                        file.write_all(&s.to_be_bytes())?;
                                    }
                                }
                                SampleData::Adpcm { data, book, .. } => {
                                    file.write_all(&book.order.to_be_bytes())?;
                                    file.write_all(&book.npredictors.to_be_bytes())?;
                                    for v in &book.book {
                                        file.write_all(&v.to_be_bytes())?;
                                    }
                                    file.write_all(data)?;
                                }
                            }
                        } else {
                            sample.write_wav(&mut file)?;
                        }
                    }
                }
                crate::sound::Sequence::MusicSample(_) => unreachable!(),
            }
            if args.outfile.is_some() {
                break;
            }
        }
    }
    Ok(())
}

impl Args {
    fn try_create_file<'a>(
        &self,
        mk_subdir: impl FnOnce() -> Option<&'a str>,
        mk_filename: impl FnOnce() -> Cow<'a, str>,
        mk_ext: impl FnOnce() -> &'a str,
        raw_ext: Option<&str>,
    ) -> std::io::Result<Option<impl std::io::Write>> {
        let Self {
            outdir,
            outfile,
            include,
            flat,
            raw,
            ..
        } = self;
        let outdir = outdir.as_deref().unwrap();
        let filename = if let Some(outfile) = &outfile {
            Cow::Borrowed(outfile)
        } else {
            #[allow(unused_mut)]
            let mut filename = mk_filename();
            #[cfg(windows)]
            {
                filename = Cow::Owned(filename.replace('\\', "^").replace('?', "@"));
            }
            if !include.is_empty() && !include.iter().any(|g| glob_match::glob_match(g, &filename))
            {
                return Ok(None);
            }
            let filename = Path::new(filename.as_ref());
            let dir = match (!flat).then_some(mk_subdir()).flatten() {
                Some(subdir) => {
                    let dir = outdir.join(Path::new(subdir));
                    std::fs::create_dir_all(&dir).unwrap();
                    Cow::Owned(dir)
                }
                None => Cow::Borrowed(outdir),
            };
            let mut filename = dir.join(filename);
            if *raw {
                filename.set_extension(raw_ext.unwrap_or("LMP"));
            } else {
                filename.set_extension(mk_ext());
            }
            Cow::Owned(filename)
        };
        log::debug!("writing `{}`", filename.display());
        std::fs::File::create(filename.as_path())
            .map(std::io::BufWriter::new)
            .map(Some)
    }
}
