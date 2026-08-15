---
description: 이 컴퓨터의 모든 환경(호스트·WSL·VM·컨테이너) 메모리 상태와 감시 현황을 표시
---
이 컴퓨터에서 감시 중인 모든 OS 환경의 메모리 상태를 보고한다.

`memory-status --all` 을 실행하고(연합이 비어 있으면 `memory-status`로 폴백) 각 환경별로
raw utilization과 adaptive admission/action을 구분하고, 가용 메모리·감시 process·누수 의심·
`PAUSED_BY_SUPERVISOR`·probation·전달 결과를 요약한다. admission이 ORANGE/RED면 기존 작업은
유지하되 신규 fan-out만 보류한다고 설명한다. action block의 원인·자동/수동 복구·exact 명령을
먼저 보고하며, 사용자가 명시적으로 재개/종료해달라고 할 때만 `memory-supervisor resume|terminate|kill`을 사용한다.
