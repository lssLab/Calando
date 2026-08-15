# 운영·알림·복구

<p align="center">
  <a href="operations.md">English</a> · <strong>한국어</strong> · <a href="operations.zh-CN.md">简体中文</a> · <a href="operations.ja.md">日本語</a>
</p>

## 알림

Memory Supervisor는 메모리 수치가 바뀔 때마다 알리지 않습니다. 실제 보호 조치가 시작되거나
완전히 해제됐을 때, 또는 연결·보호 상태에 사용자의 확인이 필요할 때만 한 번씩 알립니다.

| 경로 | 어디에 표시되는가 | 언제 무엇을 알리는가 |
| --- | --- | --- |
| 터미널 | 조치 대상 Claude Code·Codex CLI가 실행 중인 정확한 터미널 | 프로세스를 일시정지·재개하거나 리드의 일시정지를 한 번 풀어 상태를 확인할 때 즉시 이유, PID와 복구 명령을 표시합니다. 항상 켜져 있습니다. |
| OS | Linux·WSL·macOS·Windows의 데스크톱 알림 | 보호 조치가 처음 시작되거나 해제됐을 때, federation 연결이나 보호 기능에 확인할 문제가 생겼을 때 표시합니다. 데스크톱 알림을 사용할 수 있을 때 동작하는 선택 경로입니다. |
| Telegram | 사용자가 연결한 봇의 개인 대화 또는 그룹 | 중요한 조치의 시작·복구와 연결·보호 문제를 알립니다. 메모리 상태, 조치 이유, 대상이 있으면 PID와 다음 행동을 남기므로 자리를 비운 동안에도 확인할 수 있습니다. |
| Discord | 사용자가 연결한 채널·웹후크 또는 개인 메시지 | Telegram과 같은 중요한 조치·복구·확인 필요 사항을 남깁니다. 팀 채널이나 개인 알림에 쓰는 선택 경로입니다. |

터미널·OS·Telegram·Discord는 사건이 기록된 직후 전달을 시도합니다. 같은 상태가 계속되는 동안은
반복해서 보내지 않으며, 리드 에이전트는 다음 Hook이 실행될 때 같은 상황과 복구 상태를 전달받습니다.
터미널 알림은 별도 설정 없이 항상 유지됩니다. 선택 알림은 아래 명령으로 연결하고 시험합니다.

```bash
memory-supervisor notifications show
memory-supervisor notifications routes os
memory-supervisor notifications discord-webhook
memory-supervisor notifications telegram
memory-supervisor notifications test
```

Discord 웹후크 URL과 Discord·Telegram 봇 토큰은 명령줄에 붙이지 않고 명령 실행 뒤 나타나는
숨김 입력창에 넣습니다. 설정은 다음 알림부터 바로 적용되며 Supervisor나 AI 프로그램을 다시
시작할 필요가 없습니다. 경로 선택, 끄는 방법, Discord 채널·DM, Telegram 그룹 연결과 오류 해결은
[알림 설정](notifications.ko.md)에 있습니다.

## Claude Code·Codex의 Skill과 명령

설치기는 자동으로 판단하는 **Hook**, 에이전트가 상태를 이해하고 설명하게 하는 **Skill**, 사용자가
바로 호출하는 **단축 명령**을 함께 연결합니다. Hook은 사용자가 부르지 않아도 작동하고, Skill은
메모리 정책을 직접 집행하지 않습니다.

| 사용하는 곳 | 입력하는 방법 | 하는 일 |
| --- | --- | --- |
| Claude Code | “메모리 상태 확인해줘”, `/memory-supervisor` 또는 `/memory-status` | 설치된 Skill이나 단축 명령으로 전체 상태를 읽고 원인·자동 복구·필요한 명령을 설명합니다. |
| Codex CLI | `$memory-supervisor 메모리 상태 확인`; `/skills`에서 설치 여부 확인. `/prompts:memory-status`는 호환용 단축 명령 | Codex의 기본 Skill 경로로 같은 상태 확인 절차를 실행합니다. Hook 신뢰·활성화는 별도로 `/hooks`에서 관리합니다. |
| Codex Desktop App | 대화에서 `$memory-supervisor 메모리 상태 확인` 또는 자연어로 요청 | CLI와 같은 사용자 Skill을 대화별로 사용합니다. 별도 App용 Skill은 없으며 Hook은 **설정 → Hooks**에서 관리합니다. |
| 운영체제 터미널 | `memory-status`, `memory-supervisor ...` | Skill이 아니라 실제 상태 확인·설정·복구 명령입니다. `resume`·`terminate`·`kill`은 사용자가 명시적으로 요청한 경우에만 실행합니다. |

Skill은 `memory-status --all` 결과를 읽어 현재 원인과 다음 행동을 설명하지만, 사용자의 허락 없이
프로세스를 재개하거나 종료하지 않습니다. 설치 뒤 Claude Code나 Codex를 새로 추가했다면
`memory-supervisor update`로 연결하고 `memory-status --connections`에서 확인합니다. 자세한 차이는
[Claude Code 사용 안내](usage-claude.ko.md)와 [Codex 사용 안내](usage-codex.ko.md)에 있습니다.

## 보안

Memory Supervisor가 확인하는 것은 운영체제의 메모리와 프로세스 정보, Claude Code·Codex Hook이
전달하는 세션·에이전트·도구·작업 경로와 연결 상태, 실행할 명령의 앞부분뿐입니다. 이 정보로 새
작업 허용 여부와 정확한 제어 대상을 판단합니다.

자동 제어는 앞으로 시작할 Claude Code·Codex 작업을 잠시 미루는 것과, 최후의 보호 단계에서
확인된 로컬 작업 프로세스 하나를 일시정지·재개하는 것까지입니다. 자동으로 프로그램을 종료하거나
다른 프로그램을 제어하지 않습니다. 평상시 감시에는 외부 통신이 없으며, GitHub 설치·업데이트와
사용자가 켠 Discord·Telegram 알림만 네트워크를 사용합니다.

**Memory Supervisor가 확인하고 제어하는 범위는 여기까지이며, 그 외는 다루지 않습니다.**
프롬프트·대화·모델 응답과 Hook에 포함될 수 있는 파일 내용은 제어 판단에 사용하지 않으며 어디에도
남기지 않습니다. 프로젝트 파일과 프로세스 메모리를 직접 열어 읽지 않으며,
브라우저·IDE 내부 데이터, Claude·ChatGPT 인증 정보와 운영체제의 커널·메모리·스왑·방화벽
설정도 확인하거나 변경하지 않습니다. 저장 정보, federation 공유 범위와 안전장치는
[보안과 데이터·제어 경계](security.ko.md)에 자세히 정리했습니다.

## 제어와 복구

메모리 상황이 안정되면 일시정지된 작업을 하나씩 자동으로 다시 이어갑니다. 리드 에이전트가 자기
메모리 증가로 멈춘 경우에는 일시정지를 한 번 자동으로 풀어 증가세가 가라앉았는지 확인합니다.
메모리가 다시 빠르게 늘어나면 리드를 다시 멈추고 사용자가 결정할 때까지 기다립니다. 직접 재개할
때는 먼저 현재 상태를 확인하고, 표시된 PID로 재개합니다.

```bash
memory-status
memory-supervisor resume [pid]
```

리드 에이전트 일시정지는 의도적으로 매우 드뭅니다. 새 작업 대기와 서브에이전트·도구 제어를
단계적으로 적용했는데도 위험이 계속되고, 같은 리드의 지속적인 메모리 증가와 정확한 터미널이
확인됐을 때만 사용하는 **최후의 보호 단계**입니다. 대부분은 그 전에 작업 범위 축소나 작업자
일시정지·자동 복구로 끝납니다.

Claude Code나 Codex를 실수로 종료했다면 해당 CLI가 대화를 복원하고, `SessionStart` hook이
보존된 메모리 사건과 현재 판단을 주 에이전트에게 한 번 다시 전달합니다.

```bash
claude --resume
codex resume
```

의도적으로 보호 기능을 끄려면 아래 두 명령만 사용합니다. `off`는 백그라운드 서비스와 자동 시작을
끄되 설치된 Claude Code·Codex hook은 그대로 두고 조용히 통과시킵니다. 이 상태는 재부팅과
`memory-supervisor update` 뒤에도 유지되며, `on` 한 번으로 다시 켭니다.

```bash
memory-supervisor off
memory-supervisor on
```

Supervisor가 관리하는 일시정지 PID나 처리 중인 프로세스 조치가 있으면 `off`는 이를 방치하지 않고
거부합니다. 먼저 표시된 PID를 재개하거나 종료해야 합니다. 사용자가 `off`를 실행하지 않았는데
서비스가 중단됐다면 기존처럼 hook이 10초 뒤 오래된 판단을 버리고 **보호 기능을 사용할 수 없음**을
알립니다.

```bash
memory-status --connections
memory-supervisor update
```

고정 한도가 필요할 때만 한 환경의 Claude Code와 Codex 전체가 함께 쓸 메모리 상한을 켤 수 있습니다.

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

명령은 제어 대상에 따라 나뉩니다.

- `memory-status` 묶음은 상태만 읽습니다. 로컬 원인, federation, 서비스·hook·알림 연결을 확인합니다.
- `on`과 `off`는 현재 설치 환경의 전체 전원을 제어합니다. 연결된 모든 Claude Code·Codex 세션에
  한 번에 적용되며, 다른 OS·WSL 배포판·가상 머신은 그 환경 안에서 따로 실행합니다.
- `resume`은 Supervisor가 일시정지한 같은 프로그램을 이어서 실행합니다. `terminate`와 `kill`은
  원인을 검토한 뒤 사용자가 선택하는 종료 명령입니다.
- `budget`은 현재 환경의 Claude Code와 Codex 전체에만 선택형 상한을 적용하며 컴퓨터 전체나
  Chrome을 제한하지 않습니다.
- `update`는 서비스와 감지된 CLI 연결을 다시 적용하고, `notifications`는 OS·Discord·Telegram
  선택 경로를 제어합니다. 주 에이전트용 hook과 정확한 터미널 알림은 항상 유지됩니다.

## 자주 쓰는 명령

| 명령 | 용도 |
| --- | --- |
| `memory-status` | 로컬 상태, 원인, 다음 조치 |
| `memory-status --all` | 같은 컴퓨터에서 함께 동작하는 Windows·WSL·가상 머신·컨테이너 상태 |
| `memory-status --connections` | 백그라운드 서비스·AI CLI·알림 연결 상태 |
| `memory-supervisor on` / `off` | 현재 환경의 보호 기능을 지속적으로 켜거나 끄기. OFF에서도 hook 연결은 유지되고 통과 모드로 동작 |
| `memory-supervisor update` | 업데이트와 감지된 CLI 재연결 |
| `memory-supervisor budget` | 현재 환경의 자동 용량과 선택한 상한 확인 |
| `memory-supervisor budget set <GiB>` / `budget off` | 현재 환경의 Claude Code·Codex 통합 상한 설정·해제 |
| `memory-supervisor resume [pid]` | Supervisor가 일시정지한 프로세스 재개. PID 생략은 대상이 하나일 때만 가능 |
| `memory-supervisor terminate <pid>` | 검증된 관리 프로세스 정상 종료 |
| `memory-supervisor kill <pid>` | 최후 수단으로 검증된 관리 프로세스 강제 종료 |
| `memory-supervisor notifications show` | 비밀값을 가린 알림 설정 |
| `memory-supervisor notifications routes <all\|none\|경로>` | OS·Discord·Telegram 선택 경로 설정 |
| `memory-supervisor notifications test` | 활성화한 선택 알림 경로 시험 |
| `memory-supervisor uninstall` | 상태를 보존하고 소유한 서비스·CLI 연결 제거 |

## 검증 방법

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust 단위·통합 테스트와 설치 테스트는 정책, 프로세스 안전, Claude Code·Codex 연결, 여러 환경
연동, 복구, 배포 묶음을 확인합니다. GitHub Actions는 Linux x86-64, Windows x86-64, Apple
Silicon macOS와 Rosetta 기반 macOS x86-64 빌드·플랫폼 계약을 검사합니다. 실제 메모리
고갈선은 제한된 실머신 검증과 결정적 시뮬레이션을 함께 사용합니다. 자세한 범위는
[테스트 범위](../testing/test-matrix.ko.md)에 있습니다.
