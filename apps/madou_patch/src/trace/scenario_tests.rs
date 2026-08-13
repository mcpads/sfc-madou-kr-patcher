use super::*;
use crate::trace::bus::TraceEvent;

#[test]
fn pass_on_empty_events() {
    let result = judge_title_leak(&[]);
    assert_eq!(result.verdict, Verdict::Pass);
    assert!(result.evidence.is_empty());
}

#[test]
fn pass_on_safe_events() {
    let events = vec![
        TraceEvent::DmaStart {
            channel: 0,
            src_bank: 0x0F,
            src_addr: 0x8000,
            dest_reg: 0x18,
            vram_addr: 0x5000,
            size: 0x1000,
            pc_bank: 0x00,
            pc_addr: 0x8100,
            seq: 1,
        },
        TraceEvent::LzCall {
            pc_bank: 0x00,
            pc_addr: 0x9440,
            dp_0b: 0x0F,
            dp_0c: 0x8000,
            seq: 2,
        },
    ];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Pass);
    assert!(result.evidence.is_empty());
}

#[test]
fn fail_on_worldmap_lz_call() {
    let events = vec![TraceEvent::LzCall {
        pc_bank: 0x00,
        pc_addr: 0x9440,
        dp_0b: 0x11,
        dp_0c: 0x8000,
        seq: 1,
    }];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Fail);
    assert_eq!(result.evidence.len(), 1);
    assert!(result.evidence[0].reason.contains("$11"));
}

#[test]
fn fail_on_worldmap_dma() {
    let events = vec![TraceEvent::DmaStart {
        channel: 1,
        src_bank: 0x12,
        src_addr: 0x8000,
        dest_reg: 0x18,
        vram_addr: 0x4000,
        size: 0x2000,
        pc_bank: 0x00,
        pc_addr: 0x8200,
        seq: 5,
    }];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Fail);
    assert_eq!(result.evidence.len(), 1);
    assert!(result.evidence[0].reason.contains("$12"));
}

#[test]
fn fail_on_hook_guard_pass() {
    let events = vec![TraceEvent::HookGuardCheck {
        hook: "sky_worldmap".to_string(),
        passed: true,
        pc_bank: 0x00,
        pc_addr: 0x9440,
        dp_0b: 0x11,
        dp_0c: 0x818C,
        seq: 9,
    }];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Fail);
    assert_eq!(result.evidence.len(), 1);
    assert!(result.evidence[0].reason.contains("sky_worldmap"));
}

#[test]
fn hook_guard_not_passed_is_ignored() {
    let events = vec![TraceEvent::HookGuardCheck {
        hook: "menu_worldmap".to_string(),
        passed: false,
        pc_bank: 0x00,
        pc_addr: 0x9440,
        dp_0b: 0x25,
        dp_0c: 0xB784,
        seq: 10,
    }];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Pass);
    assert!(result.evidence.is_empty());
}

#[test]
fn multiple_violations() {
    let events = vec![
        TraceEvent::LzCall {
            pc_bank: 0x00,
            pc_addr: 0x9440,
            dp_0b: 0x10,
            dp_0c: 0x8000,
            seq: 1,
        },
        TraceEvent::DmaStart {
            channel: 0,
            src_bank: 0x11,
            src_addr: 0x9000,
            dest_reg: 0x18,
            vram_addr: 0x5000,
            size: 0x100,
            pc_bank: 0x00,
            pc_addr: 0x8300,
            seq: 2,
        },
        // Safe event — should not trigger
        TraceEvent::DmaStart {
            channel: 0,
            src_bank: 0x0F,
            src_addr: 0x8000,
            dest_reg: 0x18,
            vram_addr: 0x5000,
            size: 0x100,
            pc_bank: 0x00,
            pc_addr: 0x8400,
            seq: 3,
        },
    ];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Fail);
    assert_eq!(result.evidence.len(), 2);
}

#[test]
fn worldmap_dma_to_non_vram_is_ignored() {
    let events = vec![TraceEvent::DmaStart {
        channel: 0,
        src_bank: 0x11,
        src_addr: 0x8000,
        dest_reg: 0x80, // WRAM, not VRAM
        vram_addr: 0,
        size: 0x100,
        pc_bank: 0x00,
        pc_addr: 0x8100,
        seq: 1,
    }];
    let result = judge_title_leak(&events);
    assert_eq!(result.verdict, Verdict::Pass);
}

#[test]
fn title_window_filters_late_events() {
    let events = vec![
        TraceEvent::FrameMarker {
            frame_num: 1,
            inidisp: 0x0F,
            vram_hash32: 1,
            cgram_hash32: 2,
            oam_hash32: 3,
            seq: 10,
        },
        TraceEvent::FrameMarker {
            frame_num: 2,
            inidisp: 0x0F,
            vram_hash32: 4,
            cgram_hash32: 5,
            oam_hash32: 6,
            seq: 20,
        },
        TraceEvent::LzCall {
            pc_bank: 0x00,
            pc_addr: 0x9440,
            dp_0b: 0x11,
            dp_0c: 0x818C,
            seq: 21,
        },
    ];

    let result = judge_title_leak_with_window(&events, 1);
    assert_eq!(result.verdict, Verdict::Pass);
    assert!(result.evidence.is_empty());
    assert_eq!(result.analyzed_events, 1);
}

#[test]
fn title_window_includes_early_events() {
    let events = vec![
        TraceEvent::FrameMarker {
            frame_num: 1,
            inidisp: 0x0F,
            vram_hash32: 1,
            cgram_hash32: 2,
            oam_hash32: 3,
            seq: 10,
        },
        TraceEvent::LzCall {
            pc_bank: 0x00,
            pc_addr: 0x9440,
            dp_0b: 0x10,
            dp_0c: 0x818C,
            seq: 9,
        },
    ];

    let result = judge_title_leak_with_window(&events, 1);
    assert_eq!(result.verdict, Verdict::Fail);
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.analyzed_events, 2);
}
