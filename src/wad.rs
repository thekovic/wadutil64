use crate::{convert_error, extract::WadType, gfx, invalid_data};
use arrayvec::ArrayVec;
use indexmap::IndexMap;
use nom::error::{context, VerboseError};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryName(pub ArrayVec<u8, 8>);

pub type EntryMap<T> = IndexMap<EntryName, WadEntry<T>>;

#[derive(Clone, Default, Debug)]
pub struct Wad {
    pub maps: EntryMap<FlatWad>,
    pub palettes: EntryMap<[gfx::RGBA; 256]>,
    pub sprites: EntryMap<gfx::Sprite>,
    pub textures: EntryMap<gfx::Texture>,
    pub flats: EntryMap<gfx::Texture>,
    pub graphics: EntryMap<gfx::Graphic>,
    pub hud_graphics: EntryMap<gfx::Sprite>,
    pub skies: EntryMap<gfx::Sprite>,
    pub other: EntryMap<Vec<u8>>,
}

#[derive(Clone, Default, Debug)]
pub struct FlatWad {
    pub entries: Vec<FlatEntry<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LumpType {
    Unknown,
    Marker,
    Sprite,
    Palette,
    Texture,
    Flat,
    Graphic,
    HudGraphic,
    Sky,
    Fire,
    Cloud,
    Map,
    MapLump,
    Demo,
    Sample,
    SoundFont,
    Sequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compression {
    None,
    Lzss(usize),
    Huffman(usize),
}

impl Compression {
    #[inline]
    pub fn is_compressed(&self) -> bool {
        !matches!(self, Compression::None)
    }
    #[inline]
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Lzss(_) => "Jaguar",
            Self::Huffman(_) => "D64",
        }
    }
}

#[derive(Clone, Debug)]
pub struct WadEntry<T> {
    pub typ: LumpType,
    pub compression: Compression,
    pub data: T,
}

impl WadEntry<Vec<u8>> {
    pub fn uncompressed_len(&self) -> usize {
        match &self.compression {
            Compression::None => self.data.len(),
            Compression::Lzss(s) => *s,
            Compression::Huffman(s) => *s,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlatEntry<T> {
    pub name: EntryName,
    pub entry: WadEntry<T>,
}

impl LumpType {
    pub fn compression(&self) -> Compression {
        match self {
            Self::Map | Self::Demo | Self::Texture | Self::Flat => Compression::Huffman(0),
            Self::Sprite
            | Self::Sky
            | Self::Fire
            | Self::Cloud
            | Self::Graphic
            | Self::HudGraphic => Compression::Lzss(0),
            _ => Compression::None,
        }
    }
}

fn replace<T>(map: &mut EntryMap<T>, name: EntryName, entry: WadEntry<T>) {
    use indexmap::map::Entry::*;
    match map.entry(name) {
        Vacant(e) => {
            e.insert(entry);
        }
        Occupied(mut e) => {
            e.insert(entry);
        }
    }
}

impl Wad {
    pub fn merge(&mut self, other: Self) {
        fn merge<T>(a: &mut EntryMap<T>, b: EntryMap<T>) {
            let iter = b.into_iter();
            let reserve = if a.is_empty() {
                iter.size_hint().0
            } else {
                (iter.size_hint().0 + 1) / 2
            };
            a.reserve(reserve);
            for (name, entry) in iter {
                replace(a, name, entry);
            }
        }
        merge(&mut self.maps, other.maps);
        merge(&mut self.palettes, other.palettes);
        merge(&mut self.sprites, other.sprites);
        merge(&mut self.textures, other.textures);
        merge(&mut self.flats, other.flats);
        merge(&mut self.graphics, other.graphics);
        merge(&mut self.hud_graphics, other.hud_graphics);
        merge(&mut self.skies, other.skies);
        merge(&mut self.other, other.other);
    }
    pub fn merge_one(&mut self, name: EntryName, entry: WadEntry<Vec<u8>>) -> std::io::Result<()> {
        let WadEntry { typ, data, .. } = entry;
        match typ {
            // important: must load and rewrite map wad to have proper 4-byte alignments
            LumpType::Map => {
                match FlatWad::parse(&data, WadType::N64Map, false, &Default::default()) {
                    Ok((_, wad)) => replace(&mut self.maps, name, WadEntry::new(typ, wad)),
                    Err(e) => return Err(invalid_data(format!(
                        "Failed to load map {}: {}",
                        name.display(),
                        crate::convert_error(data.as_slice(), e)
                    ))),
                }
            }
            LumpType::Palette => {
                if let Some(data) = data.get(8..8 + 256 * 2) {
                    let mut palette = [gfx::RGBA::default(); 256];
                    gfx::palette_16_to_rgba(data, &mut palette);
                    replace(&mut self.palettes, name, WadEntry::new(typ, palette));
                } else {
                    return Err(invalid_data(format!("Palette {} does not have enough entries", name.display())));
                }
            }
            LumpType::Sprite => match gfx::Sprite::parse(&data) {
                Ok((_, sprite)) => replace(&mut self.sprites, name, WadEntry::new(typ, sprite)),
                Err(e) => return Err(invalid_data(format!(
                    "Invalid sprite {}: {}",
                    name.display(),
                    crate::convert_error(data.as_slice(), e)
                ))),
            },
            LumpType::Texture => match gfx::Texture::parse(&data) {
                Ok((_, texture)) => replace(&mut self.textures, name, WadEntry::new(typ, texture)),
                Err(e) => return Err(invalid_data(format!(
                    "Invalid texture {}: {}",
                    name.display(),
                    crate::convert_error(data.as_slice(), e)
                ))),
            },
            LumpType::Flat => match gfx::Texture::parse(&data) {
                Ok((_, flat)) => replace(&mut self.flats, name, WadEntry::new(typ, flat)),
                Err(e) => return Err(invalid_data(format!(
                    "Invalid flat {}: {}",
                    name.display(),
                    crate::convert_error(data.as_slice(), e)
                ))),
            },
            LumpType::Graphic | LumpType::Fire | LumpType::Cloud => {
                match gfx::Graphic::parse(&data, typ) {
                    Ok((_, graphic)) => {
                        replace(&mut self.graphics, name, WadEntry::new(typ, graphic))
                    }
                    Err(e) => return Err(invalid_data(format!(
                        "Invalid graphic {}: {}",
                        name.display(),
                        crate::convert_error(data.as_slice(), e)
                    ))),
                }
            }
            LumpType::HudGraphic => match gfx::Sprite::parse(&data) {
                Ok((_, sprite)) => {
                    replace(&mut self.hud_graphics, name, WadEntry::new(typ, sprite))
                }
                Err(e) => return Err(invalid_data(format!(
                    "Invalid HUD graphic {}: {}",
                    name.display(),
                    crate::convert_error(data.as_slice(), e)
                ))),
            },
            LumpType::Sky => match gfx::Sprite::parse(&data) {
                Ok((_, sprite)) => replace(&mut self.skies, name, WadEntry::new(typ, sprite)),
                Err(e) => return Err(invalid_data(format!(
                    "Invalid sky {}: {}",
                    name.display(),
                    crate::convert_error(data.as_slice(), e)
                ))),
            },
            LumpType::Marker => {}
            _ => replace(&mut self.other, name, WadEntry::new(typ, data)),
        }
        Ok(())
    }
    #[inline]
    pub fn merge_flat(&mut self, other: FlatWad, ignore_errors: bool) -> std::io::Result<()> {
        for FlatEntry { name, entry } in other.entries {
            if let Err(err) = self.merge_one(name, entry) {
                match ignore_errors {
                    true => log::warn!("{err}"),
                    false => return Err(err),
                }
            }
        }
        Ok(())
    }
}

impl FlatWad {
    pub fn append<T: Into<Vec<u8>>>(&mut self, other: EntryMap<T>) {
        self.entries.reserve(other.len());
        for (name, entry) in other {
            self.entries.push(FlatEntry::new_entry(name, entry));
        }
    }
}

impl<T> FlatEntry<T> {
    #[inline]
    pub fn new(name: &str, typ: LumpType, data: T) -> Self {
        Self {
            name: EntryName::new(name).unwrap(),
            entry: WadEntry::new(typ, data),
        }
    }
    #[inline]
    pub fn new_entry<U: Into<T>>(name: EntryName, entry: WadEntry<U>) -> Self {
        Self {
            name,
            entry: WadEntry::new(entry.typ, entry.data.into()),
        }
    }
    #[inline]
    pub fn marker(name: &str) -> Self
    where
        T: Default,
    {
        Self::new(name, LumpType::Marker, Default::default())
    }
}

impl<T> WadEntry<T> {
    pub fn new(typ: LumpType, data: T) -> Self {
        Self {
            typ,
            compression: Compression::None,
            data,
        }
    }
}

impl WadEntry<Vec<u8>> {
    #[inline]
    pub fn padded_len(&self) -> Option<u32> {
        let len = u32::try_from(self.data.len()).ok()?;
        Some(len.checked_add(3)? & !3)
    }
    pub fn decompress(&self) -> std::io::Result<Cow<'_, Self>> {
        let res = match self.compression {
            Compression::None => return Ok(Cow::Borrowed(self)),
            Compression::Lzss(size) => context("Jag Decompression", |d| {
                crate::compression::decode_jaguar::<VerboseError<_>>(d, size)
            })(&self.data),
            Compression::Huffman(size) => context("D64 Decompression", |d| {
                crate::compression::decode_d64::<VerboseError<_>>(d, size)
            })(&self.data),
        };
        let data = res
            .map_err(|e| invalid_data(convert_error(self.data.as_slice(), e)))?
            .1;
        Ok(Cow::Owned(Self {
            typ: self.typ,
            compression: Compression::None,
            data,
        }))
    }
}

impl EntryName {
    #[inline]
    pub fn new(name: &str) -> Option<Self> {
        ArrayVec::try_from(name.as_bytes()).ok().map(Self)
    }
    #[inline]
    pub fn display(&self) -> std::borrow::Cow<str> {
        String::from_utf8_lossy(&self.0)
    }
}

impl std::borrow::Borrow<[u8]> for EntryName {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}
