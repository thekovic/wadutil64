use crate::{invalid_data, too_large, LumpType};
use arrayvec::ArrayVec;
use nom::{
    bytes::complete::{tag, take, take_while},
    combinator::{eof, map_res},
    error::{FromExternalError, ParseError},
    number::complete::{be_i16, be_i32, be_u16},
};
use std::{collections::HashMap, io, num::TryFromIntError};

#[allow(clippy::upper_case_acronyms)]
pub type RGBA = rgb::RGBA8;

#[allow(clippy::needless_range_loop)]
pub fn palette_16_to_rgba(input: &[u8], output: &mut [RGBA]) {
    for i in 0..(input.len() / 2) {
        let j = i * 2;
        let p = u16::from_be_bytes([input[j], input[j + 1]]);
        let r = ((p & 0xf800) >> 8) as u8;
        let g = ((p & 0x07c0) >> 3) as u8;
        let b = ((p & 0x003e) << 2) as u8;
        let a = if p & 1 == 1 { 255 } else { 0 };
        output[i] = RGBA::new(r, g, b, a);
    }
}

pub fn palette_rgba_to_16(input: &[RGBA], output: &mut [u8]) {
    let mut o = 0usize;
    for rgba in input {
        let r = rgba.r as u16;
        let g = rgba.g as u16;
        let b = rgba.b as u16;
        let a = rgba.a as u16;
        let p = ((r << 8) & 0xf800) | ((g << 3) & 0x07c0) | ((b >> 2) & 0x003e) | ((a > 0) as u16);
        let p = p.to_be_bytes();
        output[o] = p[0];
        output[o + 1] = p[1];
        o += 2;
    }
}

pub fn palette_16_to_rgb(input: &[u8], output: &mut [u8]) {
    let mut o = 0usize;
    for i in 0..(input.len() / 2) {
        let j = i * 2;
        let p = u16::from_be_bytes([input[j], input[j + 1]]);
        let r = ((p & 0xf800) >> 8) as u8;
        let g = ((p & 0x07c0) >> 3) as u8;
        let b = ((p & 0x003e) << 2) as u8;
        output[o] = r;
        output[o + 1] = g;
        output[o + 2] = b;
        o += 3;
    }
}

pub fn palette_rgb_to_16(input: &[u8], output: &mut [u8]) {
    let mut o = 0usize;
    for i in 0..(input.len() / 3) {
        let j = i * 3;
        let r = input[j] as u16;
        let g = input[j + 1] as u16;
        let b = input[j + 2] as u16;
        let p = ((r << 8) & 0xf800) | ((g << 3) & 0x07c0) | ((b >> 2) & 0x003e) | ((i > 0) as u16);
        let p = p.to_be_bytes();
        output[o] = p[0];
        output[o + 1] = p[1];
        o += 2;
    }
}

const CELL_SIZE: usize = 8;

fn unpack_pixels(n64: &[u8], width: u16, height: u16, rgb8: bool) -> Option<Vec<u8>> {
    let (row_stride, raw_width) = if rgb8 {
        let row_stride = ((width as u32) + 7) & !7;
        (row_stride, width)
    } else {
        let row_stride = (((width as u32) + 15) & !15) >> 1;
        (row_stride, ((width + 1) & !1) >> 1)
    };
    let tile_height = 2048u32
        .checked_div(row_stride)
        .unwrap_or(0)
        .min(height as u32);
    let num_tiles = (height as u32 + tile_height - 1)
        .checked_div(tile_height)
        .unwrap_or(0);
    let bufsize = (row_stride as usize).checked_mul(height as usize)?;
    let cell_count = ((raw_width as usize + (CELL_SIZE - 1)) & !(CELL_SIZE - 1)) / CELL_SIZE;

    //println!("{width}x{height} rgb8 {rgb8} row_stride {row_stride} raw_width {raw_width} tile_height {tile_height} num_tiles {num_tiles} cell_count {cell_count}");

    let mut pos = 0usize;
    let mut pixels = Vec::with_capacity(bufsize);
    let mut height_left = height as u32;
    for _ in 0..num_tiles {
        let cur_height = height_left.min(tile_height);
        for y in 0..cur_height {
            let row = n64.get(pos..(pos + row_stride as usize))?;
            if y & 1 == 0 {
                pixels.extend_from_slice(&row[..raw_width as usize]);
            } else {
                for cx in 0..cell_count {
                    let x = cx * CELL_SIZE;
                    let cw = (raw_width as usize - x).min(CELL_SIZE);
                    let mut c = <[u8; CELL_SIZE]>::try_from(&row[x..x + CELL_SIZE]).unwrap();
                    c.swap(0, 4);
                    c.swap(1, 5);
                    c.swap(2, 6);
                    c.swap(3, 7);
                    pixels.extend_from_slice(&c[..cw]);
                }
            }
            pos += row_stride as usize;
        }
        height_left -= cur_height;
    }
    Some(pixels)
}

fn pack_pixels(
    pixels: &[u8],
    width: u16,
    height: u16,
    rgb8: bool,
    out: &mut impl io::Write,
) -> io::Result<()> {
    let (row_stride, raw_width) = if rgb8 {
        let row_stride = ((width as u32) + 7) & !7;
        (row_stride, width)
    } else {
        let row_stride = (((width as u32) + 15) & !15) >> 1;
        (row_stride, ((width + 1) & !1) >> 1)
    };
    let tile_height = 2048u32
        .checked_div(row_stride)
        .unwrap_or(0)
        .min(height as u32);
    let num_tiles = (height as u32 + tile_height - 1)
        .checked_div(tile_height)
        .unwrap_or(0);
    let cell_count = ((raw_width as usize + (CELL_SIZE - 1)) & !(CELL_SIZE - 1)) / CELL_SIZE;

    //println!("{width}x{height} rgb8 {rgb8} row_stride {row_stride} raw_width {raw_width} tile_height {tile_height} num_tiles {num_tiles} cell_count {cell_count} len {}", pixels.len());

    let mut pos = 0usize;
    let mut tmp = ArrayVec::<u8, CELL_SIZE>::new();
    let mut height_left = height as u32;
    for _ in 0..num_tiles {
        let cur_height = height_left.min(tile_height);
        for y in 0..cur_height {
            let mut written = 0usize;
            let row = pixels
                .get(pos..(pos + raw_width as usize))
                .ok_or_else(|| invalid_data("image buffer too small"))?;
            if y & 1 == 0 {
                out.write_all(row)?;
                written = row.len();
            } else {
                for cx in 0..cell_count {
                    let x = cx * CELL_SIZE;
                    let cw = (raw_width as usize - x).min(CELL_SIZE);
                    tmp.clear();
                    tmp.try_extend_from_slice(&row[x..x + cw]).unwrap();
                    while !tmp.is_full() {
                        tmp.push(0);
                    }
                    let mut c = tmp.take().into_inner().unwrap();
                    c.swap(0, 4);
                    c.swap(1, 5);
                    c.swap(2, 6);
                    c.swap(3, 7);
                    out.write_all(&c)?;
                    written += CELL_SIZE;
                }
            }
            for _ in 0..(row_stride as usize - written) {
                out.write_all(&[0])?;
            }
            pos += raw_width as usize;
        }
        height_left -= cur_height;
    }
    Ok(())
}

/// basic RGBA32 -> CI8 conversion for remaster assets, just discard
/// any palette entries beyond 256
fn convert_rgba32_to_ci8(png: lodepng::Image) -> (Box<[RGBA; 256]>, lodepng::Bitmap<u8>) {
    let b = match png {
        lodepng::Image::RGBA(b) => b,
        _ => unreachable!(),
    };
    let mut palette = Box::new([RGBA::default(); 256]);
    let mut pal_indices = HashMap::with_capacity(256);
    // force pal index 0 to be transparent
    pal_indices.insert(RGBA::default(), 0usize);
    let mut buffer = vec![0u8; b.width * b.height];
    for y in 0..b.height {
        for x in 0..b.width {
            let p = b.buffer[y * b.width + x];
            let next = pal_indices.len();
            let index = match pal_indices.entry(p) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    if next < palette.len() {
                        palette[next] = p;
                        Some(*e.insert(next))
                    } else {
                        None
                    }
                }
                std::collections::hash_map::Entry::Occupied(e) => Some(*e.get()),
            };
            if let Some(index) = index {
                buffer[y * b.width + x] = index as u8;
            }
        }
    }
    (
        palette,
        lodepng::Bitmap {
            buffer,
            width: b.width,
            height: b.height,
        },
    )
}

fn convert_rgba32_to_ci4(png: lodepng::Image) -> (Box<[RGBA; 16]>, lodepng::Bitmap<u8>) {
    let b = match png {
        lodepng::Image::RGBA(b) => b,
        _ => unreachable!(),
    };
    let mut palette = Box::new([RGBA::default(); 16]);
    let mut pal_indices = HashMap::with_capacity(16);
    // force pal index 0 to be transparent
    pal_indices.insert(RGBA::default(), 0usize);
    let stride = b.width / 2 + (b.width & 1);
    let mut buffer = vec![0u8; stride * b.height];
    for y in 0..b.height {
        for x in 0..b.width {
            let p = b.buffer[y * b.width + x];
            let next = pal_indices.len();
            let index = match pal_indices.entry(p) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    if next < palette.len() {
                        palette[next] = p;
                        Some(*e.insert(next))
                    } else {
                        None
                    }
                }
                std::collections::hash_map::Entry::Occupied(e) => Some(*e.get()),
            };
            if let Some(index) = index {
                let shift = (!(x & 1) & 1) * 4;
                buffer[y * stride + x / 2] |= (index as u8) << shift;
            }
        }
    }
    (
        palette,
        lodepng::Bitmap {
            buffer,
            width: b.width,
            height: b.height,
        },
    )
}

#[derive(Clone, Debug)]
pub struct Graphic {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
    pub palette: Option<Box<[RGBA; 256]>>,
}

impl Graphic {
    pub fn parse<'a, E: ParseError<&'a [u8]>>(
        data: &'a [u8],
        typ: LumpType,
    ) -> nom::IResult<&'a [u8], Self, E> {
        let _len = data.len();
        let (data, _) = be_i16(data)?;
        let (data, _) = be_u16(data)?;
        let (data, mut width) = be_u16(data)?;
        let (mut data, mut height) = be_u16(data)?;

        let pixels = if matches!(typ, LumpType::Cloud) {
            width = 64;
            height = 64;
            let size = width as usize * height as usize;
            let (d, pixels) = take(size)(data)?;
            data = d;
            unpack_pixels(pixels, width, height, true).ok_or_else(|| too_large(data))?
        } else {
            let size = (width as u32)
                .checked_mul(height as u32)
                .ok_or_else(|| too_large(data))?;
            let offset = size.checked_add(7).ok_or_else(|| too_large(data))? & !7;
            let (d, raw_pixels) = take(offset)(data)?;
            data = d;

            //println!("{a} {b} {width}x{height} size {size} offset {offset} len {_len}");

            raw_pixels[..size as usize].to_owned()
        };

        let palette = if matches!(typ, LumpType::Graphic | LumpType::Cloud) {
            let (d, paldata) = take(2usize * 256)(data)?;
            data = d;
            let mut palette = Box::new([RGBA::default(); 256]);
            palette_16_to_rgba(paldata, palette.as_mut_slice());
            Some(palette)
        } else {
            None
        };
        Ok((
            data,
            Self {
                width,
                height,
                data: pixels,
                palette,
            },
        ))
    }
    pub fn write(&self, w: &mut impl io::Write, typ: LumpType) -> io::Result<()> {
        if matches!(typ, LumpType::Cloud) {
            w.write_all(&2u16.to_be_bytes())?;
            w.write_all(&(-1i16).to_be_bytes())?;
            w.write_all(&6u16.to_be_bytes())?;
            w.write_all(&5u16.to_be_bytes())?;
            pack_pixels(&self.data, self.width, self.height, true, w)?;
        } else {
            w.write_all(&(-1i16).to_be_bytes())?;
            w.write_all(&0u16.to_be_bytes())?;
            w.write_all(&self.width.to_be_bytes())?;
            w.write_all(&self.height.to_be_bytes())?;
            w.write_all(&self.data)?;
        }
        if let Some(palette) = &self.palette {
            let paloffset = (self.width as u32)
                .checked_mul(self.height as u32)
                .and_then(|s| s.checked_add(7))
                .map(|s| s & !7)
                .ok_or_else(|| invalid_data("graphic too large"))?;
            for _ in 0..(paloffset as usize - self.data.len()) {
                w.write_all(&[0])?;
            }
            let mut data = [0u8; 2 * 256];
            palette_rgba_to_16(palette.as_slice(), data.as_mut_slice());
            w.write_all(&data)?;
        }
        Ok(())
    }
    pub fn to_vec(&self, typ: LumpType) -> Vec<u8> {
        let imgsize = self.width as usize * self.height as usize;
        let palsize = match &self.palette {
            Some(_) => 2 * 256,
            _ => 0,
        };
        let bufsize = 8 + imgsize + palsize;
        let mut buf = Vec::with_capacity(bufsize);
        self.write(&mut buf, typ).unwrap();
        buf
    }
    pub fn read_png(data: &[u8], convert: bool) -> io::Result<Self> {
        let mut decoder = lodepng::Decoder::new();
        decoder.color_convert(false);
        let png = decoder.decode(data).map_err(invalid_data)?;
        let info = decoder.info_png();
        let ct = info.color.colortype();
        if ct == lodepng::ColorType::RGBA && convert {
            let (palette, b) = convert_rgba32_to_ci8(png);
            return Ok(Self {
                width: b.width.try_into().map_err(invalid_data)?,
                height: b.height.try_into().map_err(invalid_data)?,
                data: b.buffer,
                palette: Some(palette),
            });
        }
        if !matches!(ct, lodepng::ColorType::PALETTE | lodepng::ColorType::GREY)
            || info.color.bitdepth() != 8
        {
            return Err(invalid_data(
                "Graphic PNG must be 4-bit or 8-bit indexed color",
            ));
        }
        let (data, palette, width, height) = match png {
            lodepng::Image::RawData(b) => (
                b.buffer,
                Some(Box::new(<_>::try_from(info.color.palette()).unwrap())),
                b.width,
                b.height,
            ),
            lodepng::Image::Grey(b) => (
                bytemuck::allocation::cast_vec(b.buffer),
                None,
                b.width,
                b.height,
            ),
            _ => unreachable!(),
        };
        Ok(Self {
            width: width.try_into().map_err(invalid_data)?,
            height: height.try_into().map_err(invalid_data)?,
            data,
            palette,
        })
    }
    pub fn write_png(&self) -> lodepng::Result<Vec<u8>> {
        let mut encoder = lodepng::Encoder::new();
        encoder.set_auto_convert(false);
        if let Some(palette) = &self.palette {
            encoder.set_palette(palette.as_slice())?;
        } else {
            encoder
                .info_raw_mut()
                .set_colortype(lodepng::ColorType::GREY);
            encoder.info_raw_mut().set_bitdepth(8);
            encoder
                .info_png_mut()
                .color
                .set_colortype(lodepng::ColorType::GREY);
            encoder.info_png_mut().color.set_bitdepth(8);
        }
        encoder.encode(&self.data, self.width as usize, self.height as usize)
    }
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub wshift: u16,
    pub hshift: u16,
    pub data: Vec<u8>,
    pub palettes: Vec<[RGBA; 16]>,
}

impl Texture {
    pub fn parse<'a, E: ParseError<&'a [u8]>>(data: &'a [u8]) -> nom::IResult<&'a [u8], Self, E> {
        let (data, _) = be_u16(data)?;
        let (data, _) = be_u16(data)?;

        let (data, wshift) = be_u16(data)?;
        if !(4..=9).contains(&wshift) {
            return Err(too_large(data));
        }

        let (data, hshift) = be_u16(data)?;
        if hshift > 9 {
            return Err(too_large(data));
        }

        let size = 1usize << ((wshift as usize + hshift as usize + 31) & 31);

        let (mut data, pixels) = take(size)(data)?;

        let pixels = unpack_pixels(pixels, 1 << wshift, 1 << hshift, false)
            .ok_or_else(|| too_large(data))?;

        let mut palettes = Vec::new();
        while !data.is_empty() {
            let (d, palette) = take(2usize * 16)(data)?;
            data = d;
            let mut palette_data = [RGBA::default(); 16];
            palette_16_to_rgba(palette, palette_data.as_mut_slice());
            palettes.push(palette_data);
        }

        Ok((
            data,
            Self {
                wshift,
                hshift,
                data: pixels,
                palettes,
            },
        ))
    }
    pub fn write(&self, w: &mut impl io::Write) -> io::Result<()> {
        w.write_all(&1u16.to_be_bytes())?;
        w.write_all(&(self.palettes.len() as u16).to_be_bytes())?;
        w.write_all(&self.wshift.to_be_bytes())?;
        w.write_all(&self.hshift.to_be_bytes())?;
        pack_pixels(&self.data, 1 << self.wshift, 1 << self.hshift, false, w)?;
        for palette in &self.palettes {
            let mut data = [0u8; 2 * 16];
            palette_rgba_to_16(palette, data.as_mut_slice());
            w.write_all(&data)?;
        }
        Ok(())
    }
    pub fn to_vec(&self) -> Vec<u8> {
        let imgsize = 1 << ((self.wshift + self.hshift + 31) & 31);
        let palsize = self.palettes.len() * 2 * 16;
        let bufsize = 8 + imgsize + palsize;
        let mut buf = Vec::with_capacity(bufsize);
        self.write(&mut buf).unwrap();
        buf
    }
    pub fn read_png(data: &[u8]) -> io::Result<Self> {
        let mut decoder = lodepng::Decoder::new();
        decoder.color_convert(false);
        decoder.remember_unknown_chunks(true);
        let png = decoder.decode(data).map_err(invalid_data)?;
        let info = decoder.info_png();
        let bitdepth = info.color.bitdepth();
        if info.color.colortype() != lodepng::ColorType::PALETTE || (bitdepth == 8 && info.color.palette().len() > 16)
        {
            return Err(invalid_data(
                "Texture PNG must be indexed color with <= 16 palette entries",
            ));
        }
        let bitmap = match png {
            lodepng::Image::RawData(b) => b,
            _ => unreachable!(),
        };
        if bitmap.width < 2 || !bitmap.width.is_power_of_two() {
            return Err(invalid_data("Texture PNG must have a power-of-two width"));
        }
        if bitmap.height < 2 || !bitmap.height.is_power_of_two() {
            return Err(invalid_data("Texture PNG must have a power-of-two height"));
        }
        if bitmap.width * bitmap.height > 4096 {
            return Err(invalid_data("Texture too large to fit in TMEM"));
        }
        let wshift = bitmap.width.trailing_zeros() as u16;
        let hshift = bitmap.height.trailing_zeros() as u16;
        let mut palettes = info
            .try_unknown_chunks(lodepng::ChunkPosition::IHDR)
            .chain(info.try_unknown_chunks(lodepng::ChunkPosition::PLTE))
            .chain(info.try_unknown_chunks(lodepng::ChunkPosition::IDAT))
            .filter_map(|c| c.ok())
            .filter(|c| c.is_type(b"sPLT"))
            .filter_map(|c| parse_splt::<(), 16>(c.data()).map(|r| r.1).ok())
            .collect::<Vec<_>>();

        for palette in info.color.palette().chunks_exact(16).rev() {
            palettes.insert(0, <_>::try_from(palette).unwrap());
        }
        let data = match info.color.bitdepth() {
            1 => bitmap
                .buffer
                .into_iter()
                .flat_map(|i| {
                    [
                        (i & 0b1) | ((i & 0b10) << 3),
                        ((i & 0b100) >> 2) | ((i & 0b1000) << 1),
                        ((i & 0b10000) >> 4) | ((i & 0b100000) >> 1),
                        ((i & 0b1000000) >> 6) | ((i & 0b10000000) >> 3),
                    ]
                    .into_iter()
                })
                .collect(),
            2 => bitmap
                .buffer
                .into_iter()
                .flat_map(|i| {
                    [
                        (i & 0b11) | ((i & 0b1100) << 2),
                        ((i & 0b110000) >> 4) | ((i & 0b11000000) >> 2),
                    ]
                    .into_iter()
                })
                .collect(),
            4 => bitmap.buffer,
            8 => bitmap
                .buffer
                .chunks_exact(2)
                .map(|c| (c[0] & 0b1111) | (c[1] << 4))
                .collect(),
            _ => unreachable!(),
        };

        Ok(Self {
            wshift,
            hshift,
            data,
            palettes,
        })
    }
    pub fn write_png(&self) -> io::Result<Vec<u8>> {
        let mut encoder = lodepng::Encoder::new();
        encoder.set_auto_convert(false);
        let mut palettes = self.palettes.iter();
        let first_palette = palettes
            .next()
            .ok_or_else(|| invalid_data("Texture must have at least one palette"))?;
        encoder.set_palette(first_palette).map_err(invalid_data)?;
        let mut pal_count = 1usize;
        let info = encoder.info_png_mut();
        for palette in palettes {
            use std::io::Write;
            let mut splt = Vec::new();
            write!(splt, "{pal_count}\0").unwrap();
            splt.push(8);
            for rgba in palette {
                splt.push(rgba.r);
                splt.push(rgba.g);
                splt.push(rgba.b);
                splt.push(rgba.a);
                splt.extend_from_slice(&[0, 0]);
            }
            info.create_chunk(lodepng::ChunkPosition::PLTE, b"sPLT", &splt)
                .map_err(invalid_data)?;
            pal_count += 1;
        }
        encoder.info_png_mut().color.set_bitdepth(4);
        encoder.info_raw_mut().set_bitdepth(4);
        let width = 1usize << self.wshift as usize;
        let height = 1usize << self.hshift as usize;
        encoder
            .encode(&self.data, width, height)
            .map_err(invalid_data)
    }
}

impl From<Texture> for Vec<u8> {
    #[inline]
    fn from(value: Texture) -> Self {
        value.to_vec()
    }
}

impl From<&Texture> for Vec<u8> {
    #[inline]
    fn from(value: &Texture) -> Self {
        value.to_vec()
    }
}

fn parse_splt<'a, E: ParseError<&'a [u8]>, const N: usize>(
    data: &'a [u8],
) -> nom::IResult<&'a [u8], [RGBA; N], E> {
    let mut palette = [RGBA::default(); N];
    let mut pc = 0usize;
    let (data, _name) = take_while(|c| c != b'\0')(data)?;
    let (data, _) = tag([b'\0'])(data)?;
    let (mut data, _depth) = tag([8])(data)?;
    while !data.is_empty() && pc < N {
        let (d, rgba) = take(4usize)(data)?;
        let (d, _freq) = take(2usize)(d)?;
        data = d;
        palette[pc] = RGBA::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        pc += 1;
    }
    Ok((data, palette))
}

#[derive(Clone, Debug)]
pub enum SpritePalette {
    Offset(u16),
    Rgb4(Box<[RGBA; 16]>),
    Rgb8(Box<[RGBA; 256]>),
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub x_offset: i16,
    pub y_offset: i16,
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
    pub palette: SpritePalette,
}

impl Sprite {
    pub fn parse<'a, E: ParseError<&'a [u8]>>(data: &'a [u8]) -> nom::IResult<&'a [u8], Self, E> {
        let (data, _num_tiles) = be_u16(data)?;
        let (data, rgb8) = be_i16(data)?;
        let (data, paloffset) = be_u16(data)?;
        let (data, x_offset) = be_i16(data)?;
        let (data, y_offset) = be_i16(data)?;
        let (data, width) = be_u16(data)?;
        let (data, height) = be_u16(data)?;
        let (data, _tile_height) = be_u16(data)?;

        let rgb8 = rgb8 < 0;

        //println!("{width}x{height} ofs +{x_offset}+{y_offset} rgb8 {rgb8} paloffset {paloffset} tileheight {_tile_height} num_tiles {_num_tiles}");

        let (mut data, raw_pixels) = if rgb8 && paloffset & 1 != 0 {
            (&[] as _, data)
        } else {
            take(paloffset as usize)(data)?
        };

        let pixels =
            unpack_pixels(raw_pixels, width, height, rgb8).ok_or_else(|| too_large(data))?;

        let palette = if rgb8 {
            if paloffset & 1 == 0 {
                let (d, paldata) = take(2usize * 256)(data)?;
                data = d;
                let mut palette = Box::new([RGBA::default(); 256]);
                palette_16_to_rgba(paldata, palette.as_mut_slice());
                SpritePalette::Rgb8(palette)
            } else {
                SpritePalette::Offset(paloffset >> 1)
            }
        } else {
            let (d, paldata) = take(2usize * 16)(data)?;
            data = d;
            let mut palette = Box::new([RGBA::default(); 16]);
            palette_16_to_rgba(paldata, palette.as_mut_slice());
            SpritePalette::Rgb4(palette)
        };
        Ok((
            data,
            Self {
                x_offset,
                y_offset,
                width,
                height,
                data: pixels,
                palette,
            },
        ))
    }
    pub fn write(&self, w: &mut impl io::Write) -> io::Result<()> {
        let row_stride = match &self.palette {
            SpritePalette::Offset(_) | SpritePalette::Rgb8(_) => {
                (self.width as u32)
                    .checked_add(7)
                    .ok_or_else(|| invalid_data("image width too large"))?
                    & !7
            }
            SpritePalette::Rgb4(_) => {
                ((self.width as u32)
                    .checked_add(15)
                    .ok_or_else(|| invalid_data("image width too large"))?
                    & !15)
                    >> 1
            }
        };
        let paloffset = match &self.palette {
            SpritePalette::Offset(o) => {
                o.checked_shl(1)
                    .ok_or_else(|| invalid_data("palette offset too large"))?
                    | 1
            }
            SpritePalette::Rgb8(_) | SpritePalette::Rgb4(_) => row_stride
                .checked_mul(self.height as u32)
                .and_then(|o| u16::try_from(o).ok())
                .ok_or_else(|| invalid_data("sprite too large"))?,
        };
        let tile_height = u16::try_from(2048u32.checked_div(row_stride).unwrap_or(0))
            .map_err(|_| invalid_data("sprite too large"))?
            .min(self.height);
        let num_tiles = u16::try_from(
            (self.height as u32 + tile_height as u32 - 1)
                .checked_div(tile_height as u32)
                .unwrap_or(0),
        )
        .map_err(|_| invalid_data("sprite too large"))?;
        let rgb8 = match &self.palette {
            SpritePalette::Rgb4(_) => 1i16,
            _ => -1,
        };

        w.write_all(&num_tiles.to_be_bytes())?;
        w.write_all(&rgb8.to_be_bytes())?;
        w.write_all(&paloffset.to_be_bytes())?;
        w.write_all(&self.x_offset.to_be_bytes())?;
        w.write_all(&self.y_offset.to_be_bytes())?;
        w.write_all(&self.width.to_be_bytes())?;
        w.write_all(&self.height.to_be_bytes())?;
        w.write_all(&tile_height.to_be_bytes())?;

        let rgb8 = !matches!(self.palette, SpritePalette::Rgb4(_));
        pack_pixels(&self.data, self.width, self.height, rgb8, w)?;
        match &self.palette {
            SpritePalette::Offset(_) => {}
            SpritePalette::Rgb8(palette_data) => {
                let mut palette = [0u8; 2 * 256];
                palette_rgba_to_16(palette_data.as_slice(), palette.as_mut_slice());
                w.write_all(&palette)?;
            }
            SpritePalette::Rgb4(palette_data) => {
                let mut palette = [0u8; 2 * 16];
                palette_rgba_to_16(palette_data.as_slice(), palette.as_mut_slice());
                w.write_all(&palette)?;
            }
        }
        Ok(())
    }
    pub fn to_vec(&self) -> Vec<u8> {
        let rgb8 = !matches!(self.palette, SpritePalette::Rgb4(_));
        let row_stride = if rgb8 {
            ((self.width as u32) + 7) & !7
        } else {
            (((self.width as u32) + 15) & !15) >> 1
        };
        let imgsize = row_stride as usize * self.height as usize;
        let palsize = match &self.palette {
            SpritePalette::Rgb4(_) => 2 * 16,
            SpritePalette::Rgb8(_) => 2 * 256,
            _ => 0,
        };
        let bufsize = imgsize + palsize + 16;
        let mut buf = Vec::with_capacity(bufsize);
        self.write(&mut buf).unwrap();
        buf
    }
    pub fn read_png(data: &[u8], convert: Option<u8>) -> io::Result<Self> {
        let mut decoder = lodepng::Decoder::new();
        decoder.color_convert(false);
        decoder.remember_unknown_chunks(true);
        let png = decoder.decode(data).map_err(invalid_data)?;
        let info = decoder.info_png();
        let depth = info.color.bitdepth();
        let (x_offset, y_offset) = info
            .get(b"grAb")
            .and_then(|grab| parse_grab::<()>(grab.data()).ok().map(|r| r.1))
            .unwrap_or_default();
        if info.color.colortype() == lodepng::ColorType::RGBA && depth == 8 {
            if convert == Some(8) {
                let (palette, b) = convert_rgba32_to_ci8(png);
                return Ok(Self {
                    x_offset,
                    y_offset,
                    width: b.width.try_into().map_err(invalid_data)?,
                    height: b.height.try_into().map_err(invalid_data)?,
                    data: b.buffer,
                    palette: SpritePalette::Rgb8(palette),
                });
            } else if convert == Some(4) {
                let (palette, b) = convert_rgba32_to_ci4(png);
                return Ok(Self {
                    x_offset,
                    y_offset,
                    width: b.width.try_into().map_err(invalid_data)?,
                    height: b.height.try_into().map_err(invalid_data)?,
                    data: b.buffer,
                    palette: SpritePalette::Rgb4(palette),
                });
            }
        }
        if info.color.colortype() != lodepng::ColorType::PALETTE || (depth != 4 && depth != 8) {
            return Err(invalid_data(
                "Sprite PNG must be 4-bit or 8-bit indexed color",
            ));
        }
        let mut bitmap = match png {
            lodepng::Image::RawData(b) => b,
            _ => unreachable!(),
        };
        let palette = if depth == 4 {
            let mut palette = Box::new([RGBA::default(); 16]);
            for (i, c) in info.color.palette().iter().take(palette.len()).enumerate() {
                palette[i] = *c;
            }
            SpritePalette::Rgb4(palette)
        } else {
            let mut palette = Box::new([RGBA::default(); 256]);
            for (i, c) in info.color.palette().iter().take(palette.len()).enumerate() {
                palette[i] = *c;
            }
            SpritePalette::Rgb8(palette)
        };
        // round up 4bpp images to even width
        if depth == 4 && (bitmap.width & 1) == 1 {
            for y in (0..bitmap.height).rev() {
                let eofs = (y + 1) * (bitmap.width + 1) / 2 - 1;
                bitmap.buffer[eofs] = 0; // clear last pixel in row
                for x in (0..bitmap.width).rev() {
                    let pofs = y * bitmap.width + x;
                    let ofs = pofs / 2;
                    let npofs = y * (bitmap.width + 1) + x;
                    let nofs = npofs / 2;
                    let mut p = bitmap.buffer[ofs];
                    if pofs & 1 == 0 {
                        p >>= 4;
                    }
                    match npofs & 1 {
                        0 => bitmap.buffer[nofs] |= p << 4,
                        1 => bitmap.buffer[nofs] = p,
                        _ => unreachable!(),
                    }
                }
            }
            bitmap.width += 1;
        }
        Ok(Self {
            x_offset,
            y_offset,
            width: bitmap.width.try_into().map_err(invalid_data)?,
            height: bitmap.height.try_into().map_err(invalid_data)?,
            data: bitmap.buffer,
            palette,
        })
    }
    pub fn write_png(&self, ext_palette: Option<&[RGBA]>) -> lodepng::Result<Vec<u8>> {
        use std::io::Write;
        let mut encoder = lodepng::Encoder::new();
        encoder.set_auto_convert(false);
        let mut rgb8 = true;
        match &self.palette {
            SpritePalette::Rgb4(palette) => {
                rgb8 = false;
                encoder.set_palette(palette.as_slice())?;
            }
            SpritePalette::Rgb8(palette) => {
                encoder.set_palette(palette.as_slice())?;
            }
            SpritePalette::Offset(_) => {
                encoder.set_palette(ext_palette.unwrap())?;
            }
        }
        if !rgb8 {
            encoder.info_png_mut().color.set_bitdepth(4);
            encoder.info_raw_mut().set_bitdepth(4);
        }
        let mut grab = [0u8; 8];
        let mut gp = grab.as_mut_slice();
        gp.write_all(&(self.x_offset as i32).to_be_bytes()).unwrap();
        gp.write_all(&(self.y_offset as i32).to_be_bytes()).unwrap();
        encoder
            .info_png_mut()
            .create_chunk(lodepng::ChunkPosition::IHDR, b"grAb", &grab)?;
        encoder.encode(&self.data, self.width as usize, self.height as usize)
    }
}

impl From<Sprite> for Vec<u8> {
    #[inline]
    fn from(value: Sprite) -> Self {
        value.to_vec()
    }
}

impl From<&Sprite> for Vec<u8> {
    #[inline]
    fn from(value: &Sprite) -> Self {
        value.to_vec()
    }
}

fn parse_grab<'a, E: ParseError<&'a [u8]> + FromExternalError<&'a [u8], TryFromIntError>>(
    data: &'a [u8],
) -> nom::IResult<&'a [u8], (i16, i16), E> {
    let (data, x) = map_res(be_i32, i16::try_from)(data)?;
    let (data, y) = map_res(be_i32, i16::try_from)(data)?;
    let (data, _) = eof(data)?;
    Ok((data, (x, y)))
}
