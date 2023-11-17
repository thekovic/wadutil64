use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    nom_fail,
    sound::{
        parse_riff_chunks, parse_riff_header, Loop, PatchInfo, PatchMap, SampleData, SoundData,
    },
};
use itertools::Itertools;
use nom::{
    branch::alt,
    bytes::complete::{tag, take},
    error::ParseError,
    multi::count,
    number::complete::{le_i16, le_i32, le_i8, le_u16, le_u32, le_u8},
};

#[inline]
fn parse_2d_list<'a, E: ParseError<&'a [u8]>>(
    data: &'a [u8],
    name: &[u8; 4],
    mut f: impl FnMut(&'a [u8]) -> nom::IResult<&'a [u8], (), E>,
) -> nom::IResult<&'a [u8], (), E> {
    parse_riff_chunks(data, |chunk_name, chunk| {
        if chunk_name == *b"LIST" {
            let (chunk, list_name) = take(4usize)(chunk)?;
            if list_name == name {
                f(chunk)?;
            }
        }
        Ok((&[], ()))
    })
}

impl SoundData {
    pub fn read_dls<'a, E: ParseError<&'a [u8]>>(
        &mut self,
        data: &'a [u8],
    ) -> nom::IResult<&'a [u8], (), E> {
        let mut ptbl = None;
        let mut wvpl = None;
        let mut lins = Vec::new(); // (patch, regions, articulators)
        let (data, _) = parse_riff_header(data, b"DLS ")?;
        // pull out the pool data first
        let (data, _) = parse_riff_chunks(data, |chunk_name, chunk| {
            match &chunk_name {
                b"ptbl" => {
                    let (chunk, ptbl_size) = le_u32(chunk)?;
                    let (chunk, head) = take(ptbl_size.saturating_sub(4))(chunk)?;
                    let (_, ncues) = le_u32(head)?;
                    ptbl.get_or_insert(chunk.chunks_exact(4).take(ncues as usize));
                }
                b"LIST" => {
                    let (chunk, list_name) = take(4usize)(chunk)?;
                    match list_name {
                        b"lins" => {
                            parse_2d_list(chunk, b"ins ", |chunk| {
                                let mut patch = None;
                                let mut regions = Vec::new();
                                let mut articulators = Vec::new();
                                parse_riff_chunks(chunk, |chunk_name, chunk| {
                                    match &chunk_name {
                                        b"insh" => {
                                            if patch.is_some() {
                                                return Err(nom_fail(chunk));
                                            }
                                            let (chunk, _regions) = le_u32(chunk)?;
                                            let (chunk, bank) = le_u32(chunk)?;
                                            let (_, program) = le_u32(chunk)?;
                                            patch = Some(
                                                ((program & 0x7f) | ((bank & 0x7f00) >> 1)) as u16,
                                            );
                                        }
                                        b"LIST" => {
                                            let (chunk, list_name) = take(4usize)(chunk)?;
                                            match list_name {
                                                b"lrgn" => {
                                                    parse_2d_list(chunk, b"rgn ", |chunk| {
                                                        regions.push(chunk);
                                                        Ok((&[], ()))
                                                    })?
                                                    .1
                                                }
                                                b"lart" => {
                                                    parse_riff_chunks(
                                                        chunk,
                                                        |chunk_name, chunk| {
                                                            if chunk_name == *b"art1" {
                                                                articulators.push(chunk);
                                                            }
                                                            Ok((&[], ()))
                                                        },
                                                    )?;
                                                }
                                                _ => {}
                                            }
                                        }
                                        _ => {}
                                    }
                                    Ok((&[], ()))
                                })?;
                                if let Some(patch) = patch {
                                    if !regions.is_empty() {
                                        lins.push((patch, regions, articulators));
                                    }
                                }
                                Ok((&[], ()))
                            })?
                            .1
                        }
                        b"wvpl" => {
                            wvpl.get_or_insert(chunk);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            Ok((&[], ()))
        })?;
        let ptbl = ptbl.ok_or_else(|| nom_fail(&[]))?;
        let wvpl = wvpl.ok_or_else(|| nom_fail(&[]))?;
        let mut sample_hash = HashMap::new();
        // parse the regions after ensuring pool data is present
        for (patch, lrgn, lart) in lins {
            let mut map_default = PatchMap::default();
            for articulator in lart {
                parse_dls_articulator(articulator, &mut map_default)?;
            }
            let mut patchmaps = Vec::with_capacity(lrgn.len());
            for region in lrgn {
                let mut samplesize = 0;
                let mut samplerate = 0;
                let mut sample_id = None;
                let mut sample_data = None;
                let mut r#loop = None;
                let mut range = None;
                let mut has_rgn_wsmp = false;
                let mut map = map_default.clone();
                parse_riff_chunks(region, |chunk_name, chunk| {
                    match &chunk_name {
                        b"rgnh" => {
                            let (chunk, keylow) = le_u16(chunk)?;
                            let (chunk, keyhigh) = le_u16(chunk)?;
                            let (chunk, _vellow) = le_u16(chunk)?;
                            let (chunk, _velhigh) = le_u16(chunk)?;
                            let (chunk, _options) = le_u16(chunk)?;
                            let (_, _keygroup) = le_u16(chunk)?;
                            range.get_or_insert((keylow, keyhigh));
                        }
                        b"wsmp" => {
                            parse_dls_wsmp(chunk, &mut map, &mut r#loop)?;
                            has_rgn_wsmp = true;
                        }
                        b"wlnk" => {
                            let (chunk, _options) = le_u16(chunk)?;
                            let (chunk, _phasegroup) = le_u16(chunk)?;
                            let (chunk, _channel) = le_u32(chunk)?;
                            let (_, tableindex) = le_u32(chunk)?;
                            sample_id.get_or_insert(tableindex);
                            let offset = le_u32(
                                ptbl.clone()
                                    .nth(tableindex as usize)
                                    .ok_or_else(|| nom_fail(&[]))?,
                            )?
                            .1;
                            parse_2d_list(&wvpl[offset as usize..], b"wave", |chunk| {
                                parse_riff_chunks(chunk, |chunk_name, chunk| {
                                    match &chunk_name {
                                        b"fmt " => {
                                            let (chunk, fmt) = le_u16(chunk)?;
                                            let (chunk, channels) = le_u16(chunk)?;
                                            if fmt != 1 || channels != 1 {
                                                return Ok((&[], ()));
                                            }
                                            let (chunk, sr) = le_u32(chunk)?;
                                            let (chunk, _datarate) = le_u32(chunk)?;
                                            let (chunk, ss) = alt((
                                                tag(1u16.to_le_bytes()),
                                                tag(2u16.to_le_bytes()),
                                            ))(
                                                chunk
                                            )?;
                                            samplerate = sr;
                                            samplesize = u16::from_le_bytes(ss.try_into().unwrap());
                                            tag((samplesize * 8).to_le_bytes())(chunk)?;
                                        }
                                        b"data" => {
                                            sample_data.get_or_insert(chunk);
                                        }
                                        b"wsmp" => {
                                            if !has_rgn_wsmp {
                                                parse_dls_wsmp(chunk, &mut map, &mut r#loop)?;
                                            }
                                        }
                                        _ => {}
                                    }
                                    Ok((&[], ()))
                                })
                            })?;
                        }
                        b"LIST" => {
                            let (chunk, list_name) = take(4usize)(chunk)?;
                            if list_name == b"lart" {
                                parse_riff_chunks(chunk, |chunk_name, chunk| {
                                    if chunk_name == *b"art1" {
                                        parse_dls_articulator(chunk, &mut map)?;
                                    }
                                    Ok((&[], ()))
                                })?;
                            }
                        }
                        _ => {}
                    }
                    Ok((&[], ()))
                })?;
                let (keylow, keyhigh) = range.ok_or_else(|| nom_fail(&[]))?;
                let sample_id = sample_id.ok_or_else(|| nom_fail(&[]))?;
                let sample_data = sample_data.ok_or_else(|| nom_fail(&[]))?;
                if samplerate == 0 {
                    return Err(nom_fail(&[]));
                }
                if samplesize == 0 {
                    return Err(nom_fail(&[]));
                }
                map.note_min = keylow.clamp(0, 127) as u8;
                map.note_max = keyhigh.clamp(0, 127) as u8;
                let pitch = crate::sound::samplerate_to_cents(samplerate);
                let info = match sample_hash.entry((sample_id, pitch, r#loop.clone())) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let samples = if samplesize == 1 {
                            let mut cvt = Vec::with_capacity(sample_data.len());
                            for s in sample_data.iter().copied().map(|s| s as i8) {
                                cvt.push((s as i16) << 8);
                            }
                            cvt
                        } else {
                            count(le_i16, sample_data.len() / 2)(sample_data)?.1
                        };
                        let info = PatchInfo {
                            samples: SampleData::Raw(samples),
                            pitch,
                            r#loop,
                        };
                        entry.insert(Rc::new(RefCell::new(info))).clone()
                    }
                    std::collections::hash_map::Entry::Occupied(entry) => entry.get().clone(),
                };
                map.sample_id = sample_id as u16;
                map.sample = Some(info);
                patchmaps.push(map);
            }
            if !patchmaps.is_empty() {
                self.instruments
                    .insert(patch, crate::sound::Instrument { patchmaps });
            }
        }
        Ok((data, ()))
    }
    pub fn read_sf2<'a, E: ParseError<&'a [u8]>>(
        &mut self,
        data: &'a [u8],
    ) -> nom::IResult<&'a [u8], (), E> {
        let mut smpl = None;
        let mut phdr = None;
        let mut pbag = None;
        let mut pgen = None;
        let mut inst = None;
        let mut ibag = None;
        let mut igen = None;
        let mut shdr = None;
        // pull the chunks out first
        let (data, _) = parse_riff_header(data, b"sfbk")?;
        let (data, _) = parse_riff_chunks(data, |chunk_name, chunk| {
            if chunk_name != *b"LIST" {
                return Ok((&[], ()));
            }
            let (chunk, list_name) = take(4usize)(chunk)?;
            match list_name {
                b"sdta" => {
                    parse_riff_chunks(chunk, |chunk_name, chunk| {
                        if chunk_name == *b"smpl" && smpl.is_none() {
                            smpl = Some(chunk.chunks_exact(2));
                        }
                        Ok((&[], ()))
                    })?;
                }
                b"pdta" => {
                    parse_riff_chunks(chunk, |chunk_name, chunk| {
                        match &chunk_name {
                            b"phdr" => {
                                phdr.get_or_insert(chunk.chunks_exact(38));
                            }
                            b"pbag" => {
                                pbag.get_or_insert(chunk.chunks_exact(4));
                            }
                            b"pgen" => {
                                pgen.get_or_insert(chunk.chunks_exact(4));
                            }
                            b"inst" => {
                                inst.get_or_insert(chunk.chunks_exact(22));
                            }
                            b"ibag" => {
                                ibag.get_or_insert(chunk.chunks_exact(4));
                            }
                            b"igen" => {
                                igen.get_or_insert(chunk.chunks_exact(4));
                            }
                            b"shdr" => {
                                shdr.get_or_insert(chunk.chunks_exact(46));
                            }
                            _ => {}
                        }
                        Ok((&[], ()))
                    })?;
                }
                _ => {}
            }
            Ok((&[], ()))
        })?;

        let smpl = smpl.ok_or_else(|| nom_fail(&[]))?;
        let phdr = phdr.ok_or_else(|| nom_fail(&[]))?;
        let pbag = pbag.ok_or_else(|| nom_fail(&[]))?;
        let pgen = pgen.ok_or_else(|| nom_fail(&[]))?;
        let inst = inst.ok_or_else(|| nom_fail(&[]))?;
        let ibag = ibag.ok_or_else(|| nom_fail(&[]))?;
        let igen = igen.ok_or_else(|| nom_fail(&[]))?;
        let shdr = shdr.ok_or_else(|| nom_fail(&[]))?;

        // (sample_id, is_loop) -> sample
        // stored this way because the wmd stores the loop flag in the sample,
        // but sf2 stores it in the instrument
        let mut sample_hash = HashMap::new();
        for (header, next) in phdr.clone().tuple_windows() {
            let (d, _name) = take(20usize)(header)?;
            let (d, preset) = le_u16(d)?;
            let (d, bank) = le_u16(d)?;
            let bagstart = le_u16(d)?.1 as usize;
            let bagend = le_u16(take(24usize)(next)?.0)?.1 as usize;

            let patch = (preset & 0x7f) | ((bank & 0x1ff) << 7);
            self.instruments.remove(&patch); // ensure it is replaced
            let baglen = bagend - bagstart;
            let mut global_default = PatchMap::default();

            for (bag, next) in pbag.clone().tuple_windows().skip(bagstart).take(baglen) {
                let genstart = le_u16(bag)?.1 as usize;
                let genend = le_u16(next)?.1 as usize;
                let genlen = genend - genstart;
                let mut preset_default = global_default.clone();
                preset_default.root_key = 255;
                let mut is_global = true;
                for gen in pgen.clone().skip(genstart).take(genlen) {
                    let (d, gid) = le_u16(gen)?;
                    let (_, amount) = le_u16(d)?;
                    if gid != SF2Generators::INSTRUMENT {
                        parse_s2f_generator(&mut preset_default, gid, amount, false);
                    } else {
                        is_global = false;
                        let (instrument, next) = inst
                            .clone()
                            .tuple_windows()
                            .nth(amount as usize)
                            .ok_or_else(|| nom_fail(&[]))?;
                        let bagstart = le_u16(take(20usize)(instrument)?.0)?.1 as usize;
                        let bagend = le_u16(take(20usize)(next)?.0)?.1 as usize;
                        let baglen = bagend - bagstart;
                        let mut inst_default = preset_default.clone();

                        for (bag, next) in ibag.clone().tuple_windows().skip(bagstart).take(baglen)
                        {
                            let genstart = le_u16(bag)?.1 as usize;
                            let genend = le_u16(next)?.1 as usize;
                            let genlen = genend - genstart;
                            let mut inst_map = inst_default.clone();
                            let mut inst_is_global = true;
                            let mut mode = 0;
                            for gen in igen.clone().skip(genstart).take(genlen) {
                                let (d, gid) = le_u16(gen)?;
                                let (_, amount) = le_u16(d)?;
                                if gid == SF2Generators::SAMPLE_MODES {
                                    mode = amount;
                                } else if gid != SF2Generators::SAMPLE_ID {
                                    parse_s2f_generator(&mut inst_map, gid, amount, true);
                                } else {
                                    inst_is_global = false;
                                    let r#loop = matches!(mode, 1 | 3);
                                    let (info, root) = match sample_hash.entry((amount, r#loop)) {
                                        std::collections::hash_map::Entry::Vacant(entry) => {
                                            let sample = shdr
                                                .clone()
                                                .nth(amount as usize)
                                                .ok_or_else(|| nom_fail(&[]))?;
                                            let (d, _name) = take(20usize)(sample)?;
                                            let (d, start) = le_u32(d)?;
                                            let (d, end) = le_u32(d)?;
                                            let (d, startloop) = le_u32(d)?;
                                            let (d, endloop) = le_u32(d)?;
                                            let (d, samplerate) = le_u32(d)?;
                                            let (d, by_orig_pitch) = le_u8(d)?;
                                            let (_, pitch_corr) = le_i8(d)?;
                                            let info = PatchInfo {
                                                samples: SampleData::Raw(
                                                    smpl.clone()
                                                        .skip(start as usize)
                                                        .take(end.saturating_sub(start) as usize)
                                                        .map(|c| {
                                                            i16::from_le_bytes(
                                                                c.try_into().unwrap(),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                pitch: crate::sound::samplerate_to_cents(
                                                    samplerate,
                                                ) + pitch_corr as i32,
                                                r#loop: r#loop.then(|| Loop {
                                                    start: startloop.saturating_sub(start),
                                                    end: endloop.saturating_sub(start),
                                                    count: u32::MAX,
                                                }),
                                            };
                                            entry
                                                .insert((
                                                    Rc::new(RefCell::new(info)),
                                                    by_orig_pitch,
                                                ))
                                                .clone()
                                        }
                                        std::collections::hash_map::Entry::Occupied(entry) => {
                                            entry.get().clone()
                                        }
                                    };
                                    if inst_map.root_key == 255 {
                                        if root == 255 {
                                            inst_map.root_key = 60;
                                        } else {
                                            inst_map.root_key = root;
                                        }
                                    }
                                    inst_map.sample_id = amount;
                                    inst_map.sample = Some(info);
                                    let i = self.instruments.entry(patch).or_default();
                                    i.patchmaps.push(inst_map.clone());
                                    break;
                                }
                            }
                            if inst_is_global {
                                inst_default = inst_map;
                            }
                        }
                        break;
                    }
                }
                if is_global {
                    global_default = preset_default;
                }
            }
        }

        Ok((data, ()))
    }
    pub fn write_sf2(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        let patch_count = self.instruments.len();
        let mut patchmap_count = 0u16;
        let mut sample_count = 0u32;
        let mut smpl_size = 0u32;
        self.foreach_instrument_sample(|sample| {
            smpl_size += (sample.samples.n_samples() + 46) as u32 * 2;
            sample_count += 1;
            Ok(())
        })?;
        for inst in self.instruments.values() {
            patchmap_count += inst.patchmaps.len() as u16;
        }

        /* this number *must* match the number of generators per patchmap added
         * in the igen loop below */
        const INST_GENERATORS: u32 = 11;

        let sdta_size = smpl_size + 12;

        let phdr_size = (patch_count as u32 + 1) * 38;
        let pbag_size = (patchmap_count as u32 + 1) * 4;
        let pmod_size = 10u32;
        let pgen_size = (patchmap_count as u32 + 1) * 4;
        let inst_size = (patchmap_count as u32 + 1) * 22;
        let ibag_size = (patchmap_count as u32 + 1) * 4;
        let imod_size = 10u32;
        let igen_size = 4 + patchmap_count as u32 * INST_GENERATORS * 4;
        let shdr_size = (sample_count + 1) * 46;
        let pdta_size = 4u32 // list id
            + 9 * 8          // chunk headers
            + phdr_size
            + pbag_size
            + pmod_size
            + pgen_size
            + inst_size
            + ibag_size
            + imod_size
            + igen_size
            + shdr_size;

        let info_size = 48;
        let riff_size = info_size + sdta_size + pdta_size + 4 * 7;

        w.write_all(b"RIFF")?;
        w.write_all(&riff_size.to_le_bytes())?;
        w.write_all(b"sfbk")?;

        w.write_all(b"LIST")?;
        w.write_all(&info_size.to_le_bytes())?;
        w.write_all(b"INFO")?;
        w.write_all(b"ifil")?;
        w.write_all(&4u32.to_le_bytes())?;
        w.write_all(&[2, 0, 0, 0])?;
        w.write_all(b"isng")?;
        w.write_all(&8u32.to_le_bytes())?;
        w.write_all(b"EMU8000\0")?;
        w.write_all(b"INAM")?;
        w.write_all(&8u32.to_le_bytes())?;
        w.write_all(b"Doom 64\0")?;

        w.write_all(b"LIST")?;
        w.write_all(&sdta_size.to_le_bytes())?;
        w.write_all(b"sdta")?;
        w.write_all(b"smpl")?;
        w.write_all(&smpl_size.to_le_bytes())?;
        self.foreach_instrument_sample(|sample| {
            for s in &*sample.samples.raw_data() {
                w.write_all(&s.to_le_bytes())?;
            }
            for _ in 0..46 {
                w.write_all(&0i16.to_le_bytes())?;
            }
            Ok(())
        })?;

        w.write_all(b"LIST")?;
        w.write_all(&pdta_size.to_le_bytes())?;
        w.write_all(b"pdta")?;

        w.write_all(b"phdr")?;
        w.write_all(&phdr_size.to_le_bytes())?;
        patchmap_count = 0;
        for (index, (patchnum, inst)) in self.instruments.iter().enumerate() {
            write!(w, "{:\0<20}", format!("PRESET_{:05}", index))?;
            w.write_all(&(*patchnum & 0x7f).to_le_bytes())?;
            w.write_all(&((*patchnum & 0xff80) >> 7).to_le_bytes())?;
            w.write_all(&patchmap_count.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?;
            patchmap_count += inst.patchmaps.len() as u16;
        }
        write!(w, "{:\0<20}", "EOP")?;
        w.write_all(&0u16.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?;
        w.write_all(&patchmap_count.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;

        w.write_all(b"pbag")?;
        w.write_all(&pbag_size.to_le_bytes())?;
        patchmap_count = 0;
        for inst in self.instruments.values() {
            for _ in &inst.patchmaps {
                w.write_all(&patchmap_count.to_le_bytes())?;
                w.write_all(&0u16.to_le_bytes())?;
                patchmap_count += 1;
            }
        }
        w.write_all(&patchmap_count.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?;

        w.write_all(b"pmod")?;
        w.write_all(&pmod_size.to_le_bytes())?;
        w.write_all(&[0u8; 10])?;

        w.write_all(b"pgen")?;
        w.write_all(&pgen_size.to_le_bytes())?;
        patchmap_count = 0;
        for inst in self.instruments.values() {
            for _ in &inst.patchmaps {
                w.write_all(&SF2Generators::INSTRUMENT.to_le_bytes())?;
                w.write_all(&patchmap_count.to_le_bytes())?;
                patchmap_count += 1;
            }
        }
        w.write_all(&0u16.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?;

        w.write_all(b"inst")?;
        w.write_all(&inst_size.to_le_bytes())?;
        patchmap_count = 0;
        for inst in self.instruments.values() {
            for _ in &inst.patchmaps {
                write!(w, "{:\0<20}", format!("INST_{:05}", patchmap_count))?;
                w.write_all(&patchmap_count.to_le_bytes())?;
                patchmap_count += 1;
            }
        }
        write!(w, "{:\0<20}", "EOI")?;
        w.write_all(&patchmap_count.to_le_bytes())?;

        w.write_all(b"ibag")?;
        w.write_all(&ibag_size.to_le_bytes())?;
        let mut igen_index = 0u16;
        for inst in self.instruments.values() {
            for _ in &inst.patchmaps {
                w.write_all(&igen_index.to_le_bytes())?;
                w.write_all(&0u16.to_le_bytes())?;
                igen_index += INST_GENERATORS as u16;
            }
        }
        w.write_all(&igen_index.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?;

        w.write_all(b"imod")?;
        w.write_all(&imod_size.to_le_bytes())?;
        w.write_all(&[0u8; 10])?;

        w.write_all(b"igen")?;
        w.write_all(&igen_size.to_le_bytes())?;
        let mut samples_written = HashMap::new();
        sample_count = 0;
        for inst in self.instruments.values() {
            for map in &inst.patchmaps {
                let sample = map.sample.as_ref().unwrap();
                let sample_id;
                match samples_written.entry(Rc::as_ptr(sample)) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(sample_count);
                        sample_id = sample_count as u16;
                        sample_count += 1;
                    }
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        sample_id = *entry.get() as u16;
                    }
                }

                w.write_all(&SF2Generators::KEY_RANGE.to_le_bytes())?;
                w.write_all(&map.note_min.to_le_bytes())?;
                w.write_all(&map.note_max.to_le_bytes())?;

                w.write_all(&SF2Generators::INITIAL_ATTENUATION.to_le_bytes())?;
                w.write_all(&vol_to_cb(map.volume).to_le_bytes())?;

                w.write_all(&SF2Generators::PAN.to_le_bytes())?; // pan
                w.write_all(&((((map.pan as f64) - 64.0) / 64.0 * 500.0) as i16).to_le_bytes())?;

                w.write_all(&SF2Generators::OVERRIDING_ROOT_KEY.to_le_bytes())?;
                w.write_all(&(map.root_key as u16).to_le_bytes())?;

                w.write_all(&SF2Generators::FINE_TUNE.to_le_bytes())?;
                w.write_all(&(-(map.fine_adj as i16)).to_le_bytes())?;

                w.write_all(&SF2Generators::ATTACK_VOL_ENV.to_le_bytes())?;
                w.write_all(&usec_to_time_cents(map.attack_time).to_le_bytes())?;

                w.write_all(&SF2Generators::DECAY_VOL_ENV.to_le_bytes())?;
                w.write_all(&usec_to_time_cents(map.decay_time).to_le_bytes())?;

                w.write_all(&SF2Generators::RELEASE_VOL_ENV.to_le_bytes())?;
                w.write_all(&usec_to_time_cents(map.release_time).to_le_bytes())?;

                w.write_all(&SF2Generators::SUSTAIN_VOL_ENV.to_le_bytes())?;
                w.write_all(&vol_to_cb(map.decay_level).to_le_bytes())?;

                w.write_all(&SF2Generators::SAMPLE_MODES.to_le_bytes())?;
                w.write_all(
                    &sample
                        .borrow()
                        .r#loop
                        .as_ref()
                        .map(|_| 1u16)
                        .unwrap_or(0)
                        .to_le_bytes(),
                )?;

                w.write_all(&SF2Generators::SAMPLE_ID.to_le_bytes())?;
                w.write_all(&sample_id.to_le_bytes())?;
            }
        }
        w.write_all(&0u16.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?;

        w.write_all(b"shdr")?;
        w.write_all(&shdr_size.to_le_bytes())?;
        let mut sample_id = 0usize;
        let mut sample_pos = 0u32;
        self.foreach_instrument_sample(|sample| {
            let samplerate = crate::sound::cents_to_samplerate(sample.pitch);
            let end = sample_pos + sample.samples.n_samples() as u32;
            write!(w, "{:\0<20}", format!("SFX_{:05}", sample_id))?;
            w.write_all(&sample_pos.to_le_bytes())?;
            w.write_all(&end.to_le_bytes())?;
            if let Some(r#loop) = &sample.r#loop {
                w.write_all(&(sample_pos + r#loop.start).to_le_bytes())?;
                w.write_all(&(sample_pos + r#loop.end).to_le_bytes())?;
            } else {
                w.write_all(&sample_pos.to_le_bytes())?;
                w.write_all(&end.to_le_bytes())?;
            }
            w.write_all(&samplerate.to_le_bytes())?;
            w.write_all(&60u8.to_le_bytes())?; // pitch
            w.write_all(&0i8.to_le_bytes())?; // pitch correction
            w.write_all(&0u16.to_le_bytes())?; // link
            w.write_all(&1u16.to_le_bytes())?; // type
            sample_id += 1;
            sample_pos = end + 46;
            Ok(())
        })?;
        write!(w, "{:\0<20}", "EOS")?;
        w.write_all(&[0u8; 26])?;
        Ok(())
    }
}

struct SF2Generators;
impl SF2Generators {
    const PAN: u16 = 17;
    const INSTRUMENT: u16 = 41;
    const KEY_RANGE: u16 = 43;
    const ATTACK_VOL_ENV: u16 = 34;
    const DECAY_VOL_ENV: u16 = 36;
    const RELEASE_VOL_ENV: u16 = 38;
    const SUSTAIN_VOL_ENV: u16 = 37;
    const INITIAL_ATTENUATION: u16 = 48;
    const FINE_TUNE: u16 = 52;
    const SAMPLE_ID: u16 = 53;
    const SAMPLE_MODES: u16 = 54;
    const OVERRIDING_ROOT_KEY: u16 = 58;
}

fn parse_s2f_generator(map: &mut PatchMap, gid: u16, amount: u16, instrument: bool) {
    match gid {
        SF2Generators::KEY_RANGE if instrument => {
            let [note_min, note_max] = amount.to_le_bytes();
            map.note_min = note_min;
            map.note_max = note_max;
        }
        SF2Generators::INITIAL_ATTENUATION => {
            map.volume = cb_to_vol(amount);
        }
        SF2Generators::PAN => map.pan = (amount as i16 as f64 / 500.0 * 64.0 + 64.0).round() as u8,
        SF2Generators::OVERRIDING_ROOT_KEY if instrument => {
            map.root_key = amount.clamp(0, 255) as u8;
        }
        SF2Generators::FINE_TUNE => {
            map.fine_adj = (-(amount as i16)).clamp(0, 255) as u8;
        }
        SF2Generators::ATTACK_VOL_ENV => {
            map.attack_time = time_cents_to_usec(amount as i16 as f64);
        }
        SF2Generators::DECAY_VOL_ENV => {
            map.decay_time = time_cents_to_usec(amount as i16 as f64);
        }
        SF2Generators::RELEASE_VOL_ENV => {
            map.release_time = time_cents_to_usec(amount as i16 as f64);
        }
        SF2Generators::SUSTAIN_VOL_ENV => {
            map.decay_level = cb_to_vol(amount);
        }
        _ => {}
    }
}

#[inline]
fn usec_to_time_cents(us: u16) -> u16 {
    (1200.0 * (us as f64 / 1000.0).log2()).round() as i16 as u16
}

#[inline]
fn time_cents_to_usec(tc: f64) -> u16 {
    (2.0f64.powf(tc / 1200.0) * 1000.0).round() as u16
}

#[inline]
fn vol_to_cb(vol: u8) -> u16 {
    ((127.0 - vol as f64) / 127.0 * 1440.0).round() as u16
}

#[inline]
fn cb_to_vol(cb: u16) -> u8 {
    (127.0 - (cb.min(1440) as f64 / 1440.0 * 127.0)).round() as u8
}

struct DLSConn;
impl DLSConn {
    const DST_PAN: u16 = 0x0004;
    //const DST_LFO_FREQUENCY: u16 = 0x0104;
    const DST_EG1_ATTACKTIME: u16 = 0x0206;
    const DST_EG1_DECAYTIME: u16 = 0x0207;
    const DST_EG1_RELEASETIME: u16 = 0x0209;
    const DST_EG1_SUSTAINLEVEL: u16 = 0x020a;
}
fn parse_dls_articulator<'a, E: ParseError<&'a [u8]>>(
    chunk: &'a [u8],
    map: &mut PatchMap,
) -> nom::IResult<&'a [u8], (), E> {
    let (chunk, art1_size) = le_u32(chunk)?;
    let (mut chunk, head) = take(art1_size.saturating_sub(4))(chunk)?;
    let (_, nconnections) = le_u32(head)?;
    for _ in 0..nconnections {
        let (d, source) = le_u16(chunk)?;
        let (d, control) = le_u16(d)?;
        let (d, dest) = le_u16(d)?;
        let (d, transform) = le_u16(d)?;
        let (d, scale) = le_i32(d)?;
        if source == 0 && control == 0 && transform == 0 {
            match dest {
                DLSConn::DST_PAN => {
                    map.pan = ((scale as f64 / 65536.0).round() as i32 + 64).clamp(0, 127) as u8;
                }
                DLSConn::DST_EG1_ATTACKTIME => {
                    map.attack_time = time_cents_to_usec(scale as f64 / 65536.0);
                }
                DLSConn::DST_EG1_DECAYTIME => {
                    map.decay_time = time_cents_to_usec(scale as f64 / 65536.0);
                }
                DLSConn::DST_EG1_RELEASETIME => {
                    map.release_time = time_cents_to_usec(scale as f64 / 65536.0);
                }
                DLSConn::DST_EG1_SUSTAINLEVEL => {
                    map.decay_level = atten_to_vol(scale);
                }
                _ => {}
            }
        }
        chunk = d;
    }
    Ok((chunk, ()))
}

#[inline]
fn atten_to_vol(cb: i32) -> u8 {
    (16129.0 / 10.0f64.powf(-cb as f64 / 200.0))
        .sqrt()
        .clamp(0.0, 127.0) as u8
}

fn parse_dls_wsmp<'a, E: ParseError<&'a [u8]>>(
    chunk: &'a [u8],
    map: &mut PatchMap,
    r#loop: &mut Option<Loop>,
) -> nom::IResult<&'a [u8], (), E> {
    let (chunk, wsmp_size) = le_u32(chunk)?;
    let (mut chunk, head) = take(wsmp_size.saturating_sub(4))(chunk)?;
    let (head, unitynote) = le_u16(head)?;
    map.root_key = unitynote.clamp(0, 127) as u8;
    let (head, finetune) = le_i16(head)?;
    map.fine_adj = (-finetune).clamp(0, 255) as u8;
    let (head, attenuation) = le_i32(head)?;
    map.volume = atten_to_vol(attenuation);
    let (head, _options) = le_u32(head)?;
    let (_, nloops) = le_u32(head)?;
    for i in 0..nloops {
        let (d, loopsize) = le_u32(chunk)?;
        let (d, l) = take(loopsize.saturating_sub(4))(d)?;
        chunk = d;
        let (l, count) = le_u32(l)?;
        let (l, start) = le_u32(l)?;
        let (_, length) = le_u32(l)?;
        if i == 0 {
            *r#loop = Some(Loop {
                start,
                end: start + length,
                count: match count {
                    1 => 1,
                    _ => u32::MAX,
                },
            });
        }
    }
    Ok((chunk, ()))
}
