use crate::{
    convert_error,
    extract::{read_rom_or_iwad, ReadFlags},
    gfx, invalid_data,
    sound::SoundData,
    wad::{EntryMap, FlatEntry},
    EntryName, FileFilters, FlatWad, LumpType, Wad, WadEntry, lumps::TEXTURE_ORDER,
};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub struct Args {
    /// Directories and ROMs to build into IWAD
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// IWAD file to output to [default: DOOM64.WAD]
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Glob patterns to exclude entry names
    #[arg(short, long)]
    exclude: Vec<String>,
    /// Do not generate WDD/WMD/WSD files
    #[arg(long, default_value_t = false)]
    no_sound: bool,
    /// Ignore errors when parsing input files
    #[arg(long, default_value_t = false)]
    ignore_errors: bool,
    /// Apply minor vanilla asset fixes
    #[arg(long, default_value_t = true)]
    apply_fixes: bool,
    /// Path to output WDD to [default: DOOM64.WDD]
    #[arg(long)]
    wdd: Option<PathBuf>,
    /// Path to output WMD to [default: DOOM64.WMD]
    #[arg(long)]
    wmd: Option<PathBuf>,
    /// Path to output WSD to [default: DOOM64.WSD]
    #[arg(long)]
    wsd: Option<PathBuf>,
}

struct LoadOptions<'a> {
    filters: &'a FileFilters,
    ignore_errors: bool,
}

fn load_entry(
    wad: &mut Wad,
    snd: &mut SoundData,
    path: impl AsRef<Path>,
    read: impl FnOnce() -> io::Result<Vec<u8>>,
    base_typ: LumpType,
    options: &LoadOptions,
) -> io::Result<()> {
    use LumpType::*;

    let path = path.as_ref();
    let name = match path.file_stem() {
        Some(n) => n,
        None => return Ok(()),
    };
    let name_str = name.to_string_lossy().to_ascii_uppercase();
    if name_str.starts_with('.') || name_str.len() > 8 {
        return Ok(());
    }
    if !options.filters.matches(&name_str) {
        log::debug!("Skipping file `{}`", path.display());
        return Ok(());
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_uppercase());
    let mut typ = match (base_typ, ext.as_deref()) {
        (Sprite, Some("LMP") | Some("PAL")) => Palette,
        (Sequence, Some("SF2") | Some("DLS")) => SoundFont,
        (Unknown, Some("PNG")) => Graphic,
        (Unknown, Some("WAD")) => Map,
        (Unknown, _) if name_str.starts_with("MAP") => Map,
        (Unknown, _) if name_str.starts_with("DEMO") => Demo,
        _ => base_typ,
    };
    if typ == Sky {
        if name_str == "FIRE" {
            typ = Fire;
        } else if name_str == "CLOUD" {
            typ = Cloud;
        }
    }
    log::debug!("Reading file `{}` of type {:?}", path.display(), typ);
    let data = read()?;
    let is_png = ext.as_deref() == Some("PNG");
    let data = match (typ, is_png) {
        (Palette, _) if ext.as_deref() == Some("PAL") => {
            if data.len() >= 256 * 3 {
                let mut palette = vec![0; 8 + 256 * 2];
                palette[2] = 1;
                gfx::palette_rgb_to_16(&data, &mut palette[8..]);
                palette
            } else {
                panic!("Palette {name_str} does not have enough entries");
            }
        }
        (Graphic | Fire | Cloud, true) => gfx::Graphic::read_png(&data, false)
            .map_err(invalid_data)?
            .to_vec(typ),
        (Texture | Flat, true) => gfx::Texture::read_png(&data)
            .map_err(invalid_data)?
            .to_vec(),
        (Sprite | HudGraphic | Sky, true) => gfx::Sprite::read_png(&data, None)
            .map_err(invalid_data)?
            .to_vec(),
        _ => data,
    };
    match typ {
        Sample => {
            let id = name_str
                .strip_prefix("SFX_")
                .and_then(|n| str::parse(n).ok())
                .unwrap_or_else(|| {
                    snd.sequences
                        .last_key_value()
                        .map(|p| *p.0 + 1)
                        .unwrap_or_default()
                });
            let res = crate::sound::Sample::read_file(&data).map_err(|e| {
                invalid_data(format!(
                    "Failed to load sound effect `{}`:\n{e}",
                    path.display(),
                ))
            });
            match res {
                Ok(sample) => {
                    snd.sequences
                        .insert(id, crate::sound::Sequence::Effect(sample));
                }
                Err(err) => match options.ignore_errors {
                    true => log::warn!("{err}"),
                    false => return Err(err),
                },
            }
        }
        SoundFont => {
            let res = if ext.as_deref() == Some("DLS") {
                snd.read_dls(&data)
            } else {
                snd.read_sf2(&data)
            };
            if let Err(err) = res.map_err(|e| {
                invalid_data(format!(
                    "Failed to load SoundFont `{}`:\n{}",
                    path.display(),
                    convert_error(data.as_slice(), e)
                ))
            }) {
                match options.ignore_errors {
                    true => log::warn!("{err}"),
                    false => return Err(err),
                }
            }
        }
        Sequence => {
            let id = name_str
                .strip_prefix("MUS_")
                .and_then(|n| str::parse(n).ok())
                .unwrap_or_else(|| {
                    snd.sequences
                        .last_key_value()
                        .map(|p| *p.0 + 1)
                        .unwrap_or_default()
                });
            let res = if data.get(0..4) == Some(b"MThd") {
                crate::music::MusicSequence::read_midi(&mut std::io::Cursor::new(data))
                    .map(crate::sound::Sequence::MusicSeq)
            } else {
                crate::sound::Sample::read_file(&data).map(|sample| {
                    crate::sound::Sequence::MusicSample(crate::music::MusicSample::new(sample))
                })
            };
            match res.map_err(|e| {
                invalid_data(format!("Failed to load music `{}`:\n{e}", path.display(),))
            }) {
                Ok(seq) => {
                    snd.sequences.insert(id, seq);
                }
                Err(err) => match options.ignore_errors {
                    true => log::warn!("{err}"),
                    false => return Err(err),
                },
            }
        }
        _ => {
            let mut upper = name_str.replace('^', "\\");
            upper = upper.replace('@', "?");
            upper.make_ascii_uppercase();
            let name = EntryName::new(&upper).unwrap();
            let entry = WadEntry::new(typ, data);
            if let Err(err) = wad.merge_one(name, entry) {
                match options.ignore_errors {
                    true => log::warn!("{err}"),
                    false => return Err(err),
                }
            }
        }
    }
    Ok(())
}

fn type_for_dir(name: &std::ffi::OsStr) -> Option<LumpType> {
    use LumpType::*;

    let upper = name.to_ascii_uppercase();
    Some(match upper.to_str() {
        Some("SPRITES") => Sprite,
        Some("PALETTES") => Palette,
        Some("TEXTURES") => Texture,
        Some("FLATS") => Flat,
        Some("GRAPHICS") => Graphic,
        Some("HUD") => HudGraphic,
        Some("SKIES") => Sky,
        Some("MAPS") => Map,
        Some("SOUNDS") => Sample,
        Some("MUSIC") => Sequence,
        Some("DEMOS") => Demo,
        _ => return None,
    })
}

fn load_entries(
    wad: &mut Wad,
    snd: &mut SoundData,
    path: impl AsRef<Path>,
    meta: Option<std::fs::Metadata>,
    base_typ: LumpType,
    depth: usize,
    options: &LoadOptions,
) -> io::Result<()> {
    let path = path.as_ref();
    let meta = match meta {
        Some(meta) => meta,
        None => path.metadata()?,
    };
    if meta.is_file() {
        let res = load_entry(wad, snd, path, || std::fs::read(path), base_typ, options);
        if let Err(err) = res {
            let err = format!("Error reading {}: {err}", path.display());
            match options.ignore_errors {
                true => log::warn!("{err}"),
                false => return Err(invalid_data(err)),
            }
        }
    } else if meta.is_dir() {
        let name = match path.file_name() {
            Some(n) => n,
            None => return Ok(()),
        };
        let base_typ = if depth < 2 {
            type_for_dir(name).unwrap_or(base_typ)
        } else {
            base_typ
        };
        let dir = std::fs::read_dir(path)?;
        for entry in dir.flatten() {
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            load_entries(
                wad,
                snd,
                entry.path(),
                Some(meta),
                base_typ,
                depth + 1,
                options,
            )?;
        }
    }
    Ok(())
}

#[inline]
fn is_map_wad(path: &impl AsRef<Path>) -> bool {
    if let Some(stem) = path.as_ref().file_stem() {
        stem.to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("MAP")
    } else {
        false
    }
}

impl FlatWad {
    pub fn write(&self, out: &mut impl std::io::Write, verbose: bool) -> io::Result<()> {
        let count =
            u32::try_from(self.entries.len()).map_err(|_| invalid_data("too many entries"))?;
        let mut offset = 0xcu32;
        for entry in &self.entries {
            offset = entry
                .entry
                .padded_len()
                .and_then(|s| s.checked_add(offset))
                .ok_or_else(|| {
                    invalid_data(format_args!("entry {} too large", entry.name.display()))
                })?;
        }
        out.write_all(b"IWAD")?;
        out.write_all(&count.to_le_bytes())?;
        out.write_all(&offset.to_le_bytes())?;

        for entry in &self.entries {
            if verbose {
                let size = entry.entry.data.len();
                let name = entry.name.display();
                let hash = blake3::hash(&entry.entry.data);
                log::debug!("  0x{size: <8x} {name: <8} 0x{hash}");
            }
            const PAD_BYTES: [u8; 4] = [0; 4];
            out.write_all(&entry.entry.data)?;
            let len = entry.entry.data.len() as u32;
            let padded_len = entry.entry.padded_len().unwrap();
            let padding = (padded_len - len) as usize;
            if padding > 0 {
                out.write_all(&PAD_BYTES[..padding])?;
            }
        }
        let mut offset = 0xcu32;
        for entry in &self.entries {
            let size = entry.entry.uncompressed_len() as u32;
            out.write_all(&offset.to_le_bytes())?;
            out.write_all(&size.to_le_bytes())?;
            let mut name = entry.name.0.clone();
            while name.len() < name.capacity() {
                name.push(0);
            }
            let mut name = name.into_inner().unwrap();
            if entry.entry.compression.is_compressed() {
                name[0] |= 0x80;
            }
            out.write_all(&name)?;
            offset += entry.entry.padded_len().unwrap();
        }
        Ok(())
    }
}

#[inline]
fn name_sort<T>(
    a: &EntryName,
    _: &WadEntry<T>,
    b: &EntryName,
    _: &WadEntry<T>,
) -> std::cmp::Ordering {
    a.cmp(b)
}

#[inline]
fn other_sort<T>(
    ak: &EntryName,
    a: &WadEntry<T>,
    bk: &EntryName,
    b: &WadEntry<T>,
) -> std::cmp::Ordering {
    match a.typ.cmp(&b.typ) {
        std::cmp::Ordering::Equal => ak.cmp(bk),
        o => o,
    }
}

fn take_entry<T>(
    map: &mut EntryMap<T>,
    mut pred: impl FnMut(&EntryName, &WadEntry<T>) -> bool,
) -> Option<(EntryName, WadEntry<T>)> {
    let index = map.iter().position(|(k, v)| pred(k, v))?;
    map.shift_remove_index(index)
}

fn order_fixed<T>(entries: &mut EntryMap<T>, order: &[&[u8]]) {
    let mut count = 0;
    for tex in order.iter().copied() {
        if let Some(index) = entries.get_index_of(tex) {
            if count != index {
                entries.move_index(index, count);
                count += 1;
            }
        }
    }
}

impl Wad {
    pub fn sort(&mut self) {
        self.maps.sort_by(name_sort);
        self.palettes.sort_by(name_sort);
        self.sprites.sort_by(name_sort);
        order_fixed(&mut self.sprites, crate::lumps::SPRITE_ORDER);
        self.textures.sort_by(name_sort);
        order_fixed(&mut self.textures, crate::lumps::TEXTURE_ORDER);
        self.flats.sort_by(name_sort);
        order_fixed(&mut self.flats, crate::lumps::FLAT_ORDER);
        self.graphics.sort_by(name_sort);
        self.hud_graphics.sort_by(name_sort);
        self.skies.sort_by(name_sort);
        self.other.sort_by(other_sort);
    }
    pub fn flatten(mut self) -> FlatWad {
        let mut flat = FlatWad::default();
        let mut sprite_prefixes = BTreeMap::new();

        flat.entries.push(FlatEntry::marker("S_START"));
        for (name, mut sprite) in self.sprites {
            let name = name.0;
            if name.len() >= 4 && !name.starts_with(b"PAL") {
                use std::collections::btree_map::Entry;

                let prefix = <[u8; 4]>::try_from(&name[..4]).unwrap();
                let palettes = match sprite_prefixes.entry(prefix) {
                    Entry::Vacant(entry) => {
                        let index = flat.entries.len();
                        let pal_prefix =
                            [b'P', b'A', b'L', prefix[0], prefix[1], prefix[2], prefix[3]];
                        let mut palettes = Vec::new();
                        while let Some((name, palette)) = take_entry(&mut self.palettes, |k, _| {
                            k.0.starts_with(&pal_prefix) && k.0.len() == 8
                        }) {
                            let mut data = vec![0; palette.data.len() * 2 + 8];
                            data[2] = 1;
                            gfx::palette_rgba_to_16(&palette.data, &mut data[8..]);
                            palettes.push(palette.data);
                            flat.entries.push(FlatEntry::new_entry(
                                name,
                                WadEntry::new(LumpType::Palette, data),
                            ));
                        }
                        (!palettes.is_empty()).then(|| entry.insert((index, palettes)))
                    }
                    Entry::Occupied(entry) => Some(entry.into_mut()),
                };
                if let Some((index, palettes)) = palettes {
                    if let gfx::SpritePalette::Rgb8(sprite_pal) = &sprite.data.palette {
                        if let Some(offset) =
                            palettes.iter().position(|p| p == sprite_pal.as_slice())
                        {
                            let index = u16::try_from(flat.entries.len() - *index + offset)
                                .expect("too many sprites");
                            sprite.data.palette = gfx::SpritePalette::Offset(index);
                        }
                    }
                }
            }
            flat.entries
                .push(FlatEntry::new_entry(EntryName(name), sprite));
        }
        flat.entries.push(FlatEntry::marker("S_END"));

        flat.entries.push(FlatEntry::marker("T_START"));
        flat.entries
            .reserve(self.textures.len() + self.flats.len() + 2);
        let texend = self.textures.iter().position(|e| &e.0.0 == *TEXTURE_ORDER.last().unwrap())
            .map(|p| p + 1)
            .unwrap_or_else(|| self.textures.len());
        let mut textures = self.textures.into_iter();
        let mut i = 0;
        while let Some((name, entry)) = textures.next() {
            flat.entries.push(FlatEntry::new_entry(name, entry));
            i += 1;
            if i >= texend {
                break;
            }
        }
        for (name, entry) in self.flats {
            flat.entries.push(FlatEntry::new_entry(name, entry));
        }
        for (name, entry) in textures {
            flat.entries.push(FlatEntry::new_entry(name, entry));
        }
        flat.entries.push(FlatEntry::marker("T_END"));

        flat.append(self.hud_graphics);
        flat.entries.reserve(self.graphics.len());
        for (name, entry) in self.graphics {
            flat.entries.push(FlatEntry {
                name,
                entry: WadEntry::new(entry.typ, entry.data.to_vec(entry.typ)),
            });
        }
        flat.append(self.skies);
        flat.entries.reserve(self.maps.len());
        for (name, entry) in self.maps {
            let mut data = Vec::new();
            entry.data.write(&mut data, false).unwrap();
            flat.entries.push(FlatEntry {
                name,
                entry: WadEntry::new(entry.typ, data),
            });
        }
        flat.append(self.other);

        flat.entries.push(FlatEntry::marker("ENDOFWAD"));

        flat
    }
}

pub fn build(args: Args) -> io::Result<()> {
    let Args {
        inputs,
        output,
        exclude,
        no_sound,
        ignore_errors,
        apply_fixes,
        wdd,
        wmd,
        wsd,
    } = args;
    let output = output.unwrap_or_else(|| PathBuf::from("DOOM64.WAD"));
    let mut iwad = Wad::default();
    let mut snd = SoundData::default();
    let paths = crate::extract::ReadPaths {
        filters: crate::FileFilters {
            includes: Vec::new(),
            excludes: exclude,
        },
        ..Default::default()
    };
    let load_options = LoadOptions {
        filters: &paths.filters,
        ignore_errors,
    };
    for input in inputs {
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .map(|p| p.to_ascii_lowercase());
        let ext = ext.as_deref();
        if ext == Some("z64") || (ext == Some("wad") && !is_map_wad(&input)) {
            let mut flags = ReadFlags::IWAD | ReadFlags::DECOMPRESS;
            if !no_sound {
                flags |= ReadFlags::SOUND;
            }
            let (flat, isnd) = read_rom_or_iwad(input, flags, &paths)?;
            let mut flat = flat.unwrap();
            if !paths.filters.is_empty() {
                flat.entries
                    .retain(|entry| paths.filters.matches(&entry.name.display()));
            }
            iwad.merge_flat(flat, ignore_errors)?;
            if let Some(isnd) = isnd {
                snd = isnd;
            }
        } else if ext == Some("zip") || ext == Some("pk3") {
            log::info!("Reading `{}`", input.display());
            let mut file = std::fs::File::open(&input)?;
            let mut arc = zip::ZipArchive::new(&mut file).map_err(invalid_data)?;
            let mut pwad = Wad::default();
            for index in 0..arc.len() {
                let mut afile = arc.by_index(index).map_err(invalid_data)?;
                let name = match (afile.is_file(), afile.enclosed_name()) {
                    (true, Some(name)) => name.to_owned(),
                    _ => continue,
                };
                let typ = name
                    .ancestors()
                    .filter(|p| !p.as_os_str().is_empty())
                    .last()
                    .and_then(|p| type_for_dir(p.as_os_str()))
                    .unwrap_or(LumpType::Unknown);
                let res = load_entry(
                    &mut pwad,
                    &mut snd,
                    &name,
                    || {
                        let mut data = Vec::new();
                        std::io::Read::read_to_end(&mut afile, &mut data)?;
                        Ok(data)
                    },
                    typ,
                    &load_options,
                );
                if let Err(err) = res {
                    let err = format!(
                        "Error reading {}: {}: {err}",
                        input.display(),
                        name.display()
                    );
                    match ignore_errors {
                        true => log::warn!("{err}"),
                        false => return Err(invalid_data(err)),
                    }
                }
            }
            pwad.sort();
            iwad.merge(pwad);
        } else {
            log::info!("Reading `{}`", input.display());
            let mut pwad = Wad::default();
            load_entries(
                &mut pwad,
                &mut snd,
                input,
                None,
                LumpType::Unknown,
                0,
                &load_options,
            )?;
            pwad.sort();
            iwad.merge(pwad);
        }
    }
    let mut flat = iwad.flatten();
    for entry in &mut flat.entries {
        if apply_fixes {
            if let Some((hash, fixes)) = crate::lumps::VANILLA_FIXES.get(&entry.name.0) {
                if hash == blake3::hash(&entry.entry.data).as_bytes() {
                    for (offset, patch) in fixes.iter().copied() {
                        log::debug!(
                            "Patching {} at offset 0x{offset:x}, len {}",
                            entry.name.display(),
                            patch.len()
                        );
                        entry.entry.data[offset..offset + patch.len()].copy_from_slice(patch);
                    }
                }
            }
        }
    }
    log::info!(
        "Writing `{}` with {} entries",
        output.display(),
        flat.entries.len()
    );
    log::debug!("  SIZE       NAME     HASH");
    {
        let out = std::fs::File::create(&output)?;
        let mut out = std::io::BufWriter::new(out);
        flat.write(&mut out, crate::is_log_level(log::LevelFilter::Debug))?;
    }

    if !no_sound {
        snd.compress();
        let mut sample_count = 0u32;
        snd.foreach_sample(|_| {
            sample_count += 1;
            Ok(())
        })
        .unwrap();
        {
            let filename = wdd.unwrap_or_else(|| output.with_extension("WDD"));
            log::info!(
                "Writing `{}` with {sample_count} samples",
                filename.display(),
            );
            let out = std::fs::File::create(filename)?;
            let mut out = std::io::BufWriter::new(out);
            snd.write_wdd(&mut out)?;
            drop(out);
        }

        {
            let filename = wmd.unwrap_or_else(|| output.with_extension("WMD"));
            log::info!(
                "Writing `{}` with {} instruments",
                filename.display(),
                snd.instruments.len(),
            );
            let out = std::fs::File::create(filename)?;
            let mut out = std::io::BufWriter::new(out);
            snd.write_wmd(&mut out)?;
        }

        {
            let filename = wsd.unwrap_or_else(|| output.with_extension("WSD"));
            log::info!(
                "Writing `{}` with {} sequences",
                filename.display(),
                snd.sequences.len()
            );
            let out = std::fs::File::create(filename)?;
            let mut out = std::io::BufWriter::new(out);
            snd.write_wsd(&mut out)?;
        }
    }

    Ok(())
}
