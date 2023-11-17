use std::{collections::HashMap, io::Cursor};

use nom::error::context;

use crate::{
    convert_error,
    extract::WadType,
    invalid_data,
    sound::{Loop, SoundData},
    wad::{FlatWad, LumpType},
    FileFilters,
};

pub const REMASTER_WAD_HASH: [u8; 32] =
    hex_literal::hex!("05ec0118cc130036d04bf6e6f7fe4792dfafc2d4bd98de349dd63e2022925365");
pub const REMASTER_WAD_SIZE: usize = 15103212;

pub const REMASTER_DLS_NAME: &str = "DOOMSND.DLS";
pub const REMASTER_DLS_HASH: [u8; 32] =
    hex_literal::hex!("88814285dea4cf3b91fd73c0195c55512725600531689c5314a2559777e71b17");
pub const REMASTER_DLS_SIZE: usize = 3230114;

const REMASTER_SPECIFIC_ENTRIES: &[&[u8]] = &[b"MAPINFO", b"ANIMDEFS", b"SKYDEFS", b"CHECKSUM"];
const PSPRITES: &[[u8; 4]] = &[
    *b"SAWG", *b"PUNG", *b"PISG", *b"SHT1", *b"SHT2", *b"CHGG", *b"ROCK", *b"PLAS", *b"BFGG",
    *b"LASR",
];

fn hash_texture_name(name: &[u8]) -> u16 {
    let mut hash = 1315423911u32;
    for c in name.iter().map(|c| c.to_ascii_uppercase()) {
        hash ^= (hash << 5).wrapping_add(c as u32).wrapping_add(hash >> 2);
    }
    hash as u16
}

fn unhash_texture(tex: &mut [u8], hashes: &HashMap<u16, u16>) {
    let mut hash = [0u8; 2];
    hash.copy_from_slice(tex);
    let hash = u16::from_le_bytes(hash);
    let id = *hashes.get(&hash).unwrap();
    tex.copy_from_slice(&id.to_le_bytes());
}

/// Convert the 2020 remaster WAD back to N64 format
pub fn read_wad(
    data: &[u8],
    mut snd: Option<&mut SoundData>,
    filters: &FileFilters,
) -> std::io::Result<FlatWad> {
    let mut wad = context("WAD", |d| {
        FlatWad::parse(d, WadType::Remaster, false, filters)
    })(data)
    .map_err(|e| invalid_data(convert_error(data, e)))?
    .1;
    let mut remove_ranges = Vec::with_capacity(3);
    let mut cur_start = None;
    let mut in_section = LumpType::Unknown;
    let mut palettes = 0usize..=0;
    let mut tex_index = 0;
    let mut texture_hashes = HashMap::new();
    for (index, entry) in wad.entries.iter_mut().enumerate() {
        let name = entry.name.0.as_slice();
        if *palettes.start() != 0 && *palettes.end() == 0 && !name.starts_with(b"PAL") {
            palettes = *palettes.start()..=index - 1;
        }
        if cur_start.is_some() {
            if in_section == LumpType::Unknown {
                if !REMASTER_SPECIFIC_ENTRIES.contains(&name) {
                    remove_ranges.push(cur_start.take().unwrap()..=index - 1);
                }
            } else if name == b"DS_END" || name == b"DM_END" {
                in_section = LumpType::Unknown;
                remove_ranges.push(cur_start.take().unwrap()..=index);
                continue;
            }
        }
        if name == b"DS_START" {
            cur_start = Some(index);
            in_section = LumpType::Sample;
        } else if name == b"DM_START" {
            cur_start = Some(index);
            in_section = LumpType::Sequence;
        } else if name == b"S_START" {
            in_section = LumpType::Sprite;
        } else if name == b"T_START" {
            in_section = LumpType::Texture;
        } else if name == b"S_END" || name == b"T_END" {
            in_section = LumpType::Unknown;
        } else if REMASTER_SPECIFIC_ENTRIES.contains(&name) {
            if cur_start.is_none() {
                cur_start = Some(index);
            }
        } else {
            if in_section == LumpType::Unknown && name.starts_with(b"PAL") {
                let mut palette = vec![0; 8 + 256 * 2];
                palette[2] = 1;
                crate::gfx::palette_rgb_to_16(&entry.entry.data, &mut palette[8..]);
                entry.entry.data = palette;
                entry.entry.typ = LumpType::Palette;
                if *palettes.start() == 0 {
                    palettes = index..=0;
                }
                continue;
            }
            match entry.entry.typ {
                LumpType::Sprite => {
                    let sname = <[u8; 4]>::try_from(&name[0..4]).unwrap();
                    let mut sprite = crate::gfx::Sprite::read_png(&entry.entry.data, None).unwrap();
                    if PSPRITES.contains(&sname) {
                        sprite.x_offset += 160;
                        sprite.y_offset += 208;
                    }
                    entry.entry.data = sprite.to_vec();
                }
                LumpType::HudGraphic if name == b"SFONT" => {
                    let mut gfx = crate::gfx::Sprite::read_png(&entry.entry.data, Some(4)).unwrap();
                    gfx.height = 16;
                    gfx.data.shrink_to((256 * 16) / 2);
                    entry.entry.data = gfx.to_vec();
                }
                LumpType::HudGraphic => {
                    let gfx = crate::gfx::Sprite::read_png(&entry.entry.data, Some(8)).unwrap();
                    entry.entry.data = gfx.to_vec();
                }
                LumpType::Sky => {
                    let gfx = crate::gfx::Sprite::read_png(&entry.entry.data, None).unwrap();
                    entry.entry.data = gfx.to_vec();
                }
                LumpType::Texture | LumpType::Flat => {
                    let hash = hash_texture_name(name);
                    texture_hashes.entry(hash).or_insert(tex_index);
                    let tex = crate::gfx::Texture::read_png(&entry.entry.data).unwrap();
                    entry.entry.data = tex.to_vec();
                    tex_index += 1;
                }
                LumpType::Graphic => {
                    let gfx = crate::gfx::Graphic::read_png(&entry.entry.data, true).unwrap();
                    entry.entry.data = gfx.to_vec(entry.entry.typ);
                }
                LumpType::Cloud | LumpType::Fire => {
                    let gfx = crate::gfx::Graphic::read_png(&entry.entry.data, false).unwrap();
                    entry.entry.data = gfx.to_vec(entry.entry.typ);
                }
                LumpType::Map => {
                    let d = std::mem::take(&mut entry.entry.data);
                    let mut map = context("Map WAD", |d| {
                        FlatWad::parse(d, WadType::RemasterMap, false, &Default::default())
                    })(&d)
                    .map_err(|e| invalid_data(convert_error(data, e)))?
                    .1;
                    let sectors = map
                        .entries
                        .iter_mut()
                        .find(|e| e.name.0.as_slice() == b"SECTORS")
                        .unwrap();
                    for sector in sectors.entry.data.chunks_exact_mut(24) {
                        unhash_texture(&mut sector[4..6], &texture_hashes);
                        unhash_texture(&mut sector[6..8], &texture_hashes);
                    }
                    let sidedefs = map
                        .entries
                        .iter_mut()
                        .find(|e| e.name.0.as_slice() == b"SIDEDEFS")
                        .unwrap();
                    for side in sidedefs.entry.data.chunks_exact_mut(12) {
                        unhash_texture(&mut side[4..6], &texture_hashes);
                        unhash_texture(&mut side[6..8], &texture_hashes);
                        unhash_texture(&mut side[8..10], &texture_hashes);
                    }
                    map.write(&mut entry.entry.data, false)?;
                }
                _ => {}
            }
        }
    }
    // extract sounds, midis and remove unused lumps
    remove_ranges.reverse();
    for range in remove_ranges {
        let mut entries = wad.entries.drain(range);
        let first = entries.next().unwrap();
        if let Some(snd) = &mut snd {
            let first_name = first.name.0.as_slice();
            if first_name == b"DS_START" {
                snd.sequences.insert(
                    0,
                    crate::sound::Sequence::Effect(crate::sound::Sample {
                        info: Default::default(),
                        priority: 99,
                        volume: 0,
                    }),
                );
                for entry in &mut entries {
                    let name = entry.name.0.as_slice();
                    if name == b"DS_END" {
                        break;
                    }
                    if let Some(id) = name
                        .strip_prefix(b"SFX_")
                        .and_then(|n| str::parse(std::str::from_utf8(n).ok()?).ok())
                    {
                        let mut sample =
                            context("WAV", crate::sound::Sample::read_wav)(&entry.entry.data)
                                .map_err(|e| invalid_data(convert_error(data, e)))?
                                .1;
                        if id == 112 {
                            sample.info.r#loop = Some(Loop {
                                count: u32::MAX,
                                start: 1713,
                                end: 23680,
                            });
                        } else if id == 115 {
                            sample.info.r#loop = Some(Loop {
                                count: u32::MAX,
                                start: 2025,
                                end: 17235,
                            });
                        }
                        let (seq, volume, priority) = *SNDFX_METADATA.get(&id).unwrap();
                        sample.volume = volume;
                        sample.priority = priority;
                        snd.sequences
                            .insert(seq, crate::sound::Sequence::Effect(sample));
                    }
                }
            } else if first_name == b"DM_START" {
                let mut counter = 93;
                for entry in &mut entries {
                    let name = entry.name.0.as_slice();
                    if name == b"DM_END" {
                        break;
                    }
                    let seq = crate::music::MusicSequence::read_midi(&mut Cursor::new(
                        &entry.entry.data,
                    ))?;
                    snd.sequences
                        .insert(counter, crate::sound::Sequence::MusicSeq(seq));
                    counter += 1;
                }
            }
        }
        entries.last();
    }
    // move palettes back into positions in S_START/S_END
    let palettes = wad.entries.drain(palettes).collect::<Vec<_>>();
    for entry in palettes {
        let sname = &entry.name.0[3..7];
        let sprite_index = wad
            .entries
            .iter()
            .position(|e| e.entry.typ == LumpType::Sprite && e.name.0.starts_with(sname))
            .unwrap();
        wad.entries.insert(sprite_index, entry);
    }
    Ok(wad)
}

pub fn read_dls(data: &[u8], snd: &mut SoundData) -> std::io::Result<()> {
    snd.read_dls(data)
        .map_err(|e| invalid_data(convert_error(data, e)))?;
    for seq in snd.sequences.values_mut() {
        if let crate::sound::Sequence::Effect(samp) = seq {
            if let Some(r#loop) = &mut samp.info.r#loop {
                r#loop.start *= 2;
                r#loop.end *= 2;
            }
        }
    }
    let inst = snd.instruments.get_mut(&0).unwrap();
    let mut map = inst.patchmaps[0].clone();
    map.note_min = 45;
    map.note_max = 66;
    inst.patchmaps.insert(1, map);
    for inst in snd.instruments.values_mut() {
        for map in &mut inst.patchmaps {
            if map.decay_time == 20000 {
                map.decay_time = 32000;
            }
            if map.release_time == 20000 {
                map.release_time = 32000;
            }
        }
    }
    Ok(())
}

// sampleid => (sequence, volume, priority)
const SNDFX_METADATA: phf::Map<u16, (u16, u8, u8)> = phf::phf_map! {
    85u16 => (1, 127, 80),
    42u16 => (2, 100, 50),
    84u16 => (3, 127, 60),
    44u16 => (4, 115, 80),
    33u16 => (5, 100, 80),
    34u16 => (6, 100, 80),
    35u16 => (7, 80, 80),
    36u16 => (8, 127, 80),
    37u16 => (9, 90, 80),
    38u16 => (10, 100, 80),
    39u16 => (11, 110, 80),
    40u16 => (12, 120, 80),
    41u16 => (13, 127, 80),
    43u16 => (14, 100, 80),
    45u16 => (15, 110, 50),
    46u16 => (16, 110, 50),
    47u16 => (17, 127, 50),
    48u16 => (18, 127, 50),
    49u16 => (19, 100, 50),
    50u16 => (20, 127, 50),
    51u16 => (21, 127, 50),
    56u16 => (22, 127, 50),
    122u16 => (23, 127, 80),
    57u16 => (24, 110, 50),
    58u16 => (25, 127, 50),
    124u16 => (26, 110, 50),
    88u16 => (27, 127, 80),
    89u16 => (28, 108, 50),
    90u16 => (29, 108, 50),
    52u16 => (30, 120, 50),
    71u16 => (31, 127, 99),
    55u16 => (32, 127, 50),
    59u16 => (33, 120, 50),
    60u16 => (34, 127, 50),
    61u16 => (35, 127, 50),
    72u16 => (36, 127, 50),
    73u16 => (37, 127, 50),
    74u16 => (38, 127, 50),
    81u16 => (39, 127, 50),
    54u16 => (40, 120, 50),
    53u16 => (41, 120, 50),
    83u16 => (42, 110, 50),
    70u16 => (43, 109, 50),
    62u16 => (44, 127, 50),
    63u16 => (45, 127, 50),
    75u16 => (46, 127, 50),
    76u16 => (47, 127, 50),
    82u16 => (48, 127, 50),
    64u16 => (49, 127, 50),
    69u16 => (50, 120, 50),
    77u16 => (51, 127, 50),
    66u16 => (52, 127, 50),
    79u16 => (53, 127, 50),
    65u16 => (54, 120, 50),
    78u16 => (55, 127, 50),
    68u16 => (56, 110, 50),
    91u16 => (57, 110, 50),
    92u16 => (58, 120, 50),
    93u16 => (59, 127, 50),
    94u16 => (60, 127, 50),
    95u16 => (61, 127, 50),
    96u16 => (62, 127, 50),
    97u16 => (63, 127, 50),
    98u16 => (64, 127, 50),
    99u16 => (65, 120, 50),
    100u16 => (66, 127, 50),
    101u16 => (67, 127, 50),
    102u16 => (68, 127, 50),
    103u16 => (69, 127, 50),
    123u16 => (70, 120, 80),
    104u16 => (71, 120, 50),
    105u16 => (72, 108, 50),
    106u16 => (73, 127, 50),
    107u16 => (74, 127, 50),
    67u16 => (75, 127, 50),
    80u16 => (76, 127, 50),
    86u16 => (77, 110, 50),
    87u16 => (78, 110, 50),
    108u16 => (79, 120, 50),
    109u16 => (80, 120, 50),
    110u16 => (81, 127, 50),
    111u16 => (82, 110, 80),
    112u16 => (83, 83, 80),
    113u16 => (84, 127, 0),
    114u16 => (85, 127, 0),
    115u16 => (86, 110, 0),
    116u16 => (87, 100, 0),
    117u16 => (88, 120, 0),
    118u16 => (89, 120, 0),
    119u16 => (90, 120, 99),
    120u16 => (91, 120, 0),
    121u16 => (92, 120, 0),
};
