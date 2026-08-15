# 플랫폼 배포와 federation

<p align="center">
  <a href="platforms.md">English</a> · <strong>한국어</strong> · <a href="platforms.zh-CN.md">简体中文</a> · <a href="platforms.ja.md">日本語</a>
</p>

## 보호 대상 사용자·PID 제어 환경마다 supervisor 하나

Supervisor는 자신의 OS 사용자와 PID namespace에서 보이는 프로세스 목록을 읽습니다. 같은 PID
제어 환경에서 그 사용자가 실행한 Claude Code와 Codex는 Windows Terminal, iTerm, VS Code, tmux,
SSH 등 어떤 터미널에서 시작했든 한 번의 설치로 감시됩니다.

Supervisor는 로컬 PID 제어 환경 밖에 signal을 보내지 않습니다. 보호할 host, WSL 배포판, VM,
PID 격리 container마다 한 번씩 설치해야 합니다. WSL 2 배포판은 관리형 VM과 kernel을 공유하면서
PID namespace는 분리될 수 있으므로 각각 로컬 instance가 필요합니다. 각 instance는 작은 상태
snapshot을 공유 federation 디렉터리에 게시합니다. Hook은 최근 10초 안의 정상 snapshot 중 가장
위험한 값을 신규 fan-out admission에 사용하지만 실제 PID pause는 해당 PID를 소유한 로컬
supervisor만 수행합니다.

| 기본 OS | 그 위의 환경 | 필요한 설치 | Federation 경계 |
| --- | --- | --- | --- |
| Windows | WSL2 한 개 또는 여러 배포판 | Windows와 각 WSL 배포판에 한 번씩 | 각 WSL이 Windows 사용자 `.memory-supervisor/instances`를 자동 감지 |
| Windows·macOS·Linux | 동적 메모리 VM | 호스트와 각 guest에 한 번씩 | 실제로 같은 물리 메모리를 경쟁하는 양쪽만 host-local 공유 폴더로 연결 |
| Windows·macOS·Linux | 고정 메모리 VM | 호스트와 각 guest에 한 번씩 | 고정 할당 경계를 넘겨 federation하지 않고 각자 독립 |
| Linux kernel(기본 Linux·WSL·Desktop VM) | PID 격리 container 한 개 또는 여러 개 | 그 kernel의 host 환경과 각 격리 container | 그 kernel 안의 host-local volume을 공유 |
| 위 환경이 다시 중첩됨 | 보호할 각 PID namespace | 각 동적 공유 메모리 경계별로 별도 연결 | 고정 VM 경계나 네트워크를 가로질러 하나의 디렉터리를 늘리지 않음 |

### Codex App은 창이 아니라 App Server 환경을 따릅니다

Codex App 창과 실행 엔진은 서로 다른 운영체제 환경에서 실행될 수 있습니다. Memory Supervisor는
데스크톱 창이 아니라 `codex ... app-server` 프로세스를 기준으로 설치 위치를 정합니다.

- Windows Codex App이 WSL 엔진을 사용하면 해당 WSL 배포판에 설치한 Supervisor가 보호합니다.
  WSL App Server를 찾아 그 프로세스가 실제로 사용하는 `CODEX_HOME`을 확인하고, 대화별 논리
  thread, hook 판단, WSL 하위 도구와 WSL 안의 물리 제동을 관리합니다. 이 경로에는 서명되지 않은
  Windows 네이티브 Supervisor가 필요하지 않으므로 Smart App Control도 바꿀 필요가 없습니다.
- 그 WSL 인스턴스는 Windows App 화면 프로세스나 Windows에서 따로 실행한 Claude Code·Codex CLI를
  측정하거나 멈출 수 없습니다. Windows 프로세스까지 보호하려면 Windows에도 설치합니다. 그러면
  Windows와 WSL은 federation으로 새 작업 판단을 공유하되 PID 제어는 각 환경 안에서만 합니다.
- Windows 또는 macOS 네이티브 App Server는 해당 운영체제의 Supervisor를 사용합니다. Linux,
  다른 WSL 배포판, 가상 머신이나 PID 격리 컨테이너에서 실제 App Server가 실행되면 그 환경 안의
  Supervisor를 사용합니다. 요청한 창이나 client가 다른 곳에 있어도 같은 원칙입니다.
- 고정 메모리 가상 머신과 다른 컴퓨터는 각자 독립적으로 보호합니다. 같은 물리 메모리를 동적으로
  경쟁하는 실행 환경만 federation으로 연결합니다.

이는 Windows/WSL 조합만 예외 처리한 것이 아니라 모든 환경에 적용하는 프로세스 경계 원칙입니다.
Windows와 WSL이 `CODEX_HOME`을 공유하는 경우도 파일 배치만 특별합니다. Hook 파일에 두 환경의
실행 명령을 모두 보존하지만, 각 명령은 여전히 자기 환경의 Supervisor와 PID에만 도달합니다.

공유 경로가 없어도 각 instance는 자기 로컬 환경을 보호합니다. 통합 `memory-status --all`과
cross-environment admission만 사용할 수 없습니다.

Federation reader는 peer의 OS 이름을 Windows/WSL 쌍으로 제한하지 않습니다. 같은 host-local 메모리
경계에 게시된 Windows, WSL, Linux, macOS instance를 동일한 snapshot 계약으로 읽고, 최근 10초의 가장
엄격한 신규 작업 판단만 적용합니다. Windows/WSL은 자동으로 같은 경로를 찾는 특례일 뿐입니다.
macOS·Linux host, 동적 VM, container와 중첩 환경은 위 표처럼 해당 경계의 공유 폴더를 지정합니다.
복제한 guest나 container의 hostname이 같으면 `MEMORY_SUPERVISOR_INSTANCE`를 서로 다른 값으로
지정합니다.

공유하는 것은 federation 디렉터리뿐입니다. 각 환경의 `CODEX_HOME`, hook 파일, 신뢰 상태와 PID
제어권은 로컬에 둡니다. Windows App과 WSL 실행기가 실제로 같은 `CODEX_HOME`을 쓰는 경우에만 하나의
Codex 파일에 Windows·POSIX 실행 칸을 함께 보존합니다. 이는 hook 파일 배치의 특례이며 federation의
OS 조합을 제한하지 않습니다.

## 런타임과 자동 시작

공개 release 설치는 Git·Python·Rust를 요구하거나 설치하지 않습니다. 같은 release의 source
bundle과 현재 OS·architecture의 native binary를 내려받아 둘의 SHA-256을 확인합니다. 붙여넣는
명령의 다운로드 기능과 운영체제 기본 압축·SHA-256 기능만 사용합니다. 수동 개발 checkout을 직접
build하려면 Rust 1.88 이상이 필요합니다.

Windows 10에는 Smart App Control이 없습니다. Supervisor 실행 파일의
[최소 Windows 기준](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html)도 Windows
10이며, 필요한 메모리·프로세스 기능도 제공됩니다. 따라서 별도 SAC 설정 없이 설치할 수 있지만
SmartScreen 안내가 나오면 다운로드 출처를 확인합니다. Windows 11 Smart App Control은 checksum이
정확해도 새 unsigned 실행 파일을 차단할
수 있고 프로그램 하나만 예외로 허용하지도 않습니다. 따라서 공개 Windows 실행 파일이 인증되기
전까지는 설치하고 사용하는 동안 Smart App Control을 Off로 유지해야 합니다. Windows 설치기는
cutover 전에 후보를 직접 실행하며 Windows가 거부하면 기존 서비스를 건드리지 않습니다. Windows
11 24H2 빌드 26100.8117·25H2 빌드 26200.8117 이상은 다시 켤 수 있는 On/Off 기능을 받을 수 있지만
PC마다 순차 배포됩니다. 끄기 전에 `winver`로 빌드를 확인하고 실제 설정 화면에 다시 켜는 선택지가
있는지도 확인합니다. 이 기능을 아직 받지 못한 PC나 구버전은 다시 켜려면 초기화·재설치가 필요할
수 있습니다. WSL 실행 파일은 Windows Smart App Control을 바꿀 필요가 없지만 WSL 안의 프로세스만
보호합니다. Windows 11 S 모드와 실행 파일을 계속 차단하는 회사 App Control 정책은 네이티브 지원
경로가 아닙니다. [Windows 서명 런북](windows-signing.ko.md), Microsoft의
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions),
[배포 기록](https://support.microsoft.com/en-au/help/5079391),
[코드 서명 안내](https://learn.microsoft.com/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)를
참고하십시오.

| 플랫폼 | 사용자 단위 자동 시작 방식 |
| --- | --- |
| Linux / WSL | `~/.config/systemd/user/memory-supervisor.service`, 가능하면 설치기 소유 linger |
| macOS | `~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist` |
| Windows | 로그인 시 실행되는 `MemorySupervisor` 예약 작업 |
| user systemd가 없는 Unix | PID 감독 fallback, 설치 즉시 실행되지만 부팅 시작은 수동 |

`memory-supervisor update`는 가능한 경우 checkout을 업데이트하고 native runtime을 검증·활성화한 뒤
로컬 서비스와 감지된 지원 CLI 연결을 갱신합니다. 데몬 교체 중 agent PID에는 신호를 보내지 않고,
새 데몬은 `runtime.json`의 paused identity를 다시 읽습니다.

가장 안전한 업데이트 시점은 활성 CLI 세션이 없을 때입니다. 활성 세션도 대개 유지되지만 잠깐
fail-open 보호 공백이 생길 수 있습니다. 업데이트 뒤에는 항상 `memory-status --connections`를
확인합니다. Codex Hook 정의가 실제로 바뀐 경우에만 사용자가 CLI의 `/hooks` 또는 Desktop App의
**설정 → Hooks**에서 직접 다시 신뢰해야 합니다. 재시작은 승인을 대신하지 않습니다. App 설정 변경은
공유 App Server의 로드된 기존 작업을 갱신하지만 별도 CLI 프로세스는 갱신하지 않습니다. Claude
Code는 Hook별 해시 승인이 없지만, 대화형 세션은 현재 폴더 또는 상위 폴더의 workspace trust를
사용자가 승인할 때까지 사용자 Hook을 포함한 모든 설정 파일 Hook을 보류합니다. 신뢰된 실행 중
세션은 사용자 설정 변경을 보통 자동으로 다시 읽습니다. 이 신뢰·재적재 경계는 데몬 재시작보다
오래갈 수 있습니다.

지원 런타임은 네이티브 Rust 실행 파일입니다. Supervisor 재시작은 저장된 상태를 다시 읽으며,
설치된 빌드를 교체할 때는 가능하면 실행 중인 CLI 세션이 없는 시점을 사용합니다.

### 머신 재시작 뒤 일어나는 일

- Linux·WSL은 enabled user unit을 사용한다. 설치기가 활성화한 linger가 user manager와 unit을
  시작하게 하지만 WSL 배포판 자체가 시작되기 전에는 WSL 서비스도 실행되지 않는다.
- macOS는 GUI 로그인 때 `RunAtLoad`, `KeepAlive` LaunchAgent를 적재한다.
- Windows는 사용자 로그인 때 예약 작업을 시작하고 daemon 비정상 종료 시 1분 간격으로 최대 5회
  다시 시도한다. Task가 console 분리를 요청해도 daemon은 자기 프로세스만 연결된 console만
  분리한다. 따라서 background 시작은 검은 프로그램 창을 열어 두지 않고 주기적 PowerShell
  sensor도 `CREATE_NO_WINDOW`로 실행된다. 기존 terminal에서 실행한 명령은 그 terminal 연결을
  유지한다.
- Claude Code·Codex hook/skill 파일은 그대로 남는다. 로그인 뒤 새 AI CLI 세션을 열고
  `memory-status --connections`를 실행한다. Hook hash가 실제로 바뀐 경우에만 Codex CLI는 `/hooks`,
  Codex App은 **설정 → Hooks** 검토를 요구할 수 있다.
- 재부팅은 업데이트가 아니다. Source·설정·나중에 설치한 CLI 연결을 다시 적용할 때만
  `memory-supervisor update`를 실행한다.

## Federation 경로

- 기본값: `~/.memory-supervisor/instances`
- 명시값: `MEMORY_SUPERVISOR_FEDERATION_DIR`
- 저장된 pointer: `~/.memory-supervisor/federation-dir`
- 상태 pointer: `~/.memory-supervisor/state-dir`
- WSL은 Windows 사용자의 공유 인스턴스 디렉터리를 자동 탐색합니다.
- WSL 기본 instance 이름에는 `WSL_DISTRO_NAME`이 들어가므로 같은 Windows hostname을 공유하는
  Ubuntu와 Debian도 서로의 snapshot을 덮어쓰지 않습니다.
- stale·malformed·error 상태는 admission에 참여하지 않습니다.
- WSL이 아닌 복제 guest의 identity가 겹치면 `MEMORY_SUPERVISOR_INSTANCE`를 고유하게 지정합니다.

```bash
memory-status --all
```

Federation은 전역 backpressure이지 scheduler가 아닙니다. Worker를 옮기거나 다른 OS가 소유한 PID에
신호를 보내지 않습니다.

## 여러 SSH/tmux 세션과 VPS 배포

사용자 단위 설치 하나는 같은 PID 제어 환경에서 그 사용자가 연 모든 SSH 로그인·터미널 창·`tmux`
pane의 Claude Code와 Codex 세션을 함께 다룹니다. 세션마다 supervisor가 경쟁하지 않고 하나의
admission 판단을 공유합니다. 여러 사용자가 쓰는 서버라면 보호할 OS 사용자마다 한 번씩
설치합니다. Linux의 `hidepid` 같은 `/proc` 제한은 다른 사용자의 프로세스 관측을 막을 수 있으며
제품은 그 경계를 우회하지 않습니다.

제한된 VPS는 native cgroup 상한·PSI·swap/reclaim과 같은 사용자의 모든 원격 세션을 한 정책에
반영할 수 있어 자연스러운 배포 대상입니다. 설치된 user service를 활성화하고, SSH 창이 없어도
필요하다면 user linger를 사용합니다. Headless server에서는 desktop OS 알림이 없을 수 있으므로
필수 hook/terminal 조치 메시지를 사용하고 필요하면 Discord·Telegram을 연결하십시오. Linux와
cgroup 계약 테스트는 이 경로를 다루지만 실제 VPS에서 유료 모델을 몇 시간 돌린 soak 검증까지
완료했다고 주장하지는 않습니다.

## Native 용량과 sensor

| 플랫폼 | 용량과 가용 메모리 | 압력과 프로세스 |
| --- | --- | --- |
| Linux / WSL | `/proc/meminfo`와 모든 상위 cgroup v1/v2 상한 중 작은 값 | 운영체제의 메모리 부족 신호(PSI, 메모리 회수, swap, 메모리 부족 종료 기록), `/proc/<pid>`, 시작 시각, 터미널 정체성 |
| macOS | `sysctl hw.memsize`, `vm_stat`의 free/inactive/purgeable page | 제공되는 경우 kernel pressure level, 항상 쓰는 `vm_stat` pageout/compression 추세, `ps` 시작 시각과 TTY |
| Windows | `GlobalMemoryStatusEx` physical memory | `GetPerformanceInfo` commit 여유, cached CIM 목록, 생성 identity, console/ConPTY 증거 |

Linux는 unlimited leaf만 믿지 않고 모든 cgroup ancestor를 확인합니다. macOS에서 pressure-level
sysctl을 읽지 못하면 `vm_stat` counter는 보존하되 native pressure를 unknown/low-confidence로 표시하고
pressure sensor 오류와 보수적 `HOLD`를 노출합니다. `vm_stat` 실패도 실제 sensor 실패입니다. 같은
형태의 anonymous RSS를 제공하지 않아 프로세스 RSS를 근사값으로 사용합니다.
Windows는 값싼 전역 counter를 매 tick 갱신하고 비용이 큰 프로세스 목록은 3초 cache합니다.

모든 플랫폼은 `sensor_ok`, `sensor_errors`, `last_process_scan_ts`를 표시합니다. 프로세스 scan이
실패하면 마지막 목록을 진단용으로 보여줄 수 있지만, 그 stale 목록으로 새 pause나 paused PID
reconciliation을 실행하지 않습니다.

적응형 admission은 실제 headroom, 단·장기 감소율, 예상 소진 시간, native distress, 최근 burst,
자동 recoverability reserve를 사용합니다. RAM의 고정 비율을 예약하지 않습니다. 안정적인 고사용량은
계속 허용하며, 여유가 큰 상태의 빠른 감소는 먼저 Observe하고 reserve 접근·지속적인 짧은 TTE·명시
하드캡·보호 저하가 있을 때 Hold합니다.

## RAM 16 GiB Windows host의 WSL2 용량

Microsoft가 현재 문서화한 WSL2 기본 `memory` 상한은 Windows RAM의 50%입니다. 따라서 16 GiB
host에서 명시적인 `memory=8GB` 줄만 지워도 보통 같은 8 GiB 상한이 남아 heavy Linux CLI에 추가
여유가 생기지 않습니다. `memory=10GB`는 여러 무거운 WSL 작업과 Windows 프로그램을 함께 쓰는
경우의 예시이고, `memory=12GB`는 Windows 쪽 프로그램이 가벼울 때만 검토할 수 있는 더 큰
예시입니다. 둘 다 Supervisor 기본값이나 자동 권장값은 아닙니다.

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

`memory`는 10 GiB를 미리 점유한다는 뜻이 아니라 최대치입니다. 그래도 VM 상한은 필요합니다.
exact-PID pause는 이후 실행만 멈추고 resident memory를 즉시 반환하지 않으며, supervisor는 관련 없는
Linux·Windows 앱을 제어하지 않기 때문입니다. WSL 상한을 높이면 agent 여유는 늘지만 외부 앱을 위한
Windows 최악 상황 reserve는 줄어듭니다. Federation은 양쪽을 관찰할 수 있지만 한 커널의 PID 신호로
다른 커널 메모리를 회수할 수는 없습니다.

`.wslconfig` 변경은 WSL VM이 완전히 멈춘 뒤 적용됩니다. `wsl --shutdown`은 실행 중인 모든 WSL
배포판과 그 안의 CLI 세션을 즉시 종료하므로 활성 세션이 없는 시점에만 실행하십시오. Microsoft의
[WSL 고급 설정](https://learn.microsoft.com/windows/wsl/wsl-config)과
[`wsl --shutdown` 명령](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown)을 참고하십시오.

## 선택형 로컬 CLI 메모리 예산

예산은 **기본 OFF**입니다. 이 설치 제어 환경에서 보이는 Claude Code와 Codex 트리 전체의 통합
상한이며, AI CLI별 제한이나 Windows+WSL 통합 quota가 아닙니다.

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

`6`은 GiB 문법 예시일 뿐 권장값이 아닙니다(`memory-supervisor hard-cap set <MB>`가 MB 정밀
별칭). Windows, WSL, 각 VM·격리 container에서 따로 실행합니다. 이 제어 환경들은 물리 머신
하나를 공유할 수 있으므로 `memory-supervisor budget`이 설정 전에 이 환경의 이론상 최대와 peer
환경의 명시적 예산을 뺀 현재 가능 총량을 먼저 보여줍니다. `budget set`은 들어가지 않는 요청을
어디에서 얼마를 줄여야 하는지와 함께 거절하고, 현재 가능 총량의 90% 이상이거나 설정 후
machine-wide 명시 예산 합계가 물리 추정치의 90% 이상이면 진행 여부를 확인합니다. 설정 없는
환경의 기본 할당(예: WSL VM 상한)은 claim으로 세지 않습니다. 상한 근처에서는
새 fan-out을 먼저 보류하고, 초과 뒤에도 반응 구간마다 검증된 증가 worker/support PID 하나만
pause합니다. Lead는 정확한 복구 안내까지 검증된 최후 수단입니다. Suspend는 이후 실행을 멈추지만
resident memory를 즉시 반환하지 않으므로 byte 단위 강제에는 cgroup/container/VM native limit를
사용합니다.

## 고급 영속 설정

일반 사용에는 설정 파일이 필요하지 않습니다. 고급 override는
`~/.config/memory-supervisor/config.json`에 두며 같은 이름의 환경변수가 우선합니다. 예산은 위
전용 명령으로 설정·해제하는 것이 권장됩니다.

```json
{
  "MEMORY_SUPERVISOR_TICK_S": 1,
  "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S": 3,
  "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB": 32768
}
```

`MEMORY_SUPERVISOR_TICK_S`의 유효 범위는 0.25~5초입니다. 5초 상한은 10초 state freshness와
5초 lease 안에서 다음 표본을 보장합니다. 범위를 벗어나면 기본 1초를 사용하고
`configuration_error`에 원인을 표시합니다.

`MEMORY_SUPERVISOR_DIR`, `MEMORY_SUPERVISOR_FEDERATION_DIR`,
`MEMORY_SUPERVISOR_FORCE_PLATFORM` 같은 bootstrap 경로는 이 JSON에 넣지 않습니다. 고급 파일을
직접 바꾼 뒤에는 `memory-supervisor update`를 실행하고 `memory-status`로 확인합니다.

## Pause, resume, restart

- Unix `SIGSTOP`과 Windows native suspend는 PID와 in-memory session을 보존합니다.
- `memory-supervisor resume <pid>`는 PID와 시작 identity를 다시 확인한 뒤 재개합니다.
- 관리 중인 paused PID가 정확히 하나일 때만 `memory-supervisor resume`에서 PID를 생략할 수 있습니다.
- 제어 의도는 신호 전에 저장되고 daemon acknowledgement 뒤에만 완료로 표시됩니다.
- Supervisor 재시작은 사건 원장을 다시 읽으며 agent를 자동 재개하지 않습니다.
- Agent CLI 자체 재시작은 별개이므로 해당 AI CLI의 transcript/session resume를 사용합니다.
- 원격 사건은 `source`에 표시된 OS에서 제어해야 합니다.

AI CLI/model 문맥은 다음 실제 hook 경계에서 전달되므로 OS resume보다 늦을 수 있습니다. 정확한
터미널·OS·Discord·Telegram 조치 알림은 서로 독립적으로 시도됩니다.


## 전체 전원 켜기·끄기

```bash
memory-supervisor off
memory-supervisor on
```

`off` 한 번은 현재 OS/PID 제어 환경의 서비스와 자동 시작을 함께 끄고
`~/.memory-supervisor/power-off`에 선택을 남깁니다. 설치된 Claude Code·Codex hook과 skill은
삭제하지 않으며 모든 hook은 조용히 통과합니다. `memory-status`와 `--connections`는 장애가 아니라
의도적인 `OFF`로 표시하고, `memory-supervisor update`도 이 상태를 켜지 않습니다. `on`은 marker를
지우고 서비스를 자동 시작 상태로 복원한 뒤 fresh state가 게시됐는지 확인합니다.

Supervisor가 직접 일시정지한 PID 또는 처리 중인 process control이 남아 있으면 `off`는 거부됩니다.
이 안전 경계가 OFF 뒤 재개할 수 없는 process를 남기는 것을 막습니다. Windows, 각 WSL 배포판,
VM guest와 PID-isolated container는 서비스와 PID namespace가 다르므로 해당 환경에서 한 번씩
실행해야 합니다.

## 저수준 서비스 복구 명령

```bash
# Linux / WSL
systemctl --user restart memory-supervisor.service
systemctl --user is-active memory-supervisor.service

# macOS: 적재된 agent 재시작
launchctl kickstart -k gui/$(id -u)/io.github.lsslab.memory-supervisor

# macOS: 명시적으로 unload한 뒤 다시 load
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist

# Windows
schtasks /End /TN MemorySupervisor
schtasks /Run /TN MemorySupervisor
```

아래 명령은 의도적인 전원 전환이 아니라 서비스 장애를 수리할 때만 사용합니다. 서비스가 없고
`off` marker도 없으면 hook은 CLI를 막는 대신 fail open하며, `memory-status`가 stale/missing
supervisor와 보호 공백을 표시합니다.
