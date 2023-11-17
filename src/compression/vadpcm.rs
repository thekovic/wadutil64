#![allow(clippy::needless_range_loop)]

use crate::sound::{AdpcmBook, Loop};
use std::borrow::Cow;

pub fn decode_vadpcm(data: &[u8], book: &AdpcmBook) -> Vec<i16> {
    let mut out = Vec::new();
    let mut buf = [0i32; 16];
    let mut ix = [0i32; 16];
    let mut in_vec = [0i32; 16];
    let order = book.order as usize;
    let table = to_table(book);
    for chunk in data.chunks_exact(9) {
        let scale = 1i32 << (chunk[0] >> 4);
        let optimalp = (chunk[0] & 0xf) as usize;
        for mut i in 0..8 {
            let c = chunk[i + 1];
            i *= 2;
            ix[i] = (c >> 4) as i32;
            ix[i + 1] = (c & 0xf) as i32;

            if ix[i] <= 7 {
                ix[i] *= scale;
            } else {
                ix[i] = (-16 - -ix[i]) * scale;
            }

            if ix[i + 1] <= 7 {
                ix[i + 1] *= scale;
            } else {
                ix[i + 1] = (-16 - -ix[i + 1]) * scale;
            }
        }
        for j in 0..2 {
            for i in 0..8 {
                in_vec[i + order] = ix[j * 8 + i];
            }

            if j == 0 {
                for i in 0..order {
                    in_vec[i] = buf[16 - order + i];
                }
            } else {
                for i in 0..order {
                    in_vec[i] = buf[j * 8 - order + i];
                }
            }

            for i in 0..8 {
                buf[i + j * 8] =
                    inner_product(&table[optimalp][i][..order + 8], &in_vec[..order + 8]);
            }
        }
        for c in buf.iter().copied() {
            out.push(c.clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16);
        }
    }
    out
}

fn to_table(book: &AdpcmBook) -> [[[i32; 10]; 8]; 8] {
    let mut book_pos = 0usize;
    let mut table = [[[0i32; 10]; 8]; 8];
    let order = book.order as usize;
    for i in 0..book.npredictors as usize {
        for j in 0..order {
            for k in 0..8 {
                table[i][k][j] = book.book[book_pos] as i32;
                book_pos += 1;
            }
        }
        for k in 1..8 {
            table[i][k][order] = table[i][k - 1][order - 1];
        }
        table[i][0][order] = 2048;
        for k in 1..8 {
            for j in 0..k {
                table[i][j][k + order] = 0;
            }
            for j in k..8 {
                table[i][j][k + order] = table[i][j - k][order];
            }
        }
    }
    table
}

fn inner_product(v1: &[i32], v2: &[i32]) -> i32 {
    let mut out = 0i32;
    for j in 0..v1.len() {
        out = out.wrapping_add(v1[j].wrapping_mul(v2[j]));
    }
    let dout = out / 2048;
    let fiout = dout * 2048;
    if out - fiout < 0 {
        dout - 1
    } else {
        dout
    }
}

pub struct AdpcmParams {
    order: usize,
    bits: usize,
    frame_size: usize,
    refine_iters: usize,
    thresh: f64,
}

impl Default for AdpcmParams {
    fn default() -> Self {
        Self {
            order: 2,
            bits: 2,
            frame_size: 16,
            refine_iters: 2,
            thresh: 10.0,
        }
    }
}

fn box2d_ref<T>(b: &[Box<[T]>]) -> &[&[T]] {
    unsafe { std::mem::transmute(b) }
}

fn box2d_ref_mut<T>(b: &mut [Box<[T]>]) -> &mut [&mut [T]] {
    unsafe { std::mem::transmute(b) }
}

pub fn encode_vadpcm(
    input: &[i16],
    params: AdpcmParams,
    r#loop: Option<&Loop>,
) -> std::io::Result<(Vec<u8>, AdpcmBook, Option<[i16; 16]>)> {
    let AdpcmParams {
        order,
        bits,
        frame_size,
        refine_iters,
        thresh,
    } = params;
    let mut temp_s1 = vec![vec![0f64; order + 1].into_boxed_slice(); 1 << bits].into_boxed_slice();
    let mut split_delta = vec![0f64; order + 1].into_boxed_slice();
    let mut workbuf = vec![0i16; frame_size * 2].into_boxed_slice();

    let mut vec = vec![0f64; order + 1].into_boxed_slice();
    let mut spf4 = vec![0f64; order + 1].into_boxed_slice();
    let mut mat = vec![vec![0f64; order + 1].into_boxed_slice(); order + 1].into_boxed_slice();

    let mut perm = vec![0usize; order + 1].into_boxed_slice();
    let mut data = Vec::new();

    for frame in input.chunks_exact(frame_size) {
        workbuf[frame_size..].copy_from_slice(frame);
        acvect(&workbuf, order, frame_size, &mut vec);
        if vec[0].abs() > thresh {
            acmat(&workbuf, order, frame_size, box2d_ref_mut(&mut mat));
            if !lud(box2d_ref_mut(&mut mat), order, &mut perm).0 {
                lubksb(box2d_ref(&mat), order, &perm, &mut vec);
                vec[0] = 1.0;
                if kfroma(&mut vec, &mut spf4, order) == 0 {
                    let mut d = vec![0f64; order + 1].into_boxed_slice();
                    d[0] = 1.0;
                    for i in 1..=order {
                        if spf4[i] >= 1.0 {
                            spf4[i] = 0.9999999999;
                        }
                        if spf4[i] <= -1.0 {
                            spf4[i] = -0.9999999999;
                        }
                    }
                    afromk(&spf4, &mut d, order);
                    data.push(d);
                }
            }
        }
        let (wstart, wend) = workbuf.split_at_mut(frame_size);
        wstart.copy_from_slice(wend);
    }
    vec[0] = 1.0;
    for j in 1..=order {
        vec[j] = 0.0;
    }

    for d in &data {
        rfroma(d, order, &mut temp_s1[0]);
        for j in 1..=order {
            vec[j] += temp_s1[0][j];
        }
    }

    for j in 1..=order {
        vec[j] /= data.len() as f64;
    }

    durbin(&vec, order, &mut spf4, &mut temp_s1[0]);

    for j in 1..=order {
        if spf4[j] >= 1.0 {
            spf4[j] = 0.9999999999;
        }
        if spf4[j] <= -1.0 {
            spf4[j] = -0.9999999999;
        }
    }

    afromk(&spf4, &mut temp_s1[0], order);
    for cur_bits in 0..bits {
        for i in 0..=order {
            split_delta[i] = 0.0;
        }
        split_delta[order - 1] = -1.0;
        split(
            box2d_ref_mut(&mut temp_s1),
            &split_delta,
            order,
            1 << cur_bits,
            0.01,
        );
        refine(
            box2d_ref_mut(&mut temp_s1),
            order,
            1 << (cur_bits + 1),
            box2d_ref_mut(&mut data),
            refine_iters,
        );
    }

    let npredictors = 1usize << bits;

    let mut book = AdpcmBook {
        order: order as i32,
        npredictors: npredictors as i32,
        book: [0; 128],
    };

    let mut book_pos = 0;
    let mut tmptable = vec![vec![0f64; order].into_boxed_slice(); 8].into_boxed_slice();
    for h in 0..npredictors {
        let row = &temp_s1[h];
        for i in 0..order {
            for j in 0..i {
                tmptable[i][j] = 0.0;
            }
            for j in i..order {
                tmptable[i][j] = -row[order - j + i];
            }
        }

        for i in order..8 {
            for j in 0..order {
                tmptable[i][j] = 0.0;
            }
        }

        for i in 1..8 {
            for j in 1..=order {
                if i as isize - j as isize >= 0 {
                    for k in 0..order {
                        tmptable[i][k] -= row[j] * tmptable[i - j][k];
                    }
                }
            }
        }

        for i in 0..order {
            for j in 0..8 {
                let fval = tmptable[j][i] * 2048.0;
                let fval = if fval < 0.0 { fval - 0.5 } else { fval + 0.5 };
                let ival = i16::try_from(fval as i32)
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Overflow"))?;
                let b = book
                    .book
                    .get_mut(book_pos)
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Overflow"))?;
                *b = ival;
                book_pos += 1;
            }
        }
    }
    let mut state = [0i32; 16];
    let mut frame_iter = input
        .chunks(16)
        .map(|frame| {
            <&[i16; 16]>::try_from(frame)
                .map(Cow::Borrowed)
                .unwrap_or_else(|_| {
                    let mut tmp_frame = [0i16; 16];
                    tmp_frame[..frame.len()].copy_from_slice(frame);
                    Cow::Owned(tmp_frame)
                })
        })
        .enumerate();
    let mut output = Vec::new();

    let table = to_table(&book);
    let loopstate = r#loop.map(|r#loop| {
        for (i, frame) in &mut frame_iter {
            encode_frame(&frame, &mut state, &table, order, npredictors, &mut output);
            if i * 16 > r#loop.start as usize {
                break;
            }
        }
        let mut loopstate = [0i16; 16];
        for (i, s) in state.iter().copied().enumerate() {
            loopstate[i] = s.clamp(-(i16::MAX as i32), i16::MAX as i32) as i16;
        }
        loopstate
    });

    for (_, frame) in frame_iter {
        encode_frame(&frame, &mut state, &table, order, npredictors, &mut output);
    }
    Ok((output, book, loopstate))
}

fn encode_frame(
    frame: &[i16; 16],
    state: &mut [i32; 16],
    table: &[[[i32; 10]; 8]; 8],
    order: usize,
    npredictors: usize,
    output: &mut Vec<u8>,
) {
    let mut ix = [0i16; 16];
    let mut prediction = [0i32; 16];
    let mut in_vector = [0i32; 16];
    let mut ie = [0i32; 16];
    let mut e = [0f32; 16];

    let llevel = -8;
    let ulevel = -llevel - 1;

    let mut min = 1e30;
    let mut optimalp = 0;
    for k in 0..npredictors {
        for i in 0..order {
            in_vector[i] = state[16 - order + i];
        }

        for i in 0..8 {
            prediction[i] = inner_product(&table[k][i][0..order + i], &in_vector);
            in_vector[i + order] = frame[i] as i32 - prediction[i];
            e[i] = in_vector[i + order] as f32;
        }

        for i in 0..order {
            in_vector[i] = prediction[8 - order + i] + in_vector[8 + i];
        }

        for i in 0..8 {
            prediction[8 + i] = inner_product(&table[k][i][0..order + i], &in_vector);
            in_vector[i + order] = frame[8 + i] as i32 - prediction[8 + i];
            e[8 + i] = in_vector[i + order] as f32;
        }

        let mut se = 0.0;
        for j in 0..16 {
            se += e[j] * e[j];
        }
        if se < min {
            min = se;
            optimalp = k;
        }
    }

    for i in 0..order {
        in_vector[i] = state[16 - order + i];
    }

    for i in 0..8 {
        prediction[i] = inner_product(&table[optimalp][i][0..order + i], &in_vector);
        in_vector[i + order] = frame[i] as i32 - prediction[i];
        e[i] = in_vector[i + order] as f32;
    }

    for i in 0..order {
        in_vector[i] = prediction[8 - order + i] + in_vector[8 + i];
    }

    for i in 0..8 {
        prediction[8 + i] = inner_product(&table[optimalp][i][0..order + i], &in_vector);
        in_vector[i + order] = frame[8 + i] as i32 - prediction[8 + i];
        e[8 + i] = in_vector[i + order] as f32;
    }

    clamp(&mut e, &mut ie, 16);

    let mut max = 0i32;
    for i in 0..16 {
        if ie[i].abs() > max.abs() {
            max = ie[i];
        }
    }

    let mut scale = 0usize;
    for s in 0..=12 {
        scale = s;
        if max <= ulevel && max >= llevel {
            break;
        }
        max /= 2;
    }

    let save_state = *state;

    scale = scale.wrapping_sub(1);
    let mut n_iter = 0;
    loop {
        n_iter += 1;
        let mut max_clip = 0;
        scale = scale.wrapping_add(1).min(12);

        for i in 0..order {
            in_vector[i] = save_state[16 - order + i];
        }

        for i in 0..8 {
            prediction[i] = inner_product(&table[optimalp][i][0..order + i], &in_vector);

            let se = frame[i] as f32 - prediction[i] as f32;
            ix[i] = qsample(se, 1 << scale);

            let c_v = ix[i].clamp(llevel as i16, ulevel as i16) - ix[i];
            max_clip = max_clip.max(c_v.abs());
            ix[i] += c_v;

            in_vector[i + order] = ix[i] as i32 * (1 << scale);
            state[i] = prediction[i] + in_vector[i + order];
        }

        for i in 0..order {
            in_vector[i] = state[8 - order + i];
        }

        for i in 0..8 {
            prediction[8 + i] = inner_product(&table[optimalp][i][0..order + i], &in_vector);
            let se = frame[8 + i] as f32 - prediction[8 + i] as f32;
            ix[8 + i] = qsample(se, 1 << scale);
            let c_v = ix[8 + i].clamp(llevel as i16, ulevel as i16) - ix[8 + i];
            max_clip = max_clip.max(c_v.abs());
            ix[8 + i] += c_v;
            in_vector[i + order] = ix[8 + i] as i32 * (1 << scale);
            state[8 + i] = prediction[8 + i] + in_vector[i + order];
        }
        if max_clip < 2 || n_iter >= 2 {
            break;
        }
    }

    let header = (scale << 4) | (optimalp & 0xf);
    output.push(header as u8);
    for mut i in 0..8 {
        i *= 2;
        let c = (ix[i] << 4) as u8 | (ix[i + 1] as u8 & 0xf);
        output.push(c);
    }
}

fn qsample(x: f32, scale: i32) -> i16 {
    if x > 0.0 {
        ((x / scale as f32) + 0.4999999) as i16
    } else {
        ((x / scale as f32) - 0.4999999) as i16
    }
}

fn clamp(e: &mut [f32], ie: &mut [i32], bits: usize) {
    let llevel = -(1 << (bits - 1)) as f32;
    let ulevel = -llevel - 1.0;
    for i in 0..e.len() {
        if e[i] > ulevel {
            e[i] = ulevel;
        }
        if e[i] < llevel {
            e[i] = llevel;
        }

        if e[i] > 0.0 {
            ie[i] = (e[i] + 0.5) as i32;
        } else {
            ie[i] = (e[i] - 0.5) as i32;
        }
    }
}

fn durbin(input: &[f64], n: usize, out1: &mut [f64], out2: &mut [f64]) -> (usize, f64) {
    out2[0] = 1.0;
    let mut div = input[0];
    let mut ret = 0;

    for i in 1..=n {
        let mut sum = 0.0;
        for j in 1..=i - 1 {
            sum += out2[j] * input[i - j];
        }

        out2[i] = if div > 0.0 {
            -(input[i] + sum) / div
        } else {
            0.0
        };
        out1[i] = out2[i];

        if out1[i].abs() > 1.0 {
            ret += 1;
        }

        for j in 1..i {
            out2[j] += out2[i - j] * out2[i];
        }

        div *= 1.0 - out2[i] * out2[i];
    }
    (ret, div)
}

fn afromk(input: &[f64], output: &mut [f64], n: usize) {
    output[0] = 1.0;
    for i in 1..=n {
        output[i] = input[i];
        for j in 1..=i - 1 {
            output[j] += output[i - j] * output[i];
        }
    }
}

fn kfroma(input: &mut [f64], output: &mut [f64], n: usize) -> usize {
    let mut ret = 0;
    let mut next = vec![0f64; n + 1];

    output[n] = input[n];
    for i in (1..=n - 1).rev() {
        for j in 0..=i {
            let temp = output[i + 1];
            let div = 1.0 - (temp * temp);
            if div == 0.0 {
                return 1;
            }
            next[j] = (input[j] - input[i + 1 - j] * temp) / div;
        }

        input[..(i + 1)].copy_from_slice(&next[..(i + 1)]);

        output[i] = next[i];
        if output[i].abs() > 1.0 {
            ret += 1
        }
    }

    ret
}

fn rfroma(input: &[f64], n: usize, output: &mut [f64]) {
    let mut mat = vec![Vec::new(); n + 1].into_boxed_slice();
    mat[n] = vec![0f64; n + 1];
    mat[n][0] = 1.0;
    for i in 1..=n {
        mat[n][i] = -input[i];
    }

    for i in (1..=n).rev() {
        mat[i - 1] = vec![0f64; i];
        let div = 1.0 - mat[i][i] * mat[i][i];
        for j in 1..=i - 1 {
            mat[i - 1][j] = (mat[i][i - j] * mat[i][i] + mat[i][j]) / div;
        }
    }

    output[0] = 1.0;
    for i in 1..=n {
        output[i] = 0.0;
        for j in 1..=i {
            output[i] += mat[i][j] * output[i - j];
        }
    }
}

fn model_dist(input: &[f64], output: &mut [f64], n: usize) -> f64 {
    let mut sp3c = vec![0f64; n + 1].into_boxed_slice();
    let mut sp38 = vec![0f64; n + 1].into_boxed_slice();
    rfroma(output, n, &mut sp3c);

    for i in 0..=n {
        sp38[i] = 0.0;
        for j in 0..=n - i {
            sp38[i] += input[j] * input[i + j];
        }
    }

    let mut ret = sp38[0] * sp3c[0];
    for i in 1..=n {
        ret += 2.0 * sp3c[i] * sp38[i];
    }

    ret
}

fn acmat(input: &[i16], n: usize, m: usize, output: &mut [&mut [f64]]) {
    for i in 1..=n {
        for j in 1..=n {
            output[i][j] = 0.0;
            for k in 0..m {
                output[i][j] += input[m + k - i] as f64 * input[m + k - j] as f64;
            }
        }
    }
}

fn acvect(input: &[i16], n: usize, m: usize, output: &mut [f64]) {
    for i in 0..=n {
        output[i] = 0.0;
        for j in 0..m {
            output[i] -= input[m + j - i] as f64 * input[m + j] as f64;
        }
    }
}

fn lud(a: &mut [&mut [f64]], n: usize, indx: &mut [usize]) -> (bool, i32) {
    let mut imax = 0;
    let mut vv = vec![0f64; n + 1].into_boxed_slice();
    let mut d = 1;
    for i in 1..=n {
        let mut big = 0.0;
        for j in 1..=n {
            let temp = a[i][j].abs();
            if temp > big {
                big = temp;
            }
        }
        if big == 0.0 {
            return (true, d);
        }
        vv[i] = 1.0 / big;
    }
    for j in 1..=n {
        for i in 1..j {
            let mut sum = a[i][j];
            for k in 1..i {
                sum -= a[i][k] * a[k][j];
            }
            a[i][j] = sum;
        }
        let mut big = 0.0;
        for i in j..=n {
            let mut sum = a[i][j];
            for k in 1..j {
                sum -= a[i][k] * a[k][j];
            }
            a[i][j] = sum;
            let dum = vv[i] * sum.abs();
            if dum >= big {
                big = dum;
                imax = i;
            }
        }
        if j != imax {
            for k in 1..=n {
                let dum = a[imax][k];
                a[imax][k] = a[j][k];
                a[j][k] = dum;
            }
            d = -d;
            vv[imax] = vv[j];
        }
        indx[j] = imax;
        if a[j][j] == 0.0 {
            return (true, d);
        }
        if j != n {
            let dum = 1.0 / (a[j][j]);
            for i in j + 1..=n {
                a[i][j] *= dum;
            }
        }
    }

    let mut min = 1e10f64;
    let mut max = 0.0f64;
    for i in 1..=n {
        let temp = a[i][i].abs();
        min = min.min(temp);
        max = max.max(temp);
    }
    (min / max < 1e-10, d)
}

fn lubksb(a: &[&[f64]], n: usize, indx: &[usize], b: &mut [f64]) {
    let mut ii = 0;

    for i in 1..=n {
        let ip = indx[i];
        let mut sum = b[ip];
        b[ip] = b[i];
        if ii != 0 {
            for j in ii..=i - 1 {
                sum -= a[i][j] * b[j];
            }
        } else if sum != 0.0 {
            ii = i;
        }
        b[i] = sum;
    }
    for i in (1..=n).rev() {
        let mut sum = b[i];
        for j in i + 1..=n {
            sum -= a[i][j] * b[j];
        }
        b[i] = sum / a[i][i];
    }
}

fn split(table: &mut [&mut [f64]], delta: &[f64], order: usize, npredictors: usize, scale: f64) {
    for i in 0..npredictors {
        for j in 0..=order {
            table[i + npredictors][j] = table[i][j] + delta[j] * scale;
        }
    }
}

fn refine(
    table: &mut [&mut [f64]],
    order: usize,
    npredictors: usize,
    data: &mut [&mut [f64]],
    refine_iters: usize,
) {
    let mut rsums = vec![vec![0f64; order + 1].into_boxed_slice(); npredictors].into_boxed_slice();
    let mut counts = vec![0usize; npredictors];
    let mut temp_s7 = vec![0f64; order + 1];

    for _ in 0..refine_iters {
        for d in data.iter_mut() {
            let mut best_value = 1e30;
            let mut best_index = 0;

            for j in 0..npredictors {
                let dist = model_dist(table[j], d, order);
                if dist < best_value {
                    best_value = dist;
                    best_index = j;
                }
            }

            counts[best_index] += 1;
            rfroma(d, order, &mut temp_s7);
            for j in 0..=order {
                rsums[best_index][j] += temp_s7[j];
            }
        }

        for i in 0..npredictors {
            if counts[i] > 0 {
                for j in 0..=order {
                    rsums[i][j] /= counts[i] as f64;
                }
            }
        }

        for i in 0..npredictors {
            durbin(&rsums[i], order, &mut temp_s7, table[i]);

            for j in 1..=order {
                if temp_s7[j] >= 1.0 {
                    temp_s7[j] = 0.9999999999;
                }
                if temp_s7[j] <= -1.0 {
                    temp_s7[j] = -0.9999999999;
                }
            }

            afromk(&temp_s7, table[i], order);
        }
    }
}
