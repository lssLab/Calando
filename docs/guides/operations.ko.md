# 운영·알림·복구

<p align="center">
  <a href="operations.md">English</a> · <strong>한국어</strong>
</p>

## 알림

Calando는 메모리 수치가 바뀔 때마다 알리지 않습니다. 실제 보호 조치가 시작되거나 완전히 해제됐을 때,
또는 사용자의 확인이 필요할 때만 한 번씩 알립니다.

| 경로 | 어디에 표시되는가 | 켜기·끄기 |
| --- | --- | --- |
| Hook 컨텍스트 | 주 에이전트의 다음 Hook | 항상 켜짐 |
| 터미널 | Hook이 보여 주는 한 줄 메시지, 일시정지된 프로세스의 터미널 고지 | 항상 켜짐 |
| Telegram | 사용자가 연결한 봇의 개인 대화 또는 그룹 | `calando telegram` / `calando telegram off` |

프로세스 고지는 그 프로세스를 가진 세션에만 갑니다. 다른 세션은 개수와 에이전트 메모리 합계로만
언급합니다. Telegram 연결 절차, 메시지 형식, 오류 해결은 [알림](notifications.ko.md)에 있습니다.

## 세션 명령과 Skill

설치기는 자동으로 판단하는 **Hook**과 `calando`라는 이름의 **Skill** 하나를 연결합니다. Skill은
`~/.claude/skills`, `~/.agents/skills`, `~/.codex/skills`에 복사되며 메모리 정책을 직접 집행하지 않습니다.

| 사용하는 곳 | 입력 |
| --- | --- |
| Claude Code | `/calando on\|off\|status\|help`, 고급: `/calando cap <GB>\|off` |
| Codex CLI·Desktop App | `$calando on\|off\|status\|help`, 고급: `$calando cap <GB>\|off` |

- 세션 명령은 UserPromptSubmit Hook이 바로 답합니다. 프롬프트는 모델에 전달되지 않으므로 토큰과
  컨텍스트를 쓰지 않습니다.
- 대상은 명령을 입력한 그 세션입니다(Hook payload의 session id). PID나 세션 목록을 고르지 않습니다.
- 세션 `off`: 그 세션은 계속 측정하지만 대기·일시정지·알림 대상에서 빠집니다.
- 세션 `cap`: 그 세션의 프로세스 트리만 제한합니다. 90% 근처에서 무거운 새 작업을 미루고, 상한을
  넘으면 증가 중인 서브에이전트 하나를 일시정지했다가 세션이 90% 아래로 내려가면 재개합니다. 첫 응답이
  설명하고 `/calando cap <GB> yes`로 적용합니다.
- `/calando status`는 그 세션의 에이전트가 보는 상태와, 필요하면 업데이트 안내를 보여 줍니다.

자세한 차이는 [Claude Code 사용 안내](usage-claude.ko.md)와 [Codex 사용 안내](usage-codex.ko.md)에 있습니다.

## 보안

Calando가 확인하는 것은 운영체제의 메모리와 프로세스 정보, Claude Code·Codex Hook이
전달하는 세션·에이전트·도구·작업 경로와 연결 상태, 실행할 명령의 앞부분뿐입니다. 이 정보로 새
작업 허용 여부와 정확한 제어 대상을 판단합니다.

자동 제어는 앞으로 시작할 Claude Code·Codex 작업을 잠시 미루는 것과, 최후의 보호 단계에서
확인된 로컬 작업 프로세스 하나를 일시정지·재개하는 것까지입니다. 자동으로 프로그램을 종료하거나
다른 프로그램을 제어하지 않습니다. 평상시 감시에는 외부 통신이 없으며, GitHub 설치·업데이트 확인과
사용자가 켠 Telegram 알림만 네트워크를 사용합니다.

**Calando가 확인하고 제어하는 범위는 여기까지이며, 그 외는 다루지 않습니다.**
프롬프트·대화·모델 응답과 Hook에 포함될 수 있는 파일 내용은 제어 판단에 사용하지 않으며 어디에도
남기지 않습니다. 프로젝트 파일과 프로세스 메모리를 직접 열어 읽지 않으며,
브라우저·IDE 내부 데이터, Claude·ChatGPT 인증 정보와 운영체제의 커널·메모리·스왑·방화벽
설정도 확인하거나 변경하지 않습니다. 저장 정보, federation 공유 범위와 안전장치는
[보안과 데이터·제어 경계](security.ko.md)에 자세히 정리했습니다.

## 제어와 복구

수동 재개 명령은 없습니다. 메모리 압박으로 멈춘 프로세스는 메모리가 회복되면 하나씩 자동으로
재개됩니다. 리드 에이전트가 자기 메모리 증가로 멈춘 경우에는 안전장치가 있는 자동 재개를 한 번
시도해 증가세가 가라앉았는지 확인합니다.

리드 에이전트 일시정지는 의도적으로 매우 드뭅니다. 새 작업 대기와 서브에이전트·도구 제어를
단계적으로 적용했는데도 위험이 계속되고, 같은 리드의 지속적인 메모리 증가와 정확한 터미널이
확인됐을 때만 쓰는 **최후의 보호 단계**입니다.

리드가 계속 멈춰 있으면 둘 중 하나를 고릅니다.

- 그 터미널을 닫고 CLI의 자체 재개 기능으로 이어갑니다.

  ```bash
  claude --resume
  codex resume
  ```

  재개된 세션의 첫 Hook은 이전 프로세스가 일시정지된 채 끝났고, 그 실행의 서브에이전트 결과가
  빠졌을 수 있다고 알려 줍니다.
- `calando off`를 실행합니다. 멈춘 프로세스를 그 자리에서 재개한 뒤 보호를 끕니다.

Codex App의 공유 App Server는 안전장치가 있는 재개가 실패해도 멈춘 채로 두지 않고 thread별 제한으로
돌아갑니다.

## 터미널 명령

터미널 명령은 실행한 환경 전체에 적용됩니다. Windows, 각 WSL 배포판, VM, 컨테이너는 그 환경 안에서
따로 실행합니다.

| 명령 | 용도 |
| --- | --- |
| `calando status [--json]` | 이 환경, CLI 연결, 함께 동작하는 다른 모든 환경의 상태 |
| `calando on` / `off` | 보호 켜기·끄기. `off`는 Calando가 멈춘 프로세스를 모두 재개한 뒤 끕니다 |
| `calando telegram [CHAT_ID]` / `telegram off` | Telegram 알림 설정·해제 |
| `calando cap [<GB> \| off]` | 고급: 이 환경의 모든 에이전트에 대한 하드캡 |
| `calando update` | 지금 업데이트 |
| `calando update auto on` / `off` | 자동 업데이트 켜기·끄기(기본값 켜짐) |
| `calando uninstall` | 설정과 상태를 보존하고 제거 |
| `calando lock on` / `off` | 관리자 잠금 |
| `calando help` | 도움말 |

0.3.x 동안에는 `memory-supervisor`와 `memory-status`도 명령 별칭으로 동작합니다. 오래된 federation
peer 파일은 일주일 뒤 자동으로 정리됩니다.

### on·off

`off`는 백그라운드 서비스와 자동 시작을 끄되 설치된 Hook은 그대로 두고 조용히 통과시킵니다. 이
상태는 재부팅과 업데이트 뒤에도 유지되며 `on` 한 번으로 다시 켭니다. 멈춘 프로세스를 재개하지 못하면
`off`는 거부됩니다. 사용자가 `off`를 실행하지 않았는데 서비스가 멈췄다면 Hook이 10초 뒤 오래된 판단을
버리고 **보호 기능을 사용할 수 없음**을 알립니다.

### cap

`calando cap`은 일반 사용에는 권하지 않습니다. 엄격한 연구나 벤치마크 실행을 위한 고급 기능입니다.

- 인수 없이 실행하면 자동 용량과 현재 상한을 보여 줍니다.
- `calando cap <GB>`는 이 환경이 지금 쓸 수 있는 양을 넘는 값을 거부하고, 설명을 출력한 뒤 `[y/N]`으로
  확인합니다. 대화형이 아닌 호출자는 `--yes`를 붙입니다.
- 상한 근처에서는 무거운 새 작업을 미루고, 상한을 넘으면 증가 중인 서브에이전트 하나를 일시정지합니다.
- 일시정지는 이미 쓴 메모리를 되찾지 못하므로 사용량이 잠깐 상한을 넘을 수 있습니다. 바이트 단위로
  정확한 한도가 필요하면 cgroup·컨테이너·VM 한도를 씁니다.

### update

설치된 감시 프로그램은 하루에 한 번 백그라운드에서 최신 GitHub 릴리스를 확인합니다. 멈춘 프로세스가
없고 메모리가 안정적일 때 자기 서비스 밖에서 업데이트를 실행하며, 바쁘면 한 시간마다 다시 시도합니다.

- Claude Code: Hook 변경을 포함해 자동 적용됩니다. 새 세션부터 새 Hook을 씁니다.
- Codex: Hook 정의가 바뀌는 릴리스(마이너 버전, 예: 0.3 → 0.4)는 Codex가 연결돼 있으면 자동 적용하지
  않습니다. Codex는 사용자가 CLI `/hooks`나 App **설정 → Hooks**에서 신뢰하기 전까지 바뀐 Hook을 조용히
  건너뛰기 때문입니다. 상태 화면과 Telegram 메시지 한 번이 `calando update` 실행과 Hook 신뢰를
  안내합니다. 패치 릴리스는 Hook을 바꾸지 않습니다.
- 자동 업데이트를 끄면 `calando status`, `/calando status`, 버전마다 한 번의 Telegram 메시지가 새 버전을
  알립니다. 에이전트 Hook은 업데이트를 언급하지 않습니다.
- 매일 확인할 때 Claude Code가 `settings.json`에서 Calando Hook을 빠뜨렸다면 다시 넣습니다.
- 0.2.x에서 0.3.0으로 올리면 Hook 명령이 바뀌므로 Codex 사용자는 Calando Hook을 한 번 다시
  신뢰해야 합니다.

### lock

`calando lock on`은 관리자만 실행할 수 있습니다. 켜져 있는 동안 터미널과 세션의 `off`, `uninstall`이
거부됩니다. 잠금 파일은 `/etc/calando/lock`(Windows `%ProgramData%\Calando\lock`)입니다. 정책 파일일 뿐
샌드박스가 아닙니다. 한계는 [보안](security.ko.md)에 있습니다.

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
