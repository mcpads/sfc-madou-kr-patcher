# sfc-madou-kr-patch

SNES **마도물어: 하나마루 대유치원아** (Madou Monogatari: Hanamaru Daiyouchienji, 1996)의 한글 패치 코드베이스.

JP ROM에서 텍스트를 추출하고, 한글 폰트/텍스트를 삽입해 패치된 ROM을 생성하는 Rust CLI 도구입니다.

## 빌드

```bash
cargo build -p madou_patch
cargo test -p madou_patch
cargo clippy -p madou_patch -- -D warnings
```

## 한글 패치 ROM 빌드

다음을 별도로 준비한 뒤 CLI 인자로 지정합니다:

- JP ROM (`.sfc`)
- TTF 폰트 (픽셀폰트 권장, e.g. [Galmuri](https://github.com/quiple/galmuri))
- `translations/` — 번역 JSON (charset은 JSON `ko` 필드에서 자동 추출)

```bash
cargo run -p madou_patch -- patch \
  --rom path/to/original.sfc \
  --ttf path/to/font.ttf --ttf-size 12 \
  --text-all --translations-dir translations \
  --engine-hooks --relocate \
  --output out/madou_ko.sfc
```

## 번역 JSON 형식

`translations/` 디렉토리에 다음 JSON 파일들을 배치합니다.

### bank_{id}_{NN}.json — 뱅크별 대사/텍스트

```json
{
  "bank": "01",
  "entries": [
    {
      "addr": "$01:B400",
      "jp": "もう　ぜんぜんダメ！",
      "ko": "이제 완전 안돼!",
      "category": "HP_STATUS",
      "notes": ""
    }
  ]
}
```

- `addr`: SNES 주소 (`$BB:AAAA` 형식)
- `ko`: 한글 번역 (빈 문자열이면 스킵)
- 제어 태그: `{NL}` (줄바꿈), `{PAGE}` (페이지 넘김), `{SEP}` (구분자), `{BOX:アルル}` (화자 지정) 등

### encyclopedia.json — 도감 (몬스터 이름/설명)

```json
{
  "entries": [
    {
      "id": 0,
      "type": "name",
      "loc_idx": 0,
      "jp": "スキヤポデス",
      "ko": "스키야보데스"
    },
    {
      "id": 0,
      "addr": "$31:BDDF",
      "type": "desc",
      "loc_idx": 1,
      "jp": "かわいいぼうしに\nドングリまなこの...",
      "ko": "귀여운 모자에\n도토리 눈알의..."
    }
  ]
}
```

- `id`: 몬스터 인덱스 (0-35)
- `type`: `"name"` 또는 `"desc"`
- `loc_idx`: 설명 시작 위치 인덱스

### code_patches.json — 코드 내장 문자열

```json
{
  "entries": [
    {
      "id": "save_prompt",
      "pc_addr": "0x9763",
      "slot_size": 9,
      "prefix_bytes": "00 00",
      "ko": "세이브할래",
      "notes": "$01:$9763 セーブするよ → 세이브할래"
    }
  ]
}
```

- `pc_addr`: ROM PC 주소 (hex)
- `slot_size`: 원본 바이트 슬롯 크기
- `prefix_bytes`: 인코딩된 텍스트 앞에 삽입할 원시 바이트 (hex, 공백 구분)

## 프로젝트 구조

```
apps/madou_patch/src/
  main.rs              CLI 서브커맨드 핸들러
  cli.rs               CLI 정의 (clap)
  rom.rs               ROM 로딩, LoROM↔PC 변환
  font_gen.rs          TTF → SNES 2bpp 폰트 생성 (16x16 + 8x8)
  verify.rs            패치 ROM 텍스트 박스 오버플로우 검증
  encoding/            JP/KO 인코딩 테이블, GameChar/Token 코덱
  text/                FF-terminated 텍스트 파서, 뱅크별 문자열 추출
  textbox/             문자 너비 계산, 줄바꿈/페이지 분할 시뮬레이션
  patch/
    font.rs            16x16 타일 패칭 + LZ 압축 유틸
    text.rs            인플레이스 텍스트 교체
    pointer.rs         포인터 테이블 스캐너/재작성
    relocate.rs        오버플로우 텍스트 재배치 + 포인터 리다이렉트
    engine_hooks.rs    텍스트 엔진 훅 (대화/전투/도감)
    asm.rs             65816 ASM 빌더 (~70종 명령어)
    builder.rs         패치 파이프라인 오케스트레이션
    ...                세이브메뉴, 월드맵, 장비/상점 OAM, 배틀 폭 등
```

## CLI 커맨드

| 커맨드 | 설명 |
|--------|------|
| `info` | ROM 헤더 정보 출력 |
| `decode` | 뱅크별 텍스트 디코딩 |
| `patch` | 한글 폰트+텍스트 ROM 패칭 |
| `verify` | 패치 ROM 텍스트 박스 오버플로우 검증 |
| `pointers` | 포인터 테이블 스캔 |
| `ips` | IPS 패치 생성 |
| `generate-font` | TTF → SNES 2bpp 폰트 생성 |

## 패치 배포

완성된 BPS 패치 파일은 [madou-monogatari-kr-patch](https://github.com/mcpads/madou-monogatari-kr-patch) 레포에서 배포됩니다.

## 라이선스

MIT License. [LICENSE](LICENSE) 참조.

게임 ROM 파일은 이 프로젝트에 포함되지 않으며, 사용자가 별도로 준비해야 합니다.
