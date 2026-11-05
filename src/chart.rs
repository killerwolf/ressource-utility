//! Mini-courbe RGBA dessinée en code (zone remplie + ligne), pour le menu.

pub const W: u32 = 240;
pub const H: u32 = 36;

const LINE: [u8; 4] = [90, 160, 255, 255];
const FILL: [u8; 4] = [90, 160, 255, 70];
const BASE: [u8; 4] = [140, 140, 140, 90];

fn put(px: &mut [u8], x: u32, y: u32, c: [u8; 4]) {
    if x < W && y < H {
        let i = ((y * W + x) * 4) as usize;
        px[i..i + 4].copy_from_slice(&c);
    }
}

/// Valeur interpolée à la colonne `x` (les points sont étalés sur toute la largeur).
fn sample(values: &[f32], x: u32) -> f32 {
    if values.len() == 1 {
        return values[0];
    }
    let pos = x as f32 / (W - 1) as f32 * (values.len() - 1) as f32;
    let i = (pos.floor() as usize).min(values.len() - 2);
    let f = pos - i as f32;
    values[i] * (1.0 - f) + values[i + 1] * f
}

/// Rend `values` entre `lo` et `hi`. Retourne W×H×4 octets RGBA.
pub fn render(values: &[f32], lo: f32, hi: f32) -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for x in 0..W {
        put(&mut px, x, H - 1, BASE);
    }
    if values.is_empty() {
        return px;
    }
    let span = (hi - lo).max(1.0);
    let ypos = |v: f32| {
        let n = ((v - lo) / span).clamp(0.0, 1.0);
        ((H - 3) as f32 * (1.0 - n)).round() as u32 + 1
    };
    let mut prev = ypos(sample(values, 0));
    for x in 0..W {
        let y = ypos(sample(values, x));
        for yy in y..H - 1 {
            put(&mut px, x, yy, FILL);
        }
        // Ligne continue : relie la colonne précédente à celle-ci.
        for yy in prev.min(y)..=prev.max(y) {
            put(&mut px, x, yy, LINE);
        }
        put(&mut px, x, y + 1, LINE);
        prev = y;
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(px: &[u8]) -> usize {
        px.chunks(4).filter(|p| p[3] == 255).count()
    }

    #[test]
    fn empty_chart_is_only_baseline() {
        let px = render(&[], 0.0, 100.0);
        assert_eq!(px.len(), (W * H * 4) as usize);
        assert!(px.chunks(4).filter(|p| p[3] != 0).count() == W as usize);
    }

    #[test]
    fn higher_values_draw_higher_lines() {
        let low = render(&[5.0; 10], 0.0, 100.0);
        let high = render(&[95.0; 10], 0.0, 100.0);
        let first_line_row = |px: &[u8]| {
            (0..H)
                .find(|y| px[((y * W + 100) * 4) as usize + 3] == 255)
                .unwrap()
        };
        assert!(first_line_row(&high) < first_line_row(&low));
        assert!(opaque(&low) >= W as usize);
    }

    #[test]
    fn single_point_and_ramp_render() {
        assert!(opaque(&render(&[50.0], 0.0, 100.0)) > 0);
        let ramp: Vec<f32> = (0..60).map(|i| i as f32).collect();
        let px = render(&ramp, 0.0, 100.0);
        // Montée continue : colonne de gauche plus basse que celle de droite.
        let top = |x: u32| {
            (0..H)
                .find(|y| px[((y * W + x) * 4) as usize + 3] == 255)
                .unwrap()
        };
        assert!(top(W - 1) < top(0));
    }

    /// Aperçu manuel : `cargo test dump_preview -- --ignored` écrit un PPM sur fond clair.
    #[test]
    #[ignore]
    fn dump_preview() {
        let vals: Vec<f32> = (0..60)
            .map(|i| 30.0 + 25.0 * ((i as f32) / 6.0).sin())
            .collect();
        let px = render(&vals, 0.0, 100.0);
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for p in px.chunks(4) {
            let a = p[3] as f32 / 255.0;
            for c in &p[..3] {
                out.push((*c as f32 * a + 240.0 * (1.0 - a)) as u8);
            }
        }
        std::fs::write(std::env::temp_dir().join("chart.ppm"), out).unwrap();
    }
}
