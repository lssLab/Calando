<p align="center">
  <img src="assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor 로고">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>한국어</strong>
</p>

<p align="center">
  <em>Claude Code와 Codex가 장시간 대규모 작업을 수행할 때 메모리 사용을 관리해,
  터미널이나 앱의 멈춤과 예기치 않은 세션 종료를 예방합니다.</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="최신 릴리스"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 이상"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 이상"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 이상 및 Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="테스트"></a>
  <a href="docs/guides/setup.ko.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, Windows"></a>
  <a href="docs/guides/performance.ko.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="감시 프로그램 계획값 10 MiB 미만"></a>
  <a href="docs/guides/security.ko.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="사용 통계 전송 없음"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT 라이선스"></a>
</p>

<p align="center">
  <a href="#설치-방법"><strong>설치</strong></a> ·
  <a href="#30초-작동-원리">작동 원리</a> ·
  <a href="#자주-쓰는-명령">명령</a> ·
  <a href="#문서">문서</a> ·
  <a href="README.full.ko.md">상세 안내</a>
</p>

## 필요성

Claude Code·Codex CLI 또는 Codex Desktop App에서 큰 작업을 오래 진행하면 서브에이전트,
빌드, 테스트, 브라우저 도구가 한꺼번에 겹칠 수 있습니다. 메모리 여유가 빠르게 사라지면
CLI에서는 터미널이 응답하지 않거나 세션이 종료될 수 있고, Desktop App에서는 App Server를
공유하는 여러 대화가 함께 영향을 받을 수 있습니다. 어느 쪽이든 아직 전달되지 않은 결과와
작업 흐름이 끊길 수 있습니다.

Calando는 메모리 사용량이 높다는 이유만으로 작업을 제한하지 않습니다. CLI와
Desktop App 모두 실제 위험이 가까워질 때 새 작업부터 단계적으로 늦추고, 진행 중인 작업과
결과 전달은 가능한 한 유지해 세션이 갑자기 끊기는 상황을 예방합니다.

보호 강도는 한 번에 올라가지 않습니다. 위험에 가까워질수록 다음 단계를 하나씩 적용하고,
상태가 회복되면 반대 순서로 해제합니다.

1. **자동 파악** — 평소처럼 `claude`나 `codex`를 실행하거나 Codex Desktop App에서 대화를
   시작하면 됩니다. Calando가 CLI 세션과 App 대화를 자동으로 구분하고, 메모리 용량,
   현재 여유, 감소 속도, 다음 작업에 필요한 완충 여유를 파악해 보호 기준을 자동으로 정합니다.
   따라서 사용자가 예산을 설정하거나 상태를 계속 확인할 필요가 없습니다.
2. **제한 없이 실행** — 메모리 사용량만 높고 남은 여유와 감소 속도가 안정적이면 에이전트와
   도구를 제한하지 않습니다.
3. **성능을 유지하며 관찰** — 남은 여유가 충분하면 빠른 감소만으로는 바로 제한하지 않습니다.
   모든 작업을 계속 허용한 채 감소가 이어지는지와 실제 위험이 가까워지는지만 확인합니다.
4. **새 서브에이전트·워크플로·작업 생성부터 대기** — 메모리 여유 감소가 이어져 위험이
   가까워지거나 새 작업을 시작할 여유가 부족해지면, 이 단계의 조치로는 진행 중인 작업을
   건드리지 않고 새 서브에이전트·워크플로·작업 생성만 잠시 미룹니다. 이 단계 자체로
   빌드·테스트 시작을 미루거나 실행 중인 프로그램을 멈추지는 않으므로, 현재 작업을 마치고
   메모리가 회복될 완충 시간을 만듭니다.
5. **작업 범위를 점진적으로 축소** — 위험이 더 가까워지면 먼저 새 서브에이전트·워크플로·작업
   생성을 모두 막습니다.
   메모리 여유 감소가 AI 작업 때문이라는 신뢰할 수 있는 근거가 있거나 사용자가 선택한 한도를
   넘었을 때만, 기존 에이전트가 앞으로 할 일을 `모든 작업 → 새 서브에이전트·워크플로·작업 생성
   없음 → 빌드·테스트처럼 메모리를 많이 쓰는 새 작업 없음 → 전달·조정·상태 확인·중단·복구와
   작은 읽기만` 순서로 좁힙니다.

   서브에이전트 전체를 한꺼번에 제한하지는 않습니다. 시간에 여유가 있으면 한 서브에이전트의
   도구 범위를 다음 호출부터 한 단계만 좁히고, 시간이 부족하면 회복선 전에 필요한 최소 묶음만
   적용한 뒤 메모리를 다시 측정합니다. 선택되지 않은 에이전트와 진행 중인 작업은 그대로 둡니다.
   도구 범위를 먼저 줄일 서브에이전트는 ① 연결 프로그램의 비정상 증가 재확인 ② 현재·직전
   도구가 에이전트·워크플로·작업 생성 또는 빌드·테스트 같은 큰 작업 ③ 이미 더 좁은 단계
   ④ 연결 프로그램의 회복선 도달이 빠름 ⑤ 최근 시작 순으로 고릅니다.

   주 에이전트는 모든 서브에이전트가 가장 좁아진 뒤에도 위험이 남을 때만 제한합니다. 단, 주
   에이전트가 재확인된 주된 원인이고 서브에이전트부터 줄이면 늦을 때는 한 단계 먼저 제한합니다.
   외부 프로그램만 원인이면 기존 AI 작업은 유지하되, 새 서브에이전트·워크플로·작업 생성과
   운영체제 압박이 심각할 때의 큰 작업 시작만 기다립니다.
6. **마지막 수단으로 실행 프로세스 하나만 일시정지** — 그래도 위험이 계속되고 Claude Code·
   Codex에 속한 특정 실행 프로세스의 지속적인 증가가 확인될 때만, 그 프로세스를 종료하지 않고
   잠시 멈춥니다. 조치 내용은 터미널에 바로 표시되고, 주 에이전트도 다음 작업 전에 같은 내용을
   전달받습니다.
7. **반대 순서로 복구** — 메모리 상태가 안정되면 결과 전달만 허용하던 단계부터 작업 범위를
   차례로 다시 열고, 일시정지한 프로세스도 한 번에 하나씩 재개합니다.

목표는 메모리를 적게 쓰게 만드는 것이 아니라, Claude Code·Codex CLI의 터미널 세션과 Codex
Desktop App의 대화를 지키면서 가능한 한 높은 성능을 오래 유지하게 하는 것입니다.

## 설치 방법

사용하는 환경의 **터미널**을 열고 아래 명령 한 줄을 그대로 붙여넣습니다. Git·Python·Rust나
별도 설치 파일을 미리 준비할 필요가 없습니다. 현재 사용자 범위에 설치되므로 `sudo`나 관리자
권한도 필요하지 않습니다.

### Linux · WSL2 · macOS 터미널

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

### Windows PowerShell 터미널

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

명령이 끝나면 백그라운드 서비스가 바로 시작되고, 발견된 Claude Code·Codex Hook도 자동으로
연결됩니다. 실행 중인 AI 프로그램이나 작업은 종료하지 않습니다.

> [!IMPORTANT]
> Windows용 실행 파일은 현재 [SignPath Foundation](https://signpath.org/)의 인증 심사가 진행 중이므로,
> 완료될 때까지 Windows 11에서는 Smart App Control을 끄고 사용해야 합니다.
>
> - **Windows 11:** Smart App Control을 `끔`으로 바꾼 뒤 설치하고, 사용하는 동안 꺼 둡니다.
> - **Windows 10:** Smart App Control이 없으므로 별도 설정 없이 설치할 수 있습니다.
> - **Codex App의 WSL 엔진:** WSL 터미널 명령으로 설치하며 Windows 보안 설정을 바꿀 필요가 없습니다.
>
> Windows 11에서 다시 켤 수 있는 조건과 설치가 차단되는 환경은
> [설치·연결·지원 환경](docs/guides/setup.ko.md#windows-powershell-터미널)에 정리되어 있습니다.

### 사용하는 프로그램 연결

| 사용 대상 | 설치 직후 할 일 |
| --- | --- |
| **Claude Code** | Hook은 자동으로 연결됩니다. 이미 작업 중이었다면 그대로 이어갑니다. |
| **Codex CLI** | 사용할 CLI에서 `/hooks`를 열고 Memory Supervisor 7개가 모두 **신뢰됨·켜짐**인지 확인한 뒤 그대로 작업합니다. 설치 전부터 따로 열려 있던 다른 CLI만 현재 작업을 마친 뒤 한 번 다시 시작합니다. |
| **Codex Desktop App** | **설정 → Hooks**에서 7개를 모두 신뢰하고 켭니다. 기존 대화로 돌아가 원래 하려던 다음 요청을 보내면 되며, App이나 대화를 새로 열 필요는 없습니다. 항목이 아직 보이지 않으면 최대 60초 뒤 설정을 다시 엽니다. |

### 설치 확인

```bash
memory-status --connections
```

- `Core daemon CONNECTED`: 백그라운드 감시 프로그램이 정상입니다.
- `Claude Code CONNECTED`: 지원 버전과 사용자 Hook이 연결됐습니다.
- `Codex CONNECTED`: CLI Hook 7개가 설치·활성화·신뢰됐습니다.
- `Codex App ACTIVE`: App Hook이 준비됐고 기존 대화나 새 대화에서 실제로 호출됐습니다.
- 사용하지 않는 프로그램의 `NOT DETECTED`는 정상입니다.

상태가 다르면 출력에 표시된 항목만 처리하면 됩니다. 전체 예외와 실행 중 설치 방법은
[설치·연결·지원 환경](docs/guides/setup.ko.md)에 있습니다.

## 30초 작동 원리

Calando는 Claude Code나 Codex 앞에 끼어 명령을 대신 실행하지 않습니다. 운영체제
환경마다 작은 감시 프로그램 하나가 옆에서 메모리 여유, 감소 속도, 압박 신호와 AI 작업의 증가를
확인하고, Hook은 새 작업을 시작하기 직전에 최신 판단을 받습니다.

```text
┌──────────────────────┐    memory / PID    ┌──────────────────────┐
│ OS environment       │ ─────────────────► │ Calando              │
└──────────────────────┘                    │ forecast / brake     │
                                            └──────────┬───────────┘
                                                       │ decision
┌──────────────────────┐      pre-run hook  ┌──────────▼───────────┐
│ Claude Code / Codex  │ ─────────────────► │ allow / hold         │
│ CLI / App thread     │ ◄── reason/state ─ │ explain / recover    │
└──────────────────────┘                    └──────────────────────┘
```

1. **자동 파악** — 메모리 용량, 남은 여유, 짧고 긴 감소 속도와 다음 작업의 예상 증가량으로
   보호 기준과 제동거리를 자동 계산합니다.
2. **성능 우선** — 사용량이 높아도 안정적이면 제한하지 않고, 여유가 충분한 빠른 감소도 위험이
   실제로 가까워지는 동안은 관찰만 합니다.
3. **새 작업부터 완충** — 위험이 가까워질 때만 새 서브에이전트·워크플로·작업 생성을 먼저
   기다리게 하고, 필요하면 선택된 에이전트 하나의 다음 도구부터 단계적으로 좁힙니다.
4. **가역적인 최후 수단** — 모든 완충 뒤에도 위험이 계속되고 지속 증가한 Claude Code·Codex
   프로세스를 정확히 확인했을 때만 하나를 종료하지 않고 일시정지합니다.
5. **반대 순서로 복구** — 여유가 안정되면 작업 범위를 한 단계씩 다시 열고, 멈춘 작업도
   하나씩 재개합니다.

### CLI와 Codex Desktop App의 차이

| Claude Code·Codex CLI | Codex Desktop App |
| --- | --- |
| 터미널 세션과 하위 프로세스가 분리되어 있어 원인 프로세스와 제어 대상을 비교적 정확히 연결합니다. | 여러 대화는 App Server 하나 안에서 **논리 thread**로 구분되지만 메모리는 공유합니다. 각 대화가 독립된 CLI 프로세스처럼 따로 측정되는 것은 아닙니다. |
| Hook으로 리드·서브에이전트·도구를 알고, 마지막 수단에서는 다시 확인한 로컬 PID 하나만 멈춥니다. | Hook으로 대화별 새 작업을 제어하고 최근 도구·서브에이전트·활동 시점과 App Server 증가를 함께 대조합니다. 원인을 특정할 수 없으면 대화 하나의 메모리라고 가장하지 않고 공유 위험에 맞춰 새 작업부터 완충합니다. App Server 일시정지는 모든 점진적 조치 뒤에도 지속 증가가 확인된 극히 드문 최후 단계입니다. |

Windows·WSL2·macOS·Linux·VM·격리 컨테이너에는 각각 감시 프로그램이 하나씩 실행됩니다. 같은
물리 메모리를 쓰는 환경을 federation으로 연결하면 각 환경은 자기 프로세스만 제어하면서
새 작업 허용 수준과 복구 상태를 함께 판단합니다.

단계별 정책, 두 아키텍처와 Federation의 전체 구조는
[작동 원리와 아키텍처](docs/guides/how-it-works.ko.md)에 원문 그대로 있습니다.

## 자주 쓰는 명령

| 목적 | 명령 |
| --- | --- |
| 현재 메모리와 보호 상태 | `memory-status` |
| 연결된 모든 환경의 상태 | `memory-status --all` |
| Claude Code·Codex Hook 연결 확인 | `memory-status --connections` |
| 프로그램과 연결을 최신 버전으로 갱신 | `memory-supervisor update` |
| 현재 환경의 보호 기능 끄기·켜기 | `memory-supervisor off` / `memory-supervisor on` |
| 알림 경로 확인 | `memory-supervisor notifications show` |

일시정지된 작업의 확인·자동 복구·수동 재개, 선택형 메모리 하드캡 설정, Discord·Telegram 알림 설정은
[운영·알림·복구](docs/guides/operations.ko.md)에 있습니다.

## 지원 환경과 안전 경계

| 항목 | 지원·경계 |
| --- | --- |
| **운영체제** | Linux·WSL2 64비트 Intel/AMD, macOS Apple Silicon·Intel, Windows 10·11 64비트 Intel/AMD |
| **AI 프로그램** | Claude Code 2.1.217 이상, Codex CLI 0.145.0 이상, Codex Desktop App |
| **상주 메모리** | 운영체제별 실측 최대 5.13 MiB, 설치된 감시 프로그램 하나당 계획값 10 MiB 미만 |
| **외부 통신** | 평상시 감시는 외부 통신과 사용 통계 전송을 하지 않습니다. 사용자가 직접 켠 Discord·Telegram 알림과 설치·업데이트만 해당 서비스에 연결합니다. |
| **읽지 않는 정보** | 프롬프트·대화·모델 응답·프로젝트 파일 내용·프로세스 메모리 내용·Claude/ChatGPT 인증 정보 |
| **제어하지 않는 대상** | 브라우저·IDE 같은 다른 프로그램, 다른 운영체제 환경의 PID, 메모리·스왑·VM 설정 |
| **자동 물리 조치** | 정확히 다시 확인된 Claude Code·Codex 작업 프로세스 하나의 가역적인 일시정지까지입니다. 자동 종료·강제 종료는 하지 않습니다. |

자세한 데이터·프로세스 경계는 [보안](docs/guides/security.ko.md), 실측 방법과 수치는
[성능](docs/guides/performance.ko.md), 플랫폼별 조건은
[설치·연결·지원 환경](docs/guides/setup.ko.md)에 있습니다.

## 문서

| 주제 | 문서 |
| --- | --- |
| 설치, 실행 중 연결, Hook 신뢰, Windows·WSL2·macOS·Linux | [설치·연결·지원 환경](docs/guides/setup.ko.md) |
| 단계적 제동, CLI·Codex App 아키텍처, Blind 제어, Federation | [작동 원리와 아키텍처](docs/guides/how-it-works.ko.md) |
| 터미널·OS·Discord·Telegram 알림, 명령, 일시정지와 복구 | [운영·알림·복구](docs/guides/operations.ko.md) |
| 기존 README의 설명을 한 문서에서 연속으로 읽기 | [상세 안내](README.full.ko.md) |
| 보안·성능·테스트 등 주제별 전문 문서 찾기 | [전체 문서 안내](docs/README.ko.md) |

## 검증

Rust 자동 테스트, 설치·업데이트·제거 E2E, Hook 계약, 저장소 개인정보 경계와 Linux·Windows·macOS
플랫폼 검사를 수행합니다. 공개 검증 범위와 제동거리 실측은
[테스트 범위](docs/testing/test-matrix.ko.md)와
[적응형 제동거리](docs/testing/stopping-distance.ko.md)에서 확인할 수 있습니다.

보안 문제는 [보안 정책](SECURITY.ko.md), 개발 참여 방법은
[기여 안내](CONTRIBUTING.ko.md)를 참고하십시오.

## 라이선스

[MIT](LICENSE)
