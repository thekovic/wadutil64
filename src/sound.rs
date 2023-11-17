use crate::{
    convert_error, invalid_data,
    music::{MusicSample, MusicSequence},
    too_large,
};
use binrw::{BinRead, BinWrite};
use nom::{
    branch::alt,
    bytes::complete::{tag, take},
    error::{context, ParseError},
    multi::{count, fill},
    number::complete::{be_i16, be_i32, be_u16, be_u32, le_i16, le_i8, le_u32, le_u8},
};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    io::Cursor,
    rc::Rc,
};

#[derive(Clone, Debug, Default)]
pub struct SoundData {
    pub instruments: BTreeMap<u16, Instrument>,
    pub sequences: BTreeMap<u16, Sequence>,
}

#[derive(Clone, Debug, Default)]
pub struct Instrument {
    pub patchmaps: Vec<PatchMap>,
}

#[derive(Clone, Debug)]
pub enum Sequence {
    Effect(Sample),
    MusicSeq(MusicSequence),
    MusicSample(MusicSample),
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub info: PatchInfo,
    pub priority: u8,
    pub volume: u8,
}

#[derive(Clone, Debug)]
#[binrw::binrw]
#[brw(big)]
struct Patch {
    #[brw(pad_after(1))]
    pub cnt: u8,
    pub idx: u16,
}

#[derive(Clone, Debug)]
#[binrw::binrw]
#[brw(big)]
pub struct PatchMap {
    pub priority: u8,
    pub volume: u8,
    pub pan: u8,
    pub reverb: u8,
    pub root_key: u8,
    pub fine_adj: u8,
    pub note_min: u8,
    pub note_max: u8,
    pub pitchstep_min: u8,
    pub pitchstep_max: u8,
    pub sample_id: u16,
    pub attack_time: u16,
    pub decay_time: u16,
    pub release_time: u16,
    pub attack_level: u8,
    pub decay_level: u8,
    #[brw(ignore)]
    pub sample: Option<Rc<RefCell<PatchInfo>>>,
}

impl Default for PatchMap {
    fn default() -> Self {
        Self {
            priority: 100,
            volume: 127,
            pan: 64,
            reverb: 1,
            root_key: 60,
            fine_adj: 0,
            note_min: 0,
            note_max: 127,
            pitchstep_min: 12,
            pitchstep_max: 12,
            sample_id: 0,
            attack_time: 0,
            decay_time: 32000,
            release_time: 4096,
            attack_level: 127,
            decay_level: 120,
            sample: None,
        }
    }
}

impl PatchMap {
    #[inline]
    pub fn new_sample(sample_id: u16, priority: u8, volume: u8) -> Self {
        Self {
            priority,
            volume,
            sample_id,
            attack_time: 1,
            decay_time: 32767,
            release_time: 1,
            decay_level: 127,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub enum SampleData {
    Raw(Vec<i16>),
    Adpcm {
        data: Vec<u8>,
        uncompressed_len: usize,
        book: Box<AdpcmBook>,
        loopstate: Option<Box<[i16; 16]>>,
    },
}

impl Default for SampleData {
    #[inline]
    fn default() -> Self {
        Self::Raw(Vec::new())
    }
}

impl SampleData {
    /// Length in bytes, possibly when compressed
    #[inline]
    pub fn stored_len(&self) -> usize {
        match self {
            Self::Raw(s) => s.len() * 2,
            Self::Adpcm { data, .. } => data.len(),
        }
    }
    /// Length in samples, possibly after decompression
    #[inline]
    pub fn n_samples(&self) -> usize {
        match self {
            Self::Raw(s) => s.len(),
            Self::Adpcm {
                uncompressed_len, ..
            } => *uncompressed_len,
        }
    }
    pub fn raw_data(&self) -> Cow<[i16]> {
        match self {
            Self::Raw(s) => Cow::Borrowed(s.as_slice()),
            Self::Adpcm { data, book, .. } => {
                Cow::Owned(crate::compression::decode_vadpcm(data, book))
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PatchInfo {
    pub samples: SampleData,
    pub pitch: i32,
    pub r#loop: Option<Loop>,
}

impl PatchInfo {
    pub fn compress(&self) -> std::io::Result<Cow<'_, Self>> {
        match &self.samples {
            SampleData::Raw(raw) => {
                let (data, book, loopstate) = crate::compression::encode_vadpcm(
                    raw,
                    crate::compression::AdpcmParams::default(),
                    self.r#loop.as_ref(),
                )?;
                Ok(Cow::Owned(Self {
                    samples: SampleData::Adpcm {
                        data,
                        uncompressed_len: raw.len(),
                        book: Box::new(book),
                        loopstate: loopstate.map(Box::new),
                    },
                    pitch: self.pitch,
                    r#loop: self.r#loop.clone(),
                }))
            }
            SampleData::Adpcm { .. } => Ok(Cow::Borrowed(self)),
        }
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
#[binrw::binrw]
#[brw(big)]
pub struct Loop {
    pub start: u32,
    pub end: u32,
    #[brw(pad_after(4))]
    pub count: u32,
}

#[derive(Clone, Debug)]
#[binrw::binrw]
#[brw(big)]
pub(crate) struct AdpcmLoop {
    pub start: u32,
    pub end: u32,
    pub count: u32,
    #[brw(pad_after(4))]
    pub state: [i16; 16],
}

impl AdpcmLoop {
    #[inline]
    fn to_loop(&self) -> Loop {
        Loop {
            start: self.start,
            end: self.end,
            count: self.count,
        }
    }
    #[inline]
    fn from_loop(r#loop: &Loop, state: [i16; 16]) -> Self {
        Self {
            start: r#loop.start,
            end: r#loop.end,
            count: r#loop.count,
            state,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdpcmBook {
    pub order: i32,
    pub npredictors: i32,
    pub book: [i16; 128],
}

#[inline]
pub fn parse_riff_header<'a, E: ParseError<&'a [u8]>>(
    data: &'a [u8],
    name: &[u8; 4],
) -> nom::IResult<&'a [u8], (), E> {
    let (data, _) = tag(b"RIFF")(data)?;
    let (data, riffsize) = le_u32(data)?;
    let data = &data[..riffsize as usize];
    let (data, _) = tag(name)(data)?;
    Ok((data, ()))
}

#[inline]
pub fn parse_riff_chunks<'a, E: ParseError<&'a [u8]>>(
    mut data: &'a [u8],
    mut f: impl FnMut([u8; 4], &'a [u8]) -> nom::IResult<&'a [u8], (), E>,
) -> nom::IResult<&'a [u8], (), E> {
    while !data.is_empty() {
        let (d, chunk_name) = take(4usize)(data)?;
        let (d, chunk_size) = le_u32(d)?;
        let (d, chunk) = take(align::<2>(chunk_size as usize))(d)?;
        let chunk = &chunk[..chunk_size as usize]; // trim any pad byte
        f(chunk_name.try_into().unwrap(), chunk)?;
        data = d;
    }
    Ok((data, ()))
}

#[inline]
pub fn samplerate_to_cents(rate: u32) -> i32 {
    (1200.0 * (rate as f64 / 22050.0).log2()) as i32
}

#[inline]
pub fn cents_to_samplerate(cents: i32) -> u32 {
    (2.0f64.powf(cents as f64 / 1200.0) * 22050.0) as u32
}

#[inline]
fn parse_flac_tag<T: std::str::FromStr>(
    r: &claxon::FlacReader<impl std::io::Read>,
    name: &str,
) -> Option<claxon::Result<T>>
where
    T::Err: std::fmt::Display,
{
    r.get_tag(name).next().map(|e| {
        str::parse(e).map_err(|e| {
            claxon::Error::IoError(invalid_data(format!("failed to parse {name} tag: {e}")))
        })
    })
}

impl Sample {
    pub fn read_file(data: &[u8]) -> std::io::Result<Self> {
        let magic = data.get(0..4);
        if magic == Some(b"RIFF") {
            Self::read_wav(data).map(|s| s.1).map_err(|e| {
                invalid_data(format!(
                    "{}\nWAV must be uncompressed 16-bit mono or 8-bit mono.",
                    convert_error(data, e)
                ))
            })
        } else if magic == Some(b"fLaC") {
            Self::read_flac(data).map_err(|e| invalid_data(e))
        } else {
            Err(invalid_data("Unknown file format"))
        }
    }
    pub fn read_flac(data: &[u8]) -> claxon::Result<Self> {
        let mut r = claxon::FlacReader::new(std::io::BufReader::new(data))?;
        let mut samples = Vec::with_capacity(r.streaminfo().samples.unwrap_or_default() as usize);
        let shift = r.streaminfo().bits_per_sample as i32 - 16;
        let mut blocks = r.blocks();
        let mut buf = Vec::new();
        while let Some(block) = blocks.read_next_or_eof(buf)? {
            let len = block.len() as usize;
            buf = block.into_buffer();
            for i in 0..len {
                let sample = if shift < 0 {
                    buf[i] << -shift
                } else {
                    buf[i] >> shift
                };
                samples.push(sample as i16);
            }
        }
        let priority = parse_flac_tag(&r, "d64_priority")
            .or_else(|| parse_flac_tag(&r, "priority"))
            .transpose()?
            .unwrap_or(50);
        let volume = parse_flac_tag(&r, "d64_volume")
            .or_else(|| parse_flac_tag(&r, "volume"))
            .transpose()?
            .unwrap_or(127);
        let mut r#loop = None;
        if let Some(start) = parse_flac_tag::<u32>(&r, "d64_loop")
            .or_else(|| parse_flac_tag(&r, "loop"))
            .transpose()?
        {
            let start = start.min(samples.len() as u32);
            let end = parse_flac_tag(&r, "d64_loop_end")
                .or_else(|| parse_flac_tag(&r, "loop_end"))
                .transpose()?
                .unwrap_or_else(|| samples.len() as u32)
                .max(start);
            r#loop = Some(Loop {
                start,
                end,
                count: u32::MAX,
            });
        }
        Ok(Self {
            priority,
            volume,
            info: PatchInfo {
                samples: SampleData::Raw(samples),
                pitch: samplerate_to_cents(r.streaminfo().sample_rate),
                r#loop,
            },
        })
    }
    pub fn read_wav<'a, E: ParseError<&'a [u8]>>(
        data: &'a [u8],
    ) -> nom::IResult<&'a [u8], Self, E> {
        let mut samplesize = 0;
        let mut samplerate = 0;
        let mut volume = 127;
        let mut priority = 50;
        let mut samples = None;
        let mut r#loop = None;
        let (data, _) = parse_riff_header(data, b"WAVE")?;
        let (data, _) = parse_riff_chunks(data, |chunk_name, chunk| {
            match &chunk_name {
                b"fmt " => {
                    let (chunk, _) = tag(1u16.to_le_bytes())(chunk)?;
                    let (chunk, _) = tag(1u16.to_le_bytes())(chunk)?;
                    let (chunk, sr) = le_u32(chunk)?;
                    samplerate = sr;
                    let (chunk, _datarate) = le_u32(chunk)?;
                    let (chunk, ss) =
                        alt((tag(1u16.to_le_bytes()), tag(2u16.to_le_bytes())))(chunk)?;
                    samplesize = u16::from_le_bytes(ss.try_into().unwrap());
                    tag((samplesize * 8).to_le_bytes())(chunk)?;
                }
                b"prio" => {
                    priority = le_u8(chunk)?.1;
                }
                b"inst" => {
                    let (chunk, _note) = le_u8(chunk)?;
                    let (chunk, _tune) = le_i8(chunk)?;
                    let (chunk, gain) = le_i8(chunk)?;
                    let (chunk, _lownote) = le_u8(chunk)?;
                    let (chunk, _hinote) = le_u8(chunk)?;
                    let (chunk, _lowvel) = le_u8(chunk)?;
                    let (_, _hivel) = le_u8(chunk)?;
                    if gain < 0 {
                        volume = (gain + 127) as u8;
                    }
                }
                b"smpl" => {
                    let (chunk, _manufacturer) = le_u32(chunk)?;
                    let (chunk, _product) = le_u32(chunk)?;
                    let (chunk, _period) = le_u32(chunk)?;
                    let (chunk, _note) = le_u32(chunk)?;
                    let (chunk, _pitchfrac) = le_u32(chunk)?;
                    let (chunk, _format) = le_u32(chunk)?;
                    let (chunk, _offset) = le_u32(chunk)?;
                    let (chunk, loops) = le_u32(chunk)?;
                    let (chunk, _extra) = le_u32(chunk)?;
                    if loops > 0 {
                        let (chunk, _id) = le_u32(chunk)?;
                        let (chunk, _type) = le_u32(chunk)?;
                        let (chunk, start) = le_u32(chunk)?;
                        let (chunk, end) = le_u32(chunk)?;
                        let (chunk, _frac) = le_u32(chunk)?;
                        let (_, mut count) = le_u32(chunk)?;
                        if count == 0 {
                            count = u32::MAX;
                        }
                        r#loop = Some(Loop { start, end, count });
                    }
                }
                b"data" => {
                    if samplesize == 0 {
                        return Err(crate::nom_fail(chunk));
                    }
                    if samplesize == 1 {
                        let mut cvt = Vec::with_capacity(chunk.len());
                        for s in chunk.iter().copied().map(|s| s as i8) {
                            cvt.push((s as i16) << 8);
                        }
                        samples = Some(cvt);
                    } else if samplesize == 2 {
                        samples = Some(count(le_i16, chunk.len() / 2)(chunk)?.1);
                    };
                }
                _ => {}
            }
            Ok((&[], ()))
        })?;
        let samples = match samples {
            Some(samples) => samples,
            None => {
                return Err(crate::nom_fail(data));
            }
        };
        Ok((
            data,
            Self {
                priority,
                volume,
                info: PatchInfo {
                    samples: SampleData::Raw(samples),
                    pitch: samplerate_to_cents(samplerate),
                    r#loop,
                },
            },
        ))
    }
    pub fn write_wav(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        let samples = self.info.samples.raw_data();
        let samplerate = cents_to_samplerate(self.info.pitch);
        let mut wave_size = samples.len() * 2 + 16 + 8 + 2 + 9 * 4;
        if self.info.r#loop.is_some() {
            wave_size += 60 + 4 * 2;
        }
        w.write_all(b"RIFF")?;
        w.write_all(&(wave_size as u32).to_le_bytes())?;
        w.write_all(b"WAVE")?;

        w.write_all(b"fmt ")?;
        w.write_all(&16u32.to_le_bytes())?; // fmt size
        w.write_all(&1u16.to_le_bytes())?; // WAVE_FORMAT_PCM
        w.write_all(&1u16.to_le_bytes())?; // nchannels
        w.write_all(&samplerate.to_le_bytes())?;
        w.write_all(&(2 * samplerate).to_le_bytes())?; // data rate
        w.write_all(&2u16.to_le_bytes())?; // sample size
        w.write_all(&16u16.to_le_bytes())?; // bits per sample

        w.write_all(b"inst")?;
        w.write_all(&7u32.to_le_bytes())?; // inst size
        w.write_all(&60u8.to_le_bytes())?; // note
        w.write_all(&0i8.to_le_bytes())?; // finetune
        w.write_all(&((self.volume as i8) - 127).to_le_bytes())?;
        w.write_all(&0u8.to_le_bytes())?; // notemin
        w.write_all(&127u8.to_le_bytes())?; // notemax
        w.write_all(&1u8.to_le_bytes())?; // velmin
        w.write_all(&127u8.to_le_bytes())?; // velmax
        w.write_all(&0u8.to_le_bytes())?; // padding

        w.write_all(b"prio")?;
        w.write_all(&1u32.to_le_bytes())?; // prio size
        w.write_all(&self.priority.to_le_bytes())?;
        w.write_all(&0u8.to_le_bytes())?; // padding

        if let Some(r#loop) = &self.info.r#loop {
            w.write_all(b"smpl")?;
            w.write_all(&60u32.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?; // manufacturer
            w.write_all(&0u32.to_le_bytes())?; // product
            w.write_all(&(1_000_000_000 / samplerate).to_le_bytes())?;
            w.write_all(&60u32.to_le_bytes())?; // note
            w.write_all(&0u32.to_le_bytes())?; // pitchfrac
            w.write_all(&0u32.to_le_bytes())?; // format
            w.write_all(&0u32.to_le_bytes())?; // offset
            w.write_all(&1u32.to_le_bytes())?; // nloops
            w.write_all(&0u32.to_le_bytes())?; // extra

            w.write_all(&0u32.to_le_bytes())?; // loop id
            w.write_all(&0u32.to_le_bytes())?; // loop type
            w.write_all(&r#loop.start.to_le_bytes())?;
            w.write_all(&r#loop.end.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?; // frac
            let count = if r#loop.count == u32::MAX {
                0
            } else {
                r#loop.count
            };
            w.write_all(&count.to_le_bytes())?;
        }

        w.write_all(b"data")?;
        w.write_all(&(samples.len() as u32 * 2).to_le_bytes())?;
        for s in &*samples {
            w.write_all(&s.to_le_bytes())?;
        }
        Ok(())
    }
}

#[inline]
fn align<const N: usize>(v: usize) -> usize {
    (v + (N - 1)) & !(N - 1)
}

#[inline]
fn align8_slice(data: &[u8], init_size: usize) -> &[u8] {
    let offset = init_size - data.len();
    let offset = align::<8>(offset) - offset;
    data.get(offset..).unwrap_or_default()
}

fn extract_instruments<'a, E: ParseError<&'a [u8]>>(
    wmd: &'a [u8],
    wdd: &[u8],
    decompress: bool,
) -> nom::IResult<&'a [u8], BTreeMap<u16, Instrument>, E> {
    let start = wmd.len();

    let (wmd, _) = tag(b"SN64")(wmd)?;
    let (wmd, _) = tag(&(2u32).to_be_bytes())(wmd)?;
    let (wmd, _) = take(6usize)(wmd)?;
    let (wmd, _sequences) = be_u16(wmd)?;
    let (wmd, _decomp_type) = tag(&[0])(wmd)?;
    let (wmd, _) = take(3usize)(wmd)?;
    let (wmd, _compress_size) = be_u32(wmd)?;
    let (wmd, _data_size) = be_u32(wmd)?;
    let (wmd, _) = take(4usize)(wmd)?;

    let (wmd, _load_flags) = be_u32(wmd)?;
    let (wmd, patch_count) = be_u16(wmd)?;
    let (wmd, _patch_size) = be_u16(wmd)?;
    let (wmd, patchmap_count) = be_u16(wmd)?;
    let (wmd, _patchmap_size) = be_u16(wmd)?;
    let (wmd, patchinfo_count) = be_u16(wmd)?;
    let (wmd, _patchinfo_size) = be_u16(wmd)?;
    let (wmd, drummap_count) = be_u16(wmd)?;
    let (wmd, drummap_size) = be_u16(wmd)?;
    let (wmd, _extra_data_size) = be_u32(wmd)?;

    let mut wmd = align8_slice(wmd, start);

    /*
    println!(
        "  SN64:\n\
        wmd_size {start}\n\
        sequences {_sequences}\n\
        compress_size {_compress_size}\n\
        data_size {_data_size}\n\
        load_flags {_load_flags}\n\
        patches {patch_count}\n\
        patch_size {_patch_size}\n\
        patchmaps {patchmap_count}\n\
        patchmap_size {_patchmap_size}\n\
        patchinfo {patchinfo_count}\n\
        patchinfo_size {_patchinfo_size}\n\
        drummaps {drummap_count}\n\
        drummap_size {drummap_size}\n\
        extra_data_size {_extra_data_size}"
    );
    */

    let mut patches = Vec::with_capacity(patch_count as usize);
    for _ in 0..patch_count {
        let (d, patch) = take(4usize)(wmd)?;
        wmd = d;
        let patch = Patch::read(&mut Cursor::new(patch)).unwrap();
        patches.push(patch);
    }

    let mut wmd = align8_slice(wmd, start);

    let mut patchmaps = HashMap::with_capacity(patchmap_count as usize);
    for index in 0..patchmap_count {
        let (d, patchmap) = take(20usize)(wmd)?;
        wmd = d;
        let patchmap = PatchMap::read(&mut Cursor::new(patchmap)).unwrap();
        patchmaps.insert(index, patchmap);
    }

    let mut wmd = align8_slice(wmd, start);

    let mut last_adpcm = None;
    let mut tmp_patchinfos = Vec::with_capacity(patchinfo_count as usize);
    for index in 0..patchinfo_count {
        let (d, base) = be_u32(wmd)?;
        let (d, len) = be_u32(d)?;
        let (d, r#type) = alt((tag(&[0]), tag(&[1])))(d)?;
        let r#type = r#type[0];
        let (d, _) = take(3usize)(d)?;
        let (d, pitch) = be_i32(d)?;
        let (d, r#loop) = be_i32(d)?;
        let (d, _) = take(4usize)(d)?;
        let base = base as usize;
        let samp = wdd
            .get(base..base + len as usize)
            .ok_or_else(|| crate::nom_fail(wmd))?
            .to_vec();
        wmd = d;
        //println!("sample ty {type} size {len} pitch {pitch} loop {loop}");
        tmp_patchinfos.push((samp, r#type, pitch, (r#loop != -1).then(|| r#loop as u32)));
        if r#type == 0 {
            last_adpcm = Some(index);
        }
    }

    let wmd = align8_slice(wmd, start);
    let (wmd, _) = take(drummap_count as usize * drummap_size as usize)(wmd)?;
    let wmd = align8_slice(wmd, start);

    let (wmd, _nsfx1) = be_u16(wmd)?;
    let (wmd, rawcount) = be_u16(wmd)?;
    let (wmd, adpcmcount) = be_u16(wmd)?;
    let (mut wmd, _nsfx2) = be_u16(wmd)?;

    /*
    println!(
        "nsfx1 {_nsfx1}\n\
        rawcount {rawcount}\n\
        adpcmcount {adpcmcount}\n\
        nsfx2 {_nsfx2}"
    );
    */

    let mut raw_loops = Vec::new();
    for _ in 0..rawcount {
        let (d, r#loop) = take(16usize)(wmd)?;
        wmd = d;
        raw_loops.push(Loop::read(&mut Cursor::new(r#loop)).unwrap());
    }

    let mut adpcm_loops = Vec::new();
    for _ in 0..adpcmcount {
        let (d, r#loop) = take(48usize)(wmd)?;
        wmd = d;
        adpcm_loops.push(AdpcmLoop::read(&mut Cursor::new(r#loop)).unwrap());
    }

    let mut books = Vec::new();
    if let Some(last_adpcm) = last_adpcm {
        for _ in 0..(last_adpcm as u32 + 1) {
            let (d, order) = be_i32(wmd)?;
            let (d, npredictors) = be_i32(d)?;
            let mut book = [0; 128];
            let (d, _) = fill(be_i16, &mut book)(d)?;
            wmd = d;
            books.push(AdpcmBook {
                order,
                npredictors,
                book,
            });
        }
    }

    // all done parsing wmd, extract loops into PatchInfo structs
    let mut samples = BTreeMap::new();
    for (index, (samp, r#type, pitch, r#loop)) in tmp_patchinfos.into_iter().enumerate() {
        let (samp, r#loop) = if r#type == 1 {
            let r#loop = if let Some(r#loop) = r#loop {
                Some(
                    raw_loops
                        .get(r#loop as usize)
                        .ok_or_else(|| too_large(&[]))?
                        .clone(),
                )
            } else {
                None
            };
            let mut out = Vec::with_capacity(samp.len() / 2);
            for s in samp.chunks_exact(2) {
                out.push(i16::from_be_bytes(<[u8; 2]>::try_from(s).unwrap()));
            }
            (SampleData::Raw(out), r#loop)
        } else {
            let r#loop = if let Some(r#loop) = r#loop {
                Some(
                    adpcm_loops
                        .get(r#loop as usize)
                        .ok_or_else(|| too_large(&[]))?,
                )
            } else {
                None
            };
            let book = books.get(index).ok_or_else(|| too_large(&[]))?;
            let out = match decompress {
                true => SampleData::Raw(crate::compression::decode_vadpcm(&samp, book)),
                false => {
                    let uncompressed_len = samp.chunks_exact(9).len() * 16;
                    SampleData::Adpcm {
                        data: samp,
                        uncompressed_len,
                        book: Box::new(book.clone()),
                        loopstate: r#loop.map(|l| Box::new(l.state)),
                    }
                }
            };
            (out, r#loop.map(|l| l.to_loop()))
        };
        samples.insert(
            index as u16,
            Rc::new(RefCell::new(PatchInfo {
                samples: samp,
                pitch,
                r#loop,
            })),
        );
    }

    // un-flatten patchmaps into Instrument structs
    let mut instruments = BTreeMap::new();
    for (index, patch) in patches.into_iter().enumerate() {
        let mut instrument = Instrument {
            patchmaps: Vec::with_capacity(patch.cnt as usize),
        };
        for index in patch.idx..patch.idx + patch.cnt as u16 {
            let mut map = patchmaps
                .remove(&index)
                .unwrap_or_else(|| panic!("Patchmap {index} not found"));
            let sample = samples
                .get(&map.sample_id)
                .unwrap_or_else(|| panic!("Sample {} not found", map.sample_id));
            map.sample = Some(sample.clone());
            instrument.patchmaps.push(map);
        }
        instruments.insert(index as u16, instrument);
    }

    Ok((wmd, instruments))
}

pub fn extract_sound(
    wmd: &[u8],
    wsd: &[u8],
    wdd: &[u8],
    decompress: bool,
) -> std::io::Result<SoundData> {
    let mut instruments = context("WMD", |wmd| extract_instruments(wmd, wdd, decompress))(wmd)
        .map_err(|e| invalid_data(convert_error(wmd, e)))?
        .1;

    // detect SNDFX_CLASS sequences
    let mut sequences = context("WSD", crate::music::extract_sequences)(wsd)
        .map_err(|e| invalid_data(convert_error(wmd, e)))?
        .1;
    for (index, seq) in sequences.iter_mut() {
        let mus = match seq {
            Sequence::MusicSeq(mus) => mus,
            _ => unreachable!(),
        };
        if mus.tracks.len() == 1 && mus.tracks[0].voices_type == 0 {
            let patch = mus.tracks[0].initpatchnum;
            let mut inst = instruments
                .remove(&patch)
                .unwrap_or_else(|| panic!("No patch for sfx sequence {index}"));
            if *index == 0 {
                *seq = Sequence::Effect(Sample {
                    info: Default::default(),
                    volume: 0,
                    priority: 99,
                });
            } else {
                let info = inst.patchmaps[0]
                    .sample
                    .take()
                    .unwrap_or_else(|| panic!("Instrument {patch} has no sample"));
                let info = Rc::try_unwrap(info)
                    .unwrap_or_else(|i| (*i).clone())
                    .into_inner();
                *seq = Sequence::Effect(Sample {
                    info,
                    volume: inst.patchmaps[0].volume,
                    priority: inst.patchmaps[0].priority,
                });
            }
        }
    }

    Ok(SoundData {
        instruments,
        sequences,
    })
}

impl SoundData {
    pub fn compress(&mut self) {
        self.foreach_sample_mut(|index, info| {
            match info.compress() {
                Ok(Cow::Owned(compressed)) => *info = compressed,
                Ok(Cow::Borrowed(_)) => {}
                Err(e) => log::warn!("Failed to encode sample {index}: {e}"),
            }
            Ok(())
        })
        .unwrap();
    }
    pub fn write_wmd(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        let numinsts = self
            .instruments
            .last_key_value()
            .map(|e| *e.0 + 1)
            .unwrap_or_default();
        let mut samples_written = HashMap::new();
        let mut patch_count = 0usize;
        let mut patchmap_count = 0usize;
        let mut sample_count = 0usize;
        let mut last_adpcm = None;
        let mut extra_data_size = 0u32;
        for index in 0..numinsts {
            patch_count += 1;
            if let Some(inst) = self.instruments.get(&index) {
                patchmap_count += inst.patchmaps.len();
                for map in &inst.patchmaps {
                    let sample = map.sample.as_ref().unwrap();
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        samples_written.entry(Rc::as_ptr(sample))
                    {
                        entry.insert(sample_count);
                        let sample = sample.borrow();
                        let compress = matches!(sample.samples, SampleData::Adpcm { .. });
                        if compress {
                            last_adpcm = Some(sample_count);
                        }
                        if sample.r#loop.is_some() {
                            extra_data_size += if compress { 48 } else { 16 };
                        }
                        sample_count += 1;
                    }
                }
            }
        }
        samples_written.clear();
        let sequence_count = self
            .sequences
            .last_key_value()
            .map(|e| *e.0 + 1)
            .unwrap_or_default();
        for seq in self.sequences.values() {
            if let Sequence::MusicSample(sample) = seq {
                let count = sample.sample_count();
                patch_count += count;
                patchmap_count += count;
                sample_count += count;
            }
        }
        for seq in self.sequences.values() {
            if let Sequence::Effect(sample) = seq {
                let compress = matches!(sample.info.samples, SampleData::Adpcm { .. });
                if compress {
                    last_adpcm = Some(sample_count);
                }
                if sample.info.r#loop.is_some() {
                    extra_data_size += if compress { 48 } else { 16 };
                }
                patch_count += 1;
                patchmap_count += 1;
                sample_count += 1;
            }
        }
        if let Some(last_adpcm) = last_adpcm {
            extra_data_size += (last_adpcm as u32 + 1) * 264;
        }
        let data_size = ((patch_count * 4)
            + (patch_count & 1) * 4
            + (patchmap_count * 20)
            + (patchmap_count & 1) * 4
            + (sample_count * 24)
            + align::<16>(extra_data_size as usize)
            + 24) as u32;

        w.write_all(b"SN64")?;
        w.write_all(&2u32.to_be_bytes())?;
        w.write_all(&[0u8; 6])?;
        w.write_all(&sequence_count.to_be_bytes())?;
        w.write_all(&[0u8; 8])?;
        w.write_all(&data_size.to_be_bytes())?;
        w.write_all([0u8; 4].as_slice())?;

        w.write_all(&31u32.to_be_bytes())?;
        w.write_all(&(patch_count as u16).to_be_bytes())?;
        w.write_all(&4u16.to_be_bytes())?;
        w.write_all(&(patchmap_count as u16).to_be_bytes())?;
        w.write_all(&20u16.to_be_bytes())?;
        w.write_all(&(sample_count as u16).to_be_bytes())?;
        w.write_all(&24u16.to_be_bytes())?;
        w.write_all([0u8; 4].as_slice())?;
        w.write_all(&extra_data_size.to_be_bytes())?;

        patchmap_count = 0;
        for index in 0..numinsts {
            let cnt = match self.instruments.get(&index) {
                Some(i) => i.patchmaps.len() as u8,
                None => 0,
            };
            let patch = Patch {
                cnt,
                idx: patchmap_count as u16,
            };
            patch.write_no_seek(w)?;
            patchmap_count += cnt as usize;
        }
        for seq in self.sequences.values() {
            if let Sequence::MusicSample(sample) = seq {
                for _ in 0..sample.sample_count() {
                    let patch = Patch {
                        cnt: 1,
                        idx: patchmap_count as u16,
                    };
                    patch.write_no_seek(w)?;
                    patchmap_count += 1;
                }
            }
        }
        for seq in self.sequences.values() {
            if let Sequence::Effect(_) = seq {
                let patch = Patch {
                    cnt: 1,
                    idx: patchmap_count as u16,
                };
                patch.write_no_seek(w)?;
                patchmap_count += 1;
            }
        }
        if (patch_count & 1) == 1 {
            w.write_all([0u8; 4].as_slice())?; // pad to 8 byte boundary
        }

        patchmap_count = 0;
        sample_count = 0;
        for mut inst in self.instruments.values().cloned() {
            for map in &mut inst.patchmaps {
                let sample = map.sample.as_ref().unwrap();
                match samples_written.entry(Rc::as_ptr(sample)) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(sample_count);
                        map.sample_id = sample_count as u16;
                        sample_count += 1;
                    }
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        map.sample_id = *entry.get() as u16;
                    }
                }
                map.write_no_seek(w)?;
            }
            patchmap_count += inst.patchmaps.len();
        }
        samples_written.clear();
        for seq in self.sequences.values() {
            if let Sequence::MusicSample(s) = seq {
                for _ in 0..s.sample_count() {
                    let map = PatchMap::new_sample(sample_count as u16, s.priority, s.volume);
                    map.write_no_seek(w)?;
                    patchmap_count += 1;
                    sample_count += 1;
                }
            }
        }
        for seq in self.sequences.values() {
            if let Sequence::Effect(s) = seq {
                let map = PatchMap::new_sample(sample_count as u16, s.priority, s.volume);
                map.write_no_seek(w)?;
                patchmap_count += 1;
                sample_count += 1;
            }
        }
        if (patchmap_count & 1) == 1 {
            w.write_all([0u8; 4].as_slice())?; // pad to 8 byte boundary
        }

        let mut sample_offset = 0u32;
        let mut rawloopcount = 0usize;
        let mut adpcmloopcount = 0usize;
        self.foreach_sample(|sample| {
            let (r#type, len) = match &sample.samples {
                SampleData::Raw(samples) => (1u32, samples.len() as u32 * 2),
                SampleData::Adpcm { data, .. } => (0u32, data.len() as u32),
            };
            w.write_all(&sample_offset.to_be_bytes())?;
            w.write_all(&len.to_be_bytes())?;
            let r#loop = sample
                .r#loop
                .as_ref()
                .map(|_| {
                    let loopcount = match &sample.samples {
                        SampleData::Raw(_) => &mut rawloopcount,
                        SampleData::Adpcm { .. } => &mut adpcmloopcount,
                    };
                    *loopcount += 1;
                    (*loopcount - 1) as i32
                })
                .unwrap_or(-1);
            w.write_all(&r#type.to_le_bytes())?; // leave it a u32 for padding
            w.write_all(&sample.pitch.to_be_bytes())?;
            w.write_all(&r#loop.to_be_bytes())?;
            w.write_all([0u8; 4].as_slice())?;
            sample_offset += len;
            Ok(())
        })?;

        w.write_all(&(sample_count as u16).to_be_bytes())?;
        w.write_all(&(rawloopcount as u16).to_be_bytes())?;
        w.write_all(&(adpcmloopcount as u16).to_be_bytes())?;
        w.write_all(&(sample_count as u16).to_be_bytes())?;

        self.foreach_sample(|sample| {
            if let Some(r#loop) = &sample.r#loop {
                if let SampleData::Adpcm { loopstate, .. } = &sample.samples {
                    AdpcmLoop::from_loop(r#loop, *loopstate.clone().unwrap()).write_no_seek(w)?;
                } else {
                    r#loop.write_no_seek(w)?;
                }
            }
            Ok(())
        })?;

        if let Some(last_adpcm) = last_adpcm {
            sample_count = 0;
            self.foreach_sample(|sample| {
                if let SampleData::Adpcm { book, .. } = &sample.samples {
                    w.write_all(&book.order.to_be_bytes())?;
                    w.write_all(&book.npredictors.to_be_bytes())?;
                    for v in &book.book {
                        w.write_all(&v.to_be_bytes())?;
                    }
                } else if sample_count <= last_adpcm {
                    w.write_all(&[0u8; 264])?;
                }
                sample_count += 1;
                Ok(())
            })?;
        }

        if (extra_data_size & 8) != 0 {
            w.write_all([0u8; 8].as_slice())?; // pad to 16 byte boundary
        }

        Ok(())
    }
    pub fn write_wsd(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        let mut seqheaders = Vec::with_capacity(16 * self.sequences.len());
        let mut trackdata = Vec::new();

        let mut samp_patchidx = self
            .instruments
            .last_key_value()
            .map(|e| *e.0 + 1)
            .unwrap_or_default();
        let mut effect_patchidx = samp_patchidx;
        for seq in self.sequences.values() {
            if let Sequence::MusicSample(samp) = seq {
                effect_patchidx += samp.sample_count() as u16;
            }
        }
        let numseqs = self
            .sequences
            .last_key_value()
            .map(|e| *e.0 + 1)
            .unwrap_or_default();
        let mut sample_seq = MusicSequence::new_effect();
        let mut loop_seq = MusicSequence::new_loop_effect();
        let empty = Sequence::MusicSeq(MusicSequence::default());
        for seqindex in 0..numseqs {
            let seq = self.sequences.get(&seqindex).unwrap_or(&empty);
            let filepos = trackdata.len();
            match seq {
                Sequence::MusicSeq(mus) => {
                    let n_tracks = u16::try_from(mus.tracks.len()).unwrap();
                    mus.write_raw(&mut trackdata)?;
                    seqheaders.extend_from_slice(&n_tracks.to_be_bytes());
                }
                Sequence::MusicSample(sample) => {
                    sample.to_seq(samp_patchidx).write_raw(&mut trackdata)?;
                    seqheaders.extend_from_slice(&1u16.to_be_bytes());
                    samp_patchidx += sample.sample_count() as u16;
                }
                Sequence::Effect(sample) => {
                    let out_seq = match sample.info.r#loop {
                        Some(_) => &mut loop_seq,
                        None => {
                            let delta = (sample.info.samples.n_samples() as f64 * 240.0
                                / 22050.0
                                / 2.0f64.powf(sample.info.pitch as f64 / 1200.0))
                            .ceil() as u32;
                            sample_seq.tracks[0].events[1].delta = delta;
                            &mut sample_seq
                        }
                    };
                    out_seq.tracks[0].initpatchnum = effect_patchidx;
                    out_seq.write_raw(&mut trackdata)?;
                    seqheaders.extend_from_slice(&1u16.to_be_bytes());
                    effect_patchidx += 1;
                }
            }
            // pad to 8 byte boundary
            while (trackdata.len() & 7) != 0 {
                trackdata.push(0);
            }
            let infolen = trackdata.len() - filepos;
            seqheaders.extend_from_slice(&[0u8; 2]);
            seqheaders.extend_from_slice(&u32::try_from(infolen).unwrap().to_be_bytes());
            seqheaders.extend_from_slice(&u32::try_from(filepos).unwrap().to_be_bytes());
            seqheaders.extend_from_slice(&[0u8; 4]);
        }

        let data_size = u32::try_from(seqheaders.len()).expect("track headers too large");

        w.write_all(b"SSEQ")?;
        w.write_all(&2u32.to_be_bytes())?;
        w.write_all(&[0u8; 6])?;
        w.write_all(&u16::try_from(self.sequences.len()).unwrap().to_be_bytes())?;
        w.write_all(&[0u8; 8])?;
        w.write_all(&data_size.to_be_bytes())?;
        w.write_all([0u8; 4].as_slice())?;
        w.write_all(&seqheaders)?;
        w.write_all(&trackdata)?;
        Ok(())
    }
    pub fn write_wdd(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        self.foreach_sample(|sample| {
            match &sample.samples {
                SampleData::Raw(samples) => {
                    for s in samples.iter().copied() {
                        w.write_all(&s.to_be_bytes())?;
                    }
                }
                SampleData::Adpcm { data, .. } => {
                    w.write_all(data)?;
                }
            }
            Ok(())
        })
    }
    pub fn foreach_instrument_sample(
        &self,
        mut f: impl FnMut(&PatchInfo) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut samples_written = HashSet::new();
        for inst in self.instruments.values() {
            for map in &inst.patchmaps {
                let sample = map.sample.as_ref().unwrap();
                if samples_written.insert(Rc::as_ptr(sample)) {
                    f(&sample.borrow())?;
                }
            }
        }
        for (_, seq) in &self.sequences {
            if let Sequence::MusicSample(sample) = seq {
                for info in sample.samples() {
                    f(info)?;
                }
            }
        }
        Ok(())
    }
    pub fn foreach_instrument_sample_mut(
        &mut self,
        mut f: impl FnMut(usize, &mut PatchInfo) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut index = 0usize;
        let mut samples_written = HashSet::new();
        for inst in self.instruments.values_mut() {
            for map in &mut inst.patchmaps {
                let sample = map.sample.as_mut().unwrap();
                if samples_written.insert(Rc::as_ptr(sample)) {
                    f(index, &mut sample.borrow_mut())?;
                    index += 1;
                }
            }
        }
        for (_, seq) in &mut self.sequences {
            if let Sequence::MusicSample(sample) = seq {
                for info in sample.samples_mut() {
                    f(index, info)?;
                    index += 1;
                }
            }
        }
        Ok(())
    }
    pub fn foreach_sample(
        &self,
        mut f: impl FnMut(&PatchInfo) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        self.foreach_instrument_sample(&mut f)?;
        for seq in self.sequences.values() {
            if let Sequence::Effect(sample) = seq {
                f(&sample.info)?;
            }
        }
        Ok(())
    }
    pub fn foreach_sample_mut(
        &mut self,
        mut f: impl FnMut(usize, &mut PatchInfo) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut last_index = None;
        self.foreach_instrument_sample_mut(|index, info| {
            last_index = Some(index);
            f(index, info)
        })?;
        let mut index = last_index.map(|i| i + 1).unwrap_or_default();
        for seq in self.sequences.values_mut() {
            if let Sequence::Effect(sample) = seq {
                f(index, &mut sample.info)?;
                index += 1;
            }
        }
        Ok(())
    }
}

pub(crate) trait NoSeekWrite {
    fn write_no_seek<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()>;
}

impl<T: BinWrite> NoSeekWrite for T
where
    Self: binrw::meta::WriteEndian,
    for<'a> T::Args<'a>: Default,
{
    fn write_no_seek<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.write(&mut binrw::io::NoSeek::new(writer))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}
