//! Full patching pipeline orchestration.
//!
//! Coordinates font patching, text replacement, and output.

use crate::encoding::ko;
use crate::font_gen;
use crate::patch::tracked_rom::TrackedRom;
use crate::patch::{
    battle_width, choice_highlight, encyclopedia, engine_hooks, equip_oam, font, item,
    options_screen, relocate, savemenu, shop_oam, text, translation_json, worldmap,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Bank configs for in-place text patching.
/// Bank 01: fc_split=false (FF-terminated, entries small enough for in-place).
const INPLACE_TEXT_BANKS: &[(&str, bool)] = &[
    ("01", false),
    ("03", false),
    ("1D", true),
    ("2A", true),
    ("2B", true),
    ("2D", true),
];

/// Patch pipeline configuration.
pub struct PatchConfig<'a> {
    pub rom_path: &'a Path,
    pub output_path: &'a Path,
    pub font_fixed_path: Option<PathBuf>,
    pub font_16x16_path: Option<PathBuf>,
    pub translations_dir: Option<PathBuf>,
    pub patch_all_text: bool,
    pub text_bank: Option<String>,
    pub text_relocate: bool,
    pub engine_hooks: bool,
    pub ko_encoding_path: Option<PathBuf>,
    pub encyclopedia_tsv_path: Option<PathBuf>,
    pub code_patches_tsv_path: Option<PathBuf>,
    pub ttf_path: Option<PathBuf>,
    pub ttf_size: f32,
    pub charset_path: Option<PathBuf>,
    pub worldmap_ttf_path: Option<PathBuf>,
    pub worldmap_ttf_size: f32,
}

/// (font_16x16_data, fixed_data, ko_encoding_table)
type FontDataBundle = (
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<HashMap<char, Vec<u8>>>,
);

/// Resolve font data and encoding from either TTF or pre-built files.
fn resolve_font_data(cfg: &PatchConfig) -> Result<FontDataBundle, String> {
    if let Some(ref ttf_path) = cfg.ttf_path {
        let chars = if let Some(ref charset_path) = cfg.charset_path {
            font_gen::load_charset(charset_path)?
        } else {
            let translations_dir = cfg
                .translations_dir
                .as_deref()
                .ok_or("--translations-dir required when --charset is not provided")?;
            auto_collect_charset(translations_dir)?
        };
        println!("\n--- Generating font from TTF ({} chars) ---", chars.len());

        let ttf_data = fs::read(ttf_path).map_err(|e| format!("Failed to read TTF: {}", e))?;
        let result = font_gen::generate_font(&ttf_data, cfg.ttf_size, &chars)?;
        println!("  Renderer: fontdue");
        Ok((
            Some(result.font_data),
            Some(result.fixed_data),
            Some(result.encoding),
        ))
    } else {
        let font_data = cfg
            .font_16x16_path
            .as_ref()
            .map(|p| fs::read(p).map_err(|e| format!("Failed to read 16x16 font: {}", e)))
            .transpose()?;
        let fixed_data = cfg
            .font_fixed_path
            .as_ref()
            .map(|p| fs::read(p).map_err(|e| format!("Failed to read fixed font: {}", e)))
            .transpose()?;
        let ko_table = cfg
            .ko_encoding_path
            .as_ref()
            .map(|p| ko::load_ko_encoding(p))
            .transpose()?;
        Ok((font_data, fixed_data, ko_table))
    }
}

/// Run the full patch pipeline.
pub fn run_patch(cfg: &PatchConfig) -> Result<(), String> {
    let rom_data = fs::read(cfg.rom_path).map_err(|e| format!("Failed to read ROM: {}", e))?;
    let mut rom = TrackedRom::new(rom_data.clone());

    println!("Base ROM: {} ({} bytes)", cfg.rom_path.display(), rom.len());

    // Resolve all font/encoding data upfront (TTF or pre-built files)
    let (font_data, fixed_data, ko_table) = resolve_font_data(cfg)?;

    // Fixed-encode tiles (char $00-$1F → Bank $0F:$8000-$87FF)
    if let Some(ref fixed_data) = fixed_data {
        println!("\n--- Patching fixed-encode font (Bank $0F:$8000) ---");
        let count = font::patch_fixed_encode(&mut rom, fixed_data)?;
        println!("  Wrote {} tiles", count);
    }

    // Bank $32 dynamic address chain: each stage returns end → next stage's start
    let mut bank32_next: Option<u16> = None;

    // 16x16 font (single-byte + FB prefix → Bank $0F)
    if let Some(ref font_data) = font_data {
        println!("\n--- Patching 16x16 font (Bank $0F) ---");
        let count = font::patch_16x16(&mut rom, font_data)?;
        println!("  Wrote {} tiles", count);

        // FA/F0 prefix tiles → Bank $32
        if cfg.engine_hooks {
            println!("\n--- Patching FA/F0 font tiles (Bank $32) ---");
            let f0_count = font::patch_fa_f0(&mut rom, font_data)?;
            println!("  Wrote {} F0 tiles (+ 256 FA)", f0_count);

            // Remap 12 JP-blank FB tile slots → F0 prefix 
            println!("\n--- Remapping FB blank slots → F0 (VRAM leak fix) ---");
            let remap_end = font::patch_fb_blank_remap(&mut rom, font_data, f0_count)?;

            // Thread address to engine hooks (align16)
            let hook_base = (remap_end + 15) & !15;
            println!(
                "  Bank $32 chain: remap_end=${:04X}, hook_base=${:04X}",
                remap_end, hook_base
            );
            bank32_next = Some(hook_base);
        }
    }

    // Engine hooks (FA/F0 prefix support in renderer + tilemap writer)
    if cfg.engine_hooks {
        let hook_base = bank32_next.unwrap_or(0xD440);
        let hooks_end = engine_hooks::apply_hooks(&mut rom, hook_base)?;
        bank32_next = Some((hooks_end + 15) & !15);
    }

    // Menu worldmap place name hook ($03:$C3F0 LZ intercept)
    // Must come before text relocation so its end address feeds into free space start
    if cfg.engine_hooks {
        let menu_code_addr = bank32_next.unwrap_or(0xD660);
        let menu_end = patch_menu_worldmap(&mut rom, cfg, menu_code_addr)?;
        bank32_next = Some(menu_end);
    }

    // Text replacement
    let banks_to_patch: Vec<&str> = if cfg.patch_all_text {
        INPLACE_TEXT_BANKS.iter().map(|(id, _)| *id).collect()
    } else if let Some(ref bank) = cfg.text_bank {
        vec![bank.as_str()]
    } else {
        Vec::new()
    };

    if cfg.translations_dir.is_none() && !banks_to_patch.is_empty() {
        println!("\n--- Text: SKIPPED (no --translations-dir) ---");
    }
    if let Some(ref translations_dir) = cfg.translations_dir {
        let ko_table = ko_table
            .as_ref()
            .ok_or("KO encoding required for text patching")?;

        if cfg.text_relocate {
            // Relocation mode: analyze fit, relocate overflows, patch in-place
            println!("\n--- Text relocation mode ---");
            let bank32_free = bank32_next.unwrap_or(0xF100);
            println!("  Bank $32 free start: ${:04X}", bank32_free);
            match relocate::relocate_all(
                &mut rom,
                &banks_to_patch,
                translations_dir,
                ko_table,
                bank32_free,
            ) {
                Ok(stats) => {
                    println!(
                        "  Banks: {}, In-place: {}, Relocated: {}, Skipped: {}",
                        stats.banks_processed, stats.inplace, stats.relocated, stats.skipped
                    );
                }
                Err(e) => return Err(format!("Text relocation failed: {}", e)),
            }
        } else {
            // In-place only mode
            for bank_id in &banks_to_patch {
                let fc_split = INPLACE_TEXT_BANKS
                    .iter()
                    .find(|(id, _)| id == bank_id)
                    .map(|(_, fc)| *fc)
                    .unwrap_or(false);

                println!("\n--- Patching text: Bank ${} ---", bank_id);
                let stats =
                    text::patch_inplace(&mut rom, bank_id, translations_dir, ko_table, fc_split)
                        .map_err(|e| {
                            format!("In-place text patching failed for Bank ${}: {}", bank_id, e)
                        })?;
                println!(
                    "  Total: {}, Replaced: {}, Truncated: {}, Skipped: {}",
                    stats.total, stats.replaced, stats.truncated, stats.skipped
                );
            }
        }
    }

    // Code-embedded string patches (not in regular text data area)
    if cfg.translations_dir.is_some() && !banks_to_patch.is_empty() {
        patch_code_embedded_strings(&mut rom, cfg, ko_table.as_ref())?;
    }

    // Stat screen level bar (tilemap-based, requires KO font)
    if cfg.patch_all_text {
        if let Some(ref ko_table) = ko_table {
            println!("\n--- Patching stat level bar ---");
            let count = patch_stat_level_bar(&mut rom, ko_table)?;
            println!("  Patched {} level entries", count);
        }
    }

    // Item name table (fixed-width 10B × 18 entries at $2B:$FD8B)
    if cfg.patch_all_text {
        if let Some(ref ko_table) = ko_table {
            println!("\n--- Patching item name table ---");
            let count = item::patch_item_name_table(&mut rom, ko_table)?;
            println!("  Patched {} item names", count);
        }
    }

    // Encyclopedia monster name hooks (must be after text relocation)
    if cfg.engine_hooks {
        let ko_table = ko_table
            .as_ref()
            .ok_or("KO encoding required for encyclopedia hooks")?;
        let enc_data = load_encyclopedia_data(cfg, ko_table)?;
        encyclopedia::apply_encyclopedia_hooks(&mut rom, &enc_data)?;
    }

    // World map place name hook (LZ decompressor intercept at $10:$B562)
    if cfg.engine_hooks {
        println!("\n--- Patching world map place names (Hook #14) ---");
        let sky_tiles = prepare_sky_tiles(cfg, ko_table.as_ref())?;
        let count = worldmap::apply_worldmap_hook(&mut rom, &sky_tiles)?;
        println!("  Hooked {} LZ conditions", count);
    }

    // Save menu UI localization 
    if cfg.engine_hooks {
        patch_save_menu(&mut rom, cfg)?;
    }

    // Options/stat/magic screen localization 
    if cfg.engine_hooks {
        patch_options_screen(&mut rom, cfg)?;
    }

    // Equipment OAM sprites 
    if cfg.engine_hooks {
        patch_equip_oam(&mut rom, cfg)?;
    }

    // Shop OAM sprites 
    if cfg.engine_hooks {
        patch_shop_oam(&mut rom, cfg)?;
    }

    // Battle dialog box width/height hook 
    if cfg.engine_hooks {
        battle_width::apply_battle_width_hook(&mut rom)?;
    }

    // Choice highlight width fix 
    if cfg.engine_hooks {
        choice_highlight::apply_choice_highlight_fix(&mut rom)?;
    }

    // Verify no ROM region collisions (hard error)
    rom.check()?;
    // Warn about untracked writes
    if let Err(report) = rom.check_untracked_writes(&rom_data) {
        eprintln!("\nWARNING: {}", report);
    }

    // Write output
    if let Some(parent) = cfg.output_path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(cfg.output_path, &*rom).map_err(|e| format!("Failed to write output ROM: {}", e))?;

    println!(
        "\n=== Patched ROM written: {} ({} bytes) ===",
        cfg.output_path.display(),
        rom.len()
    );

    if rom.len() != rom_data.len() {
        println!(
            "  WARNING: ROM size changed! {} -> {}",
            rom_data.len(),
            rom.len()
        );
    }

    Ok(())
}

/// Apply code-embedded string patches (JSON/TSV-encoded + byte-level).
fn patch_code_embedded_strings(
    rom: &mut TrackedRom,
    cfg: &PatchConfig,
    ko_table: Option<&HashMap<char, Vec<u8>>>,
) -> Result<(), String> {
    println!("\n--- Patching code-embedded strings ---");

    let ko_table = ko_table.ok_or("KO encoding required for code patches")?;

    // Try JSON first (in translations dir)
    let encoded_count = if let Some(ref dir) = cfg.translations_dir {
        let json_path = dir.join("code_patches.json");
        if json_path.exists() {
            patch_code_strings_from_entries(
                rom,
                &translation_json::load_code_patches_json(&json_path)?,
                ko_table,
            )?
        } else if let Some(ref tsv_path) = cfg.code_patches_tsv_path {
            patch_code_strings_from_tsv(rom, tsv_path, ko_table)?
        } else {
            0
        }
    } else if let Some(ref tsv_path) = cfg.code_patches_tsv_path {
        patch_code_strings_from_tsv(rom, tsv_path, ko_table)?
    } else {
        0
    };

    let byte_count = patch_code_byte_patches(rom);
    println!(
        "  Patched {} encoded + {} byte-level string(s)",
        encoded_count, byte_count
    );
    Ok(())
}

/// Apply save menu UI localization .
/// Uses --ttf (Galmuri11 등 픽셀폰트 권장).
fn patch_save_menu(rom: &mut TrackedRom, cfg: &PatchConfig) -> Result<(), String> {
    let ttf_path = cfg.ttf_path.as_ref().ok_or("Save menu requires --ttf")?;

    println!("\n--- Patching save menu UI  ---");
    println!("  Font: {} (size {})", ttf_path.display(), cfg.ttf_size);

    let ttf_data = fs::read(ttf_path).map_err(|e| format!("Failed to read TTF: {}", e))?;
    let sm_tiles = font_gen::render_savemenu_tiles(&ttf_data, cfg.ttf_size, savemenu::KO_CHARS)?;
    println!("  Renderer: fontdue");
    savemenu::apply_savemenu_hook_with_tiles(rom, &sm_tiles)
}

/// Default 8x8 bitmap font path (dalmoori: pixel-perfect 8x8 Korean font).
const DALMOORI_TTF: &str = "assets/fonts/dalmoori.ttf";
/// Default worldmap TTF size (8px for dalmoori bitmap font).
const WORLDMAP_TTF_SIZE_DEFAULT: f32 = 8.0;

/// Load 8×8 worldmap font: --worldmap-ttf > dalmoori.ttf > --ttf.
fn load_worldmap_8x8_font(cfg: &PatchConfig) -> Result<(Vec<u8>, f32, String), String> {
    if let Some(ref wm_path) = cfg.worldmap_ttf_path {
        let data = fs::read(wm_path).map_err(|e| format!("Failed to read worldmap TTF: {}", e))?;
        let size = if cfg.worldmap_ttf_size > 0.0 {
            cfg.worldmap_ttf_size
        } else {
            WORLDMAP_TTF_SIZE_DEFAULT
        };
        Ok((data, size, wm_path.to_string_lossy().into_owned()))
    } else if let Ok(data) = fs::read(DALMOORI_TTF) {
        Ok((data, WORLDMAP_TTF_SIZE_DEFAULT, DALMOORI_TTF.to_string()))
    } else {
        let ttf_path = cfg
            .ttf_path
            .as_ref()
            .ok_or("Worldmap requires --worldmap-ttf, dalmoori.ttf, or --ttf")?;
        let data = fs::read(ttf_path).map_err(|e| format!("Failed to read TTF: {}", e))?;
        Ok((data, 7.0, ttf_path.to_string_lossy().into_owned()))
    }
}

/// Prepare KO 8×8 tiles for sky worldmap Block B/C injection.
///
/// For each unique character in sky worldmap place names, looks up the KO encoding
/// byte and renders an 8×8 glyph tile using dalmoori font.
///
/// Tile index mapping:
/// - Single-byte ($20-$EF): tile index = byte value
/// - FB prefix: tile index = $100 + byte (fits in Block B's 512 tiles)
/// - F1/F0 prefix: not supported (game's 8x8 renderer doesn't handle these)
fn prepare_sky_tiles(
    cfg: &PatchConfig,
    ko_table: Option<&HashMap<char, Vec<u8>>>,
) -> Result<Vec<(u16, [u8; 16])>, String> {
    let ko_table = match ko_table {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    let sky_chars = worldmap::sky_ko_chars();
    if sky_chars.is_empty() {
        return Ok(Vec::new());
    }

    let (wm_ttf, wm_size, font_name) = load_worldmap_8x8_font(cfg)?;
    let rendered = font_gen::render_menu_worldmap_tiles(&wm_ttf, wm_size, &sky_chars)?;

    let mut tiles = Vec::new();
    for (i, &ch) in sky_chars.iter().enumerate() {
        if let Some(bytes) = ko_table.get(&ch) {
            match bytes.as_slice() {
                [b] => tiles.push((*b as u16, rendered[i])),
                [0xFB, b] => tiles.push((0x100 + *b as u16, rendered[i])),
                _ => println!(
                    "  WARNING: sky char '{}' encoding {:02X?} — F1/F0 prefix, skipped",
                    ch, bytes
                ),
            }
        } else {
            println!("  WARNING: sky char '{}' not in KO table", ch);
        }
    }

    println!(
        "  {} KO sky glyphs prepared (8×8, {})",
        tiles.len(),
        font_name
    );
    Ok(tiles)
}

/// Apply menu worldmap localization .
/// Returns the align16 end address in Bank $32 (for pipeline chain).
fn patch_menu_worldmap(
    rom: &mut TrackedRom,
    cfg: &PatchConfig,
    menu_code_addr: u16,
) -> Result<u16, String> {
    println!("\n--- Patching menu worldmap place names  ---");

    let (ttf_data, ttf_size, font_name) = load_worldmap_8x8_font(cfg)?;

    let ko_chars = worldmap::menu_ko_chars();
    println!(
        "  Font: {} (size {}), {} KO glyphs",
        font_name,
        ttf_size,
        ko_chars.len()
    );

    let tiles = font_gen::render_menu_worldmap_tiles(&ttf_data, ttf_size, &ko_chars)?;

    // OBJ title sprites: use --ttf (Galmuri11) for 16x16 rendering
    let obj_bitmaps = if let Some(path) = cfg.ttf_path.as_ref() {
        let data = fs::read(path).map_err(|e| format!("Failed to read TTF: {}", e))?;
        println!("  OBJ title: {} (size {})", path.display(), cfg.ttf_size);
        Some(font_gen::render_obj_title_bitmaps(
            &data,
            cfg.ttf_size,
            worldmap::OBJ_TITLE_CHARS,
        )?)
    } else {
        None
    };

    // OBJ bubble text: use worldmap 8×8 font for "이것", "현위치", "목적지"
    let bubble_bitmaps: Option<Vec<Vec<[bool; 64]>>> = if obj_bitmaps.is_some() {
        let font = fontdue::Font::from_bytes(ttf_data.as_slice(), fontdue::FontSettings::default())
            .map_err(|e| format!("Failed to load TTF for bubbles: {}", e))?;
        let bubble_groups: &[&[char]] = &[
            worldmap::BUBBLE_KORE_CHARS,
            worldmap::BUBBLE_IMAWA_CHARS,
            worldmap::BUBBLE_IKISAKI_CHARS,
        ];
        let groups: Vec<Vec<[bool; 64]>> = bubble_groups
            .iter()
            .map(|chars: &&[char]| {
                chars
                    .iter()
                    .map(|&ch| font_gen::render_glyph_to_bitmap_8x8(&font, ch, ttf_size))
                    .collect()
            })
            .collect();
        println!("  Bubble text: {} groups rendered (8×8)", groups.len());
        Some(groups)
    } else {
        None
    };

    worldmap::apply_menu_worldmap_hook(
        rom,
        &tiles,
        obj_bitmaps.as_deref(),
        bubble_bitmaps.as_deref(),
        menu_code_addr,
    )
}

/// Apply options/stat/magic screen localization .
/// 8×8: dalmoori.ttf (same priority as menu worldmap).
/// 16×16: --savemenu-ttf > --ttf (same as save menu).
fn patch_options_screen(rom: &mut TrackedRom, cfg: &PatchConfig) -> Result<(), String> {
    println!("\n--- Patching options/stat/magic screens  ---");

    // 8×8 font: --worldmap-ttf > dalmoori > --ttf (same as worldmap/equip/shop)
    let (ttf_8, size_8, name_8) = load_worldmap_8x8_font(cfg)?;

    // 16×16 font: --ttf (Galmuri11 등 픽셀폰트 권장)
    let (ttf_16, size_16, name_16) = {
        let ttf_path = cfg
            .ttf_path
            .as_ref()
            .ok_or("Options screen 16x16 requires --ttf")?;
        let data = fs::read(ttf_path).map_err(|e| format!("Failed to read TTF: {}", e))?;
        (data, cfg.ttf_size, ttf_path.to_string_lossy().into_owned())
    };

    // Phase 2a: Stat+Magic screen
    let chars_8 = options_screen::collect_ko_chars_8x8();
    let chars_16 = options_screen::collect_ko_chars_16x16();
    println!(
        "  Phase 2a 8x8: {} (size {}), {} glyphs",
        name_8,
        size_8,
        chars_8.len()
    );
    println!(
        "  Phase 2a 16x16: {} (size {}), {} glyphs",
        name_16,
        size_16,
        chars_16.len()
    );

    let tiles_8 = font_gen::render_menu_worldmap_tiles(&ttf_8, size_8, &chars_8)?;
    let tiles_16 = font_gen::render_options_16x16_tiles(&ttf_16, size_16, &chars_16)?;
    options_screen::apply_options_screen_hook(rom, &tiles_8, &tiles_16)?;

    // Phase 2b: Options screen
    let opt_chars_8 = options_screen::collect_ko_chars_8x8_options();
    let opt_chars_16 = options_screen::collect_ko_chars_16x16_options();
    println!(
        "  Phase 2b 8x8: {} glyphs, 16x16: {} glyphs",
        opt_chars_8.len(),
        opt_chars_16.len()
    );

    let opt_tiles_8 = font_gen::render_menu_worldmap_tiles(&ttf_8, size_8, &opt_chars_8)?;
    let opt_tiles_16 = font_gen::render_options_16x16_tiles(&ttf_16, size_16, &opt_chars_16)?;
    options_screen::apply_options_phase2b(rom, &opt_tiles_8, &opt_tiles_16)
}

/// Apply equipment OAM sprite localization .
///
/// Uses dalmoori 8×8 font (same as worldmap/options/shop) for pixel-perfect 8×8 OAM tiles.
fn patch_equip_oam(rom: &mut TrackedRom, cfg: &PatchConfig) -> Result<(), String> {
    let (ttf_data, ttf_size, font_name) = load_worldmap_8x8_font(cfg)?;

    println!("\n--- Patching equipment OAM sprites  ---");
    println!("  Font: {} (size {})", font_name, ttf_size);
    equip_oam::apply_equip_oam_hook(rom, &ttf_data, ttf_size)
}

/// Apply shop OAM sprite localization .
///
/// Uses dalmoori 8×8 font (same as worldmap/options) for pixel-perfect 8×8 OAM tiles.
fn patch_shop_oam(rom: &mut TrackedRom, cfg: &PatchConfig) -> Result<(), String> {
    let (ttf_data, ttf_size, font_name) = load_worldmap_8x8_font(cfg)?;

    println!("\n--- Patching shop OAM sprites  ---");
    println!("  Font: {} (size {})", font_name, ttf_size);
    shop_oam::apply_shop_oam_hook(rom, &ttf_data, ttf_size)
}

/// Load and encode encyclopedia data (JSON-first, TSV fallback).
fn load_encyclopedia_data(
    cfg: &PatchConfig,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<encyclopedia::EncyclopediaData, String> {
    println!("\n--- Loading encyclopedia data ---");
    println!("  KO encoding: {} entries", ko_table.len());

    // Try JSON first (in translations dir)
    if let Some(ref dir) = cfg.translations_dir {
        let json_path = dir.join("encyclopedia.json");
        if json_path.exists() {
            println!("  Source: {} (JSON)", json_path.display());
            let data = translation_json::load_encyclopedia_json(&json_path, ko_table)?;
            println!(
                "  Encoded {} names, {} descriptions",
                data.names.len(),
                data.descs.len()
            );
            return Ok(data);
        }
    }

    // Fall back to TSV
    let enc_path = cfg
        .encyclopedia_tsv_path
        .as_ref()
        .ok_or("--encyclopedia-tsv path required for encyclopedia hooks")?;
    println!("  Source: {} (TSV)", enc_path.display());

    let data = encyclopedia::load_encyclopedia_tsv(enc_path, ko_table)?;
    println!(
        "  Encoded {} names, {} descriptions",
        data.names.len(),
        data.descs.len()
    );

    Ok(data)
}

/// Load code_patches.tsv and encode each entry into game bytes.
///
/// TSV format: ID\tPC_ADDR\tSLOT_SIZE\tPREFIX_BYTES\tKO\tNOTES
/// PREFIX_BYTES is optional hex bytes (e.g., "00 00") to prepend before encoded text.
/// Remaining slot space after prefix + encoded text is filled with FF.
fn patch_code_strings_from_tsv(
    rom: &mut TrackedRom,
    tsv_path: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<usize, String> {
    let content = fs::read_to_string(tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    let entries = parse_code_patches_tsv(&content)?;
    patch_code_strings_from_entries(rom, &entries, ko_table)
}

/// Apply code patch entries (shared between TSV and JSON loaders).
fn patch_code_strings_from_entries(
    rom: &mut TrackedRom,
    entries: &[translation_json::CodePatchEntry],
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<usize, String> {
    let mut count = 0;

    for entry in entries {
        let encoded = ko::encode_simple(&entry.ko_text, ko_table)
            .map_err(|e| format!("Code patch '{}' encoding error: {}", entry.id, e))?;

        // Build patch: prefix + encoded text + FF fill
        let mut patch = entry.prefix.clone();
        patch.extend_from_slice(&encoded);

        if patch.len() >= entry.slot_size {
            return Err(format!(
                "Code patch '{}': data {} bytes exceeds slot {} bytes (need room for FF terminator)",
                entry.id,
                patch.len(),
                entry.slot_size
            ));
        }

        // Fill remaining with FF
        patch.resize(entry.slot_size, 0xFF);

        if entry.pc_addr + patch.len() > rom.len() {
            println!(
                "  SKIP '{}': PC 0x{:X} + {} bytes exceeds ROM",
                entry.id,
                entry.pc_addr,
                patch.len()
            );
            continue;
        }

        rom.write(entry.pc_addr, &patch, &format!("code_patch:{}", entry.id));
        println!(
            "  '{}' at PC 0x{:X}: \"{}\" ({} bytes)",
            entry.id,
            entry.pc_addr,
            entry.ko_text,
            patch.len()
        );
        count += 1;
    }

    Ok(count)
}

/// Parse code_patches.tsv content into entries.
fn parse_code_patches_tsv(content: &str) -> Result<Vec<translation_json::CodePatchEntry>, String> {
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("ID") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            return Err(format!(
                "Invalid code_patches.tsv line (need 5+ columns): {}",
                line
            ));
        }
        let id = parts[0].to_string();
        let pc_addr = usize::from_str_radix(parts[1].trim_start_matches("0x"), 16)
            .map_err(|_| format!("Invalid PC_ADDR '{}' for '{}'", parts[1], id))?;
        let slot_size: usize = parts[2]
            .parse()
            .map_err(|_| format!("Invalid SLOT_SIZE '{}' for '{}'", parts[2], id))?;
        let prefix: Vec<u8> = if parts[3].trim().is_empty() {
            Vec::new()
        } else {
            parts[3]
                .split_whitespace()
                .map(|h| {
                    u8::from_str_radix(h, 16)
                        .map_err(|_| format!("Invalid prefix hex '{}' for '{}'", h, id))
                })
                .collect::<Result<Vec<u8>, String>>()?
        };
        let ko_text = parts[4].to_string();

        entries.push(translation_json::CodePatchEntry {
            id,
            pc_addr,
            slot_size,
            prefix,
            ko_text,
        });
    }

    Ok(entries)
}

// ── Stat screen level bar ────────────────────────────────────────

/// Stat level bar: 12 entries × 6 tiles × 2 bytes at $01:$86DE (PC 0x86DE).
/// Tilemap pair format: (tile_encoding, page_flag) where
///   page=0: FIXED ($00-$1F) or single-byte ($20-$EF)
///   page=1: FB-prefix range
const STAT_BAR_PC: usize = 0x86DE;
const STAT_BAR_TILES_PER_ENTRY: usize = 6;

/// KO level descriptions — weakest (0) to strongest (11).
/// Reordered intensity scale: 조금→나름→꽤→제법→매우→엄청→진짜.
const KO_STAT_LEVELS: [&str; 12] = [
    "약해요~  ",   // 0: 弱いですぅ
    "아직 약해 ",  // 1: まだまだ弱い
    "강한가?  ",   // 2: 強いかな？
    "조금 강해 ",  // 3: ちょっと強い
    "나름 강해 ",  // 4: まぁまぁ強い
    "꽤 강해  ",   // 5: そこそこ強い
    "제법 강해 ",  // 6: なかなか強い
    "매우 강해 ",  // 7: とっても強い
    "엄청 강해 ",  // 8: すっごく強い
    "진짜 강해 ",  // 9: めっちゃ強い
    "유치원 최강", // 10: 幼稚園で最強
    "무적의 원장", // 11: むてきの園長
];

/// Convert a character to stat bar tilemap pair (tile_lo, page_flag).
fn char_to_tilemap_pair(ch: char, ko_table: &HashMap<char, Vec<u8>>) -> Result<[u8; 2], String> {
    // FIXED_ENCODE characters (not in ko_table)
    match ch {
        ' ' => return Ok([0x00, 0x00]),
        '!' => return Ok([0x0B, 0x00]),
        '~' => return Ok([0x0C, 0x00]),
        '.' => return Ok([0x0D, 0x00]),
        '?' => return Ok([0x0E, 0x00]),
        '0'..='9' => return Ok([ch as u8 - b'0' + 1, 0x00]),
        _ => {}
    }
    // KO table lookup
    let bytes = ko_table
        .get(&ch)
        .ok_or_else(|| format!("Stat bar: '{}' (U+{:04X}) not in KO table", ch, ch as u32))?;
    match bytes.as_slice() {
        [b] => Ok([*b, 0x00]),       // single-byte ($20-$EF)
        [0xFB, b] => Ok([*b, 0x01]), // FB prefix → page 1
        _ => Err(format!(
            "Stat bar: '{}' encoding {:02X?} unsupported (need single-byte or FB prefix)",
            ch, bytes
        )),
    }
}

/// Patch the stat screen level bar at $01:$86DE with KO tilemap data.
fn patch_stat_level_bar(
    rom: &mut TrackedRom,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<usize, String> {
    let total_bytes = KO_STAT_LEVELS.len() * STAT_BAR_TILES_PER_ENTRY * 2;
    if STAT_BAR_PC + total_bytes > rom.len() {
        return Err("Stat level bar: ROM offset out of bounds".to_string());
    }
    let mut buf = Vec::with_capacity(total_bytes);
    for (i, &text) in KO_STAT_LEVELS.iter().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() != STAT_BAR_TILES_PER_ENTRY {
            return Err(format!(
                "Stat level {}: expected {} chars, got {} in {:?}",
                i,
                STAT_BAR_TILES_PER_ENTRY,
                chars.len(),
                text
            ));
        }
        for &ch in &chars {
            let pair = char_to_tilemap_pair(ch, ko_table)?;
            buf.push(pair[0]);
            buf.push(pair[1]);
        }
    }
    rom.write(STAT_BAR_PC, &buf, "stat_level_bar");
    Ok(KO_STAT_LEVELS.len())
}

// ── Code byte patches ────────────────────────────────────────────

/// Apply byte-level patches for encyclopedia "unconfirmed" placeholder.
/// These are JP byte substitutions, not KO text encoding.
fn patch_code_byte_patches(rom: &mut TrackedRom) -> usize {
    let mut count = 0;

    // $03:$B6B2-$B6E9: Encyclopedia unconfirmed stats — C7(？) → 0E(?)
    let enc_desc_pc = 0x1B6B2;
    let enc_desc_end = 0x1B6E9;
    if enc_desc_end < rom.len() {
        let len = enc_desc_end - enc_desc_pc + 1;
        let mut rgn = rom.region(enc_desc_pc, len, "code_byte:enc_desc");
        for byte in rgn.data_mut().iter_mut() {
            if *byte == 0xC7 {
                *byte = 0x0E;
            }
        }
        count += 1;
    }

    // $03:$B6EA: Encyclopedia unconfirmed name — みかくにん → ?????
    let enc_name_pc = 0x1B6EA;
    let ko_enc_name: &[u8] = &[0x0E, 0x0E, 0x0E, 0x0E, 0x0E, 0xFF];
    if enc_name_pc + ko_enc_name.len() <= rom.len() {
        rom.write(enc_name_pc, ko_enc_name, "code_byte:enc_name");
        count += 1;
    }

    // $01:$AE89-$AE8E: Battle suffix "は"($5B) → NOP×6
    // JP appends は (topic marker) after monster name.
    // KO renders $5B as "들" (plural) — remove entirely.
    let suffix_pc = 0xAE89; // lorom_to_pc(0x01, 0xAE89)
    if suffix_pc + 6 <= rom.len() {
        rom.fill(suffix_pc, 6, 0xEA, "code_byte:battle_suffix");
        count += 1;
    }

    count
}

// ── Auto charset collection ──────────────────────────────────────

/// Collect unique KO characters from KO_STAT_LEVELS.
fn stat_level_chars() -> Vec<char> {
    KO_STAT_LEVELS
        .iter()
        .flat_map(|s| s.chars())
        .filter(|ch| {
            !ch.is_control()
                && *ch != ' '
                && !translation_json::is_fixed_encode_char(*ch)
        })
        .collect()
}

/// Auto-collect charset from translation JSON files + hardcoded module constants.
///
/// Returns chars sorted by frequency (most frequent first) for optimal
/// single-byte encoding slot assignment ($20-$EF).
pub fn auto_collect_charset(translations_dir: &Path) -> Result<Vec<char>, String> {
    // 1. Collect from translation JSON files (with frequency counts)
    let mut freq = translation_json::collect_charset_from_translations(translations_dir)?;

    // 2. Merge hardcoded KO characters that go through ko_table encoding.
    //    Modules with independent rendering (savemenu, options_screen,
    //    worldmap menu/OBJ/bubble, equip_oam, shop_oam) are excluded —
    //    their chars are rendered via dedicated tile generators and
    //    don't need charset encoding slots.
    let hardcoded_sources: [Vec<char>; 3] = [
        item::all_ko_chars(),
        stat_level_chars(),
        worldmap::sky_ko_chars(),
    ];

    for source in &hardcoded_sources {
        for &ch in source {
            if !ch.is_control()
                && ch != ' '
                && !translation_json::is_fixed_encode_char(ch)
            {
                freq.entry(ch).or_insert(0);
            }
        }
    }

    // 3. Sort by frequency (descending), then codepoint for determinism
    let mut chars: Vec<(char, usize)> = freq.into_iter().collect();
    chars.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let result: Vec<char> = chars.into_iter().map(|(ch, _)| ch).collect();
    println!("  Auto-collected charset: {} unique chars", result.len());
    Ok(result)
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
