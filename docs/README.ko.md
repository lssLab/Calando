# 문서 안내

<p align="center">
  <a href="README.md">English</a> · <strong>한국어</strong> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a>
</p>

처음부터 모든 문서를 읽을 필요는 없습니다. 사용 목적에 맞는 아래 세 문서 중 하나에서
시작하면 됩니다.

| 시작점 | 이런 때 읽습니다 |
| --- | --- |
| [설치·연결·지원 환경](guides/setup.ko.md) | 처음 설치할 때, 실행 중이던 Claude Code·Codex를 연결할 때, Hook 신뢰·Windows·WSL2·macOS·Linux 조건을 확인할 때 |
| [작동 원리와 아키텍처](guides/how-it-works.ko.md) | 단계적 제동, CLI와 Codex App의 차이, Blind 제어와 Federation 구조를 이해할 때 |
| [운영·알림·복구](guides/operations.ko.md) | 상태·명령·알림을 설정하거나 일시정지된 작업과 자동 복구를 다룰 때 |

기존 README의 설명을 처음부터 끝까지 한 문서에서 읽으려면
[상세 안내](detailed-guide.ko.md)를 사용합니다.

<details>
<summary><strong>주제별 전문 문서 모두 보기</strong></summary>

### 구조와 플랫폼

- [아키텍처](guides/architecture.ko.md) — 터미널, 에이전트, Hook, 감시 프로그램의 연결 구조
- [Codex Desktop App](guides/codex-app.ko.md) — 공유 App Server 안의 대화별 관찰과 제어
- [여러 환경 연동](guides/federation-topology.ko.md) — 한 머신의 여러 커널과 터미널을 함께 판단하는 원리
- [플랫폼](guides/platforms.ko.md) — Windows, WSL2, Linux, macOS와 VM·컨테이너
- [자원 경계](guides/resource-boundaries.ko.md) — 자동 기준, 사용자 상한과 복구 경계

### 연결과 운영

- [Claude Code 연결](guides/usage-claude.ko.md) — Claude Code Hook과 연결 확인
- [Codex 연결](guides/usage-codex.ko.md) — Codex CLI·Desktop App Hook과 신뢰
- [알림](guides/notifications.ko.md) — 터미널, 운영체제, Discord, Telegram
- [Windows 실행 신뢰](guides/windows-signing.ko.md) — 서명 전 Windows 실행 파일과 Smart App Control

### 보안·성능·검증

- [보안](guides/security.ko.md) — 확인하는 정보, 제어 범위와 다루지 않는 정보
- [성능](guides/performance.ko.md) — 상주 메모리와 Hook·상태 조회 지연
- [테스트 범위](testing/test-matrix.ko.md) — 공개 테스트가 확인하는 기능과 플랫폼
- [적응형 제동거리](testing/stopping-distance.ko.md) — 속도에 따른 제동 계산과 통제된 실측

</details>

모든 공개 문서는 영어 `.md`, 한국어 `.ko.md`, 중국어 간체 `.zh-CN.md`, 일본어 `.ja.md`로 제공합니다.
