//! Shrinking an image like QImage::scaled(…, IgnoreAspectRatio, SmoothTransformation) + Format_RGB888.
//! Qt: ARGB32 → ARGB32_Premultiplied (qPremultiply), qSmoothScaleImage (qimagescale.cpp, area averaging in
//! 14-bit fixed point), then RGB888 via qUnpremultiply. Here is the same arithmetic
//! for shrinking along both axes; enlarging (an image smaller than 112) — an honest box filter.
use image::RgbaImage;

/// qPremultiply for a single channel.
fn premultiply(c: u32, a: u32) -> u32 {
    let t = c * a;
    (t + (t >> 8) + 0x80) >> 8
}

/// qUnpremultiply for a single channel (qt_inv_premul_factor = 0x00ff00ff / a).
fn unpremultiply(c: u32, a: u32) -> u8 {
    match a {
        0 => 0,
        255 => c.min(255) as u8,
        _ => ((c * (0x00ff_00ff / a) + 0x8000) >> 16).min(255) as u8,
    }
}

/// qimageCalcXPoints / ypoints when shrinking: the index of the first source pixel.
fn points(src: u32, dst: u32) -> Vec<usize> {
    let inc = (i64::from(src) << 16) / i64::from(dst);
    (0..i64::from(dst))
        .map(|i| ((i * inc) >> 16) as usize)
        .collect()
}

/// qimageCalcApoints when shrinking: (the first pixel's share, the rest's share), the sum is 1 << 14.
fn apoints(src: u32, dst: u32) -> Vec<(u32, u32)> {
    let (s, d) = (i64::from(src), i64::from(dst));
    let inc = (s << 16) / d;
    let cp = ((d << 14) + s - 1) / s;
    (0..d)
        .map(|i| {
            let val = i * inc;
            let ap = ((0x10000 - (val & 0xffff)) * cp) >> 16;
            (ap as u32, cp as u32)
        })
        .collect()
}

/// qt_qimageScaleAARGBA_helper: convolution along the row starting at `start`.
fn helper(pix: &[[u32; 4]], start: usize, xyap: u32, cxy: u32) -> [u64; 4] {
    let at = |i: usize| pix[i.min(pix.len() - 1)];
    let mut acc = [0u64; 4];
    let add = |acc: &mut [u64; 4], p: [u32; 4], w: u64| {
        for c in 0..4 {
            acc[c] += u64::from(p[c]) * w;
        }
    };
    let mut i = start;
    add(&mut acc, at(i), u64::from(xyap));
    let mut j = (1i64 << 14) - i64::from(xyap);
    while j > i64::from(cxy) {
        i += 1;
        add(&mut acc, at(i), u64::from(cxy));
        j -= i64::from(cxy);
    }
    i += 1;
    add(&mut acc, at(i), j.max(0) as u64);
    acc
}

/// RGB pixels row by row, `width × height`.
pub fn smooth_scale_rgb(image: &RgbaImage, width: u32, height: u32) -> Vec<[u8; 3]> {
    let (sw, sh) = image.dimensions();
    if sw == 0 || sh == 0 || width == 0 || height == 0 {
        return Vec::new();
    }
    if width > sw || height > sh {
        return box_scale_rgb(image, width, height);
    }
    // ARGB32_Premultiplied, channel order r, g, b, a
    let pix: Vec<[u32; 4]> = image
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0.map(u32::from);
            [premultiply(r, a), premultiply(g, a), premultiply(b, a), a]
        })
        .collect();
    let row = sw as usize;
    let xpoints = points(sw, width);
    let ypoints = points(sh, height);
    let xapoints = apoints(sw, width);
    let yapoints = apoints(sh, height);

    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        let (yap, cy) = yapoints[y];
        for x in 0..width as usize {
            let (xap, cx) = xapoints[x];
            let column =
                |sptr: usize| helper(&pix, sptr.min(pix.len() - 1), xap, cx).map(|v| v >> 4);
            let mut sptr = ypoints[y] * row + xpoints[x];
            let mut acc = column(sptr).map(|v| v * u64::from(yap));
            let mut j = (1i64 << 14) - i64::from(yap);
            while j > i64::from(cy) {
                sptr += row;
                let c = column(sptr);
                for k in 0..4 {
                    acc[k] += c[k] * u64::from(cy);
                }
                j -= i64::from(cy);
            }
            sptr += row;
            let c = column(sptr);
            for k in 0..4 {
                acc[k] += c[k] * j.max(0) as u64;
            }
            let [r, g, b, a] = acc.map(|v| ((v >> 24) & 0xff) as u32);
            out.push([
                unpremultiply(r, a),
                unpremultiply(g, a),
                unpremultiply(b, a),
            ]);
        }
    }
    out
}

/// Enlarging (a rare case — tiny covers): area averaging in premultiplied alpha.
fn box_scale_rgb(image: &RgbaImage, width: u32, height: u32) -> Vec<[u8; 3]> {
    let (sw, sh) = image.dimensions();
    let spans = |src: u32, dst: u32| -> Vec<(usize, Vec<f64>)> {
        let ratio = f64::from(src) / f64::from(dst);
        (0..dst)
            .map(|i| {
                let start = f64::from(i) * ratio;
                let end = (f64::from(i) + 1.0) * ratio;
                let first = (start.floor() as usize).min(src as usize - 1);
                let last = (end.ceil() as usize).clamp(first + 1, src as usize);
                let weights = (first..last)
                    .map(|p| (end.min(p as f64 + 1.0) - start.max(p as f64)).max(0.0))
                    .collect();
                (first, weights)
            })
            .collect()
    };
    let xs = spans(sw, width);
    let ys = spans(sh, height);
    let mut out = Vec::with_capacity((width * height) as usize);
    for (y0, wy) in &ys {
        for (x0, wx) in &xs {
            let mut acc = [0.0f64; 4];
            let mut area = 0.0;
            for (dy, fy) in wy.iter().enumerate() {
                for (dx, fx) in wx.iter().enumerate() {
                    let w = fy * fx;
                    let [r, g, b, a] = image
                        .get_pixel((*x0 + dx) as u32, (*y0 + dy) as u32)
                        .0
                        .map(f64::from);
                    acc[0] += w * r * a / 255.0;
                    acc[1] += w * g * a / 255.0;
                    acc[2] += w * b * a / 255.0;
                    acc[3] += w * a;
                    area += w;
                }
            }
            let a = (acc[3] / area).round() as u32;
            let px = |v: f64| unpremultiply((v / area).round() as u32, a);
            out.push([px(acc[0]), px(acc[1]), px(acc[2])]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_roundtrip() {
        assert_eq!(premultiply(255, 255), 255);
        assert_eq!(premultiply(200, 0), 0);
        assert_eq!(premultiply(200, 128), 100);
        assert_eq!(unpremultiply(100, 128), 199);
        assert_eq!(unpremultiply(77, 0), 0);
    }

    #[test]
    fn solid_color_stays() {
        let img = RgbaImage::from_pixel(300, 200, image::Rgba([10, 120, 250, 255]));
        let px = smooth_scale_rgb(&img, 112, 112);
        assert_eq!(px.len(), 112 * 112);
        assert!(px.iter().all(|p| *p == [10, 120, 250]));
        let up = smooth_scale_rgb(
            &RgbaImage::from_pixel(5, 7, image::Rgba([1, 2, 3, 255])),
            112,
            112,
        );
        assert!(up.iter().all(|p| *p == [1, 2, 3]));
    }

    /// Qt's pixels for seed.png (tests/seed_112.rgb): 17 of 12544 currently differ, each by 1
    /// in one channel (Qt's qUnpremultiply rounding depends on the SIMD path).
    #[test]
    fn close_to_qt() {
        let root = env!("CARGO_MANIFEST_DIR");
        let img = image::open(format!("{root}/tests/seed.png"))
            .unwrap()
            .to_rgba8();
        let qt = std::fs::read(format!("{root}/tests/seed_112.rgb")).unwrap();
        let ours = smooth_scale_rgb(&img, 112, 112);
        let differ = ours
            .iter()
            .zip(qt.as_chunks::<3>().0)
            .filter(|(a, b)| a != b)
            .count();
        println!("pixels differing: {differ}");
        assert!(differ < 112 * 112 / 100, "{differ}");
    }
}
