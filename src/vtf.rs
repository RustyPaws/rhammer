//! VTF texture decoder (frame 0, first face), producing RGBA8.

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// Size of the full-resolution texture (before any mip reduction).
    pub full_w: u32,
    pub full_h: u32,
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn mip_size(fmt: i32, w: u32, h: u32) -> usize {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    match fmt {
        13 | 20 => w.div_ceil(4) * h.div_ceil(4) * 8,
        14 | 15 => w.div_ceil(4) * h.div_ceil(4) * 16,
        0 | 1 | 11 | 12 | 16 | 22 | 23 | 26 => w * h * 4,
        2 | 3 | 9 | 10 => w * h * 3,
        4 | 6 | 17 | 18 | 19 | 21 => w * h * 2,
        5 | 7 | 8 => w * h,
        24 => w * h * 8,
        25 => w * h * 8,
        _ => 0,
    }
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

fn decode_dxt(fmt: i32, data: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    let bsize = if fmt == 13 || fmt == 20 { 8 } else { 16 };
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) * bsize..][..bsize];
            let cb = if bsize == 16 { &blk[8..16] } else { &blk[0..8] };
            let c0 = rd16(cb, 0);
            let c1 = rd16(cb, 2);
            let p0 = rgb565(c0);
            let p1 = rgb565(c1);
            let mut pal = [[0u8; 4]; 4];
            pal[0] = [p0[0], p0[1], p0[2], 255];
            pal[1] = [p1[0], p1[1], p1[2], 255];
            if c0 > c1 || bsize == 16 {
                for k in 0..3 {
                    pal[2][k] = ((2 * p0[k] as u32 + p1[k] as u32) / 3) as u8;
                    pal[3][k] = ((p0[k] as u32 + 2 * p1[k] as u32) / 3) as u8;
                }
                pal[2][3] = 255;
                pal[3][3] = 255;
            } else {
                for k in 0..3 {
                    pal[2][k] = ((p0[k] as u32 + p1[k] as u32) / 2) as u8;
                }
                pal[2][3] = 255;
                pal[3] = [0, 0, 0, if fmt == 20 { 0 } else { 255 }];
            }
            let bits = rd32(cb, 4);
            // alpha
            let mut alpha = [255u8; 16];
            if fmt == 14 {
                for i in 0..16 {
                    let v = (blk[i / 2] >> (4 * (i % 2))) & 15;
                    alpha[i] = v * 17;
                }
            } else if fmt == 15 {
                let a0 = blk[0] as u32;
                let a1 = blk[1] as u32;
                let mut tbl = [0u8; 8];
                tbl[0] = a0 as u8;
                tbl[1] = a1 as u8;
                if a0 > a1 {
                    for i in 1..7 {
                        tbl[i + 1] = (((7 - i as u32) * a0 + i as u32 * a1) / 7) as u8;
                    }
                } else {
                    for i in 1..5 {
                        tbl[i + 1] = (((5 - i as u32) * a0 + i as u32 * a1) / 5) as u8;
                    }
                    tbl[6] = 0;
                    tbl[7] = 255;
                }
                let mut abits: u64 = 0;
                for i in 0..6 {
                    abits |= (blk[2 + i] as u64) << (8 * i);
                }
                for i in 0..16 {
                    alpha[i] = tbl[((abits >> (3 * i)) & 7) as usize];
                }
            }
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= w || y >= h {
                        continue;
                    }
                    let idx = ((bits >> (2 * (py * 4 + px))) & 3) as usize;
                    let mut c = pal[idx];
                    if fmt == 14 || fmt == 15 {
                        c[3] = alpha[py * 4 + px];
                    }
                    out[(y * w + x) * 4..][..4].copy_from_slice(&c);
                }
            }
        }
    }
    out
}

fn half(h: u16) -> f32 {
    let s = ((h >> 15) & 1) as f32;
    let e = ((h >> 10) & 31) as i32;
    let m = (h & 1023) as f32;
    let v = if e == 0 {
        m / 1024.0 * 2f32.powi(-14)
    } else if e == 31 {
        65504.0
    } else {
        (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    };
    if s > 0.0 { -v } else { v }
}

fn decode(fmt: i32, d: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let n = w * h;
    let mut o = vec![255u8; n * 4];
    match fmt {
        13 | 14 | 15 | 20 => return Some(decode_dxt(fmt, d, w, h)),
        0 => o.copy_from_slice(d.get(..n * 4)?),
        1 => {
            for i in 0..n {
                let s = d.get(i * 4..i * 4 + 4)?;
                o[i * 4..i * 4 + 4].copy_from_slice(&[s[3], s[2], s[1], s[0]]);
            }
        }
        2 => {
            for i in 0..n {
                let s = d.get(i * 3..i * 3 + 3)?;
                o[i * 4..i * 4 + 3].copy_from_slice(s);
            }
        }
        3 => {
            for i in 0..n {
                let s = d.get(i * 3..i * 3 + 3)?;
                o[i * 4..i * 4 + 3].copy_from_slice(&[s[2], s[1], s[0]]);
            }
        }
        4 | 17 => {
            for i in 0..n {
                let c = rgb565(rd16(d.get(i * 2..i * 2 + 2)?, 0));
                let c = if fmt == 17 { [c[2], c[1], c[0]] } else { c };
                o[i * 4..i * 4 + 3].copy_from_slice(&c);
            }
        }
        5 => {
            for i in 0..n {
                let v = *d.get(i)?;
                o[i * 4..i * 4 + 3].copy_from_slice(&[v, v, v]);
            }
        }
        6 => {
            for i in 0..n {
                let s = d.get(i * 2..i * 2 + 2)?;
                o[i * 4..i * 4 + 4].copy_from_slice(&[s[0], s[0], s[0], s[1]]);
            }
        }
        8 => {
            for i in 0..n {
                o[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, *d.get(i)?]);
            }
        }
        11 => {
            for i in 0..n {
                let s = d.get(i * 4..i * 4 + 4)?;
                o[i * 4..i * 4 + 4].copy_from_slice(&[s[1], s[2], s[3], s[0]]);
            }
        }
        12 | 16 => {
            for i in 0..n {
                let s = d.get(i * 4..i * 4 + 4)?;
                o[i * 4..i * 4 + 4].copy_from_slice(&[s[2], s[1], s[0], if fmt == 16 { 255 } else { s[3] }]);
            }
        }
        19 => {
            for i in 0..n {
                let v = rd16(d.get(i * 2..i * 2 + 2)?, 0);
                let c = |sh: u16| (((v >> sh) & 15) * 17) as u8;
                o[i * 4..i * 4 + 4].copy_from_slice(&[c(8), c(4), c(0), c(12)]);
            }
        }
        18 | 21 => {
            for i in 0..n {
                let v = rd16(d.get(i * 2..i * 2 + 2)?, 0);
                let c = |sh: u16| ((((v >> sh) & 31) as u32) * 255 / 31) as u8;
                o[i * 4..i * 4 + 4].copy_from_slice(&[c(10), c(5), c(0), if fmt == 21 && (v >> 15) == 0 { 0 } else { 255 }]);
            }
        }
        24 => {
            for i in 0..n {
                for k in 0..4 {
                    let v = half(rd16(d.get(i * 8 + k * 2..i * 8 + k * 2 + 2)?, 0));
                    // simple tonemap for HDR
                    let t = if k < 3 { (v / (1.0 + v)).powf(1.0 / 2.2) * 1.5 } else { v };
                    o[i * 4 + k] = (t.clamp(0.0, 1.0) * 255.0) as u8;
                }
            }
        }
        _ => return None,
    }
    Some(o)
}

/// Decode a VTF, choosing the largest mip whose dimensions do not exceed `max_dim`.
pub fn read_vtf(b: &[u8], max_dim: u32) -> Option<Image> {
    if b.len() < 64 || &b[0..4] != b"VTF\0" {
        return None;
    }
    let minor = rd32(b, 8);
    let header_size = rd32(b, 12) as usize;
    let w = rd16(b, 16) as u32;
    let h = rd16(b, 18) as u32;
    let flags = rd32(b, 20);
    let frames = rd16(b, 24) as usize;
    let hr_fmt = rd32(b, 52) as i32;
    let mips = b[56] as usize;
    let lr_fmt = rd32(b, 57) as i32;
    let (lw, lh) = (b[61] as u32, b[62] as u32);
    let faces = if flags & 0x4000 != 0 { if minor < 5 { 7 } else { 6 } } else { 1 };
    let mut data_off = header_size + if lw > 0 && lh > 0 && lr_fmt >= 0 { mip_size(lr_fmt, lw, lh) } else { 0 };
    if minor >= 3 {
        let nres = rd32(b, 68) as usize;
        for i in 0..nres {
            let o = 80 + i * 8;
            if o + 8 > b.len() {
                break;
            }
            if b[o] == 0x30 && b[o + 1] == 0 && b[o + 2] == 0 {
                data_off = rd32(b, o + 4) as usize;
            }
        }
    }
    if mips == 0 || frames == 0 {
        return None;
    }
    // choose mip level
    let mut level = 0usize;
    while level + 1 < mips && ((w >> level).max(1) > max_dim || (h >> level).max(1) > max_dim) {
        level += 1;
    }
    // data is stored smallest mip first
    let mut off = data_off;
    for m in (level + 1..mips).rev() {
        off += mip_size(hr_fmt, w >> m, h >> m) * frames * faces;
    }
    let mw = (w >> level).max(1);
    let mh = (h >> level).max(1);
    let sz = mip_size(hr_fmt, mw, mh);
    let slice = b.get(off..off + sz)?;
    let rgba = decode(hr_fmt, slice, mw as usize, mh as usize)?;
    Some(Image { w: mw, h: mh, rgba, full_w: w, full_h: h })
}
