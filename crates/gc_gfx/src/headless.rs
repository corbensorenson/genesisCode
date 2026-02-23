use super::*;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessRenderOutput {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub png: Vec<u8>,
    pub pixel_hash: [u8; 32],
    pub png_hash: [u8; 32],
}

pub fn render_frame_graph_headless(
    frame_graph: &Term,
    width: u32,
    height: u32,
) -> Result<HeadlessRenderOutput, String> {
    if width == 0 || height == 0 {
        return Err("headless render size must be non-zero".to_string());
    }
    if !matches!(
        map_get(frame_graph, ":type"),
        Some(Term::Symbol(s)) if s == ":gfx/frame-graph"
    ) {
        return Err("expected :gfx/frame-graph term".to_string());
    }

    let render_passes = map_get(frame_graph, ":render-passes")
        .and_then(as_vec)
        .ok_or_else(|| "frame graph missing :render-passes vector".to_string())?;

    let frame_h = hash_term(frame_graph);
    let px_len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let mut rgba = vec![0u8; px_len];

    // Deterministic background gradient keyed by frame hash.
    for y in 0..height {
        for x in 0..width {
            let i = ((y as usize) * (width as usize) + (x as usize)) * 4;
            rgba[i] = frame_h[0].wrapping_add((x % 251) as u8);
            rgba[i + 1] = frame_h[1].wrapping_add((y % 241) as u8);
            rgba[i + 2] = frame_h[2].wrapping_add(((x ^ y) % 239) as u8);
            rgba[i + 3] = 255;
        }
    }

    for (pi, pass) in render_passes.iter().enumerate() {
        let Some(commands) = map_get(pass, ":commands").and_then(as_vec) else {
            continue;
        };
        let pass_label = map_get(pass, ":label").and_then(as_str).unwrap_or_default();
        for (ci, cmd) in commands.iter().enumerate() {
            let cmd_hash = hash_term(cmd);
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"GCv0.2\0gfx/headless-raster\0");
            hasher.update(&frame_h);
            hasher.update(&(pi as u64).to_le_bytes());
            hasher.update(&(ci as u64).to_le_bytes());
            hasher.update(pass_label.as_bytes());
            hasher.update(&cmd_hash);
            let seed = hasher.finalize();
            let b = seed.as_bytes();

            let x0 = read_u32(b, 0) % width;
            let y0 = read_u32(b, 4) % height;
            let w_max = (width / 2).max(1);
            let h_max = (height / 2).max(1);
            let rw = 1 + (read_u32(b, 8) % w_max);
            let rh = 1 + (read_u32(b, 12) % h_max);
            let x1 = x0.saturating_add(rw).min(width);
            let y1 = y0.saturating_add(rh).min(height);

            let op = map_get(cmd, ":op").and_then(as_sym).unwrap_or_default();
            let op_bias = match op {
                ":draw" => 17u8,
                ":draw-indexed" => 43u8,
                ":dispatch" => 71u8,
                ":set-pipeline" => 97u8,
                _ => 131u8,
            };
            let color = [
                b[16].wrapping_add(op_bias),
                b[17].wrapping_add(op_bias / 2),
                b[18].wrapping_add(op_bias / 3),
                96u8.wrapping_add(b[19] % 160),
            ];
            blend_rect(&mut rgba, width, x0, y0, x1, y1, color);
        }
    }

    let pixel_hash = *blake3::hash(&rgba).as_bytes();
    let png = encode_png_rgba(width, height, &rgba)?;
    let png_hash = *blake3::hash(&png).as_bytes();
    Ok(HeadlessRenderOutput {
        width,
        height,
        rgba,
        png,
        pixel_hash,
        png_hash,
    })
}

fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc
        .write_header()
        .map_err(|e| format!("png header write failed: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png data write failed: {e}"))?;
    drop(writer);
    Ok(out)
}

fn read_u32(bytes: &[u8], off: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[off..off + 4]);
    u32::from_le_bytes(b)
}

fn blend_rect(rgba: &mut [u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
    let a = color[3] as u16;
    let inv = 255u16.saturating_sub(a);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y as usize) * (width as usize) + (x as usize)) * 4;
            let dr = rgba[i] as u16;
            let dg = rgba[i + 1] as u16;
            let db = rgba[i + 2] as u16;
            rgba[i] = ((dr * inv + (color[0] as u16) * a) / 255) as u8;
            rgba[i + 1] = ((dg * inv + (color[1] as u16) * a) / 255) as u8;
            rgba[i + 2] = ((db * inv + (color[2] as u16) * a) / 255) as u8;
            rgba[i + 3] = 255;
        }
    }
}

fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::Symbol(key.to_string())))
}

fn as_vec(t: &Term) -> Option<&Vec<Term>> {
    let Term::Vector(v) = t else { return None };
    Some(v)
}

fn as_str(t: &Term) -> Option<&str> {
    let Term::Str(s) = t else { return None };
    Some(s.as_str())
}

fn as_sym(t: &Term) -> Option<&str> {
    let Term::Symbol(s) = t else { return None };
    Some(s.as_str())
}
