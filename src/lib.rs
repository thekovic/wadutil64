pub mod build;
mod compression;
pub mod extract;
mod gfx;
pub mod inspect;
mod lumps;
mod music;
mod remaster;
mod sound;
mod soundfont;
mod wad;

pub use wad::*;

#[derive(Debug, Default)]
pub struct FileFilters {
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
}

impl FileFilters {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }
    pub fn matches(&self, s: &str) -> bool {
        if !self.includes.is_empty() && !self.includes.iter().any(|f| glob_match::glob_match(f, s))
        {
            return false;
        }
        !self.excludes.iter().any(|f| glob_match::glob_match(f, s))
    }
}

#[inline]
fn too_large<'a, E: nom::error::ParseError<&'a [u8]>>(input: &'a [u8]) -> nom::Err<E> {
    nom::Err::Error(nom::error::make_error(
        input,
        nom::error::ErrorKind::TooLarge,
    ))
}

#[inline]
fn nom_fail<'a, E: nom::error::ParseError<&'a [u8]>>(input: &'a [u8]) -> nom::Err<E> {
    nom::Err::Error(nom::error::make_error(input, nom::error::ErrorKind::Fail))
}

fn convert_error<I: std::ops::Deref<Target = [u8]>>(
    input: I,
    e: nom::Err<nom::error::VerboseError<I>>,
) -> String {
    use std::fmt::Write;

    let e = match e {
        nom::Err::Incomplete(nom::Needed::Unknown) => return "Incomplete".into(),
        nom::Err::Incomplete(nom::Needed::Size(n)) => return format!("Need {n} more bytes"),
        nom::Err::Error(e) | nom::Err::Failure(e) => e,
    };
    let mut result = String::new();
    for (i, (substring, kind)) in e.errors.iter().enumerate() {
        let offset = nom::Offset::offset(&*input, substring);

        if i == 0 {
            write!(&mut result, "Parse error at position 0x{offset:x}")
        } else {
            write!(&mut result, ", 0x{offset:x}")
        }
        .unwrap();

        match kind {
            nom::error::VerboseErrorKind::Char(_) => unreachable!(),
            nom::error::VerboseErrorKind::Context(context) => write!(&mut result, " in {context}",),
            nom::error::VerboseErrorKind::Nom(err) => write!(&mut result, " ({err:?})",),
        }
        .unwrap();
    }
    result
}

#[inline]
fn invalid_data(args: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, args.to_string())
}

#[inline]
fn is_log_level(lvl: log::LevelFilter) -> bool {
    lvl <= log::STATIC_MAX_LEVEL && lvl <= log::max_level()
}
