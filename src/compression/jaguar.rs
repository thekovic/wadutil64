use nom::error::ParseError;

pub fn decode_jaguar<'a, E: ParseError<&'a [u8]>>(
    input: &'a [u8],
    cap: usize,
) -> nom::IResult<&'a [u8], Vec<u8>, E> {
    let mut get_id_byte = 0u8;
    let mut id_byte = 0;
    let mut iter = input.iter().copied();
    let mut count = 0;
    let mut next = || {
        let b = iter.next().ok_or_else(|| {
            nom::Err::Error(nom::error::make_error(
                &input[count..],
                nom::error::ErrorKind::Eof,
            ))
        })?;
        count += 1;
        Ok(b)
    };
    let mut output = Vec::with_capacity(cap);

    loop {
        if get_id_byte == 0 {
            id_byte = next()?;
        }
        get_id_byte = (get_id_byte + 1) & 7;
        if id_byte & 1 != 0 {
            const LENSHIFT: u32 = 4;
            let pos = (next()? as i32) << LENSHIFT;
            let d = next()? as i32;
            let pos = pos | (d >> LENSHIFT);
            let len = (d & 0xf) + 1;
            if len == 1 {
                break;
            }
            if len > 0 {
                let mut i = 0;
                let source = output.len() - pos as usize - 1;
                if len & 3 != 0 {
                    while i != len & 3 {
                        output.push(output[source + i as usize]);
                        i += 1;
                    }
                }
                while i != len {
                    for _ in 0..4 {
                        output.push(output[source + i as usize]);
                        i += 1;
                    }
                }
            }
        } else {
            output.push(next()?);
        }
        id_byte >>= 1;
    }
    Ok((&[], output))
}
