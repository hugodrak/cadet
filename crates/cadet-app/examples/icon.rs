//! Renders the app icon: `cargo run -p cadet-app --example icon` writes assets/icon.png.

type Face<'a> = (&'a [(f32, f32)], [f32; 3]);

fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            c = !c;
        }
    }
    c
}

fn main() {
    const N: u32 = 1024;
    const SS: u32 = 3;
    let (cx, cy, r) = (512.0, 540.0, 300.0);
    let (dx, dy) = (r * 0.866, r * 0.5);
    let top = [(cx, cy - r), (cx + dx, cy - dy), (cx, cy), (cx - dx, cy - dy)];
    let left = [(cx - dx, cy - dy), (cx, cy), (cx, cy + r), (cx - dx, cy + dy)];
    let right = [(cx, cy), (cx + dx, cy - dy), (cx + dx, cy + dy), (cx, cy + r)];
    let faces: [Face; 3] = [(&top, [255.0, 196.0, 92.0]), (&left, [245.0, 165.0, 36.0]), (&right, [196.0, 120.0, 14.0])];

    let img = image::RgbaImage::from_fn(N, N, |px, py| {
        let mut acc = [0.0f32; 4];
        for sy in 0..SS {
            for sx in 0..SS {
                let (x, y) = (px as f32 + (sx as f32 + 0.5) / SS as f32, py as f32 + (sy as f32 + 0.5) / SS as f32);
                let (qx, qy) = ((x - 512.0).abs() - 412.0 + 180.0, (y - 512.0).abs() - 412.0 + 180.0);
                let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - 180.0;
                if d > 0.0 {
                    continue;
                }
                let t = y / N as f32;
                let mut c = [43.0 - 20.0 * t, 47.0 - 22.0 * t, 54.0 - 24.0 * t];
                if let Some((_, col)) = faces.iter().find(|(f, _)| inside(f, x, y)) {
                    c = *col;
                }
                for i in 0..3 {
                    acc[i] += c[i];
                }
                acc[3] += 255.0;
            }
        }
        let n = (SS * SS) as f32;
        let a = acc[3] / n;
        let k = if a > 0.0 { 255.0 / acc[3] } else { 0.0 };
        image::Rgba([(acc[0] * k) as u8, (acc[1] * k) as u8, (acc[2] * k) as u8, a as u8])
    });
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icon.png");
    img.save(out).expect("write icon");
    println!("wrote {out}");
}
