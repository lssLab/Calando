# 알림 — 터미널, Hook, Telegram

<p align="center">
  <a href="notifications.md">English</a> · <strong>한국어</strong> · <a href="notifications.zh-CN.md">简体中文</a> · <a href="notifications.ja.md">日本語</a>
</p>

Calando의 알림 경로는 세 가지다.

| 경로 | 켜기·끄기 | 내용 |
| --- | --- | --- |
| Hook 컨텍스트 | 항상 켜짐 | 에이전트가 다음 Hook에서 받는 상태와 이유 |
| 터미널 한 줄 | 항상 켜짐 | Hook이 터미널에 보여 주는 한 줄 메시지와, 일시정지된 프로세스의 터미널에 쓰는 고지 |
| Telegram | 선택 | 실제 보호 조치를 휴대폰으로 받는 유일한 푸시 경로 |

운영체제 팝업이나 다른 메신저 경로는 없다. 언제든 `calando status`(환경 전체) 또는
`/calando status`(그 세션)로 상태를 확인한다.

## 터미널 고지

터미널 고지는 색상 변화마다 출력되지 않는다. lead pause·resume·probation처럼 실제 조치가 있을 때만
대상 PID의 정확한 TTY 또는 Windows console identity를 다시 확인하고 평문 한 번을 쓴다. 입력을
주입하거나 terminal mode를 바꾸지 않으므로 CLI 세션이나 TUI 상태를 손상시키지 않는다. full-screen
TUI가 한두 줄 다시 그릴 수는 있지만 다음 redraw에서 화면이 복원된다. 정확한 대상 terminal에 쓸 수
없으면 lead를 멈추지 않는다. 차단된 도구 호출의 이유는 항상 lead에게 반환된다.

## Telegram 연결

Calando는 Telegram 명령을 받는 webhook server를 만들지 않는다. Bot API의 `sendMessage`로 보내기만
한다.

1. Telegram에서 `@BotFather`를 열고 `/newbot`으로 봇을 만든 뒤 token을 복사한다.
2. 개인 알림이면 새 봇과의 대화를 열어 둔다. 그룹 알림이면 봇을 그룹에 추가한다.
3. 아래 명령을 실행한다.

```bash
calando telegram
```

4. `Telegram bot token from @BotFather (hidden):`에 token을 붙여넣고 Enter를 누른다. 입력 문자는
   화면에 보이지 않고 shell history에도 남지 않는다. 명령은 먼저 이미 와 있는 메시지를 확인하고,
   없으면 120초 동안 기다린다. 그때 봇에게 아무 메시지나 보낸다(그룹이면 그룹에 보낸다).
5. chat이 하나만 보이면 ID를 저장하고 시험 메시지를 보낸다. `Telegram on. A test message was sent.`가
   나오고 Telegram에 메시지가 오면 끝이다.

여러 chat이 이 봇에 메시지를 보냈다면 ID와 이름 목록을 출력하고 아무것도 저장하지 않는다. 원하는
ID를 골라 다시 실행한다. 그룹 ID는 보통 음수다.

```bash
calando telegram -1001234567890
```

| 오류 | 뜻 | 조치 |
| --- | --- | --- |
| `HTTP 401` | token이 잘못됐거나 폐기됨 | `@BotFather`에서 현재 token을 다시 복사해 재실행 |
| `HTTP 409` | 이 봇에 webhook 또는 다른 `getUpdates` 소비자가 이미 있음 | 기존 연동은 건드리지 않으므로 Calando 전용 봇을 사용 |
| `connection failed or timed out` | Telegram API까지 네트워크 연결 실패 | 인터넷·방화벽·프록시를 확인한 뒤 재실행 |
| `No message reached the bot within 120 seconds` | 기다리는 동안 새 메시지가 오지 않음 | 명령이 기다리는 동안 정확한 봇에 새 메시지 전송 |

실패하면 token과 chat ID는 저장되지 않는다. Calando는 `deleteWebhook`을 호출하지 않는다.

Telegram 설정을 지우려면:

```bash
calando telegram off
```

설정은 비공개 파일에 원자적으로 저장되며 Unix 계열에서는 권한이 `600`이다. 직접 열 필요는 없다.

| 환경 | 저장 위치 |
| --- | --- |
| Linux, WSL, macOS | `~/.config/calando/notifications.conf` |
| Windows | `%USERPROFILE%\.config\calando\notifications.conf` |

환경변수 `MEMORY_SUPERVISOR_TELEGRAM_*`를 지정했다면 그 값이 저장된 값보다 우선한다.

## 메시지 형식

Telegram 메시지는 영어로, 첫 줄은 상태, 둘째 줄은 환경, 그 아래는 한 줄에 `Label: value` 하나씩이다.

```text
Memory protection on
Windows (LAPTOP-1)
Free: 1.5 of 15.7 GB
WSL VM: holds 9.6 GB, uses 3.2 GB
Action: New heavy work is on hold. Running work continues.
```

사용하는 이름은 `Free`, `WSL VM`(Windows에서만), `Restricted agents`, `Paused processes`, `Duration`,
`Action`이다. 프로세스 고지는 그 프로세스를 가진 세션에만 간다. 다른 세션은 개수와 에이전트 메모리
합계로만 언급하며 이름·경로·PID는 보여 주지 않는다.

## 이 알림이 오는 순간들

- `DRAIN`, 실제로 작업을 미룬 `HOLD`, 논리 제한, managed 정지 PID, lead probation 중 하나가 처음
  활성화될 때 `pressure-episode / active` 1회
- 위 조건이 admission 안정 확인 시간 동안 모두 해소된 상태로 유지될 때 `recovered`, 확인된 재개 전에 정지 worker가 사라졌다면
  `ended-with-loss` 1회
- exact-PID pause/resume의 정확한 터미널 안전 고지
- fresh하던 federation peer의 stale 전환과 이후 회복
- 데몬 없이 훅이 fail-open일 때의 rate-limit된 보호-불능 경고
- sensor/runtime/notification 보호 저하와 probation 실패처럼 별도 조치가 필요한 실패
- 업데이트 안내: 자동 업데이트가 꺼져 있을 때 새 버전마다 한 번, Codex Hook을 다시 신뢰해야 하는
  업데이트가 자동 적용되지 않았을 때 한 번

Raw utilization 전이와 아직 조치하지 않은 leak suspect는 알림 없이 사건 원장에만 남는다. 평범한
`SessionStart/End`, `SubagentStart/Stop`, 그대로 유지되는 `ACTIVE`, 변화 없는 `HOLD/DRAIN` tick도
알림을 다시 만들지 않는다. 에피소드 안의 개별 spawn 거부·worker 시작 지연·논리 완충·PID별
pause/resume·정상 probation 단계는 `importance=detail`이다. 거부된 Hook의 `systemMessage`는 해당
lead에 바로 전달되지만 Telegram으로 따로 복제하지 않는다.

기준은 event 이름이 아니라 의도다. 관측 근거를 lead가 알아야 할 선제 지시로 올렸다면 사용자에게
보이는 조치이므로 한 번 전달한다. 누구에게도 새 행동을 요구하지 않는 sensor 표본이나 그대로인 경계는
원장에만 남고 model context를 쓰지 않는다.

lead 사건 문장은 PID와 함께 근거가 직접 프로세스인지 머신 pressure인지, 시스템 귀속이
`agent|external|mixed|unknown` 중 무엇인지, 자동 복구를 기다리면 되는지를 설명한다. lead가 멈춰
Hook을 실행할 수 없어도 터미널 고지와 Telegram은 따로 동작한다. 터미널·Telegram은 즉시 시도되지만
model/lead는 다음 Hook 경계에서 알게 된다. 반복은 event type·status·source·incident/session
epoch로 억제한다. 실제 정상화는 새 전이라 한 번 전달하지만, 그대로 유지되는 경계는 새 알림이 아니다.

Hook, `calando status`, 터미널 고지, Telegram은 같은 구조화 사건을 같은 사용자 경계에서 렌더링한다.
이전 release가 저장한 사건도 이 경계에서 정규화하므로 업데이트 뒤 낡은 debug 문구가 재생되지 않는다.

정확한 사건 원장은 로컬 `runtime.json`/`state.json`의 notification ledger다. Telegram 전송은
실패해도 감지·보호를 막지 않는 best-effort 복제본이다.
