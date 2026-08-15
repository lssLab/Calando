# Codex 사용 안내

<p align="center">
  <a href="usage-codex.md">English</a> · <strong>한국어</strong>
</p>

## 지원 기준

Memory Supervisor는 다음 명령에서 Hook 기능이 stable이자 enabled로 표시되는 Codex CLI
**0.145.0 이상**을 지원합니다.

```bash
codex --version
codex features list | grep '^hooks'
codex update
```

설치기도 같은 조건을 검사합니다. 지원하지 않거나 Hook이 꺼진 버전에는 Memory Supervisor Codex
Hook을 연결하지 않습니다. 재설치 때 지원 버전보다 낮아진 것이 확인되면 이전에 설치기가 소유하던
Hook을 제거해 보호 중이라고 잘못 표시하지 않습니다. 원래 설치 방식에서 `codex update`를 지원하지
않으면 같은 패키지 관리자나 설치기로 Codex를 업데이트한 뒤 `memory-supervisor update`를 실행합니다.

현재 Codex Hook은 `PreToolUse`에서 로컬 함수 도구를 관찰할 수 있습니다. 0.145.0은 thread 생성
Hook 입력에 선택적인 서브에이전트 번호를 제공하고 root thread에서는 생략합니다. 설치된 matcher는
일반 작업을 입력 내용에 따라 보존할 수 있도록 의도적으로 넓습니다.

```text
.*
```

적응형 새 작업 판단이 `GREEN` 또는 `YELLOW`이면 새 확장을 조용히 허용합니다. `ORANGE` 또는
`RED`에서는 실제 확장 호출만 회복을 잠깐 기다린 뒤 `ADMISSION_DEFERRED`인 유효한 거부 판단을
돌려줍니다. 논리 자식 상태는 별도로 그 에이전트의 새 확장, 새 고메모리 작업, 넓은 탐색 또는 실제
수정 작업 가운데 제외된 종류만 거부할 수 있습니다. 사용률 색이 RED여도 안정적인 작업은 적응형
판단이 허용할 수 있으며, 결과·메시지·상태·중단·취소·복구는 모든 상태에서 사용할 수 있습니다.

공식 기준 문서:

- [Codex Hook과 도구 범위](https://learn.chatgpt.com/docs/hooks)
- [Codex 고급 설정](https://developers.openai.com/codex/config-advanced#hooks)

## 설치와 신뢰

README의 한 줄 설치기를 실행합니다. Supervisor를 먼저 설치한 뒤 Codex를 추가했다면
`memory-supervisor update`를 실행합니다. 이후에는 평소 Codex 명령을 사용합니다.

```bash
codex
codex exec "your task"
```

Hook은 `CODEX_HOME`이 설정돼 있으면 `$CODEX_HOME/hooks.json`, 아니면
`~/.codex/hooks.json`에 병합됩니다. 다른 그룹은 보존하고 기존 파일을 백업한 뒤 원자적으로
교체합니다. Codex는 관리형이 아닌 Hook이 새로 생기거나 정의가 바뀌면 검토와 신뢰를 요구합니다.
사용자는 대화형 CLI에서 직접 `/hooks`를 열어 현재 명령을 검토·신뢰하고, 꺼진 항목이 있으면
켜야 합니다. 설치기는 이 사용자 결정을 자동화하지 않으며 Codex를 다시 시작하는 것으로 신뢰를
대신할 수도 없습니다.

`/hooks` 절차는 Codex CLI용입니다. Codex Desktop App에서는 **설정 → Hooks**에서 같은 결정을
직접 합니다. App에서 활성화나 신뢰 상태를 저장하면 공유 App Server에 이미 올라간 모든 대화가
Hook 설정을 다시 읽으므로, 기존 대화에서 다음 요청을 계속하면 됩니다. 새 대화나 App 재시작은
필요하지 않지만 이미 지나간 `SessionStart`를 다시 실행하지는 않습니다. 반대로 CLI의 `/hooks`
저장은 그 CLI 프로세스만 갱신하고 별도로 실행 중인 Desktop App은 갱신하지 않습니다. App 설정
저장도 다른 CLI 프로세스를 갱신하지 않습니다. 두 화면이 모두 설치 전부터 실행 중이었다면 App
설정에서 먼저 승인하고, 기존 CLI 프로세스는 작업을 마친 뒤 해당 프로세스만 다시 시작합니다.
다른 프로세스가 승인 기록을 썼는데 현재 화면에는 저장할 변경이 전혀 없으면, 이미 실행 중이던
App이나 CLI를 한 번 다시 시작해 공유 신뢰 기록을 읽게 합니다.

Memory Supervisor는 소유한 이벤트 7개를 하나의 연결 기준으로 봅니다.
`memory-status --connections`는 각 이벤트의 정의, 활성화 상태와 현재 정확한 신뢰 hash를
확인합니다. 따라서 누락·중복·비활성·미신뢰·변경 항목이 하나라도 있으면 Codex를 `CONNECTED`로
표시하지 않습니다. 다른 한 이벤트에서 최근 호출이 왔다는 이유만으로 App 경로를 `ACTIVE`라고
표시하지도 않습니다. 정의가 누락되거나 오래됐으면 `memory-supervisor update`, 비활성 또는 미신뢰
항목은 `/hooks`에서 고칩니다. 정상 `SessionStart` Hook은 같은 감사를 실행하고, 남은 항목과 영향을
리드와 사용자에게 알리며 정확한 해결 방법을 제시합니다.

`memory-supervisor update` 뒤에는 항상 `memory-status --connections`를 실행합니다. Codex는
Supervisor 버전 번호가 아니라 현재 Hook 정의를 신뢰합니다. 같은 명령 뒤의 바이너리만 바뀌면
다시 승인할 필요가 없습니다. 설치기가 명령·matcher 또는 다른 hash 대상 필드를 바꾸면 해당 CLI나
App 화면에서 새 정의를 다시 신뢰해야 합니다. 프로세스 재시작은 같은 `CODEX_HOME`을 공유하는
다른 프로세스가 이미 저장한 승인을 읽게 할 수는 있지만, 승인을 새로 만들 수는 없습니다.

생성되는 각 Codex 명령에는 자신이 속한 `hooks.json` 절대경로도 들어갑니다. Gate는 이 경로와
현재 프로세스의 `CODEX_HOME`을 비교합니다. 이렇게 하면 한 환경의 사용자 Hook을 다른 Codex
home이 프로젝트 Hook으로 다시 발견해 두 번 실행하는 일을 막습니다. 다른 운영체제 경로가 설치되지
않았을 때는 그 명령 필드가 다른 셸 오류를 내는 대신 유효한 no-op으로 남습니다. Windows와 WSL이
의도적으로 `CODEX_HOME`을 공유하면 각 네이티브 경로를 모두 보존하되, 각 Supervisor는 자기 경로와
PID 공간만 감사·제어합니다. Federation은 여전히 최신 새 작업 판단만 공유하며 Hook 소유권이나
다른 환경의 PID 권한을 공유하지 않습니다.

사용자 수준 Hook은 프로젝트 신뢰와 별도로 적용됩니다. 프로젝트 로컬 `.codex` Hook 계층은 신뢰하지
않은 저장소에서 무시되지만 이 설치기는 프로젝트 로컬 계층에 의존하지 않습니다. Codex가 신뢰된
모든 출처의 일치 Hook을 병합하므로, 위 출처 경로 검사는 문서상 경고가 아니라 실제 설치 명령에
포함됩니다.

## Hook 이벤트

| 이벤트 | 용도 |
| --- | --- |
| `SessionStart` | 시작 시 계약을 전달하고 resume·clear·compact에서 아직 전달하지 않은 사건을 주입 |
| `UserPromptSubmit` | 메모리 압박 또는 아직 전달하지 않은 일시정지·재개 사건을 알림 |
| `PreToolUse` | 로컬 함수 도구를 분류하고, 시스템 새 작업 판단 또는 정확한 논리 에이전트 상태가 제외한 앞으로의 작업만 거부 |
| `SubagentStart` / `SubagentStop` | 수명주기 관찰. 시작에는 RED에서만 같은 12초 보조 대기를 적용 |
| `PostToolUse` | 완료된 작업을 지연하지 않고 아직 전달하지 않은 사건 버전을 전달 |
| `Stop` | 정상 종료를 막지 않고 현재 논리 수명주기 기록을 닫음 |

ORANGE는 `SubagentStart`에서 이미 허용된 작업자를 지연하지 않습니다. Codex에는 도구 실행 뒤의
협력 대기가 없습니다. RED 압박은 새 작업 전 판단과 독립된 PID 최후 보호 장치가 처리합니다.
설치된 명령 Hook은 선택 항목인 `statusMessage`를 넣지 않으므로, 평상시 Pre/Post Hook 실행이
TUI에 Memory Supervisor spinner 문구를 남기지 않습니다. 실제 조치나 아직 전달하지 않은 사건이
있을 때만 메시지가 나타납니다. 기존 세션에 `Running PreToolUse/PostToolUse hook` 문구가 계속
보이면 `memory-supervisor update`를 다시 적용하고, hash가 바뀌었다면 `/hooks`에서 검토한 뒤 화면을
닫고 그 CLI 세션을 계속합니다. 이미 별도로 열려 있어 해당 프로세스의 재적재를 받지 못한 다른
CLI만 다시 시작합니다.

모든 명령 래퍼는 fail-open으로 동작합니다. 데몬 누락, 오래된 상태, 잘못된 Hook 입력 또는 Rust
gate 오류는 거부 판단을 만들지 않습니다. OS 데몬은 독립적인 최후 보호 장치로 계속 동작합니다.

Codex Hook 신뢰는 hash 기반입니다. Hook 명령이 바뀐 재설치는 해당 항목을 검토 대기로 만들므로,
영향받은 CLI 프로세스에서 `/hooks`를 열어 정확한 정의를 확인하고 신뢰합니다. 저장하면 같은 CLI
프로세스가 호스팅하는 세션이 다시 읽습니다. `SessionStart` 자체를 다시 실행하려는 경우에만 새
세션이 필요합니다. Supervisor 데몬 재시작은 Codex를 다시 시작하지 않습니다. 일시정지된 Codex
리드가 재개되면 정확한 대상 터미널과 OS·원격 알림 경로가 실제로 확인된 지속적 프로세스 증가와
별도의 `agent|mixed|external|unknown` 시스템 원인 추정치를 구분해 즉시 설명합니다. Hook은 다음
프롬프트나 PostTool 경계에서 같은 안전 사건을 한 번 다시 전달하며, 이 시점은 운영체제 수준의
재개와 일치한다고 보장할 수 없습니다. Codex 자체가 종료됐다면 Codex의 세션 복원 기능을 사용합니다.
Codex는 새 프로세스에서 대화를 복원하고, 설치된 `SessionStart` Hook이 보존된 미전달 자원 사건과
현재 판단을 한 번 자동으로 주입합니다. `runtime.json`은 자원 사건을 보존할 뿐 Codex의 대화 복원
기능을 대신하지 않습니다.

## 검증

저장소 검증 명령:

```bash
bash tests/run.sh
memory-status --connections
```

`tests/native_codex.rs`는 공식 바이너리를 추가로 사용해 감지와 일회용 Codex 프로세스의 네이티브
일시정지·재개 왕복을 확인합니다. 다음 명령으로 선택형 canary를 실행합니다.

```bash
MEMORY_SUPERVISOR_NATIVE_CODEX_SMOKE=1 \
  cargo +1.88.0 test --test native_codex -- --nocapture
```

나머지 Rust 통합 test는 최소 버전과 기능 보고, 설치 Hook 형태, ORANGE `Agent` 거부, 정확한
터미널 선택과 잘못되거나 오래된 상태의 fail-open을 확인합니다. App Server를 시작하거나 모델
인증을 사용하는 에이전트 생성을 요구하지 않습니다.

자동 검사는 지원하는 최소 Codex 버전과 Hook 기능 계약을 고정해, Codex 쪽 기능 상태나 명령 형식이
바뀌면 공개 실행 파일을 만들기 전에 변화를 발견하도록 합니다.
