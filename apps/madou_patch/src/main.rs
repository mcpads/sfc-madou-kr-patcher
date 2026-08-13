mod cli;
mod disasm;
mod encoding;
mod font_gen;
mod patch;
mod rom;
mod text;
mod textbox;
mod trace;
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
    let title_main_path = args
        .path("--title-main")
        .or_else(|| resolve_default("assets/title_concepts/components/madoujeongi_main_v3.png"));
    let title_hanamaru_path = args
        .path("--title-hanamaru")
        .or_else(|| resolve_default("assets/title_concepts/components/hanamaru_v2.png"));
    let title_subtitle_path = args
        .path("--title-subtitle")
        .or_else(|| resolve_default("assets/title_concepts/components/daeyuchiwona_v2.png"));

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
        title_main_path,
        title_hanamaru_path,
        title_subtitle_path,
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

    println!("ROM:   {} ({} bytes)", rom_path.display(), source.len());
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

    println!("Output: {} ({} bytes)", output_path.display(), result.len());
}

fn cmd_trace(args: &Args) {
    let rom_path = args.require_path("--rom");
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    let target_vram = args
        .value("--target-vram")
        .and_then(|s| u16::from_str_radix(s, 16).ok())
        .unwrap_or(0x5000);

    let max_inst = args
        .value("--max-inst")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(500_000);

    let max_nmi = args
        .value("--max-nmi")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(60);
    let screenshot_palette = args
        .value("--screenshot-pal")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let screenshot_cols = args
        .value("--screenshot-cols")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(64);

    // Backwards-compatible default is ON. Explicit --no-force-loop-break disables it.
    // If both are provided, --force-loop-break wins.
    let force_loop_break = args.flag("--force-loop-break") || !args.flag("--no-force-loop-break");

    let config = trace::TracerConfig {
        max_instructions: max_inst,
        target_vram,
        stop_on_target: !args.flag("--no-stop"),
        verbose: args.flag("--verbose"),
        inject_nmi: !args.flag("--no-nmi"),
        max_nmi,
        start_button: args.flag("--start-button"),
        start_interrupt: args.flag("--start-interrupt"),
        log_lz_calls: args.flag("--log-lz"),
        force_loop_break,
    };

    eprintln!(
        "Tracing {} (target VRAM=${:04X}, max={})",
        rom_path.display(),
        target_vram,
        max_inst,
    );

    let result = trace::run_trace(data, &config);

    println!("Stop reason: {:?}", result.stop_reason);
    println!("Instructions: {}", result.instructions_executed);
    println!("DMA transfers: {}", result.dma_records.len());
    println!("VRAM direct writes: {}", result.vram_write_records.len());
    println!(
        "Target hits (DMA VRAM ${:04X}): {}",
        target_vram,
        result.target_hits.len()
    );
    println!(
        "Target hits (direct VRAM ${:04X}): {}",
        target_vram,
        result.target_vram_write_hits.len()
    );
    println!("Trace events: {}", result.trace_events.len());
    println!(
        "Final PC: ${:02X}:${:04X}",
        result.final_pc.0, result.final_pc.1
    );

    if let Some(path) = args.path("--screenshot") {
        trace::capture::write_vram_tiles_ppm(
            &path,
            &result.final_vram,
            &result.final_cgram,
            screenshot_palette,
            screenshot_cols,
        )
        .unwrap_or_else(|e| {
            eprintln!("Error writing screenshot: {}", e);
            process::exit(1);
        });
        println!("Wrote screenshot to {}", path.display());
    }

    // Write event JSON if requested.
    if let Some(json_path) = args.path("--event-json") {
        let json = serde_json::to_string_pretty(&result.trace_events).unwrap_or_else(|e| {
            eprintln!("Error serializing trace events: {}", e);
            process::exit(1);
        });
        std::fs::write(&json_path, json).unwrap_or_else(|e| {
            eprintln!("Error writing {}: {}", json_path.display(), e);
            process::exit(1);
        });
        println!(
            "Wrote {} events to {}",
            result.trace_events.len(),
            json_path.display()
        );
    }
}

fn cmd_trace_scenario(args: &Args) {
    let scenario = args.value("--scenario").or_else(|| {
        // Positional: `trace scenario title-leak` → index 3
        args.args_ref().get(3).map(|s| s.as_str())
    });
    match scenario {
        Some("title-leak") => {
            trace::scenario::run_title_leak(args);
        }
        Some(other) => {
            eprintln!("Unknown scenario: {}", other);
            process::exit(1);
        }
        None => {
            eprintln!("Error: scenario name required (e.g. 'title-leak')");
            process::exit(1);
        }
    }
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

fn cmd_audit_translations(args: &Args) {
    let rom_path = args.require_path("--rom");
    let output_path = args.require_path("--output");
    let translations_dir = args
        .path("--translations-dir")
        .or_else(|| resolve_default("translations"))
        .unwrap_or_else(|| {
            eprintln!("Error: --translations-dir is required");
            process::exit(1);
        });
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    let summary =
        patch::translation_review::write_review_json(&data, &translations_dir, &output_path)
            .unwrap_or_else(|e| {
                eprintln!("Translation audit failed: {}", e);
                process::exit(1);
            });

    println!("JP-KR review written: {}", output_path.display());
    println!("  total entries: {}", summary.total_entries);
    println!("  bank entries: {}", summary.bank_entries);
    println!("  encyclopedia entries: {}", summary.encyclopedia_entries);
    println!("  code patch entries: {}", summary.code_patch_entries);
    println!("  ROM JP exact matches: {}", summary.rom_matches);
    println!("  derived subentries: {}", summary.derived_subentries);
    println!("  ROM JP mismatches: {}", summary.rom_mismatches);
    println!("  untranslated candidates: {}", summary.untranslated);
    println!("  tier A certain: {}", summary.tier_a_certain);
    println!("  tier B strong: {}", summary.tier_b_strong);
    println!("  tier C context: {}", summary.tier_c_context);
    println!("  tier D no signal: {}", summary.tier_d_no_signal);
}

fn cmd_audit_translation_growth(args: &Args) {
    let baseline_dir = args.require_path("--baseline-translations-dir");
    let output_path = args.require_path("--output");
    let translations_dir = args
        .path("--translations-dir")
        .or_else(|| resolve_default("translations"))
        .unwrap_or_else(|| {
            eprintln!("Error: --translations-dir is required");
            process::exit(1);
        });

    let summary = patch::translation_growth::write_growth_audit(
        &baseline_dir,
        &translations_dir,
        &output_path,
    )
    .unwrap_or_else(|e| {
        eprintln!("Translation growth audit failed: {}", e);
        process::exit(1);
    });

    println!(
        "Translation growth audit written: {}",
        output_path.display()
    );
    println!("  baseline entries: {}", summary.baseline_entries);
    println!("  current entries: {}", summary.current_entries);
    println!("  changed entries: {}", summary.changed_entries);
    println!("  new entries: {}", summary.new_entries);
    println!("  removed entries: {}", summary.removed_entries);
    println!("  growth candidates: {}", summary.growth_candidates);
    println!(
        "  crossed common 10-cell line (candidate signal): {}",
        summary.crossed_common_10_cell_line
    );
    println!("  confirmed fits: {}", summary.confirmed_fits);
    println!(
        "  confirmed new overflows: {}",
        summary.confirmed_new_overflows
    );
    println!(
        "  confirmed existing overflows: {}",
        summary.confirmed_existing_overflows
    );
}

fn cmd_disasm(args: &Args) {
    let rom_path = args.require_path("--rom");
    let start_str = args.value("--start").unwrap_or_else(|| {
        eprintln!("Error: --start is required (e.g. $00:CE9E)");
        usage();
        process::exit(1);
    });
    let length = args
        .value("--length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(64);

    disasm::run_disasm(&rom_path, start_str, length).unwrap_or_else(|e| {
        eprintln!("Disasm failed: {}", e);
        process::exit(1);
    });
}

fn cmd_extract_lz(args: &Args) {
    let rom_path = args.require_path("--rom");
    let output_path = args.require_path("--output");
    let source_str = args.value("--source").unwrap_or_else(|| {
        eprintln!("Error: --source is required (e.g. $11:EA80)");
        usage();
        process::exit(1);
    });
    let source = rom::SnesAddr::parse(source_str).unwrap_or_else(|| {
        eprintln!("Error: invalid SNES address: {}", source_str);
        process::exit(1);
    });
    if source.addr < 0x8000 {
        eprintln!(
            "Error: LoROM source address must be in $8000-$FFFF: {}",
            source
        );
        process::exit(1);
    }

    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let source_pc = source.to_pc();
    let (decompressed, consumed) =
        patch::font::decompress_lz(&data, source_pc).unwrap_or_else(|e| {
            eprintln!("LZ extraction failed at {}: {}", source, e);
            process::exit(1);
        });

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("Failed to create '{}': {}", parent.display(), e);
            process::exit(1);
        });
    }
    std::fs::write(&output_path, &decompressed).unwrap_or_else(|e| {
        eprintln!("Failed to write '{}': {}", output_path.display(), e);
        process::exit(1);
    });

    println!("LZ source: {} (PC 0x{:06X})", source, source_pc);
    println!("Compressed bytes consumed: {}", consumed);
    println!("Decompressed bytes written: {}", decompressed.len());
    println!("Output: {}", output_path.display());
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
        Some("trace") => {
            // Check for `trace scenario <name>` subcommand.
            if args.args_ref().get(2).map(|s| s.as_str()) == Some("scenario") {
                cmd_trace_scenario(&args);
            } else {
                cmd_trace(&args);
            }
        }
        Some("lookup") => cmd_lookup(&args),
        Some("generate-font") => cmd_generate_font(&args),
        Some("disasm") => cmd_disasm(&args),
        Some("extract-lz") => cmd_extract_lz(&args),
        Some("convert-translations") => cmd_convert_translations(&args),
        Some("audit-translations") => cmd_audit_translations(&args),
        Some("audit-translation-growth") => cmd_audit_translation_growth(&args),
        _ => {
            usage();
            process::exit(1);
        }
    }
}
