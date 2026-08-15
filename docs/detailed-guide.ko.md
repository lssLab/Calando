<p align="center">
  <img src="../assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor 로고">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="detailed-guide.md">English</a> · <strong>한국어</strong> · <a href="detailed-guide.zh-CN.md">简体中文</a> · <a href="detailed-guide.ja.md">日本語</a>
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
  <a href="guides/setup.ko.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, Windows"></a>
  <a href="guides/performance.ko.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="감시 프로그램 계획값 10 MiB 미만"></a>
  <a href="guides/security.ko.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="사용 통계 전송 없음"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT 라이선스"></a>
</p>

## Memory Supervisor는 무엇을 해결하나?

Claude Code·Codex CLI 또는 Codex Desktop App에서 큰 작업을 오래 진행하면 서브에이전트,
빌드, 테스트, 브라우저 도구가 한꺼번에 겹칠 수 있습니다. 메모리 여유가 빠르게 사라지면
CLI에서는 터미널이 응답하지 않거나 세션이 종료될 수 있고, Desktop App에서는 App Server를
공유하는 여러 대화가 함께 영향을 받을 수 있습니다. 어느 쪽이든 아직 전달되지 않은 결과와
작업 흐름이 끊길 수 있습니다.

Memory Supervisor는 메모리 사용량이 높다는 이유만으로 작업을 제한하지 않습니다. CLI와
Desktop App 모두 실제 위험이 가까워질 때 새 작업부터 단계적으로 늦추고, 진행 중인 작업과
결과 전달은 가능한 한 유지해 세션이 갑자기 끊기는 상황을 예방합니다.

보호 강도는 한 번에 올라가지 않습니다. 위험에 가까워질수록 다음 단계를 하나씩 적용하고,
상태가 회복되면 반대 순서로 해제합니다.

1. **자동 파악** — 평소처럼 `claude`나 `codex`를 실행하거나 Codex Desktop App에서 대화를
   시작하면 됩니다. Memory Supervisor가 CLI 세션과 App 대화를 자동으로 구분하고, 메모리 용량,
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

## 작동 방식

Memory Supervisor는 터미널 명령을 가로채는 중간 프로그램이 아닙니다. 사용자는 평소처럼
`claude`와 `codex`를 실행하고, 작은 백그라운드 감시 프로그램이 옆에서 상태를 확인합니다.

1. 감시 프로그램은 **운영체제 환경마다 하나씩** 실행됩니다. Windows·macOS·Linux 본체뿐 아니라
   각 WSL 배포판·가상 머신·격리 컨테이너도 별도 환경입니다. 예를 들어 Windows와 WSL에서 함께
   사용하면 Windows에 하나, WSL에 하나가 실행됩니다. 각 감시 프로그램은 자신이 실행된 환경의
   남은 메모리, 운영체제 압박 신호, 짧은 구간과 긴 구간의 감소 속도, 예상되는 다음 증가량,
   Claude Code와 Codex 관련 프로그램을 측정합니다. 같은 환경에서 연 여러 터미널은 같은 감시
   프로그램과 새 작업 판단을 공유합니다.
2. 여러 터미널은 창 이름이 아니라 프로세스 표와 hook으로 구분합니다. 각 최상위 Claude Code·
   Codex 프로세스를 독립 리드로 보고 하위 프로세스를 작업자·도구로 묶으며, hook의 세션·에이전트
   번호와 PID·시작 정보를 함께 기록해 다른 세션이나 재사용된 PID를 잘못 제어하지 않습니다.
3. 고정 사용률 대신 현재 여유와 감소 속도로 제동거리를 계산합니다. 천천히 줄면 더 가까이까지
   작업하고, 빨리 줄면 더 긴 거리를 남겨 같은 반응 시간 안에 안전하게 멈춥니다.
4. Claude Code와 Codex의 실행 전 연결 기능(hook)이 새 서브에이전트·워크플로·작업이나 큰
   작업을 시작해도 되는지 확인합니다. `ALLOW`는 허용, `OBSERVE`는 허용하며 관찰, `HOLD`는
   새 서브에이전트·워크플로·작업 생성만 대기시키고, `DRAIN`은 이런 생성 요청을 막는 판단입니다.
5. Windows·Linux·macOS의 기본 OS와 그 위에서 실행되는 각 WSL 배포판·가상 머신·프로세스 격리
   컨테이너에는 자기 PID 공간을 담당하는 Supervisor가 따로 실행됩니다. 같은 물리 메모리를 쓰는
   환경끼리는 federation으로 10초 이내의 새 작업 판단만 공유하며, 그중 가장 엄격한 판단을
   적용합니다. 각 Supervisor는 자기 hook과 PID만 제어하므로 다른 환경의 프로그램을 직접
   멈추지는 않습니다.
6. `DRAIN`에서도 Chrome이나 IDE 같은 외부 프로그램만 원인이면 기존 AI 작업을 멈추지 않습니다.
   AI 작업이 원인이거나 사용자가 정한 메모리 한도를 넘었을 때만 `ACTIVE(모든 작업) →
   NO_EXPANSION(새 서브에이전트·워크플로·작업 생성 없음) →
   LIGHT_WORK_ONLY(빌드·테스트처럼 메모리를 많이 쓰는 새 작업도 시작하지 않음) →
   HANDOFF_ONLY(전달·조정·상태 확인·중단·복구와 작은 읽기만)` 순서로 앞으로 할 일을 줄입니다.
   여러 세션을 한꺼번에 줄이지 않고, 연결 프로세스의 지속 증가 확인 → 현재·직전의 에이전트
   확장·빌드·테스트 → 이미 제한이 시작된 대상 → 위험 기준에 먼저 닿는 대상 → 최근 시작 순으로
   서브에이전트부터 고릅니다. 매 주기에는 남은 제동거리에 필요한 최소 단계만 적용하며, 선택되지
   않은 세션과 진행 중인 작업은 그대로 둡니다. 주 에이전트는 서브에이전트부터 줄이면 늦는다는
   근거가 있을 때만 먼저 제한합니다.
7. 그래도 위험이 계속되면 PID(운영체제가 붙이는 프로그램 번호)와 시작 정보를 다시 확인한
   로컬 프로그램 하나만 일시정지합니다. 주 에이전트는 정확한 터미널에 알림을 쓸 수 있을 때만
   멈추며, 알림에 실패하면 즉시 재개합니다.

상태가 안정적으로 좋아지면 반대 순서로 작업 범위를 다시 열고, 일시정지한 프로그램도 한 번에
하나씩 재개합니다. 정확한 제동 계산과 실측값은
[적응형 제동거리](testing/stopping-distance.ko.md)에 있습니다.

`GREEN`~`RED` 색은 현재 상태를 쉽게 보여주는 표시일 뿐입니다. 새 작업 허용 여부는
`ALLOW`, `OBSERVE`, `HOLD`, `DRAIN`이 결정하며 색만 보고 프로그램을 멈추지는 않습니다.

### Codex Desktop App에서는 무엇이 다른가?

CLI에서는 세션마다 독립된 주 프로세스와 하위 프로세스가 있어 어느 세션이 메모리를 늘리는지
비교적 정확히 알 수 있습니다. Codex Desktop App에서도 여러 대화가 하나의 세션으로 합쳐지는 것은
아닙니다. 각 대화는 App Server 안에서 `session_id`를 가진 **논리 thread**로 구분됩니다. 여기서
논리 thread는 운영체제가 실행하는 OS thread가 아니라, App Server와 Supervisor가 대화 흐름을
구분하는 단위입니다. Supervisor는 각 논리 thread를 독립 리드로 보고, `agent_id`가 있으면 그 리드의
서브에이전트로 연결합니다. 따라서 대화별 에이전트 목록, 다음 hook에서 허용할 작업 범위, 조치와
복구 알림은 따로 관리할 수 있습니다.

하지만 논리 thread 하나가 CLI 세션 하나와 물리적으로 같은 것은 아닙니다. CLI 세션은 독립된 주
PID, 그 아래의 프로세스 트리와 터미널을 가지지만, App의 논리 thread에는 독립된 주 PID·완전한
하위 프로세스 트리·터미널·전용 메모리 수치가 없습니다. 모든 논리 thread가 App Server PID 하나와
그 내부 메모리를 공유하므로, 운영체제에는 대화별 사용량이 아니라 App Server 전체 사용량만 보이고
대화 하나만 운영체제 기능으로 멈출 수도 없습니다. 즉, 대화는 논리적으로 분리되지만 프로세스와
메모리는 물리적으로 공유됩니다.

Supervisor는 공유 App Server 메모리를 한 번만 계산합니다. 별도로 실행된 도구 프로세스는 hook
직전의 프로세스 목록·부모 자식 관계·PID 시작 정보가 모두 맞을 때만 특정 논리 thread의 작업으로
확정합니다. 반면 App Server 내부에서 직접 사용한 메모리나 여러 대화의 도구 실행이 겹쳐 생긴
하위 프로세스는 어느 논리 thread의 것인지 확정하지 못할 수 있습니다. 이것이 여기서 말하는
**blind 제어**입니다. 아무것도 모른다는 뜻은 아닙니다. 시스템의 남은 여유와 감소 속도, App
전체와 각 하위 프로세스의 증가 속도, 열려 있는 대화, 실행 중인 도구 종류는 알지만 그 증가분의
대화별 소유자만 확정할 수 없는 상태입니다.

이 한계 안에서도 CLI와 같은 보호 정책을 다음 순서로 달성합니다.

1. **성능을 먼저 유지합니다.** 사용량이 높아도 안정적이거나 App 증가가 시스템의 메모리 감소를
   설명하지 못하면 App을 원인으로 대화별 작업 범위를 줄이지 않습니다. 실제 위험까지 남은 시간과
   필요한 제동 시간을 비교해 아직 늦지 않은 가장 마지막 시점까지 기다립니다. 대화별 주인을
   확정하지 못한 증가 비중이 크면 후보를 하나씩 시험하고 효과를 확인할 시간만 제동거리에
   추가하며, 불확실하다는 이유만으로 무조건 일찍 막지는 않습니다.
2. **새 고메모리 작업부터 완충합니다.** App의 지속 증가가 실제 위험의 원인이고 제동거리 안에
   들어왔지만 대화별 원인이 불명확하면, App 전체에서 앞으로 시작할 빌드·테스트 같은 고메모리
   작업만 잠시 기다리게 합니다. 이미 실행 중인 작업, 결과·메시지 전달, 상태 확인과 복구는
   계속됩니다.
3. **원인을 설명하는 최소 대상만 줄입니다.** 정확히 연결된 증가가 있으면 그 증가를 설명하는 데
   필요한 최소 대화만 고르고, 정상 상황에서는 각 대화의 다음 작업 범위를 한 단계씩 줄입니다.
   대화별 연결이 불명확하면 실행 중인 고메모리 도구, 서브에이전트 여부와 최근 활동을 이용해
   후보 순서를 정하고 첫 후보 하나만 줄인 뒤 다시 측정합니다. 증가가 둔화되면 거기서 멈추고,
   계속될 때만 다음 blind 후보로 넘어갑니다. 위험선까지 시간이 부족할 때도 그 전에 필요한 최소
   묶음만 함께 줄입니다. 추정 정보는 이 순서를 정하는 데만 쓰며 특정 대화를 물리적으로 멈출
   권한으로 사용하지 않습니다.
4. **물리 제동은 끝까지 작은 대상부터 사용합니다.** 모든 논리 조치 뒤에도 위험이 계속될 때
   정확히 연결되어 계속 증가하는 하위 프로세스 하나 → 대화는 몰라도 App에서 만든 것이 확실하고
   계속 증가하는 하위 프로세스 하나 → 공유 App Server 전체 순으로 검토합니다. 공유 Server는 모든
   활성 대화와 서브에이전트가 결과 전달·상태 확인·복구 같은 가벼운 작업만 남기는 마지막 단계에
   도달했음을 hook으로 확인하고, Server 자체의 지속 증가가 주원인이며 다른 방법이 없을 때만 잠시
   멈춥니다. 독립 복구 장치가 제한시간 뒤 정확한 Server를 자동으로 재개합니다. 이 제동은 메모리를
   즉시 비우는 것이 아니라 추가 증가를 막아 다른 작업이 끝나고 시스템이 회복할 시간을 버는
   조치입니다.

회복할 때는 반대 순서로 한 단계씩 다시 엽니다. 조치 이유와 현재 범위는 해당 대화의 다음 hook에
전달되고, blind 하위 프로세스나 공유 Server를 제동하면 영향을 받는 모든 활성 대화에 알립니다.
App hook 연결이 끊기면 새로운 대화별 제한이나 물리 제동이 집행됐다고 가정하지 않으며, 이를 다음
보호 수단으로 계산하지 않고 보호 저하 상태를 표시합니다. 시스템 전체의 새 작업 판단과 App
프로세스 관측은 계속합니다.

CLI와 App을 동시에 실행해도 어느 한쪽이 관측에서 빠지지는 않습니다. 같은 OS·WSL 배포판·VM처럼
하나의 PID 공간에서는 Supervisor 하나가 CLI 프로세스 트리와 Codex App Server를 함께 봅니다.
다만 둘을 한 세션으로 합치지는 않습니다.

- CLI 세션은 각각 **터미널·주 PID·하위 프로세스 트리를 가진 독립 리드**입니다.
- App Server는 터미널 하나가 아니라 **여러 대화가 함께 쓰는 물리적 프로세스 호스트 하나**입니다.
  그 아래에서 각 `session_id`를 독립된 논리 리드로 나누되, Server PID와 내부 메모리는 한 번만
  계산합니다.

두 실행 방식은 같은 로컬 메모리 상황과 새 작업 판단을 공유하지만 제어 대상은 섞이지 않습니다.
App 원인으로 여는 blind 완충은 App hook에만 적용되고 일반 CLI 요청을 함께 막지 않습니다. 반대로
CLI나 App 어느 쪽의 메모리든 시스템 위험 계산에는 반영되지만, 한쪽이 늘었다는 이유만으로 다른
쪽을 제동 대상으로 정하지는 않습니다. 각 대상은 자기 증가와 연결 근거로 따로 선정합니다.

Federation은 App 대화나 터미널을 합치는 기능이 아니라, 같은 물리 메모리를 경쟁하는 **Supervisor
인스턴스끼리** 연결하는 기능입니다. Windows·각 WSL 배포판·동적 메모리 VM·프로세스 격리
컨테이너는 10초 이내의 새 작업 판단만 서로 사용하고 가장 엄격한 최신 판단을 적용합니다. 다른
인스턴스의 대화 목록이나 PID를 로컬 제어 대상으로 합치지 않으며, 각 Supervisor는 자기 PID
공간의 CLI와 App만 제어합니다. 예를 들어 WSL의 App 때문에 `DRAIN`이 되면 federation을 통해
Windows CLI도 새 서브에이전트나 큰 작업 시작을 미룰 수 있지만, WSL Supervisor가 Windows CLI를
멈추거나 Windows Supervisor가 WSL App Server를 멈출 수는 없습니다.

## 터미널과 에이전트를 어떤 구조로 제어하나?

### 1. Claude Code·Codex CLI

CLI에서는 Claude Code와 Codex가 계속 터미널에 직접 연결됩니다. 백그라운드 감시 프로그램은 같은
프로세스 공간에서 보이는 관련 프로그램을 관찰하며, 제어는 다음 두 단계로 나뉩니다.

- 실행 전 검사: 아직 시작하지 않은 새 작업을 허용하거나 미룹니다.
- 프로그램 일시정지: 위험이 계속될 때 확인된 PID 하나만 운영체제 기능으로 멈춥니다.

```text
A. 사용자 작업 경로

                ┌──────────────────────┐
                │ 사용자 터미널        │
                │ 명령·결과 표시       │
                └──────────┬───────────┘
                           │ 직접 연결
                           ▼
                ┌──────────────────────┐
                │ Claude / Codex 리드  │
                │ 주 에이전트          │
                └──────────┬───────────┘
                           │ 지원 작업 전 호출
                           ▼
                ┌──────────────────────┐
                │ 실행 전 hook         │
                │ 원장의 최신 판단 조회│
                │ 판단·이유 반환       │
                └──────────┬───────────┘
                           │ 판단
           ┌───────────────┴───────────────┐
           ▼                               ▼
┌──────────────────────┐        ┌──────────────────────┐
│ ALLOW / OBSERVE      │        │ HOLD / DRAIN         │
│ 요청한 작업 실행     │        │ 대상 새 작업만 대기  │
│ 새 작업 지연 없음    │        │ 진행 중 결과 유지    │
└──────────────────────┘        └──────────────────────┘

B. 백그라운드 보호 경로

┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS 메모리·프로세스   │──── 측정 ─────►│ 로컬 Supervisor      │──── 기록 ─────►│ 상태·사건 원장       │
│ 남은 여유·감소 속도  │                │ 측정·제동·복구 판단  │                │ hook이 최신 판단 조회│
└──────────────────────┘                └──────────┬───────────┘                └──────────────────────┘
                                                   │ 보호 조치 발생 시
                               ┌───────────────────┴───────────────────┐
                               ▼                                       ▼
                    ┌──────────────────────┐                ┌──────────────────────┐
                    │ 알림·리드 인지       │                │ 확인된 로컬 PID 1개  │
                    │ 터미널: 즉시 알림    │                │ 최후 조건에서만      │
                    │ 리드: 다음 hook 1회  │                │ 일시정지·자동 재개   │
                    └──────────────────────┘                └──────────────────────┘

지원하는 기본 OS 축과 그 위에 추가되는 독립 실행 환경

                         ┌────────────────────────────────────┐
                         │ Federation 공유 판단               │
                         │ 새 작업 판단만 공유                │
                         │ 10초 동안 유효                     │
                         │ 가장 엄격한 최신 판단 적용         │
                         └─────────────────┬──────────────────┘
                                           ↕
                         공유 메모리를 경쟁하는 경계만 연결

       ┌────────────────────────────┐  ┌────────────────────────────┐  ┌────────────────────────────┐
       │ WSL 배포판 / VM / 컨테이너 │  │ VM / 컨테이너              │  │ VM / 컨테이너              │
       │ 각각 독립 Supervisor       │  │ 독립 Supervisor            │  │ 독립 Supervisor            │
       └──────────────▲─────────────┘  └──────────────▲─────────────┘  └──────────────▲─────────────┘
                      │ 위에서 실행                    │ 위에서 실행                    │ 위에서 실행
       ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐
       │ Windows 기본 OS            │  │ Linux 기본 OS              │  │ macOS 기본 OS              │
       │ 호스트 Supervisor          │  │ 호스트 Supervisor          │  │ 호스트 Supervisor          │
       └────────────────────────────┘  └────────────────────────────┘  └────────────────────────────┘

                    각 Supervisor는 자기 상태·hook·PID 공간만 제어
                         메모리 합산 없음 · 다른 환경의 PID 제어 없음
```

CLI의 주 에이전트 인지는 다음 순서로 보장합니다.

1. Supervisor가 원인, 대상, 현재 제한, 복구 방법을 사건 원장에 먼저 기록합니다.
2. hook에서 막힌 작업은 그 호출에 이유를 즉시 돌려줍니다.
3. 실제 프로그램 조치는 정확한 터미널에 즉시 표시하고, 별도 터미널이 없는 작업자 사건도
   주 에이전트의 다음 실제 hook에서 한 번 전달합니다.
4. OS·Discord·Telegram을 선택했다면 정상 보호의 시작과 완전 회복을 각각 한 번 알립니다.

예를 들어 Windows의 Claude 주 에이전트가 편집 결과를 정리하는 동안 WSL의 Codex가 새
서브에이전트와 큰 테스트를 시작하려 하고, 두 환경이 쓰는 물리 메모리 여유가 빠르게 줄었다고
가정합니다. WSL Supervisor가 `DRAIN`을 기록하면 federation이 그 판단을 Windows에 전달하고,
양쪽 hook은 새 서브에이전트와 테스트 시작만 기다리게 합니다. 진행 중인 편집·결과·메시지는
계속 허용합니다. 원인이 외부 VM이면 AI 프로그램은 멈추지 않습니다. 여유가 안정적으로 회복되면
새 작업을 다시 열고, 같은 AI 작업자의 증가가 원인으로 확인된 경우에만 논리 단계를 줄인 뒤
정확한 로컬 PID를 최후 수단으로 일시정지합니다.

전체 상태 흐름, 다중 터미널 배치와 실패 경계는
[아키텍처와 실행 구조](guides/architecture.ko.md)를 참고하십시오.

### 2. Codex Desktop App

Codex Desktop App에서는 대화 하나가 `session_id`를 가진 논리 thread 하나로 구분됩니다. 서로
다른 `session_id`는 각각 독립 리드이고, 같은 대화를 여러 창에서 열어도 하나의 논리 thread이자
하나의 리드로 계산합니다. 이 구분 덕분에 hook 단계의 작업 범위와 알림은 대화별로 관리할 수
있습니다. 다만 논리 thread마다 별도 PID와 메모리가 생기는 것은 아니며, 모두 App Server 하나를
공유합니다. 아래 구조는 이 논리적인 대화·에이전트 원장과 물리적인 프로세스·메모리 정보를 따로
모은 뒤, 확실한 연결과 blind 후보를 구분해 제어하는 과정을 보여 줍니다.

```text
                                        ┌──────────────────────┐
                                        │ Codex Desktop App    │
                                        │ 대화별 논리 thread   │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ 공유 App Server      │
                                        │ PID·메모리 공유      │
                                        └──────────┬───────────┘
                                                   │ hook·프로세스 관측
                           ┌───────────────────────┴───────────────────────┐
                           ▼                                               ▼
                ┌──────────────────────┐                        ┌──────────────────────┐
                │ 대화·에이전트 원장   │                        │ 프로세스·메모리 분류 │
                │ session = 독립 lead  │                        │ 확정 / blind 후보    │
                │ agent = 서브에이전트 │                        │ 공유 메모리 중복 없음│
                └──────────┬───────────┘                        └──────────┬───────────┘
                           └───────────────────────┬───────────────────────┘
                                                   ▼
┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS 메모리·프로세스   │──── 측정 ─────►│ 로컬 Supervisor      │──── 기록 ─────►│ 상태·사건 원장       │
│ 여유·감소 속도       │                │ App 전용 제어기      │                │ 대화 단계·hook 확인  │
│ App 지속 증가        │                │ 원인 집합·제동거리   │                │ 복구·알림 범위       │
└──────────────────────┘                └──────────┬───────────┘                └──────────┬───────────┘
                                                   │                                       │
                                                   ▼                                       ▼
                                        ┌──────────────────────┐                ┌──────────────────────┐
                                        │ App 단계별 완충      │                │ 해당 리드 다음 hook  │
                                        │ 새 고메모리 시작 대기│                │ 범위·복구 상태 전달  │
                                        │ 선택한 대화만 축소   │                └──────────────────────┘
                                        └──────────┬───────────┘
                                                   │ 위험 계속
                                                   ▼
                                        ┌──────────────────────┐
                                        │ 하위 작업 PID 하나   │
                                        │ 정확 귀속 우선       │
                                        │ blind 후보 하나씩    │
                                        └──────────┬───────────┘
                                                   │ 최후 조건
                                                   ▼
                                        ┌──────────────────────┐
                                        │ 공유 App Server 제동 │
                                        │ 모든 대화 함께 정지  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ 독립 자동 복구 장치  │
                                        │ 제한시간 뒤 자동 재개│
                                        └──────────────────────┘
```

예를 들어 대화 A가 빌드를 실행하고 대화 B가 답변을 정리하는 중이라고 가정합니다. 빌드 프로세스가
A의 hook과 정확히 연결되면 A의 새 작업부터 줄이고 B는 그대로 둡니다. 위험이 계속될 때도 공유
Server가 아니라 그 빌드 프로세스 하나가 먼저 물리 제동 후보가 됩니다.

빌드 프로세스가 A와 B 중 어디에서 시작됐는지 확정할 수 없다면 B를 임의로 원인으로 지목하지
않습니다. 먼저 App 전체의 새 고메모리 시작만 막고, 현재 큰 작업을 실행 중이거나 실제 증가와 가장
가까운 대화 하나의 앞으로 할 일만 줄입니다. 조치 효과를 확인할 짧은 관찰 시간 뒤 메모리 감소가
둔화됐는지 확인하고, 효과가 없을 때만 다음 후보를 추가합니다. 이 순차 확인 시간을 처음부터 App
제동거리에 포함하기 때문에, 성능을 너무 일찍 낮추지 않으면서도 위험선 전에 필요한 후보 확인을
끝낼 수 있습니다.

즉, 수단은 CLI와 다르지만 정책의 효과는 같습니다. 진행 중인 결과보다 새 작업을 먼저 줄이고,
리드보다 서브에이전트를 먼저 보며, 원인을 설명하는 최소 대상만 한 단계씩 제어하고, 물리 제동과
복구는 한 번에 하나씩 수행합니다. 정확한 하위 프로세스가 있으면 그 대상을 우선하고, blind 하위
프로세스는 관련 대화와 서브에이전트가 마지막 논리 단계를 실제로 확인한 뒤 하나만 제동합니다.
공유 App Server는 그보다 작은 방법이 모두 소진된 경우에만 모든 대화가 함께 멈추는 최후
수단입니다. 기본 OS·WSL·VM·컨테이너 사이의 federation 경계와 각 Supervisor가 자기 PID 공간만
제어한다는 원칙도 CLI와 같습니다. 세부 안전 조건은
[Codex Desktop App 안내](guides/codex-app.ko.md)에 정리되어 있습니다.

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
  [플랫폼과 여러 환경의 연동 방식](guides/platforms.ko.md)에 있습니다.

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
사용합니다. 자세한 측정 조건과 원자료는 [성능 측정](guides/performance.ko.md)에 있습니다.

한 물리 컴퓨터 안에서 Windows, WSL 배포판, 가상 머신, 격리 컨테이너처럼 실행 환경이 여러
개라면 Claude Code·Codex를 사용하는 환경마다 설치합니다. 같은 환경의 여러 터미널은 감시
프로그램 하나를 공유합니다. 각 환경의 설치와 federation 경로 설정이 끝나면 커널 수와 관계없이
같은 컴퓨터 전체가 최신 새 작업 허용 상태를 자동으로 공유합니다. 각 감시 프로그램은 자기 환경의
메모리와 프로세스만 측정·제어하므로 다른 환경의 프로세스 번호를 직접 조작하지 않습니다.
Windows와 WSL은 설치기가 같은 로컬 공유 폴더를 연결하고, 가상 머신·컨테이너는 호스트와 공유하는
로컬 폴더를 federation 경로로 지정합니다. 다른 물리 컴퓨터나 클라우드 서버를 네트워크 폴더로
묶지는 않습니다. 설정 방법은
[플랫폼과 여러 환경의 연동 방식](guides/platforms.ko.md)을 참고하십시오.

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
[알림 설정](guides/notifications.ko.md)에 있습니다.

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
[Claude Code 사용 안내](guides/usage-claude.ko.md)와 [Codex 사용 안내](guides/usage-codex.ko.md)에 있습니다.

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
[보안과 데이터·제어 경계](guides/security.ko.md)에 자세히 정리했습니다.

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
[테스트 범위](testing/test-matrix.ko.md)에 있습니다.

## 문서

| 안내서 | 용도 |
| --- | --- |
| [문서 전체 보기](README.ko.md) | 설치·사용 안내, 보안 경계와 공개 테스트 문서 찾기 |
| [아키텍처](guides/architecture.ko.md) | 백그라운드 감시, 실행 전 검사, 상태 파일, 프로그램 제어 구조 |
| [Codex Desktop App](guides/codex-app.ko.md) | 공유 App Server의 논리 대화 구분, blind 제어와 복구 |
| [적응형 제동거리](testing/stopping-distance.ko.md) | 제동 계산식, 실제 수치, 단계적 감속과 회복 결과 |
| [플랫폼과 여러 환경 연동](guides/platforms.ko.md) | 여러 운영체제와 가상 환경이 새 작업 허용 상태를 공유하는 방식 |
| [보안과 데이터·제어 경계](guides/security.ko.md) | 확인·저장·공유 정보와 자동·수동 제어의 한계 |
| [테스트 범위](testing/test-matrix.ko.md) | 공개 테스트가 확인하는 기능과 플랫폼 |
| [Claude Code](guides/usage-claude.ko.md) / [Codex](guides/usage-codex.ko.md) | CLI와 Desktop App 연결 설정·세션 동작 |
| [알림](guides/notifications.ko.md) | 터미널·OS·Discord·Telegram 전달 |
| [성능](guides/performance.ko.md) | 백그라운드 메모리 사용량과 실행 전 검사 시간 |
| [보안 정책](../.github/SECURITY.ko.md) | 취약점 비공개 제보 방법 |
| [기여 방법](../.github/CONTRIBUTING.ko.md) | 변경 원칙과 제출 전 검사 |

## 라이선스

[MIT](../LICENSE)
