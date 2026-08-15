# 보안 정책

<p align="center">
  <a href="SECURITY.md">English</a> · <strong>한국어</strong>
</p>

Memory Supervisor는 운영체제의 메모리·프로세스 정보만 확인하고 비공개 로컬 상태를 기록하며,
자신의 로컬 제어 범위에서 정확히 확인한 Claude Code·Codex 프로세스만 일시정지하거나 재개합니다.
프롬프트, 응답, 소스 파일, 브라우저 데이터, IDE 내용은 읽지 않습니다. 전체 경계는
[보안과 데이터·제어 경계](docs/guides/security.ko.md)를 참고하세요.

보안 취약점으로 의심되는 문제는 저장소의 **Security → Report a vulnerability** 양식으로
비공개 제보해 주세요. 공개 Issue에는 인증 정보, 알림 토큰, 비공개 소스 코드, 가리지 않은 로컬
경로를 올리지 마세요.

제보에는 영향을 받는 release, 운영체제, 최소 재현 절차와 필요한 경우 민감값을 가린
`memory-status --json` 결과를 포함해 주세요. 민감하지 않은 일반 오류는 공개 Issue를 사용합니다.
