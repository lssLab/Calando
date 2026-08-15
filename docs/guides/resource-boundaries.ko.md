# 세션 탐지·용량 감지·메모리 경계

<p align="center">
  <a href="resource-boundaries.md">English</a> · <strong>한국어</strong>
</p>

이 문서는 설치 한 번이 어디까지 볼 수 있는지, wrapper 없이 모든 터미널 세션을 어떻게 찾는지,
OS·guest가 실제 사용할 수 있는 메모리를 어떻게 알아내는지, 각 설정 경계를 바꾸면 어디까지 영향을
주는지 설명한다.

## 가장 짧은 구조

물리 컴퓨터 한 대가 항상 하나의 제어 경계인 것은 아니다. 서로 독립적으로 관측·집행해야 하는 PID와
메모리 영역을 여러 개 가질 수 있다.

```text
물리 컴퓨터
├─ Windows host                -> Windows supervisor
├─ WSL distribution           -> Linux/WSL supervisor
├─ Linux 또는 Windows VM      -> 해당 guest 안의 supervisor
└─ Apple Silicon Mac host
   └─ macOS 또는 Linux VM     -> 해당 guest 안의 supervisor

fresh 상태 사본(10초 이내)     -> shared admission 결정
local process table            -> 그 PID 소유 instance만 pause/resume 가능
```

Claude Code나 Codex를 실행하는 host, WSL distribution, VM guest, PID 격리 container마다 한 번씩
설치한다. Host 설치는 guest PID에 signal을 보낼 수 없다. Federation은 backpressure를 공유할 뿐
cross-kernel process controller를 만들거나 RAM 총량을 서로 더하지 않는다.

## 각 터미널은 어느 경계에 속하는가

| CLI를 시작한 곳 | 실제 관측 경계 | Capacity 근거 | 설치·설정 위치 |
| --- | --- | --- | --- |
| PowerShell, 명령 프롬프트, native Windows Terminal tab | Windows host | `GlobalMemoryStatusEx` 물리 total/available, `GetPerformanceInfo` commit headroom | Windows에 한 번 |
| WSL terminal tab | 해당 WSL distribution에 보이는 Linux PID·메모리 영역 | `/proc/meminfo`와 모든 상위 cgroup limit 중 작은 값, 최종적으로 WSL VM memory 상한 | CLI를 실행하는 WSL distribution마다 설치하고 host 보호를 위해 Windows에도 설치 |
| Bare Linux, SSH session, tmux pane | 해당 Linux kernel/PID namespace와 사용자 권한 | `/proc/meminfo`와 모든 상위 cgroup v1/v2 limit 중 작은 값 | 보호할 OS 사용자·환경마다 한 번 |
| Apple Silicon Mac의 Terminal·iTerm | macOS `arm64` host | `sysctl hw.memsize`, `vm_stat`의 free/inactive/purgeable page | macOS에 한 번 |
| Apple Silicon Mac의 macOS VM | guest macOS `arm64` VM | guest의 `hw.memsize`와 `vm_stat`, VM 할당량이 최종 상한 | Guest 안에 한 번, host 설치는 별도 |
| 어느 host에서든 Linux·Windows VM | guest OS | guest에 맞는 위 Linux·Windows native sensor와 hypervisor 할당량 | Guest 안에 한 번, host 설치는 별도 |
| PID 격리 container | container에 보이는 process·cgroup 영역 | 물리 메모리와 모든 상위 cgroup limit 중 작은 값 | 격리 container 안에 설치하거나 의도적으로 host PID namespace 공유 |
| Intel 계열 Mac | macOS `x86_64` host | 동일한 macOS sensor | 해당 Mac에 한 번 |

Apple Silicon 위의 macOS VM도 `arm64`입니다. Rosetta의 x86_64 실행은 호환성 검사이며 물리 Intel
Mac 검증과는 구분합니다.

WSL2 distribution들은 같은 utility VM을 공유할 수 있지만 process namespace는 분리된다. 한
distribution이 다른 distribution PID를 안정적으로 조사·signal할 수 없으므로 CLI를 실행하는 곳마다
설치한다. Federation은 가장 나쁜 fresh 결정을 택하며 공유 WSL memory pool을 중복 합산하지 않는다.

## Wrapper 없이 세션을 잡는 방법

사용자는 계속 `claude`나 `codex`를 평소처럼 실행한다. Daemon은 terminal 창 목록을 조사하지 않고
`claude-governed`·`codex-governed` 같은 시작 명령도 요구하지 않는다.

1. Native daemon이 자기 OS 계정에서 볼 수 있는 전체 process inventory를 scan한다. 기본 control
   loop는 1초이며, Windows는 비싼 CIM 목록만 최대 3초간 cache하고 싼 전역 memory counter는 매 tick
   읽는다.
2. 실행 파일이나 첫 command argument가 `claude`, `codex`, 공식 architecture별 Codex binary이면
   지원 CLI root로 인식한다.
3. Parent link로 중첩 지원 CLI root를 worker로 묶고 다른 descendant를 support process로 묶는다.
   잘못된 process graph에서 무한 순환하지 않도록 ancestry walk는 64단계로 제한한다.
4. 모든 descendant가 root tree RSS 추정에 포함된다. Anonymous memory가 32 MiB 미만인 작은 child는
   개별 pause 후보 목록에서만 빠지며 tree 총량에서는 빠지지 않는다.
5. PID와 process start identity를 함께 확인해 재사용된 PID가 다른 프로세스를 가리키지 않게 한다.
   Linux는 `/proc/<pid>/stat` start tick, macOS는 `ps` start time, Windows는 CIM `CreationDate`를 쓴다.
6. Lead/worker/support 역할, 증가 속도, root-tree 총량과 플랫폼이 제공하는 경우 검증된 terminal
   identity를 기록한다. OS 권한, Linux `hidepid`, container, VM 경계를 우회하지 않는다.

AI CLI hook은 process detector와 다른 두 번째 경로다. 새 fan-out 전에 최신 local/federation 상태를
묻고 다음 실제 hook 경계에서 main agent에게 사건을 주입한다. Hook이 빠져도 daemon은 local process
table을 관측할 수 있지만 그 AI CLI가 새 process를 만들기 전에 admission을 막을 수는 없다.

```bash
memory-status --connections
memory-status --all
```

## 실제 사용 가능 capacity를 알아내는 방법

| 플랫폼 | Capacity | Available/headroom | 추가 distress 근거 |
| --- | --- | --- | --- |
| Linux·WSL | `MemTotal`을 모든 finite cgroup ancestor limit 중 최소값으로 축소 | `MemAvailable`과 각 cgroup의 `limit - current` 중 최소값 | PSI `some/full`, reclaim, swap, OOM counter |
| macOS | `sysctl -n hw.memsize` | `vm_stat` free + inactive + purgeable page | 제공되는 kernel pressure level, pageout/compression·swap 추세 |
| Windows | `GlobalMemoryStatusEx.totalPhys` | `GlobalMemoryStatusEx.availPhys` | `GetPerformanceInfo`의 commit limit - committed page |
| 모든 VM guest | Guest 안에서 위 해당 OS 행 사용 | 고정·동적 VM 할당량이 이미 반영된 guest-visible 값 | Guest native pressure signal |

Resolved capacity와 적응형 정책은 매 tick 다시 계산한다. VM dynamic memory나 cgroup이 바뀌어도 고정
machine-size profile 없이 반영한다. 주 sensor가 실패하면 보호 저하를 표시하고 admission을 Hold한다.
8 GiB fallback 표시는 진단용이지 머신이 실제 8 GiB라는 주장이 아니다.

Supervisor는 container runtime, systemd unit, scheduler, 관리자가 이미 만든 enclosing cgroup
limit를 **읽기만** 한다. Cgroup을 만들거나 CLI를 옮기거나 wrapper 명령을 요구하지 않는다.
그래서 평소대로 실행한 `claude`·`codex`도 탐지하며, byte-exact cgroup 할당은 이 제품의 기본
actuator가 아닌 선택적 외부 경계로 남는다.

Supervisor는 프로세스에 RAM을 할당하지 않는다. 고정 비율을 남기는 대신 실제 제동거리를 계산한다.

```text
최소 호흡 공간 = 감지 capacity의 0.5%, 256–1024 MiB 범위
검증된 소진 속도 = max(지속된 물리/commit 여유 감소,
                         지속되고 머신 감소와 일치하는 CLI 증가)
자동 reserve = min(최소 호흡 공간 + 검증된 소진 속도 × 반응 구간 1회,
                    감지 capacity의 25%)
신규 fan-out floor = min(자동 reserve + 신규 작업용 최소 호흡 구간 1회,
                          감지 capacity의 30%)
```

물리 여유 감소에는 이미 CLI 할당이 들어 있으므로 두 속도를 더해 같은 증가를 두 번 세지 않고
`max`로 결합한다. 추세는 최소 표본 3개와 반응 구간 1회의 길이, 위험 방향 interval 60% 이상,
반대 방향 rebound보다 두 배 이상의 위험 방향 이동을 모두 요구한다. 따라서 reclaim 한 번이 실제
하강을 지우지도 않고 순간 spike 하나가 지속 하강으로 둔갑하지도 않는다.

달리는 자동차와 같은 계산이다. 소진 속도가 빠르면 MiB 단위 제동거리는 길어지지만 시간상 더 일찍
막지는 않고, 느리면 같은 반응시간에 도달할 때까지 머신을 더 많이 쓴다. `HOLD`는 reserve까지 반응
구간 2회 이하이거나 신규 최소 작업 구간조차 없을 때 **새 fan-out만** 닫는다. `DRAIN`은 반응 구간
1회 안쪽이고 agent/mixed 귀속 또는 명시 hard cap이 있을 때만 기존 에이전트의 단계적 완충을 시작한다.
매 1초 tick의 논리 조치량은 `ceil(남은 최소 단계 / reserve 전까지 남은 control tick)`이므로,
worker 8개든 세션 수백 개든 고정 개체수 상한 없이 같은 최소 사다리를 경계에서 끝낸다.

안정적인 고사용량은 계속 열어둘 수 있다. Raw GREEN/YELLOW/ORANGE/RED utilization은 진단값이며
단독으로 admission을 닫거나 PID pause 권한을 주지 않는다. 작은 머신부터 초대형 머신까지와 실제
질식선 근접 결과는 [적응형 제동거리](../testing/stopping-distance.ko.md)에
기록했다.

## 서로 다른 다섯 경계

| 경계 | 기본값 | 변경 방법 | 직접 범위 | 다른 영향 |
| --- | --- | --- | --- | --- |
| 물리 또는 VM 할당량 | OS/hypervisor 기본 | 물리 RAM은 software로 바꾸지 못한다. WSL, Hyper-V, Parallels, VMware, UTM, cloud VM memory를 해당 플랫폼에서 변경 | Host 또는 guest OS 자체 | Guest memory를 올리면 guest 여유는 늘지만 host의 최악 reserve가 줄어든다. 내리면 guest 적응형 임계·reserve가 작아진다. 보통 guest 종료·재시작 필요. |
| 자동 감지 capacity | Native sensor | 정상 상태에서는 변경하지 않는다. `MEMORY_SUPERVISOR_CAPACITY_MB`는 native 값이 명백히 틀릴 때만 쓰는 고급 보정 | 설치 instance 하나 | 정책 계산만 바꾸고 실제 OS/VM limit는 바꾸지 않는다. 너무 높이면 위험하고 너무 낮추면 불필요하게 보수적이다. |
| 적응형 pressure 정책 | `balanced`, 수동 budget 없음 | 선택형 `protect`·`balanced`·`performance` profile 또는 고급 threshold override | 설치 instance 하나 | 그 instance의 더 엄격한 admission 결정이 federation peer에 전달될 수 있다. `performance`도 실제 collapse, 보호 저하, 명시 hard cap을 우회하지 않는다. |
| 지원 CLI aggregate 메모리 예산(hard cap) | **OFF** | 해당 환경에서 `memory-supervisor budget set <GiB>` 또는 `budget off` | 해당 OS/PID 영역에서 보이는 모든 Claude Code·Codex root tree. Chrome이나 머신 전체가 아님 | Cap 근처에서는 신규 fan-out을 먼저 Hold한다. 초과하면 reaction interval당 검증된 growing worker/support 하나만 pause 가능하다. Cap 근접은 로컬에 머문다: `near/exceeded`는 더 이상 federated peer의 admission을 닫지 않으며(측정된 압력만 연합), remote PID도 pause하지 못한다. |
| Federation admission | Instance가 shared directory를 쓰면 활성 | shared `MEMORY_SUPERVISOR_FEDERATION_DIR` 설정. WSL 배포판 이름은 자동이고 다른 cloned guest는 고유 `MEMORY_SUPERVISOR_INSTANCE` 지정 | Fresh peer 사이의 신규 fan-out만 | 최근 10초의 valid snapshot 중 최악을 쓴다. Hard cap pooling, RAM 합산, job migration, remote 설정 변경은 하지 않는다. |

## 지원 CLI 메모리 예산 변경

Process tree 경계를 바꿀 **각 환경 안에서** 실행한다.

```bash
memory-supervisor budget
memory-supervisor budget set 12
memory-supervisor budget off
```

`12`는 GiB 문법 예시일 뿐 권장값이 아니다(`memory-supervisor hard-cap set <MB>`가 MB 정밀
별칭). 인자 없는 `budget`은 shared federation snapshot으로 이 환경의 이론상 최대와, peer 환경의
명시적 예산을 뺀 현재 가능 총량을 보여준다. 명시적 예산만 claim으로 세고 환경의 기본 할당은
세지 않는다. `set`은 현재 가능 총량과 대조해, 초과 요청은 들어가려면 어느 환경에서 얼마를
줄여야 하는지와 함께 거절하고, 현재 가능 총량의 90% 이상이거나 설정 후 machine-wide 명시 예산
합계가 물리 추정치의 90% 이상이 되는 요청은 진행 여부를 확인한다(스크립트는 `--yes`).
`set`은 다른 설정을 보존하고 local service를 다시 적용한다. `off`는 해당 환경을 adaptive-only
기본으로 되돌린다.

| 원하는 결과 | 실행 위치와 방법 |
| --- | --- |
| Native Windows Claude Code와 Codex 공용 예산 | PowerShell에서 `budget set <GiB>` 한 번 |
| WSL session만 다른 예산 | 해당 WSL distribution 안에서 별도 값 실행 |
| Host와 guest VM에 같은 정책 | Host와 guest에서 같은 명령을 각각 한 번 |
| VM 두 개에 서로 다른 budget | 각 VM 안에서 다른 값 실행 |
| 모든 환경을 기본 자동 모드로 복귀 | 예전에 override한 각 환경에서 `budget off` 실행 |

Cap은 complete 지원 CLI root tree를 한 번씩 센다. Sample 방식이라 tick 사이 burst가 초과할 수 있고
pause는 이미 resident인 memory를 즉시 반환하지 않는다. Byte-exact 할당 상한은 native
cgroup/container/VM limit를 사용한다.

## WSL 또는 VM 할당량 변경

WSL2는 host의 `%UserProfile%\.wslconfig`가 shared WSL VM 최대 memory를 정한다.

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

이 값은 선점이 아니라 최대값이다. WSL VM이 완전히 멈춘 뒤 적용된다. `wsl --shutdown`은 실행 중인
CLI도 종료하므로 활성 session 중 실행하지 말고 유휴 경계에서만 사용한다. Microsoft의
[WSL 설정](https://learn.microsoft.com/windows/wsl/wsl-config)과
[`wsl --shutdown`](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown) 문서를 참고한다.

Hyper-V, Parallels, VMware, UTM, cloud VM은 해당 hypervisor·cloud control plane에서 고정·동적 memory를
바꾸며 보통 guest를 끈 상태에서 수행한다. Supervisor에 같은 숫자를 다시 넣을 필요는 없다. 다음 boot
뒤 guest kernel이 노출하는 값을 읽어 자동 재계산한다. Host·guest에는 별도 설치가 필요하고 shared
admission을 원하면 federation folder도 공유해야 한다.

## 고급 정책 변경

정상 사용자는 아래 값을 설정하지 않는다. Unix는 `~/.config/memory-supervisor/config.json`, Windows는
`$HOME\.config\memory-supervisor\config.json`을 사용한다.

```json
{
  "MEMORY_SUPERVISOR_POLICY_PROFILE": "performance"
}
```

수동 편집 뒤 `memory-supervisor update`를 실행하고 `memory-status`를 확인한다. `protect`는 더 일찍,
`performance`는 더 늦게 조치하며 `balanced`가 기본이다. 세밀한
`MEMORY_SUPERVISOR_MEM_*`, `MEMORY_SUPERVISOR_PSI_*`, process observation override도 있지만 순서를
검증하고 잘못된 group은 adaptive 값으로 돌아간다. Slope나 raw threshold는 여전히 관찰값이며 shared
actuator 불변조건이 pause 권한을 통제한다.


## 검증 경계

저장소는 GitHub Actions에서 Linux·Windows·macOS에 같은 테스트 묶음을 실행합니다. 네이티브 센서,
프로세스 식별, 정책 판단, Hook 동작, 설치 수명주기와 릴리스 파일을 확인합니다. Windows와 WSL2의
제어된 부하 시험으로 복구 경계 근처의 제동거리도 검증했습니다. 자세한 범위는
[테스트 표](../testing/test-matrix.ko.md)와
[제동거리 검증](../testing/stopping-distance.ko.md)을 참고하십시오.

Hosted runner와 결정적 simulation은 반복 가능한 제품 계약을 검증합니다. 모든 물리 host·guest·
container·장시간 작업 조합을 그대로 재현했다고 주장하지는 않습니다.

## 의도적으로 불가능한 것

- Windows 명령 하나로 WSL·macOS VM·Linux VM hard cap을 바꿀 수 없다.
- WSL instance는 Windows PID를, guest는 host PID를 pause할 수 없다.
- Federation은 16 GiB host RAM과 10 GiB WSL capacity를 가상의 26 GiB pool로 합치지 않는다.
- Supervisor는 꺼진 guest나 자기 PID·권한 영역 밖의 CLI를 보지 못한다.
- Apple Silicon의 macOS VM은 Intel Mac test가 아니다. Rosetta는 compatibility coverage일 뿐이다.
- `MEMORY_SUPERVISOR_CAPACITY_MB` 변경은 physical memory를 할당하거나 회수하지 않는다.

설치 위치와 federation 경로는 [플랫폼 배포 문서](platforms.ko.md), instance당 실측 점유량은
[성능 문서](performance.ko.md)를 참고한다.
