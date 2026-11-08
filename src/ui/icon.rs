//! Procedural app icon and menu-bar glyph (the approved Flatline symbol: an
//! orb setting on the horizon, one last heartbeat on the left; geometry from
//! `design/logo/flatline-symbol-geometry.json`), plus a tiny PNG encoder so
//! the bundle script can build an .icns without extra tools.

use std::io::Write;

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let (wx, wy) = (p.0 - a.0, p.1 - a.1);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (dx, dy) = (wx - t * vx, wy - t * vy);
    (dx * dx + dy * dy).sqrt()
}

fn polyline_dist(p: (f32, f32), pts: &[(f32, f32)]) -> f32 {
    pts.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min)
}

/// Signed distance to a rounded square (negative inside).
fn rounded_rect(p: (f32, f32), x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (p.0 - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - r);
    let cy = (p.1 - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - r);
    (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - r
}

fn hex(c: u32) -> [f32; 3] {
    [((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32]
}

/// Piecewise-linear colour ramp.
fn ramp(stops: &[(f32, u32)], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            let (a, b) = (hex(c0), hex(c1));
            return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
        }
    }
    hex(stops[stops.len() - 1].1)
}

/// Straight-alpha "over" compositing of `src` (opacity `a`) onto `dst`.
fn over(dst: &mut [f32; 4], src: [f32; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    let out = a + dst[3] * (1.0 - a);
    if out <= 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = (src[i] * a + dst[i] * dst[3] * (1.0 - a)) / out;
    }
    dst[3] = out;
}

const PULSE: u32 = 0xFF2E4D;
const MILK: u32 = 0xF4EFEA;

/// The app icon: straight (non-premultiplied) RGBA pixels, `size`×`size`.
pub fn render(size: u32) -> Vec<u8> {
    // Designed on a 1024 grid (macOS icon body 824 px, inset 100).
    let k = size as f32 / 1024.0;
    let (bx0, by0, bx1, by1, br) = (100.0, 100.0, 924.0, 924.0, 185.4);
    let (ocx, ocy, or) = (546.78f32, 512.0f32, 148.0f32);
    let line: Vec<(f32, f32)> =
        [(-2.344, 0.3), (-1.964, 0.3), (-1.744, -0.45), (-1.444, 0.70), (-1.274, 0.3), (1.874, 0.3)]
            .iter()
            .map(|&(x, y)| (ocx + x * or, ocy + y * or))
            .collect();
    let half_w = 0.11 * or;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            // Supersample 2x2 for clean edges at small sizes.
            let mut acc = [0f32; 4];
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let p = ((x as f32 + sx) / k, (y as f32 + sy) / k);
                let aa = 0.7 / k; // ~0.7 px of edge softness in design units
                let mut c = [0f32; 4];
                // Drop shadow.
                let sd = rounded_rect((p.0, p.1 - 12.0), bx0, by0, bx1, by1, br);
                over(&mut c, [0.0; 3], 0.5 * (-(sd.max(0.0) / 14.0).powi(2)).exp());
                let d = rounded_rect(p, bx0, by0, bx1, by1, br);
                let body_a = (0.5 - d / aa).clamp(0.0, 1.0);
                if body_a > 0.0 {
                    let t = (p.1 - by0) / (by1 - by0);
                    let mut b = [0f32; 4];
                    over(&mut b, ramp(&[(0.0, 0x1A1420), (0.45, 0x0C0A10), (1.0, 0x07060A)], t), 1.0);
                    // Lighter top edge: a 3 px crescent fading over the top 35 %.
                    let below = rounded_rect((p.0, p.1 - 3.0), bx0, by0, bx1, by1, br);
                    if below > 0.0 && t < 0.35 {
                        over(&mut b, hex(0xFFFFFF), 0.22 * (1.0 - t / 0.35) * (below / 3.0).min(1.0));
                    }
                    // Glow behind the orb.
                    let gd = ((p.0 - ocx).powi(2) + (p.1 - ocy).powi(2)).sqrt() / 340.4;
                    let ga = if gd < 0.45 {
                        0.42 - (0.42 - 0.16) * gd / 0.45
                    } else {
                        (0.16 * (1.0 - (gd - 0.45) / 0.55)).max(0.0)
                    };
                    over(&mut b, hex(PULSE), ga);
                    // Orb: the cap above the horizon, the veiled reflection below.
                    let od = ((p.0 - ocx).powi(2) + (p.1 - ocy).powi(2)).sqrt() - or;
                    let in_orb = (0.5 - od / aa).clamp(0.0, 1.0);
                    let yr = (p.1 - ocy) / or;
                    if in_orb > 0.0 && yr <= 0.07 {
                        let gr = ((p.0 - ocx).powi(2) + (p.1 - (ocy - 0.5 * or)).powi(2)).sqrt() / (1.62 * or);
                        let col = ramp(
                            &[(0.0, 0xFFC56E), (0.07, 0xFFC56E), (0.4, 0xFF7A5C), (0.71, PULSE), (1.0, 0x3A0A16)],
                            gr,
                        );
                        over(&mut b, col, in_orb * ((0.07 - yr) * or / aa + 0.5).clamp(0.0, 1.0));
                    } else if in_orb > 0.0 && yr >= 0.53 {
                        let t = (yr - 0.53) / 0.47;
                        over(
                            &mut b,
                            hex(PULSE),
                            in_orb * (0.5 - 0.46 * t) * ((yr - 0.53) * or / aa + 0.5).clamp(0.0, 1.0),
                        );
                    }
                    // Horizon line with the heartbeat (round caps and joins).
                    let ld = polyline_dist(p, &line) - half_w;
                    over(&mut b, hex(MILK), (0.5 - ld / aa).clamp(0.0, 1.0));
                    let rgb = [b[0], b[1], b[2]];
                    over(&mut c, rgb, body_a);
                }
                for i in 0..3 {
                    acc[i] += c[i] * c[3];
                }
                acc[3] += c[3];
            }
            let o = ((y * size + x) * 4) as usize;
            let a = acc[3] / 4.0;
            for i in 0..3 {
                px[o + i] = if acc[3] > 0.0 { (acc[i] / acc[3]).clamp(0.0, 255.0) as u8 } else { 0 };
            }
            px[o + 3] = (a * 255.0).round() as u8;
        }
    }
    px
}

/// The window / Dock icon at 256 px.
pub fn window_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: render(256), width: 256, height: 256 }
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Minimal PNG writer (uncompressed deflate blocks).
pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut f = std::fs::File::create(path)?;
    f.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut chunk = |kind: &[u8], data: &[u8]| -> std::io::Result<()> {
        f.write_all(&(data.len() as u32).to_be_bytes())?;
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        f.write_all(&c)?;
        f.write_all(&crc32(&c).to_be_bytes())
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr)?;
    chunk(b"IDAT", &z)?;
    chunk(b"IEND", &[])
}

/// Writes a macOS .iconset directory (for `iconutil -c icns`).
pub fn write_iconset(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (base, scale) in
        [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)]
    {
        let px = base * scale;
        let name = if scale == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
        write_png(&dir.join(name), px, px, &render(px))?;
    }
    Ok(())
}

/// Menu-bar template image: black glyph on transparent (macOS tints it).
/// Solid orb knocked out around the horizon line, designed at 18 pt.
pub fn render_template(size: u32) -> Vec<u8> {
    let k = size as f32 / 18.0;
    let line = [(1.407, 10.25), (2.107, 10.25), (3.407, 6.75), (5.207, 12.25), (6.307, 10.25), (16.593, 10.25)];
    let (ocx, ocy, or) = (11.45f32, 8.9f32, 4.5f32);
    let aa = 0.6 / k;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut sum = 0.0;
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let p = ((x as f32 + sx) / k, (y as f32 + sy) / k);
                let od = ((p.0 - ocx).powi(2) + (p.1 - ocy).powi(2)).sqrt() - or;
                let band = (p.1 - 8.5).min(12.0 - p.1); // > 0 inside the knock-out band
                let orb = (0.5 - od / aa).clamp(0.0, 1.0) * (0.5 + band / aa).clamp(0.0, 1.0).mul_add(-1.0, 1.0);
                let ld = polyline_dist(p, &line) - 0.75;
                let ln = (0.5 - ld / aa).clamp(0.0, 1.0);
                sum += orb.max(ln);
            }
            px[((y * size + x) * 4 + 3) as usize] = (sum / 4.0 * 255.0).round() as u8;
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_glyph_has_orb_line_and_knockout() {
        let n = 72; // 18 pt at 4x
        let px = render_template(n);
        let a = |x: f32, y: f32| px[(((y * 4.0) as u32 * n + (x * 4.0) as u32) * 4 + 3) as usize];
        assert!(a(11.45, 6.0) > 250, "orb cap");
        assert!(a(11.45, 10.25) > 250, "horizon line");
        assert_eq!(a(11.45, 8.9), 0, "knock-out between cap and line");
        assert!(a(11.45, 12.8) > 250, "reflection");
        assert_eq!(a(1.0, 2.0), 0, "background");
    }

    #[test]
    fn app_icon_matches_the_symbol() {
        let n = 128;
        let px = render(n);
        let at = |x: f32, y: f32| {
            let (ix, iy) = ((x / 1024.0 * n as f32) as u32, (y / 1024.0 * n as f32) as u32);
            let o = ((iy * n + ix) * 4) as usize;
            [px[o], px[o + 1], px[o + 2], px[o + 3]]
        };
        assert_eq!(at(10.0, 10.0)[3], 0, "outside the body");
        let cap = at(546.0, 440.0);
        assert!(cap[3] == 255 && cap[0] > 200 && cap[1] > 120, "warm orb top: {cap:?}");
        let line = at(700.0, 556.0);
        assert!(line[0] > 230 && line[1] > 225, "milk horizon line: {line:?}");
    }
}
