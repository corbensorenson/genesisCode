#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageFormat {
    Rgba8,
    Bgra8,
    Rgb8,
    Bgr8,
    Gray8,
    Gray16Le,
    Rgba16Le,
}

impl ImageFormat {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "rgba8" => Some(Self::Rgba8),
            "bgra8" => Some(Self::Bgra8),
            "rgb8" => Some(Self::Rgb8),
            "bgr8" => Some(Self::Bgr8),
            "gray8" => Some(Self::Gray8),
            "gray16le" => Some(Self::Gray16Le),
            "rgba16le" => Some(Self::Rgba16Le),
            _ => None,
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8 | Self::Bgra8 => 4,
            Self::Rgb8 | Self::Bgr8 => 3,
            Self::Gray8 => 1,
            Self::Gray16Le => 2,
            Self::Rgba16Le => 8,
        }
    }
}

pub(super) fn image_supported_formats() -> &'static [&'static str] {
    &[
        "rgba8", "bgra8", "rgb8", "bgr8", "gray8", "gray16le", "rgba16le",
    ]
}

pub(super) fn image_bytes_per_pixel(format: &str) -> Option<usize> {
    ImageFormat::parse(format).map(ImageFormat::bytes_per_pixel)
}

fn pixel_luma_u16(r: u16, g: u16, b: u16) -> u16 {
    let r = r as u32;
    let g = g as u32;
    let b = b as u32;
    ((19_595_u32 * r + 38_470_u32 * g + 7_471_u32 * b + 32_768_u32) >> 16) as u16
}

fn u16_to_u8(v: u16) -> u8 {
    ((v as u32 + 128_u32) / 257_u32) as u8
}

fn decode_image_pixel(bytes: &[u8], pixel_idx: usize, format: ImageFormat) -> [u16; 4] {
    let offset = pixel_idx * format.bytes_per_pixel();
    match format {
        ImageFormat::Rgba8 => {
            let px = &bytes[offset..offset + 4];
            [
                (px[0] as u16) * 257,
                (px[1] as u16) * 257,
                (px[2] as u16) * 257,
                (px[3] as u16) * 257,
            ]
        }
        ImageFormat::Bgra8 => {
            let px = &bytes[offset..offset + 4];
            [
                (px[2] as u16) * 257,
                (px[1] as u16) * 257,
                (px[0] as u16) * 257,
                (px[3] as u16) * 257,
            ]
        }
        ImageFormat::Rgb8 => {
            let px = &bytes[offset..offset + 3];
            [
                (px[0] as u16) * 257,
                (px[1] as u16) * 257,
                (px[2] as u16) * 257,
                u16::MAX,
            ]
        }
        ImageFormat::Bgr8 => {
            let px = &bytes[offset..offset + 3];
            [
                (px[2] as u16) * 257,
                (px[1] as u16) * 257,
                (px[0] as u16) * 257,
                u16::MAX,
            ]
        }
        ImageFormat::Gray8 => {
            let gray = (bytes[offset] as u16) * 257;
            [gray, gray, gray, u16::MAX]
        }
        ImageFormat::Gray16Le => {
            let gray = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            [gray, gray, gray, u16::MAX]
        }
        ImageFormat::Rgba16Le => {
            let px = &bytes[offset..offset + 8];
            [
                u16::from_le_bytes([px[0], px[1]]),
                u16::from_le_bytes([px[2], px[3]]),
                u16::from_le_bytes([px[4], px[5]]),
                u16::from_le_bytes([px[6], px[7]]),
            ]
        }
    }
}

fn encode_image_pixel(out: &mut Vec<u8>, format: ImageFormat, rgba: [u16; 4]) {
    match format {
        ImageFormat::Rgba8 => {
            out.push(u16_to_u8(rgba[0]));
            out.push(u16_to_u8(rgba[1]));
            out.push(u16_to_u8(rgba[2]));
            out.push(u16_to_u8(rgba[3]));
        }
        ImageFormat::Bgra8 => {
            out.push(u16_to_u8(rgba[2]));
            out.push(u16_to_u8(rgba[1]));
            out.push(u16_to_u8(rgba[0]));
            out.push(u16_to_u8(rgba[3]));
        }
        ImageFormat::Rgb8 => {
            out.push(u16_to_u8(rgba[0]));
            out.push(u16_to_u8(rgba[1]));
            out.push(u16_to_u8(rgba[2]));
        }
        ImageFormat::Bgr8 => {
            out.push(u16_to_u8(rgba[2]));
            out.push(u16_to_u8(rgba[1]));
            out.push(u16_to_u8(rgba[0]));
        }
        ImageFormat::Gray8 => {
            let r = u16_to_u8(rgba[0]) as u32;
            let g = u16_to_u8(rgba[1]) as u32;
            let b = u16_to_u8(rgba[2]) as u32;
            out.push(((77_u32 * r + 150_u32 * g + 29_u32 * b + 128_u32) >> 8) as u8);
        }
        ImageFormat::Gray16Le => {
            out.extend_from_slice(&pixel_luma_u16(rgba[0], rgba[1], rgba[2]).to_le_bytes());
        }
        ImageFormat::Rgba16Le => {
            out.extend_from_slice(&rgba[0].to_le_bytes());
            out.extend_from_slice(&rgba[1].to_le_bytes());
            out.extend_from_slice(&rgba[2].to_le_bytes());
            out.extend_from_slice(&rgba[3].to_le_bytes());
        }
    }
}

pub(super) fn image_transcode(
    source_format: &str,
    target_format: &str,
    width: usize,
    height: usize,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    let source = ImageFormat::parse(source_format)
        .ok_or_else(|| format!("unsupported source format `{source_format}`"))?;
    let target = ImageFormat::parse(target_format)
        .ok_or_else(|| format!("unsupported target format `{target_format}`"))?;

    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| format!("pixel count overflow for width={width} height={height}"))?;

    let expected_input_len = pixel_count
        .checked_mul(source.bytes_per_pixel())
        .ok_or_else(|| "input length overflow".to_string())?;
    if data.len() != expected_input_len {
        return Err(format!(
            "input bytes mismatch: expected {expected_input_len}, got {}",
            data.len()
        ));
    }

    if source == target {
        return Ok(data.to_vec());
    }

    let mut output = Vec::with_capacity(
        pixel_count
            .checked_mul(target.bytes_per_pixel())
            .ok_or_else(|| "output length overflow".to_string())?,
    );
    for i in 0..pixel_count {
        let decoded = decode_image_pixel(data, i, source);
        encode_image_pixel(&mut output, target, decoded);
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
enum AudioFormat {
    PcmU8,
    PcmS16Le,
    PcmS24Le,
    PcmS32Le,
    PcmF32Le,
    PcmF64Le,
}

impl AudioFormat {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pcm-u8" => Some(Self::PcmU8),
            "pcm-s16le" => Some(Self::PcmS16Le),
            "pcm-s24le" => Some(Self::PcmS24Le),
            "pcm-s32le" => Some(Self::PcmS32Le),
            "pcm-f32le" => Some(Self::PcmF32Le),
            "pcm-f64le" => Some(Self::PcmF64Le),
            _ => None,
        }
    }

    fn bytes_per_sample(self) -> usize {
        match self {
            Self::PcmU8 => 1,
            Self::PcmS16Le => 2,
            Self::PcmS24Le => 3,
            Self::PcmS32Le | Self::PcmF32Le => 4,
            Self::PcmF64Le => 8,
        }
    }
}

pub(super) fn audio_supported_formats() -> &'static [&'static str] {
    &[
        "pcm-u8",
        "pcm-s16le",
        "pcm-s24le",
        "pcm-s32le",
        "pcm-f32le",
        "pcm-f64le",
    ]
}

pub(super) fn audio_bytes_per_sample(format: &str) -> Option<usize> {
    AudioFormat::parse(format).map(AudioFormat::bytes_per_sample)
}

fn quantize_signed(sample: f64, max_pos: i64, min_neg: i64) -> i64 {
    if sample >= 1.0 {
        return max_pos;
    }
    if sample <= -1.0 {
        return min_neg;
    }
    (sample * max_pos as f64).round() as i64
}

fn decode_audio_sample(format: AudioFormat, bytes: &[u8]) -> Result<f64, String> {
    match format {
        AudioFormat::PcmU8 => Ok(((bytes[0] as i16 - 128_i16) as f64) / 128.0),
        AudioFormat::PcmS16Le => Ok(i16::from_le_bytes([bytes[0], bytes[1]]) as f64 / 32_768.0),
        AudioFormat::PcmS24Le => {
            let raw = (bytes[0] as u32) | ((bytes[1] as u32) << 8) | ((bytes[2] as u32) << 16);
            let signed = if (raw & 0x0080_0000) != 0 {
                (raw | 0xFF00_0000) as i32
            } else {
                raw as i32
            };
            Ok(signed as f64 / 8_388_608.0)
        }
        AudioFormat::PcmS32Le => Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            as f64
            / 2_147_483_648.0),
        AudioFormat::PcmF32Le => {
            let sample = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !sample.is_finite() {
                return Err("input contains non-finite pcm-f32 sample".to_string());
            }
            Ok(sample as f64)
        }
        AudioFormat::PcmF64Le => {
            let sample = f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            if !sample.is_finite() {
                return Err("input contains non-finite pcm-f64 sample".to_string());
            }
            Ok(sample)
        }
    }
}

fn encode_audio_sample(format: AudioFormat, sample: f64, out: &mut Vec<u8>) {
    let clamped = sample.clamp(-1.0, 1.0);
    match format {
        AudioFormat::PcmU8 => {
            let quantized = (clamped * 128.0 + 128.0).round();
            let as_u8 = quantized.clamp(0.0, 255.0) as u8;
            out.push(as_u8);
        }
        AudioFormat::PcmS16Le => {
            let quantized = quantize_signed(clamped, 32_767, -32_768) as i16;
            out.extend_from_slice(&quantized.to_le_bytes());
        }
        AudioFormat::PcmS24Le => {
            let quantized = quantize_signed(clamped, 8_388_607, -8_388_608) as i32;
            let le = quantized.to_le_bytes();
            out.extend_from_slice(&le[..3]);
        }
        AudioFormat::PcmS32Le => {
            let quantized = quantize_signed(clamped, 2_147_483_647, -2_147_483_648) as i32;
            out.extend_from_slice(&quantized.to_le_bytes());
        }
        AudioFormat::PcmF32Le => {
            out.extend_from_slice(&(clamped as f32).to_le_bytes());
        }
        AudioFormat::PcmF64Le => {
            out.extend_from_slice(&clamped.to_le_bytes());
        }
    }
}

pub(super) fn audio_transcode(
    source_format: &str,
    target_format: &str,
    channels: usize,
    data: &[u8],
) -> Result<(Vec<u8>, usize), String> {
    let source = AudioFormat::parse(source_format)
        .ok_or_else(|| format!("unsupported source format `{source_format}`"))?;
    let target = AudioFormat::parse(target_format)
        .ok_or_else(|| format!("unsupported target format `{target_format}`"))?;

    let input_frame_bytes = source
        .bytes_per_sample()
        .checked_mul(channels)
        .ok_or_else(|| "input frame-size overflow".to_string())?;

    if data.len() % input_frame_bytes != 0 {
        return Err(format!(
            "input bytes ({}) not aligned to frame size {}",
            data.len(),
            input_frame_bytes
        ));
    }

    let frames = data.len() / input_frame_bytes;
    if source_format == target_format {
        return Ok((data.to_vec(), frames));
    }

    let sample_count = frames
        .checked_mul(channels)
        .ok_or_else(|| "sample count overflow".to_string())?;
    let source_sample_bytes = source.bytes_per_sample();
    let target_sample_bytes = target.bytes_per_sample();

    let mut output = Vec::with_capacity(
        sample_count
            .checked_mul(target_sample_bytes)
            .ok_or_else(|| "output frame-size overflow".to_string())?,
    );

    for i in 0..sample_count {
        let start = i * source_sample_bytes;
        let end = start + source_sample_bytes;
        let decoded = decode_audio_sample(source, &data[start..end])?;
        encode_audio_sample(target, decoded, &mut output);
    }

    Ok((output, frames))
}
