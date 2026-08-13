# 마도물어 하나마루 대유치원아 (슈퍼패미컴) 한글 패처

슈퍼패미컴판 《마도물어 하나마루 대유치원아》 일본판에 한글 패치를 적용하는 Rust 코드입니다. 원본 검증, 문자 인코딩과 폰트 생성, 텍스트 재배치, 65816 훅, Expected Write 검사와 패치 생성을 제공합니다.

배포용 BPS와 적용 방법은 [마도물어 시리즈 한글 번역 프로젝트](https://github.com/mcpads/madou-monogatari-kr-patch#마도물어-하나마루-대유치원아-snes)에서 제공합니다.

## 빌드와 테스트

```bash
cargo build -p madou_patch
cargo test -p madou_patch
cargo clippy -p madou_patch -- -D warnings
```

사용 가능한 명령과 옵션은 다음과 같이 확인할 수 있습니다.

```bash
cargo run -p madou_patch -- --help
cargo run -p madou_patch -- patch --help
```

## 라이선스

이 저장소의 소스 코드는 [MIT License](LICENSE)로 제공합니다.
