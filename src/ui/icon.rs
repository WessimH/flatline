//! Procedural app icon (rounded square + glowing "flatline"), plus a tiny
//! PNG encoder so the bundle script can build an .icns without extra tools.

use std::io::Write;

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let (wx, wy) = (p.0 - a.0, p.1 - a.1);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (dx, dy) = (wx - t * vx, wy - t * vy);
    (dx * dx + dy * dy).sqrt()
}

/// Straight (non-premultiplied) RGBA pixels, `size`×`size`.
pub fn render(size: u32) -> Vec<u8> {
    let s = size as f32;
    let inset = s * 0.1;
    let (x0, y0, x1, y1) = (inset, inset, s - inset, s - inset);
    let radius = (x1 - x0) * 0.225;
    let body = |fx: f32, fy: f32| (x0 + fx * (x1 - x0), y0 + fy * (y1 - y0));
    let pts = [
        body(0.14, 0.52),
        body(0.38, 0.52),
        body(0.46, 0.27),
        body(0.57, 0.75),
        body(0.65, 0.40),
        body(0.70, 0.52),
        body(0.86, 0.52),
    ];
    let lw = s * 0.026;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-rect signed distance.
            let cx = (p.0 - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - radius);
            let cy = (p.1 - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - radius);
            let outside = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - radius;
            let a = (0.5 - outside).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let t = (p.1 - y0) / (y1 - y0);
            let mut c = [22.0 - 12.0 * t, 34.0 - 20.0 * t, 40.0 - 24.0 * t];
            // Subtle top highlight.
            let edge = (-outside / (s * 0.012)).clamp(0.0, 1.0);
            let rim = (1.0 - edge) * (1.0 - t) * 40.0;
            for v in &mut c {
                *v += rim;
            }
            let d = pts.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min);
            let glow = (-(d / (s * 0.06)).powi(2)).exp() * 0.55;
            let core = (lw - d + 0.5).clamp(0.0, 1.0);
            let accent = [44.0, 232.0, 212.0];
            for i in 0..3 {
                c[i] += (accent[i] - c[i]) * glow;
                c[i] += (accent[i] * 0.6 + 255.0 * 0.4 - c[i]) * core;
            }
            let o = ((y * size + x) * 4) as usize;
            px[o] = c[0].clamp(0.0, 255.0) as u8;
            px[o + 1] = c[1].clamp(0.0, 255.0) as u8;
            px[o + 2] = c[2].clamp(0.0, 255.0) as u8;
            px[o + 3] = (a * 255.0) as u8;
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
pub fn render_template(size: u32) -> Vec<u8> {
    let s = size as f32;
    let at = |fx: f32, fy: f32| (fx * s, fy * s);
    let pts = [
        at(0.04, 0.55),
        at(0.30, 0.55),
        at(0.40, 0.18),
        at(0.54, 0.86),
        at(0.64, 0.40),
        at(0.71, 0.55),
        at(0.96, 0.55),
    ];
    let lw = s * 0.055;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let d = pts.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min);
            let a = (lw - d + 0.5).clamp(0.0, 1.0);
            px[((y * size + x) * 4 + 3) as usize] = (a * 255.0) as u8;
        }
    }
    px
}
