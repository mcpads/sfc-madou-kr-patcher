# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 프로젝트 개요

SNES 게임 **마도물어: 하나마루 대유치원아**(Madou Monogatari: Hanamaru Daiyouchienji)의 한글 패치 프로젝트. JP ROM에서 텍스트를 추출하고, 한글 폰트/텍스트를 삽입해 IPS 패치를 생성한다.

## 빌드 & 테스트

```bash
cargo build -p madou_patch                 # 빌드
cargo test -p madou_patch                  # 테스트 (630개, 5 ignored)
cargo test -p madou_patch -- --nocapture   # stdout 포함
cargo clippy -p madou_patch -- -D warnings # lint
cargo fmt --all                            # 포맷
```

CLI 실행:
```bash
cargo run -p madou_patch -- <command> [flags]
```

풀 빌드 예시 (TTF 모드 — 권장):
```bash
cargo run -p madou_patch -- patch \
  --rom "roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc" \
  --ttf assets/fonts/Galmuri11.ttf --ttf-size 12 \
  --text-all --translations-dir translations \
  --engine-hooks --relocate \
  --output out/madou_ko.sfc
```

> **charset 자동 수집**: `--charset` 미지정 시 `translations/` JSON의 `ko` 필드에서
> 한글 문자를 자동 추출 + 하드코딩 소스(item/stat_level/sky) 병합, 빈도순 정렬.
> `--charset path` 명시 시 기존 파일 기반 동작.

풀 빌드 예시 (파일 모드 — 레거시):
```bash
cargo run -p madou_patch -- patch \
  --rom "roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc" \
  --font-16x16 assets/font_16x16/ko_font.bin \
  --font-fixed assets/font_16x16/ko_fixed.bin \
  --text-all --translations-dir translations \
  --engine-hooks --relocate \
  --output out/madou_ko.sfc
```

## 아키텍처

단일 크레이트 `apps/madou_patch/`, 의존성: `fontdue 0.9` (TTF 렌더링).

```
apps/madou_patch/src/
  main.rs           서브커맨드 핸들러 (info, decode, patch, verify, pointers, ips, generate-font)
  cli.rs            CLI 구조체 정의 (clap Args, Commands enum)
  rom.rs            ROM 로딩, LoROM↔PC 변환, SnesAddr
  font_gen.rs       TTF → SNES 2bpp 폰트 생성 (16x16 대화용 + 8x8 UI용) + 인코딩 테이블
  verify.rs         패치 ROM 검증 (텍스트 박스 오버플로우 리포트)
  encoding/
    jp.rs           JP 인코딩 (211 FB + ~200 single-byte, const 배열)
    ko.rs           KO 인코딩 (TSV 런타임 로딩) — 테스트: ko_tests.rs
    codec.rs        GameChar/Token enum, decode/encode 함수
  text/
    control.rs      BankConfig, KNOWN_BANKS (12개 뱅크)
    stream.rs       FF-terminated + FC-split 텍스트 파서, Bank $2D 노이즈 필터
    bank.rs         뱅크별 문자열 추출/출력
  textbox/
    layout.rs       문자 너비 계산, 줄바꿈, 페이지 분할 시뮬레이션
    simulator.rs    텍스트 박스 오버플로우 검증
  patch/
    font.rs         16x16 타일 패칭 (Bank $0F) + LZ 압축/디컴프레스 유틸리티
    text.rs         인플레이스 텍스트 교체
    pointer.rs      포인터 테이블 스캐너/재작성 — 테스트: pointer_tests.rs
    pointer_catalog.rs  EN RE 기반 정밀 포인터 카탈로그 + 2바이트 테이블 (Bank $01/$03)
    relocate.rs     오버플로우 텍스트 재배치 + 포인터 리다이렉트 (Bank $01/$03/$31-$33)
    engine_hooks.rs F1/F0 엔진 훅: 세이브메뉴($02) + 인게임대화($00) + 배틀텍스트($03) + 뱅크 오버라이드
    encyclopedia.rs 도감 몬스터 이름/설명 훅 (Bank $03 C6EE + 이름 테이블) — 테스트: encyclopedia_tests.rs
    savemenu.rs     세이브 메뉴 UI 패치 (CHR/타일맵/DMA 훅, Bank $19) — 테스트: savemenu_tests.rs
    item.rs         아이템 이름 테이블 패치 ($2B:$FD8B, 18개 고정폭)
    worldmap.rs     월드맵 장소 이름 훅 (하늘다람쥐+메뉴+OAM 말풍선, Bank $33/$32/$10) — 테스트: worldmap_tests.rs
    options_screen.rs 스탯/마법/옵션 화면 LZ 인터셉트 훅 (CHR 오버레이+TM 리맵, Bank $1C) — 테스트: options_screen_tests.rs
    equip_oam.rs    장비 화면 OAM 스프라이트 한글화 (ROM→VRAM DMA Ch6, Bank $25) — 테스트: equip_oam_tests.rs
    shop_oam.rs     상점 화면 OAM 스프라이트 한글화 (ROM→VRAM DMA Ch6, Bank $25) — 테스트: shop_oam_tests.rs
    battle_width.rs 배틀 대사창 width/height 동적 훅 + screen-boundary clamping 위치 보정 + $FD Choice 스킵 (Bank $03 $FB00, 182B, 1 width=2 tilemap cols) — 테스트: battle_width_tests.rs
    choice_highlight.rs 선택지 하이라이트 폭 인라인 패치 (Bank $01 DMA 크기 4곳, $28=풀라인) — 테스트: choice_highlight_tests.rs
    hook_common.rs  LZ 인터셉트 훅 공통 유틸 (JSL_LZ_BYTES, lookup_lz_source) — savemenu/options_screen 공유
    asm.rs          65816 ASM 빌더 (~70종 Inst enum → 바이트 어셈블러, Label 기반 분기, LdxImm16/LdyImm16/Mvn/AdcDp/SbcDp 포함)
    tracked_rom.rs  TrackedRom 래퍼 (쓰기 추적 + Expect 사전 조건) — 테스트: tracked_rom_tests.rs
    rom_regions.rs  ROM 쓰기 영역 충돌 검증 (RomRegionTracker) — 테스트: rom_regions_tests.rs
    translation.rs  번역 JSON 로딩/변환 파이프라인 — 테스트: translation_tests.rs
    translation_json.rs  JSON 번역 로더 + charset 자동 수집 (collect_charset_from_translations)
    ips.rs          IPS 패치 생성
    builder.rs      PatchConfig 기반 파이프라인 오케스트레이션 + 코드 내장 문자열 패치 + auto_collect_charset
```

### 패치 훅 시스템 요약

세 가지 패턴이 존재:

1. **텍스트 엔진 훅** (Bank $32): 16x16 대화/전투/도감 텍스트. F1/F0 디스패치 + 뱅크 오버라이드.
2. **LZ 인터셉트 훅**: 8x8 UI 화면. JSL $009440 사이트에 DMA 훅 삽입, KO CHR+타일맵 교체.
3. **OAM DMA 훅**: 4bpp OAM 스프라이트. JSL $009440 사이트에 ROM→VRAM Direct DMA Ch6 삽입, KO 타일 교체.
4. **배틀 텍스트 크기 훅** (Bank $03): 커맨드 $48 width/height를 KO 텍스트 런타임 스캔으로 동적 설정 + display_params screen-boundary clamping 위치 보정 (1 width unit = 2 tilemap columns).

| 훅 대상 | 패턴 | 사이트 | 구현 파일 |
|---------|------|--------|----------|
| 대화/전투 텍스트 | 텍스트 엔진 | $00:$CCA3, $00:$CECD | engine_hooks.rs |
| 도감 | 텍스트 엔진 | $03:$C742, $03:$B626 | encyclopedia.rs |
| 세이브 메뉴 | LZ 인터셉트 | $01:$820A 외 3곳 | savemenu.rs |
| 월드맵 (하늘다람쥐 flag-clear) | LZ 인터셉트 | $10:$B562 → JSL $10:$D000 | worldmap.rs |
| 월드맵 (하늘다람쥐 KO loader) | JML 훅 | $03:$CC56 → JML $10:$D020 | worldmap.rs |
| 월드맵 (메뉴) | LZ 인터셉트 | $03:$C3F0 | worldmap.rs |
| 스탯/마법 화면 | LZ 인터셉트 | $01:$820A | options_screen.rs |
| 옵션 화면 | LZ 인터셉트 | $01:$8F46, $01:$8F83 | options_screen.rs |
| 스탯/마법 TM6 리맵 | LZ 인터셉트 | $01:$8322 | options_screen.rs |
| 장비 OAM | OAM DMA | $00:$83FF | equip_oam.rs |
| 상점 OAM | OAM DMA | $02:$8ADA | shop_oam.rs |
| 배틀 대사창 크기 | 텍스트 스캔 | $03:$9D84 | battle_width.rs |
| 선택지 하이라이트 | 인라인 패치 | $01:$DE7B/$DEC3/$DF0E/$DF56 | choice_highlight.rs |

### 8x8 2bpp 타일 포맷 (UI 화면용)

JP 원본 규칙 — **반드시 준수**:
- BP1 = 항상 $FF (투명 픽셀 없음, 완전 불투명)
- Color 3 (BP0=1, BP1=1) = 글자 획 (전경)
- Color 2 (BP0=0, BP1=1) = 배경 채움
- Color 0 = 투명 → JP 텍스트 타일에서 사용하지 않음

### CLI 커맨드

| 커맨드 | 설명 |
|--------|------|
| `info` | ROM 헤더 정보 출력 |
| `decode` | 뱅크별 텍스트 추출/디코딩 |
| `patch` | 한글 폰트+텍스트 ROM 패칭 (`--relocate`, `--engine-hooks`) |
| `verify` | 패치 ROM 텍스트 박스 오버플로우 검증 |
| `pointers` | 포인터 테이블 스캔/덤프 |
| `ips` | IPS 패치 생성 (원본↔패치 diff) |
| `generate-font` | TTF → SNES 2bpp 폰트/인코딩 생성 (`--ttf`, `--ttf-size`) |

## 코딩 규약

- Rust 2021 edition, resolver v2
- `snake_case`(함수/모듈), `PascalCase`(타입), `SCREAMING_SNAKE_CASE`(상수)
- `Result<T, E>` 기반 에러 전파. `unwrap()`/`expect()`는 테스트 외 금지
- 테스트는 별도 파일 `*_tests.rs`에 분리 (`#[cfg(test)] #[path = "xxx_tests.rs"] mod tests;`)
- 커밋 접두사: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`

## 에셋 관리

- `roms/`, `out/`, `tools/asar/`는 gitignore 대상
- 저작권 ROM 커밋 금지. 코드/문서/패치(`.ips`)만 커밋
