//! CPU-only port of serenitymojo/pipeline/minimax_h3_control_media.mojo.
//! Neural encoding stays in the existing native video encoder. `rows` inputs
//! are normalized posterior means, without Ref2VA's FP16 round trip. A paired
//! SOURCE must encode the pixel-masked RGB produced by `mask`, not the original.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};

type Result<T> = std::result::Result<T, String>;

fn product(values: &[usize]) -> Result<usize> {
    values.iter().try_fold(1usize, |a, &b| {
        if b == 0 { return Err("dimensions must be positive".into()); }
        a.checked_mul(b).ok_or_else(|| "dimension product overflow".into())
    })
}

fn number(text: &str) -> Result<usize> {
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid nonnegative integer: {text}"));
    }
    text.parse().map_err(|_| format!("integer out of range: {text}"))
}

fn invert(text: &str) -> Result<bool> {
    match text { "0" => Ok(false), "1" => Ok(true), _ => Err("INVERT must be 0 or 1".into()) }
}

fn rgb_input(path: &str, width: usize, height: usize, frames: usize) -> Result<File> {
    let expected = product(&[width, height, frames, 3])?;
    let file = File::open(path).map_err(|e| format!("cannot open {path}: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() != expected as u64 {
        return Err(format!("RGB file {path} must contain exactly {expected} bytes"));
    }
    Ok(file)
}

fn rgb_read(path: &str, width: usize, height: usize, frames: usize) -> Result<Vec<u8>> {
    let mut file = rgb_input(path, width, height, frames)?;
    let mut bytes = vec![0; product(&[width, height, frames, 3])?];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    require_eof(&mut file)?;
    Ok(bytes)
}

fn require_eof(input: &mut impl Read) -> Result<()> {
    let mut byte = [0];
    if input.read(&mut byte).map_err(|e| e.to_string())? != 0 {
        return Err("input has trailing bytes or changed while reading".into());
    }
    Ok(())
}

fn output(path: &str) -> Result<BufWriter<File>> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .map(BufWriter::new).map_err(|e| format!("cannot create new output {path}: {e}"))
}

fn gray(rgb: &[u8]) -> Vec<i32> {
    rgb.chunks_exact(3).map(|p| {
        (4899 * i32::from(p[0]) + 9617 * i32::from(p[1])
            + 1868 * i32::from(p[2]) + 8192) >> 14
    }).collect()
}

fn magnitude_at(mag: &[i32], width: usize, height: usize, y: isize, x: isize) -> i32 {
    if y < 0 || x < 0 || y >= height as isize || x >= width as isize { return 0; }
    mag[y as usize * width + x as usize]
}

fn canny_thresholds(low: usize, high: usize) -> Result<()> {
    if low >= high || high > 255 {
        return Err("Canny requires 0 <= low < high <= 255".into());
    }
    Ok(())
}

fn canny(rgb: &[u8], width: usize, height: usize, low: usize, high: usize) -> Result<Vec<u8>> {
    canny_thresholds(low, high)?;
    let n = product(&[width, height])?;
    if rgb.len() != product(&[n, 3])? { return Err("Canny RGB shape mismatch".into()); }
    let gray = gray(rgb);
    let mut gx = vec![0i32; n];
    let mut gy = vec![0i32; n];
    let mut mag = vec![0i32; n];
    // Source Sobel borders replicate pixels; the NMS magnitude border is zero.
    // Every possible 8-bit Sobel value and TG22 product fits in i32.
    for y in 0..height {
        let ym = y.saturating_sub(1);
        let yp = (y + 1).min(height - 1);
        for x in 0..width {
            let xm = x.saturating_sub(1);
            let xp = (x + 1).min(width - 1);
            let a = gray[ym * width + xm];
            let b = gray[ym * width + x];
            let c = gray[ym * width + xp];
            let d = gray[y * width + xm];
            let e = gray[y * width + xp];
            let g = gray[yp * width + xm];
            let h = gray[yp * width + x];
            let j = gray[yp * width + xp];
            let at = y * width + x;
            gx[at] = -a + c - 2 * d + 2 * e - g + j;
            gy[at] = -a - 2 * b - c + g + 2 * h + j;
            mag[at] = gx[at].abs() + gy[at].abs();
        }
    }
    let mut state = vec![0u8; n];
    let mut stack = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let at = y * width + x;
            let ax = gx[at].abs();
            let ay = gy[at].abs();
            let m0 = mag[at];
            if m0 <= low as i32 { continue; }
            let get = |dy, dx| magnitude_at(&mag, width, height, y as isize + dy, x as isize + dx);
            let keep = if ay * 32768 < ax * 13573 {
                m0 > get(0, -1) && m0 >= get(0, 1)
            } else if ay * 32768 > ax * 79109 {
                m0 > get(-1, 0) && m0 >= get(1, 0)
            } else if gx[at] * gy[at] >= 0 {
                m0 > get(-1, -1) && m0 > get(1, 1)
            } else {
                m0 > get(-1, 1) && m0 > get(1, -1)
            };
            if !keep { continue; }
            if m0 > high as i32 {
                state[at] = 2;
                stack.push(at);
            } else if m0 > low as i32 {
                state[at] = 1;
            }
        }
    }
    while let Some(at) = stack.pop() {
        let y = (at / width) as isize;
        let x = (at % width) as isize;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 { continue; }
                let yy = y + dy;
                let xx = x + dx;
                if yy < 0 || xx < 0 || yy >= height as isize || xx >= width as isize { continue; }
                let other = yy as usize * width + xx as usize;
                if state[other] == 1 {
                    state[other] = 2;
                    stack.push(other);
                }
            }
        }
    }
    let mut out = vec![0; rgb.len()];
    for (pixel, &state) in out.chunks_exact_mut(3).zip(&state) {
        if state == 2 { pixel.fill(255); }
    }
    Ok(out)
}

fn mask_visibility(rgb: &[u8], at: usize, invert: bool) -> f32 {
    let sum = u16::from(rgb[3 * at]) + u16::from(rgb[3 * at + 1]) + u16::from(rgb[3 * at + 2]);
    let mut masked = sum > 382;
    if invert { masked = !masked; }
    if masked { 0.0 } else { 1.0 }
}

fn axis(output: usize, input_size: usize, output_size: usize) -> (usize, usize, f32) {
    // Preserve the source's F32 casts, multiplication before division, and
    // half-pixel alignment. Do not replace these with an F64 scale or mul_add.
    let f = (output as f32 + 0.5f32) * input_size as f32 / output_size as f32 - 0.5f32;
    let mut lo = f.floor() as isize;
    let mut weight = f - lo as f32;
    if lo < 0 { lo = 0; weight = 0.0; }
    let hi = (lo as usize + 1).min(input_size - 1);
    (lo as usize, hi, weight)
}

fn visibility_at(mask: &[u8], input: [usize; 3], output: [usize; 3], at: [usize; 3], invert: bool) -> f32 {
    let [frames, height, width] = input;
    let (t0, t1, wt) = axis(at[0], frames, output[0]);
    let (y0, y1, wy) = axis(at[1], height, output[1]);
    let (x0, x1, wx) = axis(at[2], width, output[2]);
    let get = |t, y, x| mask_visibility(mask, (t * height + y) * width + x, invert);
    let v000 = get(t0, y0, x0);
    let v001 = get(t0, y0, x1);
    let v010 = get(t0, y1, x0);
    let v011 = get(t0, y1, x1);
    let v100 = get(t1, y0, x0);
    let v101 = get(t1, y0, x1);
    let v110 = get(t1, y1, x0);
    let v111 = get(t1, y1, x1);
    let a0 = v000 + (v001 - v000) * wx;
    let a1 = v010 + (v011 - v010) * wx;
    let b0 = v100 + (v101 - v100) * wx;
    let b1 = v110 + (v111 - v110) * wx;
    let a = a0 + (a1 - a0) * wy;
    let b = b0 + (b1 - b0) * wy;
    a + (b - a) * wt
}

fn mask_channel(value: u8, visibility: f32) -> u8 {
    // Mojo: UInt8(Int(Float32(Int(value)) * visibility)), i.e. truncate.
    (f32::from(value) * visibility) as i32 as u8
}

fn mask_source(source: &mut [u8], mask: &[u8], dims: [usize; 3], invert: bool) -> Result<()> {
    if source.len() != product(&[dims[0], dims[1], dims[2], 3])? || source.len() != mask.len() {
        return Err("source/mask RGB shape mismatch".into());
    }
    for t in 0..dims[0] {
        for y in 0..dims[1] {
            for x in 0..dims[2] {
                let visibility = visibility_at(mask, dims, dims, [t, y, x], invert);
                let at = ((t * dims[1] + y) * dims[2] + x) * 3;
                for c in 0..3 { source[at + c] = mask_channel(source[at + c], visibility); }
            }
        }
    }
    Ok(())
}

struct Tensor { dims: Vec<usize>, values: Vec<f32> }

fn latent_shape(tensor: &Tensor) -> Result<[usize; 3]> {
    if tensor.dims.len() != 5 || tensor.dims[0] != 1 || tensor.dims[1] != 24
        || tensor.dims[3] % 2 != 0 || tensor.dims[4] % 2 != 0 {
        return Err("latent must be normalized F32 [1,24,T,H,W] with even H and W".into());
    }
    if product(&tensor.dims)? != tensor.values.len() || tensor.values.iter().any(|v| !v.is_finite()) {
        return Err("latent has invalid length or nonfinite values".into());
    }
    Ok([tensor.dims[2], tensor.dims[3], tensor.dims[4]])
}

fn union_rows(guide: &Tensor, source: Option<(&Tensor, &[u8], [usize; 3], bool)>) -> Result<Tensor> {
    let dims = latent_shape(guide)?;
    let volume = product(&dims)?;
    if let Some((source, mask, pixels, _)) = source {
        if latent_shape(source)? != dims || mask.len() != product(&[pixels[0], pixels[1], pixels[2], 3])? {
            return Err("source latent or mask shape mismatch".into());
        }
    }
    let [frames, height, width] = dims;
    let row_count = product(&[frames, height / 2, width / 2])?;
    let mut values = vec![0.0; product(&[row_count, 196])?];
    // Same indexing as minimax_h3_patchify_video(union,49,T,H,W,1,2,2).
    // Select union channels directly, avoiding a redundant 49-channel copy.
    for t in 0..frames {
        for y in 0..height / 2 {
            for x in 0..width / 2 {
                let row = (t * (height / 2) + y) * (width / 2) + x;
                for c in 0..49 {
                    for iy in 0..2 {
                        for ix in 0..2 {
                            let yy = 2 * y + iy;
                            let xx = 2 * x + ix;
                            let at = (t * height + yy) * width + xx;
                            let value = if c < 24 {
                                guide.values[c * volume + at]
                            } else if let Some((source, mask, pixels, invert)) = source {
                                if c == 24 { visibility_at(mask, pixels, dims, [t, yy, xx], invert) }
                                else { source.values[(c - 25) * volume + at] }
                            } else { 0.0 };
                            values[row * 196 + c * 4 + iy * 2 + ix] = value;
                        }
                    }
                }
            }
        }
    }
    Ok(Tensor { dims: vec![row_count, 196], values })
}

// DiffTensor v1 framing and SHA-256 are ported from the compiler's
// src/runtime/tensor_io.cpp and src/support/sha256.cpp, without a dependency.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
    ];
    fn transform(state: &mut [u32; 8], block: &[u8]) {
        let mut words = [0u32; 64];
        for (word, bytes) in words[..16].iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = words[i - 15].rotate_right(7) ^ words[i - 15].rotate_right(18) ^ (words[i - 15] >> 3);
            let s1 = words[i - 2].rotate_right(17) ^ words[i - 2].rotate_right(19) ^ (words[i - 2] >> 10);
            words[i] = words[i - 16].wrapping_add(s0).wrapping_add(words[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(sum1).wrapping_add(choose).wrapping_add(K[i]).wrapping_add(words[i]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g; g = f; f = e; e = d.wrapping_add(temp1);
            d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        for (word, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *word = word.wrapping_add(value);
        }
    }
    let mut state = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    let mut blocks = bytes.chunks_exact(64);
    for block in &mut blocks { transform(&mut state, block); }
    let remainder = blocks.remainder();
    let mut tail = [0u8; 128];
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()] = 0x80;
    let tail_len = if remainder.len() < 56 { 64 } else { 128 };
    tail[tail_len - 8..tail_len].copy_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for block in tail[..tail_len].chunks_exact(64) { transform(&mut state, block); }
    let mut digest = [0u8; 32];
    for (bytes, value) in digest.chunks_exact_mut(4).zip(state) { bytes.copy_from_slice(&value.to_be_bytes()); }
    digest
}

fn tensor_bytes(tensor: &Tensor) -> Result<Vec<u8>> {
    if product(&tensor.dims)? != tensor.values.len() || tensor.values.iter().any(|v| !v.is_finite()) {
        return Err("invalid F32 tensor shape or values".into());
    }
    let payload = product(&[tensor.values.len(), 4])?;
    let header = tensor.dims.len().checked_mul(8).and_then(|n| n.checked_add(28)).ok_or("header overflow")?;
    let size = header.checked_add(payload).and_then(|n| n.checked_add(32)).ok_or("tensor file size overflow")?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(b"DIFTNS01");
    bytes.extend_from_slice(&1u32.to_le_bytes()); // version
    bytes.extend_from_slice(&1u32.to_le_bytes()); // F32
    bytes.extend_from_slice(&(tensor.dims.len() as u32).to_le_bytes());
    for &dim in &tensor.dims { bytes.extend_from_slice(&(dim as u64).to_le_bytes()); }
    bytes.extend_from_slice(&(payload as u64).to_le_bytes());
    for value in &tensor.values { bytes.extend_from_slice(&value.to_le_bytes()); }
    let digest = sha256(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn parse_tensor(bytes: &[u8]) -> Result<Tensor> {
    if bytes.len() < 60 || &bytes[..8] != b"DIFTNS01" { return Err("invalid DiffTensor header".into()); }
    let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if u32_at(8) != 1 || u32_at(12) != 1 { return Err("DiffTensor must be version 1, F32".into()); }
    let rank = u32_at(16) as usize;
    if rank != 2 && rank != 5 { return Err("DiffTensor rank must be 2 or 5".into()); }
    let header = 28 + rank * 8;
    if bytes.len() < header + 32 { return Err("truncated DiffTensor dimensions".into()); }
    let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let dims: Vec<usize> = (0..rank).map(|i| usize::try_from(u64_at(20 + i * 8)).map_err(|_| "dimension out of range".to_string())).collect::<Result<_>>()?;
    let payload = product(&[product(&dims)?, 4])?;
    let expected = header.checked_add(payload).and_then(|n| n.checked_add(32)).ok_or("tensor size overflow")?;
    if u64_at(header - 8) != payload as u64 || expected != bytes.len() {
        return Err("DiffTensor dimensions/payload/file length mismatch".into());
    }
    if sha256(&bytes[..bytes.len() - 32]) != bytes[bytes.len() - 32..] {
        return Err("DiffTensor SHA-256 mismatch".into());
    }
    let values: Vec<f32> = bytes[header..header + payload].chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect();
    if values.iter().any(|v| !v.is_finite()) { return Err("DiffTensor has nonfinite values".into()); }
    Ok(Tensor { dims, values })
}

fn read_tensor(path: &str) -> Result<Tensor> {
    // File-sized, not an unbounded read_to_end if the producer changes the file.
    let mut file = File::open(path).map_err(|e| format!("cannot open {path}: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() { return Err("DiffTensor input must be a regular file".into()); }
    let size = usize::try_from(metadata.len()).map_err(|_| "tensor file too large")?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    require_eof(&mut file)?;
    parse_tensor(&bytes)
}

const USAGE: &str = "usage: h3-control-media canny INPUT.rgb OUTPUT.rgb WIDTH HEIGHT FRAMES LOW HIGH\n       h3-control-media mask SOURCE.rgb MASK.rgb OUTPUT.rgb WIDTH HEIGHT FRAMES INVERT(0|1)\n       h3-control-media rows GUIDE.diftensor OUTPUT.diftensor [SOURCE.diftensor MASK.rgb WIDTH HEIGHT FRAMES INVERT(0|1)]";

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("canny") if args.len() == 8 => {
            let (width, height, frames) = (number(&args[3])?, number(&args[4])?, number(&args[5])?);
            let (low, high) = (number(&args[6])?, number(&args[7])?);
            canny_thresholds(low, high)?;
            let mut input = BufReader::new(rgb_input(&args[1], width, height, frames)?);
            let mut rgb = vec![0; product(&[width, height, 3])?];
            let mut output = output(&args[2])?;
            // The source explicitly forbids cross-frame hysteresis. Process a
            // frame at a time, retaining exactly the same within-frame order.
            for _ in 0..frames {
                input.read_exact(&mut rgb).map_err(|e| e.to_string())?;
                output.write_all(&canny(&rgb, width, height, low, high)?).map_err(|e| e.to_string())?;
            }
            require_eof(&mut input)?;
            output.flush().map_err(|e| e.to_string())?;
        }
        Some("mask") if args.len() == 8 => {
            let (width, height, frames) = (number(&args[4])?, number(&args[5])?, number(&args[6])?);
            let invert = invert(&args[7])?;
            let mut source = rgb_read(&args[1], width, height, frames)?;
            let mask = rgb_read(&args[2], width, height, frames)?;
            mask_source(&mut source, &mask, [frames, height, width], invert)?;
            let mut output = output(&args[3])?;
            output.write_all(&source).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
        }
        Some("rows") if args.len() == 3 || args.len() == 9 => {
            let guide = read_tensor(&args[1])?;
            let latent = latent_shape(&guide)?;
            let rows = if args.len() == 9 {
                let source = read_tensor(&args[3])?;
                let (width, height, frames) = (number(&args[5])?, number(&args[6])?, number(&args[7])?);
                let invert = invert(&args[8])?;
                product(&[width, height, frames])?;
                if frames % 17 != 5 || width % 32 != 0 || height % 32 != 0 {
                    return Err("source/mask media geometry must be H3 aligned (17n+5 frames, spatial multiples of 32)".into());
                }
                if latent != [(frames - 5) / 17 * 5 + 2, height / 16, width / 16] {
                    return Err("mask geometry does not match encoded guide latent".into());
                }
                let mask = rgb_read(&args[4], width, height, frames)?;
                union_rows(&guide, Some((&source, &mask, [frames, height, width], invert)))?
            } else { union_rows(&guide, None)? };
            let bytes = tensor_bytes(&rows)?;
            let mut output = output(&args[2])?;
            output.write_all(&bytes).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() { eprintln!("h3-control-media: {error}"); std::process::exit(1); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_gray_uses_source_fixed_point_coefficients() {
        assert_eq!(gray(&[255,0,0, 0,255,0, 0,0,255, 255,255,255]), vec![76,150,29,255]);
    }

    #[test]
    fn canny_sobel_border_nms_tie_and_strict_high_threshold() {
        // One replicated-border row: gx=[200,200,0]. NMS keeps only x=0
        // because left comparison is strict and right comparison is >=.
        let rgb = [0,0,0, 50,50,50, 50,50,50];
        assert_eq!(canny(&rgb, 3, 1, 100, 199).unwrap(), [255,255,255, 0,0,0, 0,0,0]);
        // m==high is weak; without a strong seed it must disappear.
        assert_eq!(canny(&rgb, 3, 1, 100, 200).unwrap(), [0; 9]);
        assert!(canny(&rgb, 3, 1, 200, 200).is_err());
        assert_eq!(canny(&[255; 9], 3, 1, 0, 1).unwrap(), [0; 9]);
    }

    #[test]
    fn mask_threshold_inversion_and_source_truncation() {
        let mask = [127,127,128, 127,128,128]; // sums 382 and 383
        let mut source = [255,129,1, 254,128,2];
        mask_source(&mut source, &mask, [1,1,2], false).unwrap();
        assert_eq!(source, [255,129,1, 0,0,0]);
        let mut source = [255,129,1, 254,128,2];
        mask_source(&mut source, &mask, [1,1,2], true).unwrap();
        assert_eq!(source, [0,0,0, 254,128,2]);
        assert_eq!(mask_channel(255, 0.5), 127);
        assert_eq!(mask_channel(129, 0.5), 64);
    }

    #[test]
    fn visibility_half_pixel_trilinear_and_replicated_boundaries() {
        let mut mask = vec![255; 2 * 2 * 2 * 3];
        mask[..3].fill(0); // exactly one visible corner
        assert_eq!(visibility_at(&mask, [2,2,2], [1,1,1], [0,0,0], false).to_bits(), 0.125f32.to_bits());
        assert_eq!(visibility_at(&mask, [2,2,2], [1,1,1], [0,0,0], true).to_bits(), 0.875f32.to_bits());
        for t in 0..3 {
            assert_eq!(visibility_at(&[0; 3], [1,1,1], [3,1,1], [t,0,0], false), 1.0);
        }
        assert_eq!(axis(0, 5, 2), (0, 1, 0.75));
        assert_eq!(axis(1, 5, 2), (3, 4, 0.25));
    }

    #[test]
    fn union_channel_major_patchify_and_unpaired_zero_channels() {
        let guide = Tensor { dims: vec![1,24,2,2,4], values: (0..24*2*2*4).map(|i| i as f32).collect() };
        let source = Tensor { dims: guide.dims.clone(), values: guide.values.iter().map(|v| v + 1000.0).collect() };
        let mut mask = vec![255; 2 * 2 * 4 * 3];
        mask[..3].fill(0);
        let rows = union_rows(&guide, Some((&source, &mask, [2,2,4], false))).unwrap();
        assert_eq!(rows.dims, [4,196]);
        for t in 0..2 {
            for x in 0..2 {
                let row = t * 2 + x;
                for c in 0..49 {
                    for iy in 0..2 {
                        for ix in 0..2 {
                            let at = t * 8 + iy * 4 + x * 2 + ix;
                            let expected = if c < 24 { (c * 16 + at) as f32 }
                                else if c == 24 { if at == 0 { 1.0 } else { 0.0 } }
                                else { ((c - 25) * 16 + at) as f32 + 1000.0 };
                            assert_eq!(rows.values[row * 196 + c * 4 + iy * 2 + ix].to_bits(), expected.to_bits());
                        }
                    }
                }
            }
        }
        let rows = union_rows(&guide, None).unwrap();
        for row in rows.values.chunks_exact(196) { assert!(row[96..].iter().all(|v| v.to_bits() == 0)); }
    }

    #[test]
    fn diftensor_sha256_and_exact_f32_bits() {
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        let tensor = Tensor { dims: vec![1,4], values: vec![0.0,-0.0,1.25,-7.0] };
        let mut bytes = tensor_bytes(&tensor).unwrap();
        assert_eq!(bytes.len(), 44 + 16 + 32);
        let parsed = parse_tensor(&bytes).unwrap();
        assert_eq!(parsed.dims, tensor.dims);
        assert_eq!(parsed.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), tensor.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
        bytes[44] ^= 1;
        assert!(parse_tensor(&bytes).is_err());
        assert!(tensor_bytes(&Tensor { dims: vec![1,1], values: vec![f32::NAN] }).is_err());
    }
}
