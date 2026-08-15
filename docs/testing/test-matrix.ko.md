# 테스트 범위

<p align="center">
  <a href="test-matrix.md">English</a> · <strong>한국어</strong> · <a href="test-matrix.zh-CN.md">简体中文</a> · <a href="test-matrix.ja.md">日本語</a>
</p>

공개 테스트는 정책 계산뿐 아니라 설치, 훅 연결, 복구, 여러 환경 연동까지 제품의 전체 경로를
확인합니다.

| 영역 | 확인하는 내용 |
| --- | --- |
| 정책·제동 | 메모리 용량과 감소 속도별 `ALLOW`·`HOLD`·`DRAIN`, 완충 순서, 원인 판별, 한 번에 선택하는 대상 수 |
| 프로세스 안전 | PID와 시작 정보 재확인, 일시정지 소유권, 한 번에 한 후보, 자동·수동 복구 |
| Claude Code | 훅 병합, 지원 이벤트, fail-open, 설치·업데이트 뒤 연결 진단 |
| Codex CLI | 일곱 훅의 경로와 이벤트, 신뢰·활성 상태 진단, 기존 세션 연결 |
| Codex Desktop App | 공유 App Server 발견, 논리 스레드 분리, 정확·추정·blind 후보 처리, 중복 창과 세대 교체 |
| 설치·전원 | Unix와 Windows 설치, 업데이트, 제거, `on`·`off`, 기존 사용자 설정 보존 |
| 여러 환경 연동 | 커널별 로컬 제어, 같은 물리 메모리의 새 작업 허용 상태 공유, 오래되거나 잘못된 peer 무시 |
| 알림·보안 | 정확한 터미널 확인, 중복 알림 억제, 선택 알림 경로, 비공개 상태 파일 권한 |
| 배포 묶음 | 공개 파일만 포함하는 source archive, checksum, 필수 플랫폼 실행 파일 목록 |
| 저장소 공개 안전 | 공개 파일 허용 목록, 개인 경로·인증 정보 금지, 영어·한국어·중국어 간체·일본어 문서 정합성, 내부 링크 유효성 |

GitHub Actions는 Linux x86-64, Windows x86-64, Apple Silicon macOS, Rosetta 기반 macOS x86-64에서
Rust 빌드·테스트와 플랫폼 계약을 검사합니다. 운영체제 신호나 실제 메모리 고갈선처럼 hosted
runner가 안전하게 재현하기 어려운 항목은 결정적 테스트와 제한된 실머신 테스트를 함께 사용합니다.

제동거리 계산과 통제된 실측 결과는 [적응형 제동거리](stopping-distance.ko.md)를 참고하세요.
