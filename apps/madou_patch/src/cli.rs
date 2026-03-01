//! CLI argument parsing for madou_patch.

use std::path::PathBuf;

pub fn usage() {
    eprintln!(
        "Usage: madou_patch <command> [options]

Commands:
  info      --rom <path>
            Show ROM information (size, header, mapper).

  decode    --rom <path> [--bank <XX>] [--label <name>] [--all-banks]
            [--all] [--dump-tsv] [--dump-json <dir>] [--chunk-size <N>]
            Decode and display text from a ROM bank.
            --bank: hex bank number (01, 03, 08, 09, 0A, 1D, 2A, 2B, 2D)
            --label: bank label (01, 01_monster, 01_save, 01_hp, 03, 08, 09, 0A, 1D, 2A, 2B, 2D)
            --all-banks: extract all 12 bank ranges
            --all: show all strings including partially decoded
            --dump-tsv: output in TSV format
            --dump-json: dump as chunked JSON files to <dir>
            --chunk-size: entries per JSON chunk (default: 48)

  patch     --rom <path> --output <path>
            [--font-fixed <path>] [--font-16x16 <path>]
            [--ttf <path>] [--ttf-size <N>] [--charset <path>]
            [--savemenu-ttf <path>] [--savemenu-ttf-size <N>]
            [--worldmap-ttf <path>] [--worldmap-ttf-size <N>]
            [--text-all] [--text-bank <XX>] [--translations-dir <path>] [--relocate]
            [--engine-hooks]
            Build a patched ROM with Korean fonts and text.
            --font-fixed: fixed-encode tiles (char $00-$0F, 1024 bytes)
            --ttf: TTF font path (generates font tiles at build time, replaces --font-*)
            --ttf-size: TTF rasterization size (default: 12)
            --savemenu-ttf: separate TTF for save menu UI (defaults to --ttf)
            --savemenu-ttf-size: save menu TTF size (default: 12)
            --worldmap-ttf: 8x8 TTF for worldmap place names (default: dalmoori.ttf)
            --worldmap-ttf-size: worldmap TTF size (default: 8)
            --charset: charset file path (default: translations/ko_charset.txt)
            --relocate: relocate overflow strings to free banks
            --engine-hooks: apply FA/F0 prefix engine hooks (Bank $32)

  generate-font --ttf <path> [--ttf-size <N>] [--charset <path>]
            [--out-font <path>] [--out-fixed <path>] [--out-encoding <path>]
            Generate Korean font tiles from TTF.

  verify    --rom <path>
            Check all text strings for text box overflow.

  pointers  --rom <path> [--bank <XX>] [--target-bank <XX>]
            Scan and dump pointer table entries.

  ips       --original <path> --patched <path> --output <path>
            Generate an IPS patch from two ROMs.

  bps       --original <path> --patched <path> --output <path>
            Generate a BPS patch from two ROMs.

  apply-bps --rom <path> --patch <path> --output <path>
            Apply a BPS patch to a ROM.

  lookup    [--hex \"FB 67 30 53\"] [--jp <char>] [--ko <char>]
            [--ko-encoding <path>]
            Encoding lookup: convert between hex bytes, JP, and KO characters.
            --hex: decode hex byte string and show JP/KO table
            --jp: look up a JP character (show bytes + KO equivalent)
            --ko: look up a KO character (show bytes + JP equivalent)
            --ko-encoding: path to ko_encoding.tsv (default: assets/font_16x16/ko_encoding.tsv)

  convert-translations --translations-dir <path> [--chunk-size <N>]
            Convert TSV translation files to JSON format.
            --translations-dir: directory containing TSV files (default: translations)
            --chunk-size: entries per JSON chunk (default: 48)"
    );
}

/// Simple argument parser.
pub struct Args {
    args: Vec<String>,
}

impl Args {
    pub fn new() -> Self {
        Self {
            args: std::env::args().collect(),
        }
    }

    pub fn command(&self) -> Option<&str> {
        self.args.get(1).map(|s| s.as_str())
    }

    pub fn flag(&self, name: &str) -> bool {
        self.args.iter().any(|a| a == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        for i in 0..self.args.len() - 1 {
            if self.args[i] == name {
                return Some(&self.args[i + 1]);
            }
        }
        None
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.value(name).map(PathBuf::from)
    }

    pub fn require_path(&self, name: &str) -> PathBuf {
        self.path(name).unwrap_or_else(|| {
            eprintln!("Error: {} is required", name);
            usage();
            std::process::exit(1);
        })
    }
}

/// Resolve a default asset path (returns Some only if the file/dir exists).
pub fn resolve_default(path: &str) -> Option<PathBuf> {
    let p = PathBuf::from(path);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}
