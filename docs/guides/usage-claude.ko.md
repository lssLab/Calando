# Claude Code 사용 안내

<p align="center">
  <a href="usage-claude.md">English</a> · <strong>한국어</strong>
</p>

## 지원 기준

Memory Supervisor `0.2.0-alpha.3`은 Claude Code **2.1.217 이상**을 지원합니다. 이 버전은
단계적 논리 제어 계약을 적용하는 최소 기준입니다. 그보다 오래된 버전에 축소된 matcher나 별도
호환 정책을 적용하지는 않습니다.

```bash
claude --version
claude update
```

설치기는 버전과 Hook 연결을 서로 다른 상태로 검사합니다. 현재 `PATH`뿐 아니라 네이티브 설치,
NVM, fnm, asdf, Volta, Windows npm 등 알려진 사용자 설치 경로를 함께 찾아 확인할 수 있는 최신
지원 버전을 선택합니다. 따라서 로그인 셸이 아닌 프로세스의 `PATH` 앞쪽에 오래된 실행 파일이
있어도 현재 사용자 설치본을 가리지 않습니다.

지원 실행 파일을 확인하지 못하면 버전 문제를 알리되, 기존 Memory Supervisor Hook은 보존합니다.
한 번의 버전 검사 실패만으로 유효한 Hook이 잘못됐다고 볼 수 없기 때문입니다.
`memory-status --connections`도 버전과 Hook 상태를 따로 보여 주며, 둘 다 준비되기 전에는 해당
프로그램을 보호 중이라고 표시하지 않습니다. 원래 설치 방식에서 `claude update`를 지원하지 않으면
같은 패키지 관리자나 설치기로 Claude Code를 업데이트한 뒤 `memory-supervisor update`를 실행합니다.

Claude Code는 지원하는 CLI 가운데 가장 넓은 문서화된 연결 범위를 제공합니다. `PreToolUse`가 모든
도구 경로를 관찰하고 실제 입력을 분류해, 시스템의 새 확장 작업과 이름이 확인된 논리 에이전트의
앞으로 할 작업만 필요한 범위에서 제어합니다. 이미 시작한 도구 실행을 되돌리지는 않습니다.

플랫폼 설치기를 실행하면 다음 이벤트가 `~/.claude/settings.json`에 병합됩니다. 기존의 다른
Hook은 교체하지 않습니다.

- `SessionStart`: 시작할 때 자원 보호 계약을 전달합니다. resume·clear·compact에서는 아직 전달하지
  않은 일시정지 사건이 없을 때만 조용히 지나갑니다.
- `UserPromptSubmit`: 현재 적응형 판단이 GREEN이 아니거나 아직 전달하지 않은 사건이 있으면,
  이미 회복된 뒤라도 해당 정보를 전달합니다.
- `PreToolUse`: 모든 도구를 분류합니다. 시스템 압박에서는 새 확장만 기다렸다가 판단을 돌려주고,
  심각한 압박에서는 새 고메모리 작업 시작만 보류하며, 정확한 논리 상태가 제외한 앞으로의 작업
  종류만 거부합니다.
- `SubagentStart`: 수명주기를 기록하고 RED에서만 12초 대기하는 보조 장치로 동작합니다. ORANGE는
  이미 허용된 작업자를 지연하지 않습니다.
- `SubagentStop`: Supervisor의 거부로 결과가 일부만 만들어졌을 가능성을 보존하면서 논리 수명주기
  기록을 닫습니다.
- `PostToolUse`와 `PostToolBatch`: 진행 상황을 기록합니다. 리드 경계에서는 아직 전달하지 않은 사건을
  전달하지만, 서브에이전트 경계가 리드의 사건 확인 위치를 대신 소비하지 않습니다. RED라고 해서
  고정 대기 시간을 추가하지도 않습니다.
- `Stop`과 `SessionEnd`: 정상 종료를 막지 않고 리드와 세션의 수명주기 기록을 닫습니다.

## Hook 활성화, 작업 폴더 신뢰와 다시 읽기

Claude Code에는 Codex처럼 Hook마다 hash를 승인하는 절차가 없습니다. 설치기가 Memory Supervisor
Hook을 사용자 설정 `~/.claude/settings.json`에 쓰며, 이 사용자 Hook에는 별도의 승인·활성화
스위치가 없습니다. 다만 대화형 Claude Code는 현재 작업 폴더나 상위 폴더의 작업 폴더 신뢰를
사용자가 승인하기 전까지 이 사용자 Hook을 포함한 모든 설정 파일 Hook을 보류합니다. Claude의
`/hooks` 화면은 읽기 전용이라 이 신뢰를 승인할 수 없습니다.

작업 폴더 신뢰는 Memory Supervisor 전용 Hook 승인이 아니라 폴더 단위의 Claude Code 결정입니다.
신뢰하는 작업 폴더에서만 승인하십시오. 승인 뒤 현재 Claude Code는 설정 파일을 감시하므로, 실행
중이어도 나중에 바뀐 사용자 Hook을 보통 자동으로 다시 읽습니다. 잠시 기다려도 항목이 나타나지
않을 때만 다시 시작하고, 세션마다 한 번 실행되는 `SessionStart` 자체를 시험하려는 경우에만 새
세션을 여십시오.

일반 비대화형 `claude -p`도 같은 사용자 설정과 Hook을 읽으므로 별도 설정 없이 Memory
Supervisor의 보호를 받습니다. 이 모드에서는 작업 폴더 신뢰 검사를 생략합니다. `--bare`를 함께
사용하면 Claude Code가 의도적으로 모든 Hook을 건너뛰므로 그 실행은 감독할 수 없습니다.

설치나 `memory-supervisor update` 뒤에는 `memory-status --connections`를 실행합니다. Claude의
`CONNECTED`는 지원 실행 파일, Skill과 현재 사용자 Hook 연결을 확인합니다. 필요하면 Claude의
읽기 전용 `/hooks`에서 `User Settings` 항목을 볼 수 있습니다. 어느 검사도 현재 폴더의 작업 폴더
신뢰까지 증명하지는 않습니다. managed-only Hook이나 `disableAllHooks` 같은 조직 정책이 사용자
Hook을 막을 수 있으며, 이 경우에는 관리자의 조치가 필요합니다.

설치된 명령 Hook에는 선택 항목인 `statusMessage`를 일부러 넣지 않습니다. 따라서 평상시 Hook
실행이 TUI에 Memory Supervisor 진행 문구를 계속 띄우지 않으며, 실제 보호 조치나 아직 전달하지
않은 사건이 있을 때만 사용자 메시지가 나타납니다. 이미 열려 있던 세션에 과거의 일반 Hook 진행
문구가 계속 보이면 `memory-supervisor update`를 실행하고 새 Claude Code 세션을 열어 최신 Hook
정의를 읽게 합니다.

새 작업 허용 판단은 `MEMORY_SUPERVISOR_FEDERATION_DIR`에서 최근 상태 가운데 가장 엄격한 적응형
조치를 사용합니다. 따라서 호스트·WSL·VM 한쪽의 압박이 모든 Claude의 새 확장을 기다리게 할 수
있지만, 실제 프로세스 일시정지는 각 환경 안에서만 이루어집니다. 사용률 색만으로 새 확장을 막지는
않습니다.

Claude 리드가 `PAUSED_BY_SUPERVISOR` 상태이면 그 프로세스 안의 Hook은 일시정지 중에 실행될 수
없습니다. 그래서 Supervisor가 원인과 정확한 복구 정책을 다시 확인한 대상 터미널에 직접 쓰고,
OS·Discord·Telegram 알림도 각각 별도로 대기열에 넣습니다. 자동 시험 재개, 성공, 실패, 수동
재개와 외부 명령으로 직접 재개한 경우에도 단계에 맞는 안내를 제공합니다. 이후 다음 프롬프트나
도구 Hook에서 사용자와 모델에 같은 사건을 한 번 전달하며, 이 시점은 운영체제 수준의 재개보다
늦을 수 있습니다. `memory-supervisor resume`은 같은 PID와 메모리 속 세션을 이어갑니다. Claude가
종료된 뒤 `--resume`으로 시작하면 Claude의 대화 복원과 별도로 `SessionStart source=resume`이
자원 사건을 전달합니다.

`StructuredOutput`을 비롯한 결과·메시지·상태 도구는 `HANDOFF_ONLY`에서도 허용됩니다. Supervisor가
거부한 도구, 이유, 시각과 논리 epoch는 기록한 뒤 다음 완료 또는 프롬프트 경계에서 리드에게
요약합니다. 제공자 사용량 소진처럼 일반적인 성공 도구 결과 문자열로만 오는 오류는 구조화된 실패
신호가 없으므로 서브에이전트가 직접 보고해야 합니다.

의도된 판단은 exit code 0인 JSON입니다. 안정 래퍼는 Rust gate, 상태 또는 정책 오류도 조용한
exit 0으로 바꾸므로 내부 오류가 Claude Code의 exit code 2 프롬프트 차단으로 잘못 해석되지
않습니다.

확인 명령:

```bash
bash tests/run.sh
memory-status --connections
memory-status
printf '{}' | hooks/gate.sh SessionStart
```

기준 문서:

- [Claude Code Hook](https://code.claude.com/docs/en/hooks)
- [Claude Code 권한과 작업 폴더 신뢰](https://code.claude.com/docs/en/permissions)
- [Claude Code 설치와 업데이트](https://code.claude.com/docs/en/installation)
- [Claude Code 설치 문제 해결](https://code.claude.com/docs/en/troubleshoot-install)

Windows에서는 다음 명령을 사용합니다.

```powershell
'{}' | powershell -File .\hooks\gate.ps1 SessionStart
```

개인 Skill은 `~/.claude/skills/memory-supervisor`에 연결됩니다. 최상위 `skills` 폴더가 처음 만들어진
경우에는 Claude Code가 Skill을 찾도록 새 세션이 필요할 수 있습니다.

프로세스가 일시정지되면 먼저 상태의 조치 안내를 따릅니다. 시스템 압박으로 멈춘 작업자와 지속적인
실제 증가가 확인돼 멈춘 리드의 첫 시험 재개는 자동으로 복구됩니다. 사용자가 수동 재개를 결정한
경우에는 raw `kill -CONT` 대신 `memory-supervisor resume <pid>`를 사용합니다. 관리 대상이 정확히
하나일 때는 PID를 생략할 수 있습니다. 이 명령은 시작 정보를 다시 확인하고, 상태를 정리하고,
`RESUMED` 사건을 저장한 뒤 재개 직후의 완충 시간을 적용합니다.

## Hook 때문에 모든 프롬프트가 막힐 때

막힌 세션 안에서 활성 Hook을 계속 수정하지 말고 별도 터미널에서 다음 순서로 확인합니다.

1. `~/.claude/settings.json`과 현재 Supervisor 소스 폴더를 백업합니다.
2. `printf '{}' | hooks/gate.sh UserPromptSubmit`을 실행합니다. 안전한 결과는 유효한 JSON 또는 출력
   없음이며, 항상 exit code 0이어야 합니다.
3. `bash tests/run.sh`를 실행합니다.
4. `memory-supervisor update`로 Supervisor 소유 Hook 항목만 원자적으로 교체하고 서비스를 다시
   읽힙니다.
5. Hook 정의가 세션 시작 때 고정됐을 수 있으므로 새 Claude Code 세션을 엽니다.

이 절차 뒤에도 모든 프롬프트가 막히면 `memory-status --connections` 결과와 gate의 exit code를
확인해 문제를 Hook 연결과 Supervisor 상태 중 어느 쪽에서 재현했는지 분리합니다.
