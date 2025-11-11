use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use gameboy_core::Gameboy;
use serde::Serialize;

pub fn parse_number_u16(input: &str) -> Result<u16, String> {
    if let Some(stripped) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u16::from_str_radix(stripped, 16).map_err(|e| e.to_string())
    } else {
        input.parse::<u16>().map_err(|e| e.to_string())
    }
}

pub fn parse_number_usize(input: &str) -> Result<usize, String> {
    if let Some(stripped) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        usize::from_str_radix(stripped, 16).map_err(|e| e.to_string())
    } else {
        input.parse::<usize>().map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct MemoryRange {
    pub start: u16,
    pub length: usize,
}

impl MemoryRange {
    pub fn capture(&self, gameboy: &Gameboy) -> Vec<u8> {
        let mut buf = vec![0u8; self.length];
        gameboy.peek_block(self.start, &mut buf);
        buf
    }
}

pub fn parse_range_arg(arg: &str) -> Result<MemoryRange, String> {
    let mut parts = arg.split(':');
    let start = parts
        .next()
        .ok_or_else(|| "expected START:LEN".to_string())
        .and_then(parse_number_u16)?;
    let length_str = parts
        .next()
        .ok_or_else(|| "expected START:LEN".to_string())?;
    if parts.next().is_some() {
        return Err("too many components; expected START:LEN".into());
    }
    let length = parse_number_usize(length_str)?;
    if length == 0 {
        return Err("length must be greater than zero".into());
    }
    Ok(MemoryRange { start, length })
}

#[derive(Clone, Debug)]
pub struct WatchSpec {
    pub name: String,
    pub start: u16,
    pub length: usize,
}

pub fn parse_watch_spec(arg: &str) -> Result<WatchSpec, String> {
    let mut parts = arg.split(':');
    let name = parts
        .next()
        .ok_or_else(|| "expected NAME:START:LEN".to_string())?
        .to_string();
    if name.trim().is_empty() {
        return Err("watch name cannot be empty".into());
    }
    let start = parts
        .next()
        .ok_or_else(|| "expected NAME:START:LEN".to_string())
        .and_then(parse_number_u16)?;
    let length = parts
        .next()
        .ok_or_else(|| "expected NAME:START:LEN".to_string())
        .and_then(parse_number_usize)?;
    if parts.next().is_some() {
        return Err("too many components; expected NAME:START:LEN".into());
    }
    if length == 0 {
        return Err("watch length must be greater than zero".into());
    }
    Ok(WatchSpec {
        name,
        start,
        length,
    })
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum DumpFormat {
    Hex,
    Binary,
}

pub fn write_dump(
    range: &MemoryRange,
    data: &[u8],
    format: DumpFormat,
    output: Option<&Path>,
) -> Result<()> {
    match (format, output) {
        (DumpFormat::Binary, Some(path)) => {
            let mut file = BufWriter::new(
                File::create(path).with_context(|| format!("failed to create {:?}", path))?,
            );
            file.write_all(data)
                .with_context(|| format!("failed writing {:?}", path))?
        }
        (DumpFormat::Binary, None) => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(data)?;
        }
        (DumpFormat::Hex, dest) => {
            let mut writer: Box<dyn Write> = match dest {
                Some(path) => Box::new(BufWriter::new(
                    File::create(path).with_context(|| format!("failed to create {:?}", path))?,
                )),
                None => Box::new(io::stdout().lock()),
            };
            writeln!(
                writer,
                "Dumping {} bytes from 0x{start:04X}",
                data.len(),
                start = range.start
            )?;
            for (offset, chunk) in data.chunks(16).enumerate() {
                write!(writer, "{:#06X}: ", range.start as usize + offset * 16)?;
                for byte in chunk {
                    write!(writer, "{:02X} ", byte)?;
                }
                writeln!(writer)?;
            }
        }
    }
    Ok(())
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum WatchFormat {
    Json,
}

pub enum WatchOutput {
    Stdout,
    File(PathBuf),
}

pub struct AutomationRecorder {
    specs: Vec<WatchSpec>,
    format: WatchFormat,
    writer: Box<dyn Write>,
}

impl AutomationRecorder {
    pub fn new(specs: Vec<WatchSpec>, format: WatchFormat, output: WatchOutput) -> Result<Self> {
        if specs.is_empty() {
            return Err(anyhow!("at least one watch must be specified"));
        }
        let writer: Box<dyn Write> = match output {
            WatchOutput::Stdout => Box::new(io::stdout().lock()),
            WatchOutput::File(path) => {
                let file =
                    File::create(&path).with_context(|| format!("failed to create {:?}", path))?;
                Box::new(BufWriter::new(file))
            }
        };
        Ok(Self {
            specs,
            format,
            writer,
        })
    }

    pub fn record(&mut self, frame: u64, gameboy: &Gameboy) -> Result<()> {
        match self.format {
            WatchFormat::Json => self.record_json(frame, gameboy),
        }
    }

    fn record_json(&mut self, frame: u64, gameboy: &Gameboy) -> Result<()> {
        let mut watches = Vec::with_capacity(self.specs.len());
        for spec in &self.specs {
            let mut buffer = vec![0u8; spec.length];
            gameboy.peek_block(spec.start, &mut buffer);
            watches.push(JsonWatch {
                name: &spec.name,
                start: spec.start,
                data_hex: encode_hex(&buffer),
            });
        }
        let payload = JsonSnapshot { frame, watches };
        let json = serde_json::to_string(&payload)?;
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()?;
        Ok(())
    }
}

#[derive(Serialize)]
struct JsonSnapshot<'a> {
    frame: u64,
    watches: Vec<JsonWatch<'a>>,
}

#[derive(Serialize)]
struct JsonWatch<'a> {
    name: &'a str,
    start: u16,
    data_hex: String,
}

fn encode_hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{:02X}", byte)).collect()
}
