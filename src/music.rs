use crate::{
    sound::{NoSeekWrite, PatchInfo, Sample, Sequence, SoundData},
    too_large,
};
use binrw::BinRead;
use ghakuf::formats::{VLQBuilder, VLQ};
use nom::{
    bytes::complete::{tag, take},
    error::ParseError,
    multi::count,
    number::complete::{be_u16, be_u32, be_u8},
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::Cursor,
};

#[derive(Clone, Debug, Default)]
pub struct MusicSequence {
    pub tracks: Vec<Track>,
}

#[derive(Default)]
struct SequenceBuilder {
    track: u8,
    initppq: u16,
    initqpm: u16,
    format: u16,
    tempo_map: Vec<TimedEvent>,
    patch_changes: HashMap<u8, u16>,
    rpns: HashMap<u8, u16>,
    pitch_sens: HashMap<u8, (u8, u8)>,
    tracks: HashMap<u8, Track>,
    cur_time: u32,
}

impl SequenceBuilder {
    fn finish_patch_changes(&mut self) {
        for (ch, patch) in std::mem::take(&mut self.patch_changes) {
            if self.cur_time == 0 {
                self.track(Some(ch)).initpatchnum = patch;
            } else {
                self.push_event(Some(ch), Event::PatchChg(patch));
            }
        }
    }
    #[inline]
    fn add_time(&mut self, delta: u32) {
        if delta > 0 {
            self.finish_patch_changes();
        }
        self.cur_time += delta;
    }
    #[inline]
    fn cur_track(&self, ch: Option<u8>) -> u8 {
        if self.format == 1 {
            self.track - 1
        } else if let Some(ch) = ch {
            ch
        } else {
            0
        }
    }
    fn track(&mut self, ch: Option<u8>) -> &mut Track {
        self.tracks.entry(self.cur_track(ch)).or_insert_with(|| {
            Track {
                voices_type: 1, // MUSIC_CLASS
                initvolume_cntrl: 127,
                initpan_cntrl: 64,
                initppq: self.initppq,
                initqpm: self.initqpm,
                ..Default::default()
            }
        })
    }
    #[inline]
    fn push_event(&mut self, ch: Option<u8>, event: Event) {
        let cur_time = self.cur_time;
        self.track(ch).events.push(TimedEvent {
            delta: cur_time,
            event,
        });
    }
    #[inline]
    fn patch_changes(&mut self, ch: u8) -> &mut u16 {
        self.patch_changes.entry(ch).or_default()
    }
    #[inline]
    fn rpn(&mut self, ch: u8) -> &mut u16 {
        self.rpns.entry(ch).or_default()
    }
    #[inline]
    fn pitch_sens(&mut self, ch: u8) -> &mut (u8, u8) {
        self.pitch_sens.entry(ch).or_default()
    }
}

impl ghakuf::reader::Handler for SequenceBuilder {
    fn header(&mut self, format: u16, _track: u16, time_base: u16) {
        self.format = format;
        self.initppq = time_base;
    }
    fn meta_event(&mut self, delta_time: u32, event: &ghakuf::messages::MetaEvent, data: &Vec<u8>) {
        use ghakuf::messages::MetaEvent;
        self.add_time(delta_time);
        match event {
            MetaEvent::SetTempo if data.len() == 3 => {
                let tempo = [0, data[0], data[1], data[2]];
                let qpm = 60_000_000 / u32::from_be_bytes(tempo);
                let qpm = u16::try_from(qpm).unwrap();
                if self.cur_time == 0 {
                    self.initqpm = qpm;
                } else {
                    self.tempo_map.push(TimedEvent {
                        delta: self.cur_time,
                        event: Event::TrkTempo(qpm),
                    });
                }
            }
            MetaEvent::SequencerSpecificMetaEvent => {
                if data.starts_with(&[0, 0x20]) && data.len() == 4 {
                    let label = u16::from_be_bytes(data[2..4].try_into().unwrap());
                    self.push_event(None, Event::TrkJump(label));
                } else if data == &[0, 0x23] {
                    let track = self.track(None);
                    track.labels.push(track.events.len());
                    self.push_event(None, Event::Null);
                }
            }
            _ => {}
        }
    }
    fn midi_event(&mut self, delta_time: u32, event: &ghakuf::messages::MidiEvent) {
        use ghakuf::messages::MidiEvent::*;
        if self.cur_time == 0
            && !matches!(
                event,
                ControlChange { control: 0, .. } | ProgramChange { .. }
            )
        {
            self.finish_patch_changes();
        }
        self.add_time(delta_time);
        match event {
            NoteOff { ch, note, .. }
            | NoteOn {
                ch,
                note,
                velocity: 0,
            } => {
                self.push_event(Some(*ch), Event::NoteOff(*note));
            }
            NoteOn { ch, note, velocity } => {
                self.push_event(Some(*ch), Event::NoteOn(*note, *velocity));
            }
            ControlChange {
                ch,
                control: 0,
                data,
            } => {
                let p = self.patch_changes(*ch);
                *p = (*p & 0x7f) | ((*data as u16 & 0x7f) << 7);
            }
            ControlChange {
                ch,
                control: 1,
                data,
            } => {
                self.push_event(Some(*ch), Event::ModuMod(*data));
            }
            ControlChange {
                ch,
                control: 6,
                data,
            } => {
                if *self.rpn(*ch) == 0 {
                    self.pitch_sens(*ch).0 = *data;
                }
            }
            ControlChange {
                ch,
                control: 7,
                data,
            } => {
                if self.cur_time == 0 {
                    self.track(Some(*ch)).initvolume_cntrl = *data;
                } else {
                    self.push_event(Some(*ch), Event::VolumeMod(*data));
                }
            }
            ControlChange {
                ch,
                control: 10,
                data,
            } => {
                if self.cur_time == 0 {
                    self.track(Some(*ch)).initpan_cntrl = *data;
                } else {
                    self.push_event(Some(*ch), Event::PanMod(*data));
                }
            }
            ControlChange {
                ch,
                control: 38,
                data,
            } => {
                if *self.rpn(*ch) == 0 {
                    self.pitch_sens(*ch).1 = *data;
                }
            }
            ControlChange {
                ch,
                control: 64,
                data,
            } => {
                self.push_event(Some(*ch), Event::PedalMod(*data));
            }
            ControlChange {
                ch,
                control: 91,
                data,
            } => {
                self.push_event(Some(*ch), Event::ReverbMod(*data));
            }
            ControlChange {
                ch,
                control: 93,
                data,
            } => {
                self.push_event(Some(*ch), Event::ChorusMod(*data));
            }
            ControlChange {
                ch,
                control: 100,
                data,
            } => {
                let rpn = self.rpn(*ch);
                *rpn = (*rpn & 0x3f80) | ((*data as u16) & 0x7f);
            }
            ControlChange {
                ch,
                control: 101,
                data,
            } => {
                let rpn = self.rpn(*ch);
                *rpn = (*rpn & 0x7f) | ((*data as u16 & 0x7f) << 7);
            }
            ControlChange {
                ch,
                control: 102,
                data,
            } => {
                let track = self.track(Some(*ch));
                let id = *data as usize;
                while track.labels.len() <= id {
                    track.labels.push(0);
                }
                track.labels[id] = track.events.len();
                self.push_event(Some(*ch), Event::Null);
            }
            ControlChange {
                ch,
                control: 103,
                data,
            } => {
                self.push_event(Some(*ch), Event::TrkJump(*data as u16));
            }
            ProgramChange { ch, program } => {
                let p = self.patch_changes(*ch);
                *p = (*p & 0x3f80) | ((*program as u16) & 0x7f);
            }
            PitchBendChange { ch, data } => {
                let range = match self.pitch_sens.get(ch) {
                    Some((semitones, cents)) => *semitones as f64 + *cents as f64 / 100.0,
                    None => 2.0,
                };
                let bend = (*data as f64 * range / 12.0).clamp(-8192.0, 8191.0) as i16;
                if self.cur_time == 0 {
                    self.track(Some(*ch)).initpitch_cntrl = bend;
                } else {
                    self.push_event(Some(*ch), Event::PitchMod(bend));
                }
            }
            _ => {}
        }
    }
    fn sys_ex_event(
        &mut self,
        delta_time: u32,
        _event: &ghakuf::messages::SysExEvent,
        _data: &Vec<u8>,
    ) {
        self.add_time(delta_time);
    }
    fn track_change(&mut self) {
        if self.format == 1 {
            self.finish_patch_changes();
            self.cur_time = 0;
            self.track += 1;
        }
    }
}

fn write_pitch_range(
    snd: &SoundData,
    ch: u8,
    note: Option<u8>,
    bend: i16,
    patch: u16,
    messages: &mut Vec<ghakuf::messages::Message>,
    laststep: &mut u8,
) {
    let (note, inst) = match (note, snd.instruments.get(&patch)) {
        (Some(n), Some(i)) => (n, i),
        _ => return,
    };
    for map in &inst.patchmaps {
        if note >= map.note_min && note <= map.note_max {
            let step = if bend < 0 {
                map.pitchstep_min
            } else {
                map.pitchstep_max
            };
            if step != *laststep {
                *laststep = step;
                messages.push(ghakuf::messages::Message::MidiEvent {
                    delta_time: 0,
                    event: ghakuf::messages::MidiEvent::ControlChange {
                        ch,
                        control: 0x06, // MSB
                        data: step,
                    },
                });
            }
        }
    }
}

impl MusicSequence {
    pub fn new_effect() -> Self {
        let mut seq = Self::default();
        let events = vec![
            TimedEvent::new(0, Event::NoteOn(60, 127)),
            TimedEvent::new(0, Event::NoteOff(60)),
            TimedEvent::new(0, Event::TrkEnd),
        ];
        let track = Track {
            initvolume_cntrl: 127,
            initpan_cntrl: 64,
            initppq: 120,
            initqpm: 120,
            events,
            ..Default::default()
        };
        seq.tracks.push(track);
        seq
    }
    pub fn new_loop_effect() -> Self {
        let mut seq = Self::default();
        let events = vec![
            TimedEvent::new(0, Event::NoteOn(60, 127)),
            TimedEvent::new(0, Event::Null),
            /* the delta value here is not important as long the label jumps
            to the null event above */
            TimedEvent::new(120, Event::TrkJump(0)),
            TimedEvent::new(0, Event::TrkEnd),
        ];
        let track = Track {
            initvolume_cntrl: 127,
            initpan_cntrl: 64,
            initppq: 120,
            initqpm: 120,
            labels: vec![1],
            events,
            ..Default::default()
        };
        seq.tracks.push(track);
        seq
    }
    pub fn read_midi(r: &mut (impl std::io::Read + std::io::Seek)) -> std::io::Result<Self> {
        let mut handler = SequenceBuilder {
            initqpm: 120,
            ..Default::default()
        };
        let mut midi = ghakuf::reader::Reader::from_reader(&mut handler, r).unwrap();
        midi.read().unwrap();
        let mut seq = MusicSequence { tracks: Vec::new() };
        for ch in 0..16 {
            if let Some(mut track) = handler.tracks.remove(&ch) {
                track.events.extend_from_slice(&handler.tempo_map);
                track.events.sort_by_key(|e| e.delta);
                let mut last_time = 0;
                for event in &mut track.events {
                    let tmp = event.delta;
                    event.delta -= last_time;
                    last_time = tmp;
                }
                track.events.push(TimedEvent {
                    delta: 0,
                    event: Event::TrkEnd,
                });
                seq.tracks.push(track);
            }
        }
        Ok(seq)
    }
    pub fn write_raw(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        let mut labels = Vec::new();
        let mut eventdata = Vec::new();
        for track in &self.tracks {
            track.write_no_seek(w)?;
            w.write_all(&u16::try_from(track.labels.len()).unwrap().to_be_bytes())?;
            let mut labelidx = 0;
            for (index, event) in track.events.iter().enumerate() {
                eventdata.append(&mut VLQ::new(event.delta).binary());
                if track.labels.get(labelidx) == Some(&index) {
                    labels
                        .extend_from_slice(&u32::try_from(eventdata.len()).unwrap().to_be_bytes());
                    labelidx += 1;
                }
                event.event.write(&mut eventdata)?;
            }
            // pad to 4 byte boundary
            while (eventdata.len() & 3) != 0 {
                eventdata.push(0);
            }
            w.write_all(&(eventdata.len() as u32).to_be_bytes())?;
            w.write_all(&labels)?;
            w.write_all(&eventdata)?;
            labels.clear();
            eventdata.clear();
        }
        Ok(())
    }
    pub fn write_midi(&self, snd: &SoundData, w: &mut impl std::io::Write) -> std::io::Result<()> {
        use ghakuf::messages::{
            Message::*,
            MetaEvent::{EndOfTrack, SetTempo},
            MidiEvent::*,
        };

        let mut midi = ghakuf::writer::Writer::new();
        midi.format(1);
        let mut tempo = 120;
        assert!(self.tracks.len() <= 16);
        if let Some(track) = self.tracks.first() {
            tempo = track.initqpm;
            midi.time_base(track.initppq);
        }
        let tempo = 60_000_000 / (tempo as u32);
        assert!(tempo <= 0xffffff);

        let mut tempo_map = Vec::new();
        tempo_map.push(MetaEvent {
            delta_time: 0,
            event: SetTempo,
            data: tempo.to_be_bytes()[1..].to_vec(),
        });

        let mut messages = Vec::new();
        for (track_index, track) in self.tracks.iter().enumerate() {
            if track.voices_type != 1 {
                continue;
            }
            // skip the drum track
            let ch = if track_index >= 9 {
                track_index + 1
            } else {
                track_index
            } as u8;
            let mut tempo_delta = 0;
            let mut delta_time = 0;
            let mut label_index = 0;
            let mut cur_patch = track.initpatchnum;
            let mut cur_bend = track.initpitch_cntrl;
            let mut cur_pitchstep = 12;
            let mut last_note = None;

            messages.push(TrackChange);
            // init messages
            if track.initpatchnum >= 0x80 {
                messages.push(MidiEvent {
                    delta_time,
                    event: ControlChange {
                        ch,
                        control: 0,
                        data: (track.initpatchnum >> 7) as u8,
                    },
                });
            }
            messages.push(MidiEvent {
                delta_time: 0,
                event: ProgramChange {
                    ch,
                    program: (track.initpatchnum & 0x7f) as u8,
                },
            });
            // pitch range RPNs
            messages.push(MidiEvent {
                delta_time: 0,
                event: ControlChange {
                    ch,
                    control: 0x64, // LSB
                    data: 0,
                },
            });
            messages.push(MidiEvent {
                delta_time: 0,
                event: ControlChange {
                    ch,
                    control: 0x65, // MSB
                    data: 0,
                },
            });
            messages.push(MidiEvent {
                delta_time: 0,
                event: ControlChange {
                    ch,
                    control: 0x26, // LSB
                    data: 0,
                },
            });
            messages.push(MidiEvent {
                delta_time: 0,
                event: ControlChange {
                    ch,
                    control: 0x06, // MSB
                    data: 12,
                },
            });
            // pitch range
            messages.push(MidiEvent {
                delta_time: 0,
                event: PitchBendChange {
                    ch,
                    data: track.initpitch_cntrl,
                },
            });
            // track events
            for (index, event) in track.events.iter().enumerate() {
                delta_time += event.delta;
                tempo_delta += event.delta;
                if track.labels.get(label_index) == Some(&index) {
                    messages.push(MidiEvent {
                        delta_time,
                        event: ControlChange {
                            ch,
                            control: 102,
                            data: label_index as u8,
                        },
                    });
                    delta_time = 0;
                    label_index += 1;
                }
                let midi_event = match event.event {
                    Event::PatchChg(val) => {
                        cur_patch = val;
                        write_pitch_range(
                            snd,
                            ch,
                            last_note,
                            cur_bend,
                            cur_patch,
                            &mut messages,
                            &mut cur_pitchstep,
                        );
                        messages.push(MidiEvent {
                            delta_time,
                            event: ControlChange {
                                ch,
                                control: 0,
                                data: (val >> 7) as u8,
                            },
                        });
                        delta_time = 0;
                        Some(ProgramChange {
                            ch,
                            program: (val & 0x7f) as u8,
                        })
                    }
                    Event::PitchMod(data) => {
                        cur_bend = data;
                        write_pitch_range(
                            snd,
                            ch,
                            last_note,
                            cur_bend,
                            cur_patch,
                            &mut messages,
                            &mut cur_pitchstep,
                        );
                        Some(PitchBendChange { ch, data })
                    }
                    Event::ModuMod(data) => Some(ControlChange {
                        ch,
                        control: 1,
                        data,
                    }),
                    Event::VolumeMod(data) => Some(ControlChange {
                        ch,
                        control: 7,
                        data,
                    }),
                    Event::PanMod(data) => Some(ControlChange {
                        ch,
                        control: 10,
                        data,
                    }),
                    Event::PedalMod(data) => Some(ControlChange {
                        ch,
                        control: 64,
                        data,
                    }),
                    Event::ReverbMod(data) => Some(ControlChange {
                        ch,
                        control: 91,
                        data,
                    }),
                    Event::ChorusMod(data) => Some(ControlChange {
                        ch,
                        control: 93,
                        data,
                    }),
                    Event::NoteOn(note, velocity) => {
                        last_note = Some(note);
                        write_pitch_range(
                            snd,
                            ch,
                            last_note,
                            cur_bend,
                            cur_patch,
                            &mut messages,
                            &mut cur_pitchstep,
                        );
                        Some(NoteOn { ch, note, velocity })
                    }
                    Event::NoteOff(note) => Some(NoteOff {
                        ch,
                        note,
                        velocity: 0,
                    }),
                    Event::TrkTempo(tempo) => {
                        if track_index == 0 {
                            let tempo = 60_000_000 / (tempo as u32);
                            assert!(tempo <= 0xffffff);
                            tempo_map.push(MetaEvent {
                                delta_time: tempo_delta,
                                event: SetTempo,
                                data: tempo.to_be_bytes()[1..].to_vec(),
                            });
                        }
                        None
                    }
                    Event::TrkJump(label) => Some(ControlChange {
                        ch,
                        control: 103,
                        data: label as u8,
                    }),
                    Event::TrkEnd => None,
                    Event::Null => None,
                };
                if let Some(event) = midi_event {
                    messages.push(MidiEvent { delta_time, event });
                }
                match &event.event {
                    Event::TrkEnd => {
                        messages.push(MetaEvent {
                            delta_time,
                            event: EndOfTrack,
                            data: Vec::new(),
                        });
                        break;
                    }
                    Event::TrkTempo(_) => tempo_delta = 0,
                    _ => delta_time = 0,
                }
            }
        }

        tempo_map.push(MetaEvent {
            delta_time: 0,
            event: EndOfTrack,
            data: Vec::new(),
        });
        for message in &tempo_map {
            midi.push(message);
        }
        for message in &messages {
            midi.push(message);
        }
        midi.write_to_io(w)
    }
}

#[derive(Clone, Debug, Default)]
#[binrw::binrw]
#[brw(big)]
pub struct Track {
    pub voices_type: u8,
    pub reverb: u8,
    pub initpatchnum: u16,
    pub initpitch_cntrl: i16,
    pub initvolume_cntrl: u8,
    pub initpan_cntrl: u8,
    pub substack_count: u8,
    pub mutebits: u8,
    pub initppq: u16,
    pub initqpm: u16,
    #[brw(ignore)]
    pub labels: Vec<usize>,
    #[brw(ignore)]
    pub events: Vec<TimedEvent>,
}

#[derive(Copy, Clone, Debug, BinRead)]
#[br(little)]
pub enum Event {
    #[br(magic = 7u8)]
    PatchChg(u16),
    #[br(magic = 9u8)]
    PitchMod(i16),
    #[br(magic = 11u8)]
    ModuMod(u8),
    #[br(magic = 12u8)]
    VolumeMod(u8),
    #[br(magic = 13u8)]
    PanMod(u8),
    #[br(magic = 14u8)]
    PedalMod(u8),
    #[br(magic = 15u8)]
    ReverbMod(u8),
    #[br(magic = 16u8)]
    ChorusMod(u8),
    #[br(magic = 17u8)]
    NoteOn(u8, u8),
    #[br(magic = 18u8)]
    NoteOff(u8),
    #[br(magic = 30u8)]
    TrkTempo(u16),
    #[br(magic = 32u8)]
    TrkJump(u16),
    #[br(magic = 34u8)]
    TrkEnd,
    #[br(magic = 35u8)]
    Null,
}

impl Event {
    fn write(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        match *self {
            Self::PatchChg(value) => {
                w.write_all(&[7u8])?;
                w.write_all(value.to_le_bytes().as_slice())?;
            }
            Self::PitchMod(value) => {
                w.write_all(&[9u8])?;
                w.write_all(value.to_le_bytes().as_slice())?;
            }
            Self::ModuMod(value) => {
                w.write_all(&[11u8, value])?;
            }
            Self::VolumeMod(value) => {
                w.write_all(&[12u8, value])?;
            }
            Self::PanMod(value) => {
                w.write_all(&[13u8, value])?;
            }
            Self::PedalMod(value) => {
                w.write_all(&[14u8, value])?;
            }
            Self::ReverbMod(value) => {
                w.write_all(&[15u8, value])?;
            }
            Self::ChorusMod(value) => {
                w.write_all(&[16u8, value])?;
            }
            Self::NoteOn(note, vel) => {
                w.write_all(&[17u8, note, vel])?;
            }
            Self::NoteOff(note) => {
                w.write_all(&[18u8, note])?;
            }
            Self::TrkTempo(value) => {
                w.write_all(&[30u8])?;
                w.write_all(value.to_le_bytes().as_slice())?;
            }
            Self::TrkJump(value) => {
                w.write_all(&[32u8])?;
                w.write_all(value.to_le_bytes().as_slice())?;
            }
            Self::TrkEnd => {
                w.write_all(&[34u8])?;
            }
            Self::Null => {
                w.write_all(&[35u8])?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct TimedEvent {
    pub delta: u32,
    pub event: Event,
}

impl TimedEvent {
    #[inline]
    pub fn new(delta: u32, event: Event) -> Self {
        Self { delta, event }
    }
}

pub fn extract_sequences<'a, E: ParseError<&'a [u8]>>(
    data: &'a [u8],
) -> nom::IResult<&'a [u8], BTreeMap<u16, Sequence>, E> {
    let (data, _) = tag(b"SSEQ")(data)?;
    let (data, _) = tag(&(2u32).to_be_bytes())(data)?;
    let (data, _) = take(6usize)(data)?;
    let (data, sequencecount) = be_u16(data)?;
    let (data, _decomp_type) = tag(&[0])(data)?;
    let (data, _) = take(3usize)(data)?;
    let (data, _compress_size) = be_u32(data)?;
    let (data, data_size) = be_u32(data)?;
    let (mut data, _) = take(4usize)(data)?;

    let base = data
        .get(data_size as usize..)
        .ok_or_else(|| too_large(data))?;

    let mut sequences = BTreeMap::new();
    for seqindex in 0..sequencecount {
        let (d, trackcount) = be_u16(data)?;
        let (d, _decomp_type) = tag(&[0, 0])(d)?;
        let (d, infolen) = be_u32(d)?;
        let (d, filepos) = be_u32(d)?;
        let (d, _trkinfo) = be_u32(d)?;

        let filepos = filepos as usize;
        let infolen = infolen as usize;
        let mut trackdata = base
            .get(filepos..filepos + infolen)
            .ok_or_else(|| too_large(base))?;

        let mut tracks = Vec::new();
        for _ in 0..trackcount {
            let (d, header) = take(14usize)(trackdata)?;
            let (d, labellist_count) = be_u16(d)?;
            let (d, data_size) = be_u32(d)?;

            let mut track = Track::read(&mut Cursor::new(header)).unwrap();
            let (d, labels) = count(be_u32, labellist_count as usize)(d)?;
            let (d, rawevents_base) = take(data_size as usize)(d)?;
            let mut rawevents = rawevents_base;
            let mut labels = VecDeque::from(labels);
            track.labels.reserve_exact(labels.len());
            while !rawevents.is_empty() {
                let mut vlq = VLQBuilder::new();
                loop {
                    let (d, c) = be_u8(rawevents)?;
                    vlq.push(c);
                    rawevents = d;
                    if (c & 0x80) == 0 {
                        break;
                    }
                }
                if let Some(label) = labels.front().copied() {
                    let off = rawevents_base.len() - rawevents.len();
                    if label as usize == off {
                        track.labels.push(track.events.len());
                        labels.pop_front();
                    }
                }
                let delta = vlq.build().val();
                let mut cursor = std::io::Cursor::new(rawevents);
                let event = Event::read(&mut cursor).map_err(|_| too_large(d))?;
                let isend = matches!(event, Event::TrkEnd);
                track.events.push(TimedEvent { delta, event });
                rawevents = &rawevents[cursor.position() as usize..];
                if isend {
                    break;
                }
            }
            tracks.push(track);
            trackdata = d;
        }
        sequences.insert(seqindex, Sequence::MusicSeq(MusicSequence { tracks }));
        data = d;
    }

    Ok((&[], sequences))
}

#[derive(Clone, Debug, Default)]
pub struct MusicSample {
    pub start: Vec<PatchInfo>,
    pub r#loop: Vec<PatchInfo>,
    pub priority: u8,
    pub volume: u8,
}

impl MusicSample {
    pub fn new(sample: Sample) -> Self {
        const CHUNK_LEN: usize = 0x8000;
        let data = sample.info.samples.raw_data();
        let (start, r#loop) = match sample.info.r#loop {
            Some(r#loop) => {
                let (start, r#loop) = data[0..r#loop.end as usize].split_at(r#loop.start as usize);
                (start.chunks(CHUNK_LEN), r#loop.chunks(CHUNK_LEN))
            }
            None => (data.chunks(CHUNK_LEN), [].chunks(CHUNK_LEN)),
        };
        Self {
            start: start
                .map(|data| PatchInfo {
                    samples: crate::sound::SampleData::Raw(data.to_vec()),
                    pitch: sample.info.pitch,
                    r#loop: None,
                })
                .collect(),
            r#loop: r#loop
                .map(|data| PatchInfo {
                    samples: crate::sound::SampleData::Raw(data.to_vec()),
                    pitch: sample.info.pitch,
                    r#loop: None,
                })
                .collect(),
            priority: sample.priority,
            volume: sample.volume,
        }
    }
    pub fn to_seq(&self, patchidx: u16) -> MusicSequence {
        let mut seq = MusicSequence::default();
        let mut events = Vec::new();
        let mut labels = Vec::new();
        for (index, info) in self.start.iter().enumerate() {
            let delta = (info.samples.n_samples() as f64 * 240.0
                / 22050.0
                / 2.0f64.powf(info.pitch as f64 / 1200.0))
                .ceil() as u32;
            events.push(TimedEvent::new(0, Event::PatchChg(patchidx + index as u16)));
            events.push(TimedEvent::new(0, Event::NoteOn(60, 127)));
            events.push(TimedEvent::new(delta, Event::NoteOff(60)));
        }
        if !self.r#loop.is_empty() {
            let loop_offset = patchidx + self.start.len() as u16;
            labels.push(events.len());
            events.push(TimedEvent::new(0, Event::Null));
            for (index, info) in self.r#loop.iter().enumerate() {
                let delta = (info.samples.n_samples() as f64 * 240.0
                    / 22050.0
                    / 2.0f64.powf(info.pitch as f64 / 1200.0))
                    .ceil() as u32;
                events.push(TimedEvent::new(0, Event::PatchChg(loop_offset + index as u16)));
                events.push(TimedEvent::new(0, Event::NoteOn(60, 127)));
                events.push(TimedEvent::new(delta, Event::NoteOff(60)));
            }
            events.push(TimedEvent::new(0, Event::TrkJump(0)));
        }
        events.push(TimedEvent::new(0, Event::TrkEnd));
        seq.tracks.push(Track {
            voices_type: 1,
            initvolume_cntrl: 127,
            initpan_cntrl: 64,
            initppq: 120,
            initqpm: 120,
            initpatchnum: patchidx,
            events,
            labels,
            ..Default::default()
        });
        seq
    }
    #[inline]
    pub fn sample_count(&self) -> usize {
        self.start.len() + self.r#loop.len()
    }
    #[inline]
    pub fn samples(&self) -> impl Iterator<Item = &PatchInfo> {
        self.start.iter().chain(self.r#loop.iter())
    }
    #[inline]
    pub fn samples_mut(&mut self) -> impl Iterator<Item = &mut PatchInfo> {
        self.start.iter_mut().chain(self.r#loop.iter_mut())
    }
}
