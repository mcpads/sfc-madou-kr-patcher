mod cli;
mod encoding;
mod font_gen;
mod patch;
mod rom;
mod text;
mod textbox;
mod verify;

use std::path::PathBuf;
use std::process;

use cli::{resolve_default, usage, Args};

fn cmd_info(args: &Args) {
    let rom_path = args.require_path("--rom");
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    rom::print_info(&data);
}

fn cmd_decode(args: &Args) {
    let rom_path = args.require_path("--rom");
    let show_all = args.flag("--all");
    let dump_tsv = args.flag("--dump-tsv");
    let dump_json_dir = args.path("--dump-json");
    let chunk_size: usize = args
        .value("--chunk-size")
        .map(|v| v.parse().expect("--chunk-size must be a number"))
        .unwrap_or(48);
    let all_banks = args.flag("--all-banks");
    let label = args.value("--label");
    let bank_str = args.value("--bank");

    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    // Determine which configs to process
    let configs: Vec<&text::control::BankConfig> = if all_banks {
        text::control::KNOWN_BANKS.iter().collect()
    } else if let Some(lbl) = label {
        match text::control::find_by_label(lbl) {
            Some(cfg) => vec![cfg],
            None => {
                let labels: Vec<&str> =
                    text::control::KNOWN_BANKS.iter().map(|b| b.label).collect();
                eprintln!(
                    "Error: unknown label '{}'. Known: {}",
                    lbl,
                    labels.join(", ")
                );
                process::exit(1);
            }
        }
    } else if let Some(bs) = bank_str {
        let bank_id = u8::from_str_radix(bs, 16).unwrap_or_else(|_| {
            eprintln!("Error: invalid bank hex: {}", bs);
            process::exit(1);
        });
        let banks = text::control::find_banks_by_number(bank_id);
        if banks.is_empty() {
            let labels: Vec<&str> = text::control::KNOWN_BANKS.iter().map(|b| b.label).collect();
            eprintln!(
                "Error: unknown bank ${:02X}. Known labels: {}",
                bank_id,
                labels.join(", ")
            );
            process::exit(1);
        }
        banks
    } else {
        eprintln!("Error: --bank, --label, or --all-banks is required");
        usage();
        process::exit(1);
    };

    // JSON dump mode: extract all, group by bank, write chunked JSON files
    if let Some(json_dir) = dump_json_dir {
        let mut all_entries = Vec::new();
        for config in &configs {
            let strings = text::bank::extract_bank(&data, config);
            let category = text::bank::label_to_category(config.label);
            for s in strings {
                all_entries.push(text::bank::CategorizedString {
                    bank: s.bank,
                    snes_addr: s.snes_addr,
                    text: s.text,
                    category: category.to_string(),
                    unknowns: s.unknowns,
                });
            }
        }
        text::bank::dump_json_chunks(all_entries, &json_dir, chunk_size).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            process::exit(1);
        });
        return;
    }

    // Normal display / TSV mode
    let mut grand_total = 0usize;

    for config in &configs {
        if !dump_tsv {
            println!(
                "=== Bank ${:02X} [{}]: {} (${:04X}-${:04X}) ===\n",
                config.bank, config.label, config.description, config.start_addr, config.end_addr
            );
        }

        let strings = text::bank::extract_bank(&data, config);
        grand_total += strings.len();

        if dump_tsv {
            text::bank::print_tsv(&strings, config);
        } else {
            text::bank::print_bank(&strings, config, show_all);
            println!();
        }
    }

    if !dump_tsv && configs.len() > 1 {
        println!(
            "=== Grand Total: {} strings from {} ranges ===",
            grand_total,
            configs.len()
        );
    }
}

fn cmd_patch(args: &Args) {
    let rom_path = args.require_path("--rom");
    let output_path = args.require_path("--output");

    // Resolve paths with defaults from assets/ directory
    let font_fixed_path = args
        .path("--font-fixed")
        .or_else(|| resolve_default("assets/font_16x16/ko_fixed.bin"));
    let font_16x16_path = args
        .path("--font-16x16")
        .or_else(|| resolve_default("assets/font_16x16/ko_font.bin"));
    let translations_dir = args
        .path("--translations-dir")
        .or_else(|| resolve_default("translations"));
    let ko_encoding_path = args
        .path("--ko-encoding")
        .or_else(|| resolve_default("assets/font_16x16/ko_encoding.tsv"));
    let encyclopedia_tsv_path = args
        .path("--encyclopedia-tsv")
        .or_else(|| resolve_default("translations/encyclopedia.tsv"));
    let code_patches_tsv_path = args
        .path("--code-patches-tsv")
        .or_else(|| resolve_default("translations/code_patches.tsv"));

    let ttf_path = args.path("--ttf");
    let ttf_size = args
        .value("--ttf-size")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(12.0);
    let charset_path = args
        .path("--charset")
        .or_else(|| resolve_default("translations/ko_charset.txt"));

    let worldmap_ttf_path = args.path("--worldmap-ttf");
    let worldmap_ttf_size = args
        .value("--worldmap-ttf-size")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0); // 0.0 = use default per font

    let cfg = patch::builder::PatchConfig {
        rom_path: &rom_path,
        output_path: &output_path,
        font_fixed_path,
        font_16x16_path,
        translations_dir,
        patch_all_text: args.flag("--text-all"),
        text_bank: args.value("--text-bank").map(String::from),
        text_relocate: args.flag("--relocate"),
        engine_hooks: args.flag("--engine-hooks"),
        ko_encoding_path,
        encyclopedia_tsv_path,
        code_patches_tsv_path,
        ttf_path,
        ttf_size,
        charset_path,
        worldmap_ttf_path,
        worldmap_ttf_size,
    };

    patch::builder::run_patch(&cfg).unwrap_or_else(|e| {
        eprintln!("Patch failed: {}", e);
        process::exit(1);
    });
}

fn cmd_verify(args: &Args) {
    let rom_path = args.require_path("--rom");
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    verify::verify_rom(&data);
}

fn cmd_pointers(args: &Args) {
    let rom_path = args.require_path("--rom");
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    let scan_bank = args
        .value("--bank")
        .and_then(|s| u8::from_str_radix(s, 16).ok())
        .unwrap_or(0x02);

    let target_bank = args
        .value("--target-bank")
        .and_then(|s| u8::from_str_radix(s, 16).ok());

    println!(
        "Scanning Bank ${:02X} for pointers{}...",
        scan_bank,
        target_bank
            .map(|b| format!(" → Bank ${:02X}", b))
            .unwrap_or_default()
    );

    let entries = patch::pointer::scan_pointers(&data, scan_bank, 0x8000, 0xFFFF, target_bank);
    patch::pointer::print_pointers(&entries);
}

fn cmd_ips(args: &Args) {
    let original_path = args.require_path("--original");
    let patched_path = args.require_path("--patched");
    let output_path = args.require_path("--output");

    let original = rom::load_rom(&original_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let patched = rom::load_rom(&patched_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    println!(
        "Original: {} ({} bytes)",
        original_path.display(),
        original.len()
    );
    println!(
        "Patched:  {} ({} bytes)",
        patched_path.display(),
        patched.len()
    );

    let ips_data = patch::ips::generate_ips(&original, &patched);
    let record_count = patch::ips::count_records(&ips_data);

    std::fs::write(&output_path, &ips_data).unwrap_or_else(|e| {
        eprintln!("Failed to write IPS: {}", e);
        process::exit(1);
    });

    println!(
        "IPS patch: {} ({} bytes, {} records)",
        output_path.display(),
        ips_data.len(),
        record_count
    );
}

fn cmd_bps(args: &Args) {
    let original_path = args.require_path("--original");
    let patched_path = args.require_path("--patched");
    let output_path = args.require_path("--output");

    let original = rom::load_rom(&original_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let patched = rom::load_rom(&patched_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    println!(
        "Original: {} ({} bytes)",
        original_path.display(),
        original.len()
    );
    println!(
        "Patched:  {} ({} bytes)",
        patched_path.display(),
        patched.len()
    );

    let bps_data = patch::bps::generate_bps(&original, &patched).unwrap_or_else(|e| {
        eprintln!("BPS generation failed: {}", e);
        process::exit(1);
    });

    std::fs::write(&output_path, &bps_data).unwrap_or_else(|e| {
        eprintln!("Failed to write BPS: {}", e);
        process::exit(1);
    });

    println!(
        "BPS patch: {} ({} bytes)",
        output_path.display(),
        bps_data.len()
    );
}

fn cmd_apply_bps(args: &Args) {
    let rom_path = args.require_path("--rom");
    let patch_path = args.require_path("--patch");
    let output_path = args.require_path("--output");

    let source = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let patch_data = std::fs::read(&patch_path).unwrap_or_else(|e| {
        eprintln!("Failed to read patch: {}", e);
        process::exit(1);
    });

    println!(
        "ROM:   {} ({} bytes)",
        rom_path.display(),
        source.len()
    );
    println!(
        "Patch: {} ({} bytes)",
        patch_path.display(),
        patch_data.len()
    );

    let result = patch::bps::apply_bps(&source, &patch_data).unwrap_or_else(|e| {
        eprintln!("BPS apply failed: {}", e);
        process::exit(1);
    });

    std::fs::write(&output_path, &result).unwrap_or_else(|e| {
        eprintln!("Failed to write output: {}", e);
        process::exit(1);
    });

    println!(
        "Output: {} ({} bytes)",
        output_path.display(),
        result.len()
    );
}

fn cmd_lookup(args: &Args) {
    let hex_input = args.value("--hex");
    let jp_input = args.value("--jp");
    let ko_input = args.value("--ko");

    if hex_input.is_none() && jp_input.is_none() && ko_input.is_none() {
        eprintln!("Error: one of --hex, --jp, or --ko is required");
        usage();
        process::exit(1);
    }

    // Load KO encoding table (optional for --hex with JP-only output, required for KO)
    let ko_encoding_path = args
        .path("--ko-encoding")
        .or_else(|| resolve_default("assets/font_16x16/ko_encoding.tsv"));

    let ko_table = if let Some(ref path) = ko_encoding_path {
        match encoding::ko::load_ko_encoding(path) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("Warning: could not load KO encoding: {}", e);
                None
            }
        }
    } else {
        None
    };

    let empty_table = std::collections::HashMap::new();
    let ko_ref = ko_table.as_ref().unwrap_or(&empty_table);

    if let Some(hex) = hex_input {
        let bytes = encoding::lookup::parse_hex_string(hex).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            process::exit(1);
        });

        println!("--- Hex decode ---");
        println!("JP text: {}", encoding::lookup::bytes_to_jp(&bytes));
        if ko_table.is_some() {
            println!("KO text: {}", encoding::lookup::bytes_to_ko(&bytes, ko_ref));
        }
        println!();
        encoding::lookup::print_lookup_table(&bytes, ko_ref);
    }

    if let Some(jp_str) = jp_input {
        let ch = jp_str.chars().next().unwrap_or_else(|| {
            eprintln!("Error: --jp requires a character");
            process::exit(1);
        });
        println!("--- JP lookup ---");
        encoding::lookup::lookup_jp_char(ch, ko_ref);
    }

    if let Some(ko_str) = ko_input {
        let ch = ko_str.chars().next().unwrap_or_else(|| {
            eprintln!("Error: --ko requires a character");
            process::exit(1);
        });
        if ko_table.is_none() {
            eprintln!("Error: --ko-encoding is required for KO lookup");
            eprintln!("  Default path: assets/font_16x16/ko_encoding.tsv");
            process::exit(1);
        }
        println!("--- KO lookup ---");
        encoding::lookup::lookup_ko_char(ch, ko_ref);
    }
}

fn cmd_generate_font(args: &Args) {
    let ttf_path = args.require_path("--ttf");
    let ttf_size = args
        .value("--ttf-size")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(12.0);
    let charset_path = args
        .path("--charset")
        .or_else(|| resolve_default("translations/ko_charset.txt"));
    let translations_dir = args
        .path("--translations-dir")
        .or_else(|| resolve_default("translations"));
    let out_font = args
        .path("--out-font")
        .unwrap_or_else(|| PathBuf::from("assets/font_16x16/ko_font.bin"));
    let out_fixed = args
        .path("--out-fixed")
        .unwrap_or_else(|| PathBuf::from("assets/font_16x16/ko_fixed.bin"));
    let out_encoding = args
        .path("--out-encoding")
        .unwrap_or_else(|| PathBuf::from("assets/font_16x16/ko_encoding.tsv"));

    let ttf_data = std::fs::read(&ttf_path).unwrap_or_else(|e| {
        eprintln!("Failed to read TTF: {}", e);
        process::exit(1);
    });

    let chars = if let Some(ref cp) = charset_path {
        font_gen::load_charset(cp).unwrap_or_else(|e| {
            eprintln!("Failed to load charset: {}", e);
            process::exit(1);
        })
    } else if let Some(ref td) = translations_dir {
        patch::builder::auto_collect_charset(td).unwrap_or_else(|e| {
            eprintln!("Failed to auto-collect charset: {}", e);
            process::exit(1);
        })
    } else {
        eprintln!("Error: --charset or --translations-dir required for generate-font");
        process::exit(1);
    };

    println!(
        "Generating font from {} ({} chars, size {})",
        ttf_path.display(),
        chars.len(),
        ttf_size
    );

    let result = font_gen::generate_font(&ttf_data, ttf_size, &chars).unwrap_or_else(|e| {
        eprintln!("Font generation failed: {}", e);
        process::exit(1);
    });

    // Write output files
    if let Some(parent) = out_font.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out_font, &result.font_data).unwrap_or_else(|e| {
        eprintln!("Failed to write font: {}", e);
        process::exit(1);
    });
    println!(
        "  Written: {} ({} bytes)",
        out_font.display(),
        result.font_data.len()
    );

    if let Some(parent) = out_fixed.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out_fixed, &result.fixed_data).unwrap_or_else(|e| {
        eprintln!("Failed to write fixed font: {}", e);
        process::exit(1);
    });
    println!(
        "  Written: {} ({} bytes)",
        out_fixed.display(),
        result.fixed_data.len()
    );

    if let Some(parent) = out_encoding.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    font_gen::write_encoding_tsv(&out_encoding, &result.encoding, &chars).unwrap_or_else(|e| {
        eprintln!("Failed to write encoding TSV: {}", e);
        process::exit(1);
    });
    println!("  Written: {}", out_encoding.display());
}

fn cmd_convert_translations(args: &Args) {
    let translations_dir = args
        .path("--translations-dir")
        .or_else(|| resolve_default("translations"))
        .unwrap_or_else(|| {
            eprintln!("Error: --translations-dir is required");
            usage();
            process::exit(1);
        });
    let chunk_size: usize = args
        .value("--chunk-size")
        .and_then(|s| s.parse().ok())
        .unwrap_or(48);

    println!("Converting TSV → JSON (chunk size: {})", chunk_size);
    println!("  Directory: {}", translations_dir.display());

    patch::translation_convert::convert_all(&translations_dir, chunk_size).unwrap_or_else(|e| {
        eprintln!("Conversion failed: {}", e);
        process::exit(1);
    });

    println!("\nConversion complete.");
}

fn main() {
    let args = Args::new();

    match args.command() {
        Some("info") => cmd_info(&args),
        Some("decode") => cmd_decode(&args),
        Some("patch") => cmd_patch(&args),
        Some("verify") => cmd_verify(&args),
        Some("pointers") => cmd_pointers(&args),
        Some("ips") => cmd_ips(&args),
        Some("bps") => cmd_bps(&args),
        Some("apply-bps") => cmd_apply_bps(&args),
        Some("lookup") => cmd_lookup(&args),
        Some("generate-font") => cmd_generate_font(&args),
        Some("convert-translations") => cmd_convert_translations(&args),
        _ => {
            usage();
            process::exit(1);
        }
    }
}
