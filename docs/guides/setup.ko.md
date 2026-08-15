# 설치·연결·지원 환경

<p align="center">
  <a href="setup.md">English</a> · <strong>한국어</strong> · <a href="setup.zh-CN.md">简体中文</a> · <a href="setup.ja.md">日本語</a>
</p>

## 설치 방법

사용하는 환경의 **터미널**을 열고 아래 명령 한 줄을 그대로 붙여넣습니다. Git·Python·Rust나
별도 설치 파일을 미리 준비할 필요가 없습니다. 일반 설치는 현재 사용자 범위에서 이루어지므로
`sudo`나 관리자 권한도 필요하지 않습니다.

### 1. Memory Supervisor 설치

#### Linux, WSL2, macOS 터미널

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

명령이 끝나면 백그라운드 서비스가 바로 시작되고, 발견된 Claude Code·Codex Hook도 자동으로
연결됩니다. 실행 중인 AI 프로그램이나 작업은 종료하지 않습니다.

#### Windows PowerShell 터미널

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

명령이 끝나면 백그라운드 서비스가 바로 시작되고, 발견된 Claude Code·Codex Hook도 자동으로
연결됩니다. 실행 중인 AI 프로그램이나 작업은 종료하지 않습니다.

> [!IMPORTANT]
> Windows용 실행 파일은 현재 [SignPath Foundation](https://signpath.org/)의 인증 심사가 진행 중이므로,
> 완료될 때까지 Windows 11에서는 Windows 네이티브 버전을 설치하고 사용하는 동안
> **Windows 보안 → 앱 및 브라우저 컨트롤 → Smart App Control을 `끔`**으로 두어야 합니다.

| Windows 상태 | 네이티브 설치 가능 여부 |
| --- | --- |
| Windows 10 64비트 | Smart App Control이 없으므로 별도 설정 없이 설치할 수 있습니다. SmartScreen 안내가 나오면 다운로드 출처가 이 저장소의 release인지 확인합니다. |
| Windows 11 24H2 빌드 26100.8117 이상, 25H2 빌드 26200.8117 이상 또는 그보다 최신 Windows 11에서 다시 켜는 스위치까지 표시됨 | Smart App Control을 `끔`으로 바꾼 뒤 설치·사용할 수 있습니다. 사용을 끝낸 뒤 같은 설정 화면에서 다시 켤 수 있습니다. |
| 그보다 오래된 Windows 11 또는 최신 빌드이지만 순차 배포 중인 새 스위치를 아직 받지 못한 PC | `끔`으로 바꾸면 설치할 수 있지만, 다시 켜려면 Windows 초기화나 재설치가 필요할 수 있습니다. 끄기 전에 이 점을 먼저 확인합니다. |
| Smart App Control이 이미 `끔` | 그대로 설치할 수 있습니다. 다운로드 평판을 확인하는 별도 SmartScreen 안내가 나오면 게시자와 파일 출처를 확인합니다. |
| Windows 11 S 모드 또는 회사의 App Control 정책이 실행 파일을 차단하는 환경 | Windows 네이티브 지원 대상이 아닙니다. S 모드 해제나 관리자 정책 변경 없이 이 설치기로 우회할 수 없습니다. |

`Win + R`에서 `winver`를 실행해 버전과 빌드를 보고, 끄기 전에는 실제 Smart App Control 설정에
다시 켜는 선택지가 있는지 확인합니다. 이 기능은 PC마다 순차 적용됩니다. 자세한 기준은 Microsoft의
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)와
[최초 배포 기록](https://support.microsoft.com/en-au/help/5079391)을 참고하십시오.

Codex App은 창이 표시되는 운영체제가 아니라 **App Server와 도구가 실제로 실행되는 환경**을
기준으로 설치합니다.

- Windows의 Codex App이 WSL 엔진을 사용하면 WSL에 설치한 Supervisor가 WSL App Server, 대화별
  논리 thread와 그 안에서 실행되는 WSL 도구를 보호합니다. 이 경로는 Windows용 Supervisor를
  실행하지 않으므로 Smart App Control을 끌 필요가 없습니다. 다만 Windows 쪽 App 화면 프로세스와
  Windows에서 따로 실행한 Claude Code·Codex CLI는 이 WSL 설치의 측정·제어 대상이 아닙니다.
- App Server나 CLI가 Windows·macOS·Linux에서 직접 실행되면 그 운영체제에 설치합니다. Windows
  네이티브 실행에는 위 Smart App Control 조건이 적용됩니다.
- App Server나 CLI가 다른 WSL 배포판·가상 머신·격리 컨테이너에서 실행되면 그 환경마다
  설치합니다. Windows와 WSL은 federation 경로를 자동으로 찾고, macOS·Linux 호스트와 동적
  메모리 가상 머신·컨테이너는 같은 머신의 공유 폴더를 연결합니다. 이렇게 연결되어 같은 물리
  메모리를 함께 쓰는 환경은 새 작업 판단을 공유하고, 고정 메모리 가상 머신과 다른
  컴퓨터·클라우드 서버는 각자 독립적으로 보호합니다. 자세한 경계는
  [플랫폼과 여러 환경의 연동 방식](platforms.ko.md)에 있습니다.

### 2. Claude Code 설정

설치기가 Memory Supervisor 사용자 Hook을 자동으로 연결합니다. 사용자가 별도로 설정하거나 Hook을
승인·활성화할 필요는 없습니다.

**설치할 때 이미 실행 중이었다면:** 현재 작업을 그대로 이어가면 됩니다. Claude Code가 사용자
설정 변경을 자동으로 다시 읽으므로 보통 재시작할 필요가 없습니다.

**확인하려면:** 5번의 `memory-status --connections`에서 `Claude Code CONNECTED`를 확인합니다.
Hook 상세 내용을 직접 보고 싶을 때만 읽기 전용 `/hooks`의 `User Settings`를 확인합니다. 이
선택적 화면에 항목이 나타나지 않는 예외에만 현재 작업을 마친 뒤 Claude Code를 한 번 다시
시작합니다.

### 3. Codex CLI 설정

1. 사용할 Codex CLI에서 `/hooks`를 엽니다.
2. Memory Supervisor Hook 7개가 모두 **신뢰됨·켜짐**인지 확인합니다.
3. 검토가 필요한 항목은 신뢰하고, 꺼진 항목은 켭니다.
4. `/hooks`를 닫고 작업을 계속합니다.

**설치할 때 이미 Codex CLI가 실행 중이었다면:** 지금 사용하는 CLI에서는 위 확인 뒤 바로
계속하면 됩니다. 설치 전부터 따로 열려 있던 다른 Codex CLI는 진행 중인 작업을 마친 뒤 해당
CLI만 한 번 다시 시작합니다.

### 4. Codex Desktop App 설정

1. Codex App을 열고 **설정 → Hooks**로 이동합니다. Memory Supervisor 항목이 아직 없으면 최대
   60초 기다린 뒤 설정 화면을 다시 엽니다.
2. Memory Supervisor Hook 7개를 모두 신뢰하고 켭니다. **모두 신뢰**를 눌러도 꺼진 스위치는
   자동으로 켜지지 않으므로 두 상태를 각각 확인합니다.
3. 작업하던 기존 대화로 돌아가 원래 하려던 다음 요청을 보냅니다. 기존 대화가 없을 때만 새
   대화를 만듭니다.

**설치할 때 이미 Codex App이 실행 중이었다면:** App이나 기존 대화를 닫지 말고 위 1~3번을 그대로
진행합니다. App 재시작이나 새 대화는 필요하지 않습니다.

### 5. 설치 확인

```bash
memory-status --connections
```

사용하는 프로그램에 따라 다음 상태를 확인합니다.

- `Core daemon CONNECTED`: 백그라운드 서비스가 정상입니다.
- `Claude Code CONNECTED`: 지원 버전과 사용자 Hook이 연결됐습니다.
- `Codex CONNECTED`: CLI Hook 7개가 모두 설치·활성화·신뢰된 상태입니다.
- `Codex App ACTIVE`: App Hook 7개가 준비됐고 기존 대화나 새 대화에서 실제로 호출됐습니다.
- 사용하지 않거나 설치하지 않은 프로그램의 `NOT DETECTED`는 정상입니다.

정상이 아니면 출력에 표시된 항목만 처리합니다.

- `disabled` 또는 `not trusted`: Codex CLI는 `/hooks`, Codex App은 **설정 → Hooks**에서
  해당 항목을 신뢰하고 켭니다.
- `missing`, `stale`, `DEGRADED`, `NOT RUNNING`: `memory-supervisor update`를 실행한 뒤 다시
  확인합니다.
- `NEEDS ATTENTION`: 출력에 표시된 프로그램 버전이나 Hook 조건을 먼저 맞춘 뒤
  `memory-supervisor update`를 실행합니다.
- `Core daemon OFF`: `memory-supervisor on`을 실행합니다.
- App의 7개가 모두 정상인데 요청을 보낸 뒤에도 `ACTIVE`가 되지 않으면 App을 한 번 다시 시작하고
  기존 대화에서 다음 요청을 보낸 뒤 다시 확인합니다.
- 첫 설치 직후 `memory-status` 명령을 찾지 못하면 터미널만 새로 열고 다시 실행합니다. Claude
  Code·Codex CLI·Codex App을 다시 시작할 필요는 없습니다.

Codex의 Hook 신뢰는 관리자 권한 승인이 아니라, 로컬에서 실행될 명령을 사용자가 확인하는
절차입니다. 회사 관리 정책이나 Windows 보안 정책이 설치를 막는 경우에만 해당 관리자 정책을
확인해야 합니다. 자세한 신뢰 기준은
[Claude Code Hook 안내](https://code.claude.com/docs/en/hooks)와
[Codex Hook 안내](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)를 참고하십시오.

위 명령은 최신 공개 릴리스를 설치합니다. Rust 빌드 도구가 없어도 릴리스에 포함된 검증된
실행 파일을 자동으로 사용합니다.

### 6. 삭제

Calando를 삭제하려면 설치한 각 환경의 터미널에서 한 번씩 실행합니다.

```bash
memory-supervisor uninstall
```

백그라운드 서비스와 실행 파일, Calando가 추가한 Hook·Skill 연결만 제거하고 상태와 사용자
설정은 보존합니다.

## 지원 환경

모든 지원 환경에서 보호 동작은 같습니다. 메모리 여유와 감소 속도를 감시하고, 새 작업부터
단계적으로 줄인 뒤, 그래도 위험하면 확인된 Claude Code·Codex 프로세스 하나를 일시정지하고
상태가 안정되면 다시 실행합니다. 운영체제마다 메모리를 읽고 프로세스를 멈추는 방법만 다릅니다.

| 환경 | 테스트 범위 |
| --- | --- |
| Linux·WSL2 64비트 Intel/AMD | 실제 WSL2와 자동화된 Linux 검사 |
| macOS Apple Silicon | 자동화된 Apple Silicon 검사 |
| Windows 10·11 64비트 Intel/AMD | Windows 11 실기기 E2E, Windows Server 2022 자동화 검사, Windows 10 런타임·API 호환성 확인 |
| Intel 계열 macOS | Rosetta 기반 자동 호환성 검사 |

연결 대상은 Claude Code 2.1.217 이상, Codex CLI 0.145.0 이상(`hooks stable true`), Codex
Desktop App입니다. CLI와 App에도 같은 보호 정책을 적용합니다.

### 상주 메모리 실측

실행이 안정된 뒤 0.2초 간격으로 20회 측정한 운영체제 집계값입니다.

| 테스트 환경 | 최소 | 평균 | 최대 | 운영체제가 세는 방식 |
| --- | ---: | ---: | ---: | --- |
| WSL2 Linux, 실제 서비스 | 4.88 MiB | 4.88 MiB | 4.88 MiB | 상주 메모리(RSS) |
| Ubuntu 64비트 Intel/AMD, 자동화 테스트 | 3.50 MiB | 3.52 MiB | 3.54 MiB | 상주 메모리(RSS) |
| Windows 64비트 Intel/AMD, 자동화 테스트 | 4.15 MiB | 4.20 MiB | 4.25 MiB | 상주 메모리(Working set) |
| macOS Apple Silicon, 자동화 테스트 | 3.38 MiB | 4.35 MiB | 5.13 MiB | 상주 메모리(RSS) |

용량을 계산할 때는 가장 작은 실측값이 아니라 **설치된 감시 프로그램 하나당 10 MiB**를
사용합니다. 자세한 측정 조건과 원자료는 [성능 측정](performance.ko.md)에 있습니다.

한 물리 컴퓨터 안에서 Windows, WSL 배포판, 가상 머신, 격리 컨테이너처럼 실행 환경이 여러
개라면 Claude Code·Codex를 사용하는 환경마다 설치합니다. 같은 환경의 여러 터미널은 감시
프로그램 하나를 공유합니다. 각 환경의 설치와 federation 경로 설정이 끝나면 커널 수와 관계없이
같은 컴퓨터 전체가 최신 새 작업 허용 상태를 자동으로 공유합니다. 각 감시 프로그램은 자기 환경의
메모리와 프로세스만 측정·제어하므로 다른 환경의 프로세스 번호를 직접 조작하지 않습니다.
Windows와 WSL은 설치기가 같은 로컬 공유 폴더를 연결하고, 가상 머신·컨테이너는 호스트와 공유하는
로컬 폴더를 federation 경로로 지정합니다. 다른 물리 컴퓨터나 클라우드 서버를 네트워크 폴더로
묶지는 않습니다. 설정 방법은
[플랫폼과 여러 환경의 연동 방식](platforms.ko.md)을 참고하십시오.
