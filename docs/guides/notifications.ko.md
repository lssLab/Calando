# 알림 설정 — 파일을 열지 않고 터미널 명령으로 끝내기

<p align="center">
  <a href="notifications.md">English</a> · <strong>한국어</strong> · <a href="notifications.zh-CN.md">简体中文</a> · <a href="notifications.ja.md">日本語</a>
</p>

Memory Supervisor를 설치하면 Linux, WSL, macOS, Windows PowerShell에서 똑같이
`memory-supervisor notifications ...` 명령을 사용할 수 있다. 설정 파일을 찾아 열거나 따옴표·변수명을
직접 입력할 필요가 없다. 경로 변경과 자격 정보는 다음 알림 사건부터 바로 적용되므로 데몬이나
Claude Code·Codex CLI를 재시작할 필요도 없다.

Discord 웹후크 URL과 Discord·Telegram 봇 토큰은 명령에 붙이지 않는다. 명령을 실행한 뒤 나타나는
`(hidden)` 프롬프트에 붙여넣는다. 입력 문자는 화면에 보이지 않고 shell history에도 남지 않는다.
Supervisor가 OS별 비공개 설정 파일에 원자적으로 저장하며 Unix 계열에서는 권한도 `600`으로
고정한다.

## 먼저 현재 상태 확인

다음 한 줄을 어느 터미널에서든 복사해 실행한다.

```bash
memory-supervisor notifications show
```

출력에는 활성 경로, Discord 방식, Telegram chat이 표시되지만 웹후크와 토큰 원문은 절대 표시되지
않는다.

## 선택 알림 경로 켜기·끄기

알림 내용은 어떤 설정에서도 **실제 보호 조치만** 대상으로 한다. GREEN/YELLOW/ORANGE/RED 색상 전이와
아직 확정되지 않은 leak 관측은 `memory-status`와 사건 원장에만 남는다. 아래 명령은 알림의 상세도를
바꾸는 것이 아니라 같은 조치 알림을 어디로 전달할지만 정한다.

`hook`은 main 에이전트의 상황 인지와 복구 계약이고, `terminal`은 lead가 멈춰 자기 hook을 실행하지
못할 때 정확한 복구 명령을 전달하는 경로이므로 둘 다 항상 켜져 있다. 명령, 내부 설정 파일,
환경변수 중 어느 것으로도 끌 수 없다. 아래 명령은 `os,discord,telegram` 세 개의 선택 경로만 바꾼다.

모든 경로를 사용하려면:

```bash
memory-supervisor notifications routes all
```

필수 훅·해당 터미널에 OS 알림만 추가하려면:

```bash
memory-supervisor notifications routes os
```

예를 들어 OS 팝업과 Discord만 사용하려면:

```bash
memory-supervisor notifications routes os,discord
```

선택 경로를 모두 끄고 필수 hook·terminal만 유지하려면:

```bash
memory-supervisor notifications routes none
```

사용 가능한 선택 이름은 `os,discord,telegram`이다. `hook`이나 `terminal`을 경로값에 넣으면
“필수라서 설정할 수 없다”는 오류와 함께 명령이 거부된다. Discord나 Telegram 연결 명령을 실행하면
그 원격 경로는 기존 선택에 자동으로 추가된다. `all`을 선택해도 Discord·Telegram 자격 정보가 없으면
해당 경로는 조용히 건너뛴다.

터미널 알림은 색상 변화마다 출력되지 않는다. lead pause·resume·probation처럼 실제 조치가 있을 때만
대상 PID의 정확한 TTY 또는 Windows console identity를 다시 확인하고 평문 한 번을 쓴다. 입력을
주입하거나 terminal mode를 바꾸지 않으므로 CLI 세션이나 TUI 상태를 손상시키지 않는다. full-screen
TUI가 한두 줄 다시 그릴 수는 있지만 이는 드문 조치를 즉시 보이게 하는 의도된 표시이며, 실행 중인
TUI는 다음 redraw에서 화면을 복원한다. 정확한 대상 terminal에 쓸 수 없으면 lead를 그대로 멈추지
않는다. 차단된 AI CLI 도구 호출의 이유도 항상 lead에게 반환된다.

OS 경로는 Linux의 `notify-send`, WSL host의 Windows 알림, macOS의 `osascript`, Windows의
NotifyIcon을 사용한다.

## Discord A — 웹후크 연결 (권장)

봇을 만들 필요가 없어 가장 간단하다.

1. Discord 데스크톱 또는 웹에서 알림을 받을 서버의 텍스트 채널을 연다.
2. **채널 편집 → 연동(Integrations) → 웹후크(Webhooks) → 새 웹후크**를 선택한다.
3. 이름과 대상 채널을 확인하고 **웹후크 URL 복사**를 누른다.
4. 아래 명령 한 줄을 실행한다.

```bash
memory-supervisor notifications discord-webhook
```

5. `Discord webhook URL (hidden):`이 나오면 복사한 URL을 붙여넣고 Enter를 누른다. 붙여넣은 문자가
   보이지 않는 것이 정상이다.
6. 연결 시험을 실행한다.

```bash
memory-supervisor notifications test
```

`discord: delivered`가 나오고 채널에 테스트 메시지가 오면 끝이다. 이 명령은 Discord 경로를 자동으로
켜고, 이전 Discord 방식이 있었다면 새 웹후크 방식으로 교체한다.

웹후크 URL은 그 채널에 메시지를 쓸 수 있는 비밀이다. 유출되면 Discord에서 해당 웹후크를 삭제하고
새로 만든 뒤 위 명령을 다시 실행한다.

## Discord B — 기존 봇으로 채널에 보내기

이미 Discord 봇을 운영하고 있을 때만 사용한다.

1. Discord Developer Portal에서 봇 토큰을 준비하고, 봇을 서버에 초대해 대상 채널의
   **메시지 보내기** 권한을 부여한다.
2. Discord **사용자 설정 → 고급 → 개발자 모드**를 켠다.
3. 대상 채널을 우클릭해 **채널 ID 복사**를 누른다.
4. 아래의 숫자를 복사한 채널 ID로 바꿔 실행한다.

```bash
memory-supervisor notifications discord-channel 123456789012345678
```

5. `Discord bot token (hidden):`에 봇 토큰을 붙여넣고 Enter를 누른 뒤 시험한다.

```bash
memory-supervisor notifications test
```

토큰 값에 `Bot ` 접두어를 붙이지 않는다. Supervisor가 API 요청에서 자동으로 붙인다.

## Discord C — 기존 봇으로 개인 DM 보내기

봇과 같은 서버에 있고 해당 서버의 DM을 허용해야 한다.

1. Discord 개발자 모드를 켠 뒤 자신의 프로필을 우클릭해 **사용자 ID 복사**를 누른다.
2. 아래의 숫자를 자신의 사용자 ID로 바꿔 실행한다.

```bash
memory-supervisor notifications discord-dm 123456789012345678
```

3. 숨김 프롬프트에 봇 토큰을 붙여넣고 시험한다.

```bash
memory-supervisor notifications test
```

첫 발송 때 봇이 DM 채널을 만들고 해당 채널 ID만 로컬에 캐시한다.

Discord 자격 정보를 지우고 경로도 끄려면 다음 한 줄을 실행한다.

```bash
memory-supervisor notifications disable-discord
```

## Telegram — 봇과 chat 자동 연결

Supervisor는 Telegram 명령을 받는 공개 webhook server를 만들지 않는다. Bot API의 `sendMessage`로
알림을 보내기만 한다.

1. Telegram에서 `@BotFather`를 열고 `/newbot`으로 봇을 만든 뒤 token을 복사한다.
2. 개인 알림이면 새 봇과의 대화를 열어 둔다. 그룹 알림이면 봇을 그룹에 추가한다.
3. 아래 한 줄을 실행한다.

```bash
memory-supervisor notifications telegram
```

4. `Telegram bot token (hidden):`에 token을 붙여넣고 Enter를 누른다. 명령은 먼저 이미 대기 중인
   update를 확인한다. 없으면 “waiting 120 seconds”를 출력하므로 그때 해당 봇에게 `/start`나 새 메시지를
   보낸다. 그룹이면 그룹에 새 메시지를 보낸다. chat이 하나만 보이면 ID를 찾아 저장하고 Telegram
   경로도 켠다.
5. 연결을 시험한다.

```bash
memory-supervisor notifications test
```

`telegram: delivered`가 나오고 Telegram에 테스트 문장이 오면 끝이다.

봇이 여러 개인 대화·그룹의 update를 보고 있어 chat이 여러 개 발견되면 명령이 ID와 이름 목록을
출력하고 아무것도 저장하지 않는다. 원하는 ID를 골라 다음처럼 다시 실행한다. 그룹 ID는 보통 음수다.

```bash
memory-supervisor notifications telegram -1001234567890
```

숨김 프롬프트에 같은 token을 다시 붙여넣는다. 120초 안에 chat을 찾지 못했다면 명령을 다시 실행하고,
대기 문장이 나타난 뒤 token과 짝이 맞는 정확한 봇에게 새 메시지를 보낸다. 오래전에 보낸 `/start`를
다시 읽을 수 있다고 가정하지 않는다.

탐지 실패는 더 이상 전부 “chat 없음”으로 표시되지 않는다.

| 오류 | 뜻 | 조치 |
| --- | --- | --- |
| `HTTP 401` | BotFather token이 잘못됐거나 폐기됨 | `@BotFather`에서 현재 token을 다시 복사해 재실행 |
| `HTTP 409` | 이 봇에 webhook 또는 다른 `getUpdates` 소비자가 이미 있음 | 기존 연동을 자동 삭제하지 않으므로 Supervisor 전용 봇을 사용 |
| `connection failed or timed out` | Telegram API까지 네트워크 연결 실패 | 인터넷·방화벽·프록시를 확인한 뒤 재실행 |
| `No Telegram update arrived within 120 seconds` | 정확한 봇/그룹에서 새 update가 오지 않음 | 명령이 기다리는 동안 새 `/start` 또는 메시지 전송 |

실패하면 token과 chat ID는 저장되지 않는다. Supervisor가 임의로 `deleteWebhook`을 호출하지도 않는다.
기존 봇 연동을 깨뜨릴 수 있기 때문이다.

Telegram 자격 정보를 지우고 경로도 끄려면:

```bash
memory-supervisor notifications disable-telegram
```

## 연결 확인과 시험 결과 읽기

현재 설정을 다시 확인한다.

```bash
memory-supervisor notifications show
```

활성화된 OS·설정 완료된 원격 경로에 테스트 메시지를 보낸다.

```bash
memory-supervisor notifications test
```

시험 출력의 뜻은 다음과 같다.

| 결과 | 뜻 | 다음 조치 |
| --- | --- | --- |
| `delivered` | 해당 경로가 테스트 메시지를 수신함 | 완료 |
| `disabled` | 경로 선택에서 꺼짐 | 필요하면 `routes ...`로 추가 |
| `not configured` | 경로는 켜졌지만 자격 정보가 없음 | 위 Discord 또는 Telegram 연결 명령 실행 |
| `unavailable` | 현재 GUI/session에서 OS 알림 수단을 찾지 못함 | desktop session인지 확인하거나 원격 경로 사용 |
| `failed` | API·권한·네트워크 오류 | 토큰, ID, 권한, 네트워크를 확인하고 다시 설정·시험 |

`hook`과 `terminal`은 실제 AI CLI hook 또는 실제 보호 조치의 정확한 대상이 필요하므로 인위적인
시험 메시지를 보내지 않는다. `memory-status --connections`는 daemon, hook, 선택 경로의 연결 상태를
함께 보여주고, `memory-status`는 실제 사건별 `delivered|failed|skipped|unavailable` 결과를 기록한다.

설정은 내부적으로 다음 위치에 보존되지만 정상 사용 중 직접 열 필요는 없다.

| 환경 | 내부 비공개 저장 위치 |
| --- | --- |
| Linux, WSL, macOS | `~/.config/memory-supervisor/notifications.conf` |
| Windows | `$HOME\.config\memory-supervisor\notifications.conf` |

환경변수 `MEMORY_SUPERVISOR_NOTIFICATION_*`를 따로 지정했다면 그 값이 명령으로 저장한 값보다 우선한다.
`show`와 저장 명령이 override 이름을 경고하므로, 명령 결과가 반영되지 않는다면 해당 환경변수를
먼저 해제한다.

## 이 알림이 오는 순간들

- `HOLD|DRAIN`, 논리 제한, managed 정지 PID, lead probation 중 하나가 처음 활성화될 때
  `pressure-episode / active` 1회
- 위 조건이 모두 해소될 때 `recovered`, 확인된 재개 전에 정지 worker가 사라졌다면
  `ended-with-loss` 1회
- exact-PID pause/resume의 정확한 터미널 안전 고지
- fresh하던 federation peer의 stale 전환과 이후 회복
- 데몬 없이 훅이 fail-open일 때의 rate-limit된 보호-불능 경고
- sensor/runtime/notification 보호 저하와 probation 실패처럼 별도 조치가 필요한 실패

Raw utilization 전이와 아직 조치하지 않은 leak suspect는 알림 없이 사건 원장에만 남는다. 평범한
`SessionStart/End`, `SubagentStart/Stop`, 그대로 유지되는 `ACTIVE`, 변화 없는 `HOLD/DRAIN` tick은
사용자 알림을 다시 만들지 않는다. Lifecycle inventory만으로 사용자에게 보이는 논리 control epoch를
올리지 않는다. 에피소드 내부의 개별 spawn 거부·worker 시작 지연·논리 완충·PID별 pause/resume·
정상 probation 단계도 `importance=detail`이다. 거부된 훅의 `systemMessage`는 해당 lead에 즉시
전달되지만 같은 사실을 Discord·Telegram·OS에 별도 메시지로 복제하지 않는다.

구분선은 event 이름이 아니라 의도다. Supervisor가 관측 근거를 lead가 알아야 할 명시적 선제 인지
지시로 승격했다면 그것도 사용자에게 보이는 조치이므로 한 번 전달한다. 누구에게도 새 행동을 요구하지
않는 sensor 표본이나 그대로인 경계는 원장에만 남고 model context를 쓰지 않는다.

lead 사건 문장은 PID뿐 아니라 직접 프로세스 근거인지 머신 pressure 근거인지, 그 근거와 별개인
시스템 귀속이 `agent|external|mixed|unknown` 중 무엇으로 추정됐는지, 지금 자동 복구를 기다릴지
수동 명령을 쓸지까지 함께 설명한다. lead가 멈춰 hook을 실행할 수 없어도 exact terminal과 원격 경로는 별도로
동작한다. terminal/OS/원격은 즉시 시도되지만 model/lead 인지는 다음 hook 경계라는 시간차를 pause,
probation, 성공/실패, 수동·외부 resume의 모든 단계별 공통 문장에 명시한다. 반복은 event
type·status·source·incident/session epoch로 억제한다. 실제 정상화는 새로운 전이라 한 번 전달하지만,
경계가 그대로 유지되는 것은 새 알림이 아니다.

hook, `memory-status`, exact terminal, OS, Discord, Telegram은 이 구조화 사건을 같은 사용자
경계에서 렌더링한다. 이전 release가 저장한 사건도 그 경계에서 정규화하므로 업데이트 뒤
`Some(...)` 같은 낡은 debug 문구가 다시 재생되지 않는다.

원격 채널 히스토리는 팝업과 달리 자리를 비운 동안에도 확인할 수 있다. 다만 정확한 사건 원장은
로컬 `runtime.json`/`state.json`의 notification ledger이며 Discord·Telegram 전송은 실패해도
supervisor의 감지·보호를 막지 않는 best-effort 복제본이다.
