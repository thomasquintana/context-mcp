//! Offline native extraction helper. Invoke only inside the configured sandbox.
//!
//! No network, shell interpolation, archive extraction paths or source execution.
//! The working directory is the single job mount. Native commands are fixed argv.

use quick_xml::{Reader, events::Event};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 1000;

fn bounded(path: impl AsRef<Path>, max: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err("source expansion limit".into());
    }
    Ok(bytes)
}
fn native(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return Err("native processing failed".into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
fn text(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[255, 254]) || bytes.starts_with(&[254, 255]) {
        if !bytes.len().is_multiple_of(2) {
            return Err("invalid UTF16".into());
        }
        let little = bytes[0] == 255;
        Ok(String::from_utf16(
            &bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if little {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect::<Vec<_>>(),
        )?)
    } else {
        Ok(std::str::from_utf8(bytes)?
            .trim_start_matches('\u{feff}')
            .to_owned())
    }
}
fn markup(input: &str) -> Result<String> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().check_end_names = false;
    let mut output = String::new();
    let mut hidden = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::DocType(value)) => {
                if !value.decode()?.trim().eq_ignore_ascii_case("html") {
                    return Err("document types are not accepted".into());
                }
            }
            Ok(Event::Start(tag)) => {
                if matches!(tag.local_name().as_ref(), b"script" | b"style") {
                    hidden += 1;
                }
            }
            Ok(Event::End(tag)) => {
                let name = tag.local_name();
                if matches!(name.as_ref(), b"script" | b"style") {
                    hidden = hidden.saturating_sub(1);
                }
                if hidden == 0
                    && matches!(
                        name.as_ref(),
                        b"p" | b"div" | b"tr" | b"t" | b"h1" | b"h2" | b"li" | b"si" | b"v"
                    )
                {
                    output.push('\n');
                }
            }
            Ok(Event::Text(value)) if hidden == 0 => {
                let value = value.decode()?;
                output.push_str(&quick_xml::escape::unescape(&value.replace("&nbsp;", " "))?);
            }
            Ok(Event::GeneralRef(value)) if hidden == 0 => {
                let name = value.decode()?;
                if name == "nbsp" {
                    output.push(' ');
                } else {
                    output.push_str(&quick_xml::escape::unescape(&format!("&{name};"))?);
                }
            }
            Ok(Event::CData(value)) if hidden == 0 => output.push_str(&value.decode()?),
            Ok(_) => {}
            Err(_) => return Err("invalid markup".into()),
        }
    }
    Ok(output)
}
fn add_text(out: &mut Vec<Value>, value: String, location: Value) -> Result<()> {
    let mut rest = value.as_str();
    let mut offset = 0;
    while !rest.is_empty() {
        let mut end = rest.len().min(128 * 1024);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let part = &rest[..end];
        if !part.trim().is_empty() {
            let location = if location["kind"] == "bytes" {
                json!({"kind":"bytes","start":offset,"end":offset+end})
            } else {
                location.clone()
            };
            out.push(json!({"kind":"text","text":part,"location":location}));
        }
        offset += end;
        rest = &rest[end..];
        if out.len() > MAX_FILES {
            return Err("segment limit".into());
        }
    }
    Ok(())
}
fn file_segment(
    out: &mut Vec<Value>,
    file: &str,
    kind: &str,
    media: &str,
    location: Value,
) -> Result<()> {
    let bytes = bounded(file, 2 * 1024 * 1024)?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    out.push(json!({"kind":kind,"file":file,"sha256":hash,"media_type":media,"location":location}));
    if out.len() > MAX_FILES {
        return Err("segment limit".into());
    }
    Ok(())
}

fn raster(out: &mut Vec<Value>, input: &str, output: &str, location: Value) -> Result<()> {
    native(
        "ffmpeg",
        &[
            "-y",
            "-v",
            "error",
            "-i",
            input,
            "-frames:v",
            "1",
            "-vf",
            "scale=1280:1280:force_original_aspect_ratio=decrease",
            output,
        ],
    )?;
    file_segment(out, output, "image", "image/jpeg", location)
}

fn verified_image_type(media: &str) -> Result<()> {
    let mut header = Vec::new();
    fs::File::open("source.input")?
        .take(16)
        .read_to_end(&mut header)?;
    let actual = if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if header.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        "image/gif"
    } else if header.starts_with(b"II*\0") || header.starts_with(b"MM\0*") {
        "image/tiff"
    } else if header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return Err("unsupported image signature".into());
    };
    if media != actual {
        return Err("declared image type mismatch".into());
    }
    Ok(())
}

fn archive(media: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let file = fs::File::open("source.input")?;
    let mut total = 0_u64;
    let mut files = Vec::new();
    if media == "application/zip" || media.ends_with("+zip") || media.contains("officedocument") {
        let mut zip = zip::ZipArchive::new(file)?;
        if zip.len() > MAX_FILES {
            return Err("archive file limit".into());
        }
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            if entry.enclosed_name().is_none()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("unsafe archive member".into());
            }
            total = total.checked_add(entry.size()).ok_or("archive size")?;
            if total > MAX_BYTES {
                return Err("archive expansion limit".into());
            }
            let mut bytes = Vec::new();
            entry.by_ref().take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != entry.size() {
                return Err("archive size mismatch".into());
            }
            files.push((entry.name()?.into_owned(), bytes));
        }
    } else {
        let source: Box<dyn Read> = if media == "application/gzip" || media == "application/x-gzip"
        {
            Box::new(flate2::read::GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        let mut tar = tar::Archive::new(source.take(MAX_BYTES + 1024 * 1024));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.header().entry_type().is_dir() {
                continue;
            }
            if !entry.header().entry_type().is_file() {
                return Err("unsafe archive member".into());
            }
            let path = entry.path()?.into_owned();
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err("unsafe archive path".into());
            }
            total = total.checked_add(entry.size()).ok_or("archive size")?;
            if total > MAX_BYTES || files.len() >= MAX_FILES {
                return Err("archive expansion limit".into());
            }
            let mut bytes = Vec::new();
            entry.by_ref().take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            files.push((path.to_string_lossy().into_owned(), bytes));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}
fn zipped(media: &str) -> Result<Vec<Value>> {
    let files = archive(media)?;
    let mut out = Vec::new();
    let mut strings = Vec::new();
    if let Some((_, bytes)) = files.iter().find(|(n, _)| n == "xl/sharedStrings.xml") {
        let xml = text(bytes)?;
        let mut reader = Reader::from_str(&xml);
        let mut current = String::new();
        loop {
            match reader.read_event()? {
                Event::Eof => break,
                Event::DocType(_) => return Err("document type".into()),
                Event::Start(tag) if tag.local_name().as_ref() == b"si" => current.clear(),
                Event::Text(t) => current.push_str(&quick_xml::escape::unescape(&t.decode()?)?),
                Event::End(tag) if tag.local_name().as_ref() == b"si" => {
                    strings.push(current.clone())
                }
                _ => {}
            }
        }
    }
    for (name, bytes) in &files {
        if (name.starts_with("word/media/")
            || name.starts_with("ppt/media/")
            || name.starts_with("xl/media/")
            || media == "application/epub+zip")
            && [".png", ".jpg", ".jpeg", ".webp", ".gif", ".tif", ".tiff"]
                .iter()
                .any(|extension| name.to_ascii_lowercase().ends_with(extension))
        {
            let index = out.len();
            let input = format!("embedded-{index}.input");
            let output = format!("embedded-{index}.jpg");
            fs::write(&input, bytes)?;
            raster(
                &mut out,
                &input,
                &output,
                json!({"kind":"file","path":name}),
            )?;
            continue;
        }
        let selected = if media.contains("wordprocessingml") {
            name == "word/document.xml"
                || name.starts_with("word/header")
                || name.starts_with("word/footer")
        } else if media.contains("presentationml") {
            name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
        } else if media.contains("spreadsheetml") {
            name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml")
        } else if media == "application/epub+zip" {
            name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm")
        } else {
            !name.split('/').any(|p| p.starts_with('.')) && !bytes.contains(&0)
        };
        if !selected {
            continue;
        }
        let value = if media.contains("spreadsheetml") {
            let xml = text(bytes)?;
            let mut reader = Reader::from_str(&xml);
            let mut shared = false;
            let mut value = false;
            let mut result = String::new();
            loop {
                match reader.read_event()? {
                    Event::Eof => break,
                    Event::DocType(_) => return Err("document type".into()),
                    Event::Start(tag) if tag.local_name().as_ref() == b"c" => {
                        shared = tag
                            .attributes()
                            .filter_map(|a| a.ok())
                            .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"s");
                    }
                    Event::Start(tag) if matches!(tag.local_name().as_ref(), b"v" | b"t") => {
                        value = true
                    }
                    Event::Text(t) if value => {
                        let t = t.decode()?;
                        if shared {
                            let index = t.parse::<usize>()?;
                            result.push_str(strings.get(index).ok_or("shared string index")?);
                        } else {
                            result.push_str(&t);
                        }
                        result.push('\t');
                    }
                    Event::End(tag) => {
                        if tag.local_name().as_ref() == b"row" {
                            result.push('\n');
                        }
                        value = false;
                    }
                    _ => {}
                }
            }
            result
        } else if name.ends_with(".xml")
            || name.ends_with(".xhtml")
            || name.ends_with(".html")
            || name.ends_with(".htm")
        {
            markup(&text(bytes)?)?
        } else {
            match text(bytes) {
                Ok(s) => format!("File: {name}\n{s}"),
                Err(_) => continue,
            }
        };
        add_text(&mut out, value, json!({"kind":"file","path":name}))?;
    }
    if out.is_empty() {
        return Err("archive contains no supported evidence".into());
    }
    Ok(out)
}
fn extract(media: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    if media == "application/pdf" {
        native("pdftotext", &["-layout", "source.input", "pages.txt"])?;
        let value = text(&bounded("pages.txt", MAX_BYTES)?)?;
        let pages = value.split('\u{c}').collect::<Vec<_>>();
        if pages.len() > MAX_FILES + 1 {
            return Err("page limit".into());
        }
        let count = if pages.last().is_some_and(|s| s.trim().is_empty()) {
            pages.len() - 1
        } else {
            pages.len()
        };
        for (index, page) in pages[..count].iter().enumerate() {
            if !page.trim().is_empty() {
                add_text(
                    &mut out,
                    (*page).into(),
                    json!({"kind":"pages","first":index+1,"last":index+1}),
                )?;
            } else {
                let page = (index + 1).to_string();
                let base = format!("scan-{page}");
                native(
                    "pdftoppm",
                    &[
                        "-scale-to",
                        "1280",
                        "-jpeg",
                        "-f",
                        &page,
                        "-l",
                        &page,
                        "-singlefile",
                        "source.input",
                        &base,
                    ],
                )?;
                file_segment(
                    &mut out,
                    &format!("{base}.jpg"),
                    "image",
                    "image/jpeg",
                    json!({"kind":"pages","first":index+1,"last":index+1}),
                )?;
            }
        }
        if out.is_empty() {
            native(
                "pdftoppm",
                &[
                    "-scale-to",
                    "1280",
                    "-jpeg",
                    "-f",
                    "1",
                    "-l",
                    "1000",
                    "source.input",
                    "scan",
                ],
            )?;
            let mut names = fs::read_dir(".")?
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with("scan-") && n.ends_with(".jpg"))
                .collect::<Vec<_>>();
            names.sort();
            for (index, name) in names.iter().enumerate() {
                file_segment(
                    &mut out,
                    name,
                    "image",
                    "image/jpeg",
                    json!({"kind":"pages","first":index+1,"last":index+1}),
                )?;
            }
        }
    } else if media.starts_with("image/") {
        verified_image_type(media)?;
        raster(
            &mut out,
            "source.input",
            "frame.jpg",
            json!({"kind":"asset"}),
        )?;
    } else if media.starts_with("audio/") || media.starts_with("video/") {
        let probe: Value = serde_json::from_str(&native(
            "ffprobe",
            &[
                "-v",
                "error",
                "-show_entries",
                "format=duration,format_name:stream=codec_type",
                "-of",
                "json",
                "source.input",
            ],
        )?)?;
        let container = probe["format"]["format_name"].as_str().ok_or("container")?;
        let valid = match media {
            "audio/mpeg" => container == "mp3",
            "audio/wav" | "audio/x-wav" => container == "wav",
            "audio/flac" => container == "flac",
            "audio/ogg" => container == "ogg",
            "audio/mp4" | "audio/x-m4a" | "video/mp4" | "video/quicktime" => {
                container.split(',').any(|c| c == "mov" || c == "mp4")
            }
            "video/webm" | "video/x-matroska" => {
                container.split(',').any(|c| c == "matroska" || c == "webm")
            }
            _ => false,
        };
        if !valid {
            return Err("declared media container mismatch".into());
        }
        let duration = probe["format"]["duration"]
            .as_str()
            .ok_or("duration")?
            .parse::<f64>()?;
        if !duration.is_finite() || duration <= 0.0 || duration > 3.0 * 3600.0 {
            return Err("media duration limit".into());
        }
        let audio = probe["streams"]
            .as_array()
            .ok_or("streams")?
            .iter()
            .any(|s| s["codec_type"] == "audio");
        if media.starts_with("video/") {
            let count = (duration / 10.0).ceil() as usize;
            if count + (duration / 30.0).ceil() as usize > MAX_FILES {
                return Err("media segment limit".into());
            }
            for index in 1..=count {
                let name = format!("video-{index:04}.jpg");
                native(
                    "ffmpeg",
                    &[
                        "-y",
                        "-v",
                        "error",
                        "-ss",
                        &((index - 1) * 10).to_string(),
                        "-i",
                        "source.input",
                        "-frames:v",
                        "1",
                        "-vf",
                        "scale=1024:1024:force_original_aspect_ratio=decrease",
                        &name,
                    ],
                )?;
                file_segment(
                    &mut out,
                    &name,
                    "image",
                    "image/jpeg",
                    json!({"kind":"time","start_ms":(index-1)*10000,"end_ms":((index*10000) as u64).min((duration*1000.0).ceil() as u64)}),
                )?;
            }
        }
        if audio {
            native(
                "ffmpeg",
                &[
                    "-y",
                    "-v",
                    "error",
                    "-i",
                    "source.input",
                    "-vn",
                    "-ac",
                    "1",
                    "-ar",
                    "16000",
                    "-f",
                    "segment",
                    "-segment_time",
                    "30",
                    "audio-%04d.wav",
                ],
            )?;
            for index in 0..(duration / 30.0).ceil() as usize {
                let name = format!("audio-{index:04}.wav");
                if Path::new(&name).exists() {
                    file_segment(
                        &mut out,
                        &name,
                        "audio",
                        "audio/wav",
                        json!({"kind":"time","start_ms":index*30000,"end_ms":(((index+1)*30000) as u64).min((duration*1000.0).ceil() as u64)}),
                    )?;
                }
            }
        }
    } else if media.contains("officedocument")
        || matches!(
            media,
            "application/epub+zip"
                | "application/zip"
                | "application/x-tar"
                | "application/gzip"
                | "application/x-gzip"
        )
    {
        out = zipped(media)?;
    } else {
        let bytes = bounded("source.input", MAX_BYTES)?;
        let utf16 = bytes.starts_with(&[255, 254]) || bytes.starts_with(&[254, 255]);
        let value = text(&bytes)?;
        if value.contains('\0') {
            return Err("binary data is not text".into());
        }
        if media == "application/json" {
            let _: Value = serde_json::from_str(&value)?;
        }
        let value = if media == "text/html" {
            markup(&value)?
        } else {
            value
        };
        let location = if media == "text/html" || utf16 {
            json!({"kind":"asset"})
        } else {
            json!({"kind":"bytes","start":0,"end":1})
        };
        add_text(&mut out, value, location)?;
    }
    if out.is_empty() {
        return Err("source has no extractable evidence".into());
    }
    Ok(out)
}
fn run() -> Result<()> {
    let request: Value = serde_json::from_slice(&bounded("request.json", 4 * 1024 * 1024)?)?;
    let media = request["asset"]["media_type"]
        .as_str()
        .ok_or("media type")?;
    let segments = if Path::new("manifest.json").exists() {
        serde_json::from_slice::<Vec<Value>>(&bounded("manifest.json", MAX_BYTES)?)?
    } else {
        let segments = extract(media)?;
        let bytes = serde_json::to_vec(&segments)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("manifest limit".into());
        }
        let mut file = fs::File::create("manifest.tmp")?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename("manifest.tmp", "manifest.json")?;
        segments
    };
    let start = request["cursor"].as_u64().unwrap_or(0) as usize;
    if start >= segments.len() {
        return Err("cursor".into());
    }
    let end = (start + 4).min(segments.len());
    println!(
        "{}",
        json!({"media_type":media,"segments":segments[start..end],"next_cursor":if end<segments.len(){json!(end)}else{Value::Null},"metadata":{"segments":segments.len()}})
    );
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("bounded source processing failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markup_excludes_executable_content_and_decodes_entities() {
        assert_eq!(
            markup("<p>Ocean &amp; sky</p><script>bad()</script><style>secret</style>")
                .unwrap()
                .trim(),
            "Ocean & sky"
        );
        assert!(markup("<!DOCTYPE x SYSTEM 'file:///etc/passwd'><p>secret</p>").is_err());
    }
    #[test]
    fn chunks_preserve_utf8_and_source_ranges() {
        let mut out = Vec::new();
        let text = "水".repeat(50000);
        add_text(&mut out, text.clone(), json!({"kind":"bytes"})).unwrap();
        assert_eq!(
            out.iter()
                .map(|s| s["text"].as_str().unwrap())
                .collect::<String>(),
            text
        );
        assert_eq!(out[1]["location"]["start"], out[0]["location"]["end"]);
    }
}
