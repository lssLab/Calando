# 기여 방법

<p align="center">
  <a href="CONTRIBUTING.md">English</a> · <strong>한국어</strong> · <a href="CONTRIBUTING.zh-CN.md">简体中文</a> · <a href="CONTRIBUTING.ja.md">日本語</a>
</p>

변경은 제품의 중심 원칙을 지켜야 합니다. 측정된 위험이 실제로 가까워지기 전까지 유용한 작업을
유지하고, 필요할 때는 가장 작은 가역적 제한부터 적용하며, 소유 관계를 확정하지 못한 정보를
프로세스 일시정지 권한으로 사용하지 않습니다.

변경을 제출하기 전에 다음 명령을 실행합니다.

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
bash tests/run.sh
```

Windows 관련 변경은 `powershell -File .\tests\run.ps1`도 실행합니다. `docs/` 아래 문서는 영어
`.md`, 한국어 `.ko.md`, 중국어 간체 `.zh-CN.md`, 일본어 `.ja.md`를 함께 수정하고, 상대 링크가
유효한지 확인하며, 개인 경로·인증 정보나 공개 사용과 무관한 내부 문서를 포함하지 않습니다.

Issue나 Pull Request에는 사용자에게 보이는 변화와 실행한 검증을 간단히 적습니다. 보안상 민감한
내용은 [보안 정책](SECURITY.ko.md)의 비공개 취약점 제보 양식을 사용합니다.
