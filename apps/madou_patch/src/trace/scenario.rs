//! Trace scenario runners for automated issue detection.
//!
//! FR-05: title-leak scenario — detect whether worldmap data leaks into the
//! title screen in a patched ROM.

use super::bus::TraceEvent;
use crate::cli::Args;
use crate::rom;
use std::process;

/// Verdict from a scenario run.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

/// A single piece of evidence supporting a FAIL verdict.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Evidence {
    pub reason: String,
    pub event: TraceEvent,
}

/// Result of the title-leak scenario.
#[derive(Debug)]
pub struct TitleLeakResult {
    pub verdict: Verdict,
    pub evidence: Vec<Evidence>,
    pub total_events: usize,
    pub analyzed_events: usize,
    pub title_frame_window: u32,
}

/// Banks that contain worldmap data. LZ calls with dp$0B in these banks
/// during the title stage are suspicious.
const WORLDMAP_BANKS: &[u8] = &[0x10, 0x11, 0x12];
pub const DEFAULT_TITLE_FRAMES: u32 = 180;

/// Check collected trace events for title-leak patterns.
pub fn judge_title_leak(events: &[TraceEvent]) -> TitleLeakResult {
    judge_title_leak_with_window(events, DEFAULT_TITLE_FRAMES)
}

/// Check collected trace events for title-leak patterns in the first N frames.
pub fn judge_title_leak_with_window(events: &[TraceEvent], title_frames: u32) -> TitleLeakResult {
    let mut evidence = Vec::new();
    let mut analyzed_events = 0usize;

    let seq_cutoff = events
        .iter()
        .find_map(|event| match event {
            TraceEvent::FrameMarker { frame_num, seq, .. } if *frame_num >= title_frames => {
                Some(*seq)
            }
            _ => None,
        })
        .unwrap_or(u64::MAX);

    for event in events {
        if event.seq() > seq_cutoff {
            continue;
        }
        analyzed_events += 1;

        match event {
            TraceEvent::LzCall { dp_0b, seq, .. } => {
                if WORLDMAP_BANKS.contains(dp_0b) {
                    evidence.push(Evidence {
                        reason: format!(
                            "LZ call with dp$0B=${:02X} (worldmap bank) at seq {}",
                            dp_0b, seq
                        ),
                        event: event.clone(),
                    });
                }
            }
            TraceEvent::HookGuardCheck {
                hook, passed, seq, ..
            } => {
                // Worldmap hook guard passing during title stage is a direct leak signal.
                if *passed && (hook == "sky_worldmap" || hook == "menu_worldmap") {
                    evidence.push(Evidence {
                        reason: format!("Hook guard passed for '{}' at seq {}", hook, seq),
                        event: event.clone(),
                    });
                }
            }
            TraceEvent::DmaStart {
                src_bank,
                dest_reg,
                seq,
                ..
            } if WORLDMAP_BANKS.contains(src_bank) && (*dest_reg == 0x18 || *dest_reg == 0x19) => {
                // DMA from worldmap banks to VRAM
                evidence.push(Evidence {
                    reason: format!(
                        "DMA from worldmap bank ${:02X} to VRAM at seq {}",
                        src_bank, seq
                    ),
                    event: event.clone(),
                });
            }
            _ => {}
        }
    }

    let verdict = if evidence.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail
    };

    TitleLeakResult {
        verdict,
        evidence,
        total_events: events.len(),
        analyzed_events,
        title_frame_window: title_frames,
    }
}

/// CLI entry point for `trace scenario title-leak`.
pub fn run_title_leak(args: &Args) {
    let rom_path = args.require_path("--rom");
    let data = rom::load_rom(&rom_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    let max_inst = args
        .value("--max-inst")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(500_000);
    let screenshot_palette = args
        .value("--screenshot-pal")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let screenshot_cols = args
        .value("--screenshot-cols")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(64);
    let title_frames = args
        .value("--title-frames")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_TITLE_FRAMES);

    let config = super::TracerConfig {
        max_instructions: max_inst,
        stop_on_target: false,
        inject_nmi: true,
        start_button: false,
        log_lz_calls: true,
        force_loop_break: true,
        ..Default::default()
    };

    eprintln!(
        "[scenario:title-leak] Tracing {} (max {} instructions, title window {} frames)",
        rom_path.display(),
        max_inst,
        title_frames,
    );

    let result = super::run_trace(data, &config);
    let judgment = if title_frames == DEFAULT_TITLE_FRAMES {
        judge_title_leak(&result.trace_events)
    } else {
        judge_title_leak_with_window(&result.trace_events, title_frames)
    };

    match judgment.verdict {
        Verdict::Pass => println!("PASS — no worldmap data leak detected in title stage"),
        Verdict::Fail => {
            println!(
                "FAIL — {} suspicious event(s) detected:",
                judgment.evidence.len()
            );
            for ev in &judgment.evidence {
                println!("  - {}", ev.reason);
            }
        }
    }

    println!(
        "Analyzed events (first {} frames): {} / {}",
        judgment.title_frame_window, judgment.analyzed_events, judgment.total_events
    );

    if let Some(path) = args.path("--screenshot") {
        super::capture::write_vram_tiles_ppm(
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

    // Write evidence JSON if requested.
    if let Some(json_path) = args.path("--event-json") {
        let output = serde_json::json!({
            "verdict": format!("{:?}", judgment.verdict),
            "evidence": judgment.evidence,
            "title_frame_window": judgment.title_frame_window,
            "total_events": judgment.total_events,
            "analyzed_events": judgment.analyzed_events,
            "all_events": result.trace_events,
        });
        let json = serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
            eprintln!("Error serializing: {}", e);
            process::exit(1);
        });
        std::fs::write(&json_path, json).unwrap_or_else(|e| {
            eprintln!("Error writing {}: {}", json_path.display(), e);
            process::exit(1);
        });
        println!("Wrote events to {}", json_path.display());
    }
}

#[cfg(test)]
#[path = "scenario_tests.rs"]
mod tests;
