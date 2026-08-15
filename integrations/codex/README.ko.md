# Codex 어댑터

<p align="center">
  <a href="README.md">English</a> · <strong>한국어</strong> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a>
</p>

최상위 수준의 한 줄 설치 프로그램을 선호한 다음 업그레이드를 위해 `memory-supervisor update`를 사용하거나 나중에 추가되는 Codex 설치를 사용하세요. 두 경로 모두 어댑터를 원자적으로 병합하고 관련 없는 hooks를 유지합니다. `hooks.json.template`는 수동 검사 및 사용자 지정 배포용입니다. `__MEMORY_SUPERVISOR_ROOT__`를 절대 슬래시 경로로 바꾸고 `__CODEX_HOOKS__`를 해당 Codex 홈의 절대 `hooks.json` 경로로 바꿉니다. 소스 경로를 사용하면 다른 Codex 집에서 이 사용자를 프로젝트 hook로 재발견하는 경우 게이트가 이 사용자 hook를 무시할 수 있습니다.

Codex hook JSON은 SessionStart, UserPromptSubmit, SubagentStart, SubagentStop, Stop, PreToolUse 및 PostToolUse에 대해 Claude와 호환됩니다. SubagentStop은 모델 컨텍스트를 추가하지 않고 기록됩니다. Codex에는 PostToolBatch가 없으므로 PostToolUse는 동일한 전환 알림 경로를 호출합니다.

어댑터에는 Codex 0.145.0 이상이 필요하며 `hooks stable true`이 필요합니다. 네이티브 `PreToolUse`는 `spawn_agent`를 `Agent`로 매핑하므로 일반 `codex`, `codex exec` 및 IDE 호스팅 세션은 동일한 사전 할당 게이트를 사용합니다. 지원되지 않는 릴리스는 수정되지 않은 상태로 유지됩니다. `../../docs/guides/usage-codex.md`을 참조하세요.
