use std::{borrow::Cow, path::PathBuf};

use crate::{
    extract::{self, PaletteCache, ReadFlags},
    gfx, EntryName, FlatEntry, LumpType, WadEntry,
};

#[derive(clap::Args)]
pub struct Args {
    /// WAD or ROM file to inspect
    input: PathBuf,
    /// Glob patterns to include entry names
    #[arg(short, long)]
    include: Vec<String>,
    /// Test conversions
    #[arg(short, long, default_value_t = false)]
    test: bool,
    /// Optional WDD file to read when inspecting IWAD [default: DOOM64.WDD]
    #[arg(long)]
    wdd: Option<PathBuf>,
    /// Optional WMD file to read when inspecting IWAD [default: DOOM64.WMD]
    #[arg(long)]
    wmd: Option<PathBuf>,
    /// Optional WSD file to read when inspecting IWAD [default: DOOM64.WSD]
    #[arg(long)]
    wsd: Option<PathBuf>,
    /// Optional DLS file to read when inspecting remaster IWAD [default: DOOMSND.DLS]
    #[arg(long)]
    dls: Option<PathBuf>,
}

fn test_conversion(
    index: usize,
    name: &EntryName,
    orig: &WadEntry<Vec<u8>>,
    data: &[u8],
    palettes: &PaletteCache,
) -> bool {
    use LumpType::*;
    let converted = match orig.typ {
        Palette => {
            let colors = data.len() / 3;
            let mut palette = vec![0; colors * 2 + 8];
            palette[2] = 1;
            gfx::palette_rgb_to_16(data, &mut palette[8..]);
            Cow::Owned(palette)
        }
        Graphic | Fire | Cloud => Cow::Owned(
            gfx::Graphic::read_png(data, false)
                .unwrap()
                .to_vec(orig.typ),
        ),
        Texture | Flat => Cow::Owned(gfx::Texture::read_png(data).unwrap().to_vec()),
        HudGraphic => Cow::Owned(gfx::Sprite::read_png(data, None).unwrap().to_vec()),
        Sprite => {
            let mut sprite = gfx::Sprite::read_png(data, None).unwrap();
            if let Some(palindex) = palettes.sprite_to_palette.get(&index) {
                sprite.palette = gfx::SpritePalette::Offset((index - *palindex) as u16);
            }
            Cow::Owned(sprite.to_vec())
        }
        _ => Cow::Borrowed(data),
    };
    let matches = orig.data == converted.as_ref();
    if !matches {
        log::debug!(
            "conversion of `{}` does not match\n  expected: {:02x?}\n       got: {converted:02x?}",
            name.display(),
            orig.data,
        );
    }
    matches
}

#[inline]
fn display_loop(lp: Option<&crate::sound::Loop>) -> String {
    lp.map(|l| {
        format!(
            " {: <5} {: <10} {}",
            if l.count == u32::MAX {
                -1i64
            } else {
                l.count as i64
            },
            l.start,
            l.end,
        )
    })
    .unwrap_or_default()
}

pub fn inspect(args: Args) -> std::io::Result<()> {
    let Args {
        input,
        include,
        test,
        wdd,
        wmd,
        wsd,
        dls,
    } = args;
    let verbose = crate::is_log_level(log::LevelFilter::Debug);
    let paths = crate::extract::ReadPaths {
        filters: crate::FileFilters {
            includes: include,
            excludes: Vec::new(),
        },
        wdd,
        wmd,
        wsd,
        dls,
    };
    let (wad, snd) = extract::read_rom_or_iwad(&input, ReadFlags::IWAD | ReadFlags::SOUND, &paths)?;
    let wad = wad.unwrap();
    log::info!("WAD Entries: {}", wad.entries.len());
    if !wad.entries.is_empty() {
        log::info!("  SIZE       REALSIZE   NAME     TEST HASH");
        let mut palettes = PaletteCache::default();
        for (index, FlatEntry { name, entry }) in wad.entries.iter().enumerate() {
            if !paths.filters.is_empty() && !paths.filters.matches(&name.display()) {
                continue;
            }
            let ok = if test {
                let data = wad.extract_one(index, &mut palettes, true)?;
                test_conversion(index, name, entry, &data, &palettes)
            } else {
                true
            };

            let realsize = entry.uncompressed_len();
            let size = entry.data.len();
            let name = name.display();
            let stat = if test {
                if ok {
                    "✓   "
                } else {
                    "FAIL"
                }
            } else {
                "-   "
            };
            let hash = blake3::hash(&entry.data);
            log::info!("  0x{size: <8x} 0x{realsize: <8x} {name: <8} {stat} 0x{hash}");
        }
    }
    if let Some(snd) = snd {
        log::info!("Instruments: {}", snd.instruments.len());
        if !snd.instruments.is_empty() {
            let mut patchmap_count = 0usize;
            log::info!("    PMAP  SAMPLE PRI VOL PAN ROOT ADJ \
                MIN MAX PMIN PMAX A     D     R     ALV DLV SAMPLELEN  PITCH       LOOP  START      END");
            for (patchindex, inst) in &snd.instruments {
                log::info!("  PATCH {patchindex}");
                for patchmap in &inst.patchmaps {
                    let sample = patchmap.sample.as_deref().unwrap();
                    let sample = sample.borrow();
                    log::info!(
                        "    {patchmap_count: <5} {: <6} {: <3} {: <3} {: <3} {: <4} {: <3} \
                        {: <3} {: <3} {: <4} {: <4} {: <5} {: <5} {: <5} {: <3} {: <3} {: <10} {: <11}{}",
                        patchmap.sample_id,
                        patchmap.priority,
                        patchmap.volume,
                        patchmap.pan,
                        patchmap.root_key,
                        patchmap.fine_adj,
                        patchmap.note_min,
                        patchmap.note_max,
                        patchmap.pitchstep_min,
                        patchmap.pitchstep_max,
                        patchmap.attack_time,
                        patchmap.decay_time,
                        patchmap.release_time,
                        patchmap.attack_level,
                        patchmap.decay_level,
                        sample.samples.n_samples(),
                        sample.pitch,
                        display_loop(sample.r#loop.as_ref()),
                    );
                    patchmap_count += 1;
                }
            }
        }

        let mut effect_count = 0usize;
        let mut music_count = 0usize;
        for seq in snd.sequences.values() {
            match seq {
                crate::sound::Sequence::Effect(_) => effect_count += 1,
                crate::sound::Sequence::MusicSeq(_) => music_count += 1,
                crate::sound::Sequence::MusicSample(_) => unreachable!(),
            }
        }
        log::info!("Effect Sequences: {}", effect_count);
        if effect_count > 0 {
            log::info!("  SEQ   PRI SAMPLES    SIZE       PITCH       LOOP  START      END");
            for (index, seq) in &snd.sequences {
                if let crate::sound::Sequence::Effect(sample) = seq {
                    log::info!(
                        "  {index: <5} {: <3} {: <10} 0x{: <8x} {: <11}{}",
                        sample.priority,
                        sample.info.samples.n_samples(),
                        sample.info.samples.stored_len(),
                        sample.info.pitch,
                        display_loop(sample.info.r#loop.as_ref()),
                    );
                }
            }
        }
        log::info!("Music Sequences: {}", music_count);
        if music_count > 0 {
            if !verbose {
                log::info!("    TRACK EVENTS     LABELS INITPATCH");
            }
            for (index, seq) in &snd.sequences {
                if let crate::sound::Sequence::MusicSeq(seq) = seq {
                    log::info!("  SEQ {index}");
                    for (track_index, track) in seq.tracks.iter().enumerate() {
                        if verbose {
                            log::debug!(
                                "    TRACK EVENTS     LABELS PATCH PITCH  VOL PAN PPQ   PPM"
                            );
                        }
                        log::info!(
                            "    {track_index: <5} {: <10} {: <6} {: <5} {: <6} {: <3} {: <3} {: <5} {}",
                            track.events.len(),
                            track.labels.len(),
                            track.initpatchnum,
                            track.initpitch_cntrl,
                            track.initvolume_cntrl,
                            track.initpan_cntrl,
                            track.initppq,
                            track.initqpm,
                        );
                        if verbose {
                            log::debug!("           DELTA EVENT");
                            for (eindex, event) in track.events.iter().enumerate() {
                                for (lindex, label) in track.labels.iter().enumerate() {
                                    if *label == eindex {
                                        log::debug!("    LABEL {lindex}:");
                                    }
                                }
                                log::debug!("      {: >10} {:?}", event.delta, event.event);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
