//! Device icons: user PNG/ICO/BMP/JPG files, plus an optional brand-logo lookup by device name.
//! Images are decoded once, resized to a small mask and cached by the taskbar renderer.
use std::path::PathBuf;

use image::imageops::FilterType;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use crate::util::fnv1a;

#[derive(Clone)]
pub struct Icon {
    pub w: i32,
    pub h: i32,
    /// straight RGBA, row-major
    pub px: Vec<u32>,
}

impl Icon {
    /// High-quality downscale (Lanczos), preserving the aspect ratio.
    pub fn scaled(self, height: i32) -> Self {
        let h = height.max(1);
        let w = ((self.w as f64 * h as f64 / self.h as f64).round() as i32).max(1);
        if w == self.w && h == self.h {
            return self;
        }
        let mut src = image::RgbaImage::new(self.w as u32, self.h as u32);
        for y in 0..self.h {
            for x in 0..self.w {
                let p = self.px[(y * self.w + x) as usize];
                src.put_pixel(x as u32, y as u32, image::Rgba([(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]));
            }
        }
        let dst = image::imageops::resize(&src, w as u32, h as u32, FilterType::Lanczos3);
        let mut px = Vec::with_capacity((w * h) as usize);
        for p in dst.pixels() {
            let [r, g, b, a] = p.0;
            px.push((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32);
        }
        Self { w, h, px }
    }
}

fn dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("BatteryHub").join("Taskbar").join("icons")
}

fn safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(48)
        .collect()
}

fn image_from_bytes(bytes: &[u8]) -> Option<Icon> {
    let mut img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let max = 128u32;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }
    if w > max || h > max {
        let scale = max as f64 / w.max(h) as f64;
        let nw = ((w as f64 * scale).round() as u32).max(1);
        let nh = ((h as f64 * scale).round() as u32).max(1);
        img = image::imageops::resize(&img, nw, nh, FilterType::Lanczos3);
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut px = Vec::with_capacity((w * h) as usize);
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        px.push((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32);
    }
    Some(Icon { w, h, px })
}

pub fn load(path: &str) -> Option<Icon> {
    let bytes = std::fs::read(path).ok()?;
    image_from_bytes(&bytes)
}

/// Copies a user file into the app data folder (so moving the original does not break it).
pub fn store_file(key: &str, source: &std::path::Path) -> Result<PathBuf, String> {
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    image_from_bytes(&bytes).ok_or_else(|| "unsupported or broken image".to_string())?;
    let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("png").to_ascii_lowercase();
    let ext = if ["png", "ico", "bmp", "jpg", "jpeg"].contains(&ext.as_str()) { ext } else { "png".into() };
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let path = d.join(format!("{}-{:08X}.{}", safe(key), fnv1a(key), ext));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn store_bytes(key: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    image_from_bytes(bytes).ok_or_else(|| "the downloaded file is not an image".to_string())?;
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let path = d.join(format!("{}-{:08X}.png", safe(key), fnv1a(key)));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Finds a product cutout for a device name and returns it as PNG with transparency.
///
/// Yandex Images is used because it is reachable where Bing/DuckDuckGo image APIs are not. The
/// best candidate is background-removed locally, trimmed and cached by name; repeated lookups do
/// not touch the network.
pub fn fetch_by_name(name: &str) -> Result<Vec<u8>, String> {
    let cache_dir = dir().join("devices");
    let cache = cache_dir.join(format!("{}-{:08X}.png", safe(name), fnv1a(name)));
    if let Ok(bytes) = std::fs::read(&cache) {
        if image_from_bytes(&bytes).is_some() {
            return Ok(bytes);
        }
    }

    let found = search_images(name)?;
    if found.is_empty() {
        return Err("no device images found".into());
    }
    let mut best: Option<(f32, Vec<u8>)> = None;
    let mut decoded = 0;
    let mut last = "no usable device image".to_string();
    for c in found.into_iter().take(16) {
        if !(0.25..=4.0).contains(&(c.w as f32 / c.h as f32)) || c.w < 240 || c.h < 180 {
            continue;
        }
        let bytes = match crate::update::http_get_web(&c.url) {
            Ok(b) if b.len() <= 12 * 1024 * 1024 => b,
            Ok(_) => continue,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let Some(img) = image::load_from_memory(&bytes).ok().map(|i| i.into_rgba8()) else {
            last = "not an image".into();
            continue;
        };
        decoded += 1;
        let Some((cut, score)) = cutout(img, &c) else { continue };
        let Some(png) = encode_png(&cut) else { continue };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, png));
        }
        if decoded >= 8 && best.is_some() {
            break;
        }
    }
    let Some((_, png)) = best else { return Err(last) };
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&cache, &png);
    Ok(png)
}

#[derive(Clone, Debug)]
struct FoundImage {
    url: String,
    w: u32,
    h: u32,
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else if *b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn unescape_html(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn unescape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('/') => out.push('/'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| it.next()).collect();
                if let Ok(v) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(v) {
                        out.push(ch);
                    }
                }
            }
            Some(c) => out.push(c),
            None => break,
        }
    }
    out
}

fn number_after(s: &str) -> Option<u32> {
    let digits: String = s.chars().skip_while(|c| *c == ':' || *c == ' ').take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// `"origWidth":942,"origHeight":887,"origUrl":"https://..."`
fn parse_found_images(page: &str) -> Vec<FoundImage> {
    let page = unescape_html(page);
    let mut out: Vec<FoundImage> = Vec::new();
    let mut pos = 0;
    while let Some(found) = page[pos..].find("\"origUrl\":\"") {
        let start = pos + found + "\"origUrl\":\"".len();
        let rest = &page[start..];
        let Some(end) = rest.find('"') else { break };
        let url = rest[..end].replace("\\/", "/");
        pos = start + end + 1;
        if !url.starts_with("https://") {
            continue;
        }
        let pre = &page[..start.saturating_sub(1)];
        let w = pre
            .rfind("\"origWidth\":")
            .and_then(|i| number_after(&pre[i + 12..]))
            .or_else(|| pre.rfind("\"width\":").and_then(|i| number_after(&pre[i + 8..])))
            .unwrap_or(0);
        let h = pre
            .rfind("\"origHeight\":")
            .and_then(|i| number_after(&pre[i + 13..]))
            .or_else(|| pre.rfind("\"height\":").and_then(|i| number_after(&pre[i + 9..])))
            .unwrap_or(0);
        let url = unescape_json(&url);
        if !out.iter().any(|c| c.url == url) {
            out.push(FoundImage { url, w, h });
        }
    }
    out
}

fn search_images(name: &str) -> Result<Vec<FoundImage>, String> {
    let query = format!("{name} png");
    let url = format!("https://yandex.ru/images/search?text={}&itype=png", url_encode(&query));
    let page = crate::update::http_get_web(&url)?;
    let page = String::from_utf8_lossy(&page);
    Ok(parse_found_images(&page))
}

fn encode_png(img: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

fn color_dist(a: [u8; 3], b: [u8; 3]) -> f32 {
    let dr = a[0] as f32 - b[0] as f32;
    let dg = a[1] as f32 - b[1] as f32;
    let db = a[2] as f32 - b[2] as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Most common colour on the outer border, quantized to 16 levels.
fn border_color(img: &image::RgbaImage) -> ([u8; 3], f32) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut bins = vec![(0u32, 0u64, 0u64, 0u64); 4096];
    let mut add = |x: i32, y: i32| {
        let p = img.get_pixel(x as u32, y as u32).0;
        let (r, g, b) = (p[0] as u64, p[1] as u64, p[2] as u64);
        let idx = (((p[0] >> 4) as usize) << 8) | (((p[1] >> 4) as usize) << 4) | (p[2] >> 4) as usize;
        bins[idx].0 += 1;
        bins[idx].1 += r;
        bins[idx].2 += g;
        bins[idx].3 += b;
    };
    for y in 0..h {
        for t in 0..2 {
            add(t.min(w - 1), y);
            add((w - 1 - t).max(0), y);
        }
    }
    for x in 0..w {
        for t in 0..2 {
            add(x, t.min(h - 1));
            add(x, (h - 1 - t).max(0));
        }
    }
    let total: u32 = bins.iter().map(|b| b.0).sum();
    let Some(bin) = bins.into_iter().max_by_key(|b| b.0) else {
        return ([255, 255, 255], 0.0);
    };
    if bin.0 == 0 {
        return ([255, 255, 255], 0.0);
    }
    ([(bin.1 / bin.0 as u64) as u8, (bin.2 / bin.0 as u64) as u8, (bin.3 / bin.0 as u64) as u8], bin.0 as f32 / total as f32)
}

fn trim_alpha(img: &mut image::RgbaImage) -> Option<()> {
    let (w, h) = (img.width(), img.height());
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y).0[3] > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let pad = 1;
    let x0 = x0.saturating_sub(pad);
    let y0 = y0.saturating_sub(pad);
    let x1 = (x1 + pad).min(w);
    let y1 = (y1 + pad).min(h);
    *img = image::imageops::crop_imm(img, x0, y0, x1 - x0, y1 - y0).to_image();
    Some(())
}

/// Keeps the largest silhouette and drops tiny islands (watermarks, dust, background leftovers).
fn keep_main_silhouette(img: &mut image::RgbaImage) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let n = (w * h) as usize;
    let mut seen = vec![false; n];
    let mut comps: Vec<Vec<usize>> = Vec::new();
    let mut q = std::collections::VecDeque::new();
    for start in 0..n {
        if seen[start] || img.get_pixel((start as i32 % w) as u32, (start as i32 / w) as u32).0[3] <= 16 {
            continue;
        }
        let mut comp = Vec::new();
        seen[start] = true;
        q.push_back(start);
        while let Some(i) = q.pop_front() {
            comp.push(i);
            let (x, y) = (i as i32 % w, i as i32 / w);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if !seen[j] && img.get_pixel(nx as u32, ny as u32).0[3] > 16 {
                    seen[j] = true;
                    q.push_back(j);
                }
            }
        }
        comps.push(comp);
    }
    let Some(largest) = comps.iter().map(Vec::len).max() else { return };
    for comp in comps {
        if comp.len() * 8 < largest {
            for i in comp {
                let p = img.get_pixel_mut((i as i32 % w) as u32, (i as i32 / w) as u32);
                p.0[3] = 0;
            }
        }
    }
}

/// Removes the connected background, smooths the edge and trims the result. Returns the cutout
/// plus a rough quality score, used to pick the best search hit.
fn cutout(img: image::RgbaImage, src: &FoundImage) -> Option<(image::RgbaImage, f32)> {
    let max = 384u32;
    let mut img = if img.width() > max || img.height() > max {
        let s = max as f64 / img.width().max(img.height()) as f64;
        image::imageops::resize(
            &img,
            ((img.width() as f64 * s).round() as u32).max(1),
            ((img.height() as f64 * s).round() as u32).max(1),
            FilterType::Lanczos3,
        )
    } else {
        img
    };
    let (w, h) = (img.width() as i32, img.height() as i32);
    if w < 80 || h < 60 {
        return None;
    }
    let total = (w * h) as usize;
    let transparent = img.pixels().filter(|p| p.0[3] < 250).count();
    let had_alpha = transparent * 100 / total >= 2;
    let mut coverage = 1.0 - transparent as f32 / total as f32;
    if had_alpha {
        trim_alpha(&mut img)?;
    } else {
        let (bg, uniform) = border_color(&img);
        if uniform < 0.18 {
            return None;
        }
        let lum = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
        let tol = if lum > 170.0 { 44.0 } else { 58.0 };
        let mut removed = vec![false; total];
        let mut q = std::collections::VecDeque::new();
        let close = |p: [u8; 3]| color_dist(p, bg) <= tol;
        let push = |i: usize, removed: &mut Vec<bool>, q: &mut std::collections::VecDeque<usize>| {
            if !removed[i] {
                let (x, y) = ((i as i32 % w) as u32, (i as i32 / w) as u32);
                if close(img.get_pixel(x, y).0[..3].try_into().unwrap()) {
                    removed[i] = true;
                    q.push_back(i);
                }
            }
        };
        for x in 0..w {
            push(x as usize, &mut removed, &mut q);
            push(((h - 1) * w + x) as usize, &mut removed, &mut q);
        }
        for y in 0..h {
            push((y * w) as usize, &mut removed, &mut q);
            push((y * w + w - 1) as usize, &mut removed, &mut q);
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                push((ny * w + nx) as usize, &mut removed, &mut q);
            }
        }
        let removed_n = removed.iter().filter(|r| **r).count();
        coverage = 1.0 - removed_n as f32 / total as f32;
        if !(0.08..=0.92).contains(&coverage) {
            return None;
        }
        let mut alpha = vec![0u8; total];
        for i in 0..total {
            if removed[i] {
                continue;
            }
            alpha[i] = 255;
        }
        // Soften the matte: edge pixels close to the background become partly transparent.
        for i in 0..total {
            if removed[i] {
                continue;
            }
            let (x, y) = (i as i32 % w, i as i32 / w);
            let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                .iter()
                .any(|(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0 && ny >= 0 && nx < w && ny < h && removed[(ny * w + nx) as usize]
                });
            if !edge {
                continue;
            }
            let p = img.get_pixel(x as u32, y as u32).0;
            let d = color_dist([p[0], p[1], p[2]], bg);
            alpha[i] = (((d - tol * 0.35) / (tol * 0.9)).clamp(0.0, 1.0) * 255.0) as u8;
        }
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if alpha[i] == 0 || alpha[i] == 255 {
                    continue;
                }
                let mut sum = 0u32;
                let mut n = 0u32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < w && ny < h {
                            sum += alpha[(ny * w + nx) as usize] as u32;
                            n += 1;
                        }
                    }
                }
                alpha[i] = (sum / n) as u8;
            }
        }
        for (i, p) in img.pixels_mut().enumerate() {
            p.0[3] = alpha[i];
        }
    }
    keep_main_silhouette(&mut img);
    trim_alpha(&mut img)?;
    let (cw, ch) = (img.width() as f32, img.height() as f32);
    let aspect = (cw / ch).max(0.01);
    let ar_penalty = (aspect.ln().abs() * 9.0).min(32.0);
    let coverage_bonus = if (0.12..=0.85).contains(&coverage) { 16.0 } else { 0.0 };
    let light_bg = if had_alpha {
        0.0
    } else {
        let (bg, _) = border_color(&img);
        let lum = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
        if lum > 160.0 { 10.0 } else { 0.0 }
    };
    let alpha_bonus = if had_alpha { 22.0 } else { 0.0 };
    let res_bonus = ((src.w.min(src.h) as f32 / 220.0).min(3.0)) * 4.0;
    let score = 40.0 + coverage_bonus + light_bg + alpha_bonus + res_bonus - ar_penalty;
    Some((img, score))
}

/// Opens the Windows file picker and returns the selected PNG/ICO/BMP/JPG.
pub fn choose_file(owner: HWND) -> Option<PathBuf> {
    let filter: Vec<u16> = "Images (*.png;*.ico;*.bmp;*.jpg;*.jpeg)\0*.png;*.ico;*.bmp;*.jpg;*.jpeg\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let mut file = vec![0u16; 1024];
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn).as_bool() } {
        return None;
    }
    let s = String::from_utf16_lossy(&file);
    let s = s.split('\0').next().unwrap_or("").to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yandex_results_are_parsed() {
        let page = r#"x &quot;width&quot;:1600,&quot;height&quot;:1200,&quot;origWidth&quot;:1600,&quot;origHeight&quot;:1200,&quot;origUrl&quot;:&quot;https://example.com/mouse.png&quot; y"#;
        let found = parse_found_images(page);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, "https://example.com/mouse.png");
        assert_eq!((found[0].w, found[0].h), (1600, 1200));
    }

    #[test]
    fn white_background_is_removed() {
        let mut img = image::RgbaImage::from_pixel(120, 100, image::Rgba([255, 255, 255, 255]));
        for y in 30..70 {
            for x in 40..80 {
                img.put_pixel(x, y, image::Rgba([180, 40, 40, 255]));
            }
        }
        let src = FoundImage { url: String::new(), w: 120, h: 100 };
        let (cut, _) = cutout(img, &src).expect("cutout");
        assert!(cut.get_pixel(0, 0).0[3] == 0);
        assert!(cut.get_pixel(cut.width() / 2, cut.height() / 2).0[3] > 200);
    }

    #[test]
    #[ignore]
    fn network_device_cutout() {
        for name in ["AJAZZ AJ159 APEX", "DualSense", "Dark Project HS5 V2"] {
            let bytes = fetch_by_name(name).expect("device image");
            let path = std::env::temp_dir().join(format!("opencode-cutout-{}.png", safe(name)));
            std::fs::write(&path, &bytes).unwrap();
            let img = image::load_from_memory(&bytes).unwrap().into_rgba8();
            assert!(img.pixels().any(|p| p.0[3] < 250));
            eprintln!("{name}: {}x{} -> {}", img.width(), img.height(), path.display());
        }
    }
}
