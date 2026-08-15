<p align="center">
  <img src="../assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="detailed-guide.md">English</a> · <a href="detailed-guide.ko.md">한국어</a> · <a href="detailed-guide.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

<p align="center">
  <em>Claude Code と Codex が長時間実行される大規模なワークロードを処理している間、メモリの使用を制御し、端末やアプリのフリーズや予期しないセッションの終了を防ぎます。</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 or newer"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 or newer"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 or newer and Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="Test"></a>
  <a href="guides/setup.ja.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="guides/performance.ja.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="guides/security.ja.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

<a id="what-problem-does-memory-supervisor-solve"></a>
## Memory Supervisor はどのような問題を解決しますか?

Claude Code、Codex CLI、または Codex Desktop App、subagents での長時間の作業中に、ビルド、テスト、ブラウザー ツールが積み重なる可能性があります。 使用可能なメモリが急速に減少すると、CLI 端末が応答を停止したり、終了したりする可能性があります。 デスクトップ アプリでは、App Server を共有する複数の会話が同時に影響を受ける可能性があります。 どちらの場合も、保留中の結果や進行中の作業が中断される可能性があります。

Memory Supervisor は、メモリ使用量が多いという理由だけで作業を制限するものではありません。 CLI とデスクトップ アプリの両方で、実際のリスクが近づいた場合にのみ、新しい作業を段階的に遅らせ、実行中の作業と結果の配信を可能な限り実行し続けます。 これにより、セッションが突然終了するのを防ぐことができます。

保護は、無制限の作業から完全な停止にジャンプすることはありません。 実際のリスクが近づくにつれて一度に 1 段階ずつ上昇し、回復後には逆に緩みます。

1. **自動セットアップ** — 通常どおり `claude` または `codex` を起動するか、Codex Desktop App で会話を開始します。 Memory Supervisor は、CLI セッションとアプリの会話を自動的に区別し、総容量、利用可能なメモリ、低下速度、今後の作業に必要なバッファを読み取り、保護レベルを設定します。 予算を設定したり、ステータスを確認し続ける必要はありません。
2. **制限なしで実行** — 利用可能なメモリとその変化率が安定している場合、メモリ使用量が多いだけでは制限は発生しません。
3. **フルパフォーマンスで観察** — メモリが十分に残っている間は、急速な低下自体は作業を制限しません。 supervisor は、下落が継続し、実際のリスクが近づいているかどうかを確認しながら、すべてをオープンに保ちます。
4. **新しい subagents、ワークフロー、タスクを最初に遅らせる** — メモリ headroom の継続的な損失によりリスクが近づくか、別の作業ブロックを入れる余地が少なすぎる場合、この段階では、すでに進行中の作業には影響せず、新しい subagents、ワークフロー、タスクの作成のみが遅延されます。 それ自体はビルドやテストの開始を遅らせたり、実行中のプログラムを一時停止したりすることはなく、現在の作業が完了するまでの時間とメモリを回復するまでの時間が与えられます。
5. **作業を徐々に減らす** — リスクが近づくと、新しい subagents、ワークフロー、タスクの作成が最初にブロックされます。 信頼できる証拠が損失の全部または一部を AI の作業によるものとしている場合、またはオプションでユーザーが設定した上限を超えた場合にのみ、既存のエージェントの今後の作業が `all work → no new subagents, workflows, or tasks → no new memory-heavy starts such as builds and tests → handoff, coordination, status, stop, recovery, and small reads only` に絞り込まれます。
 Subagents は一度に制限されません。 十分な時間があれば、subagent は次のツール呼び出しで 1 段下に移動します。 より短い時間で、supervisor はリザーブの前にラダーを完了するために必要な最小バッチを適用し、再測定します。 選択されていないエージェントと実行中の作業は変更されません。 Subagents は、次の順序で制限対象として選択されます: (1) リンクされたプロセスの異常な増大が確認された、(2) エージェント、ワークフロー、またはタスク作成または負荷の高い作業用の現在または最近のツール、(3) より厳しい現在の状態、(4) リンクされたプロセスが予備に達するまでの時間が短い、および (5) より新しい開始。
 leadを制限するのは、すべてのsubagentが最も狭い段階になっても危険が残る場合だけです。ただしleadが確認済みの主因で、subagentから制限していては間に合わない場合は、leadを先に一段階だけ狭めます。外部プログラムだけが原因なら既存のAI作業はそのまま続け、新しいsubagents・ワークフロー・タスクの作成と、OSのメモリ逼迫が深刻なときの重い作業開始だけを待たせます。
6. **最後の手段として 1 つのプロセスを一時停止する** — 危険が継続し、Claude Code または Codex に属する 1 つのプロセスが持続的な成長を確認した場合にのみ、supervisor はそのプロセスを終了せずに一時停止します。 端末はアクションを即座に表示し、lead は次のタスクの前に同じコンテキストを受信します。
7. **逆方向に回復** — メモリが安定した後、結果の配信から始めて作業が一度に 1 段階ずつ再開され、一時停止したプロセスが一度に 1 つずつ再開されます。

目標は、メモリの使用量を減らすことではありません。 これは、可能な限り有益なパフォーマンスを維持しながら、Claude Code と Codex CLI の端末セッションと Codex Desktop App の会話を保護するためです。

<a id="how-does-it-work"></a>
## どのように機能するのでしょうか?

Memory Supervisor は、あなたとどちらの CLI の間にも存在しません。 小さなバックグラウンド プログラムがメモリ使用量を監視している間、`claude` と `codex` は通常通り起動し続けます。

1. 各**オペレーティング システム環境**で 1 つのバックグラウンド モニターが実行されます。 Windows、macOS、または Linux ベース システムは 1 つの環境であり、各 WSL ディストリビューション、仮想マシン、または分離されたコンテナーは別の環境です。 Windows と WSL を一緒に使用する場合、1 つのモニターは Windows で実行され、もう 1 つのモニターは WSL で実行されます。 各モニターは、利用可能なメモリ、ネイティブ圧力信号、短期および長期ウィンドウの減少速度、予想される短期的な成長、および目に見える Claude Code および Codex プログラムを測定します。 1 つの環境内の複数の端末が同じモニターと新しい作業の決定を共有します。
2. ウィンドウタイトルではなく、プロセステーブルとhooksによって端末を区別します。 各トップレベルの Claude Code または Codex プロセスは、独立した lead です。 子孫は workers とツールとしてグループ化されます。 Hook セッション ID とエージェント ID は PID とプロセス開始 ID とともに記録されるため、別のセッションや再利用された PID が誤って制御されることはありません。
3. 固定の使用率の代わりに、現在の部屋と消費速度から停止距離を計算します。 境界に近づくとゆっくりと減速します。 急降下すると、同じ反応時間内に停止するために必要な距離が長くなります。
4. Claude Code または Codex hook は、新しい subagent、ワークフロー、タスク、またはメモリを大量に使用するコマンドが開始される前にチェックします。 `ALLOW` はそれを許可し、`OBSERVE` は監視中に許可し、`HOLD` は新しい subagents、ワークフロー、タスクの作成のみを遅延させ、`DRAIN` はそれらの作成リクエストをブロックします。
5. Windows、Linux、macOS の各ベース OS は、その上に階層化されたすべての WSL ディストリビューション、VM、またはプロセス分離コンテナーと同様に、独自の Supervisor を実行します。 物理メモリを共有する環境では、federation を使用して、最長 10 秒前の新しい作業の決定のみを交換し、最も厳密な決定を適用します。 各 Supervisor は依然として独自の hooks と PID 空間のみを制御するため、別の環境でプログラムを一時停止することはできません。
6. `DRAIN` であっても、Chrome または IDE のみによって引き起こされるプレッシャーによって、既存の AI の作業が一時停止されることはありません。 AI によるプレッシャーやオペレータが設定したメモリ上限の場合、今後の作業は `ACTIVE → NO_EXPANSION → LIGHT_WORK_ONLY → HANDOFF_ONLY` までに狭まります。 すべてのセッションを一度に絞り込むわけではありません。 Subagents は、リンクされたプロセスで検証された持続的な成長、現在または最近の拡張/ビルド/テスト作業、制限がすでに開始されているかどうか、リスク境界への早期到達、および新しい開始時間によってランク付けされます。 各ティックは、残りの停止距離に必要な最小セットのはしごステップのみを適用します。 選択されていないセッションと実行中の作業は変更されません。 lead は、subagents から始めるのでは遅すぎるという証拠がある場合にのみ、最初に移動します。
7. まだ危険が残っている場合、supervisor は PID (オペレーティング システムのプロセス番号) を再チェックして ID を開始し、最大 1 つのローカル プログラムを一時停止します。 lead は、その端末そのものが書き込み可能な場合にのみ一時停止できます。 通知が失敗すると、一時停止が即座にロールバックされます。

改善が続いた後、エージェント機能は逆の順序で再開され、一時停止したプログラムが 1 つずつ再開されます。 正確な計算と物理的な測定値は、[適応停止距離](testing/stopping-distance.ja.md)に含まれます。

`GREEN` から `RED` の色はステータスを簡単に表示します。 新しい作業は実際には、`ALLOW`、`OBSERVE`、`HOLD`、または `DRAIN` によって制御されます。 色だけではプログラムが一時停止することはありません。

<a id="what-changes-in-codex-desktop-app"></a>
### Codex Desktop App では何が変わりますか?

CLI では、各セッションに独自の lead プロセスと child プロセス ツリーがあるため、通常、supervisor はどのセッションが増加しているかを知ることができます。 Codex Desktop App は、すべての会話を 1 つのセッションにマージしません。 代わりに、App Server は、各会話を独自の `session_id` を持つ**論理スレッド**として保持します。 ここで、論理スレッドはオペレーティング システムのスレッドではありません。 これは、App Server と supervisor によって使用される会話 ID です。 supervisor は、各論理スレッドを独立した lead として扱い、`agent_id` を使用して subagents をその lead に接続します。 したがって、エージェント リスト、次のhook の作業範囲、アクション、および回復通知を会話ごとに管理できます。

論理アプリ スレッドは、物理的に CLI セッションと同等ではありません。 CLI セッションには、独自の lead PID、子孫プロセス ツリー、およびターミナルがあります。 アプリ論理スレッドには、独立した lead PID、完全な child プロセス ツリー、ターミナル、または専用メモリの合計はありません。 すべての論理スレッドは 1 つの App Server PID とその内部メモリを共有します。 したがって、オペレーティング システムは合計 1 つの App Server を表示し、会話ごとのメモリを測定できず、1 つの会話だけを一時停止することもできません。 つまり、会話は論理的には分離されていますが、プロセスとメモリは物理的に共有されています。

supervisor は、共有メモリ App Server を 1 回カウントします。 個別に起動されたツールプロセスは、hook、タスクの前にキャプチャされたプロセスリスト、親-child チェーン、および PID 開始 ID がすべて一致する場合にのみ、特定の論理スレッドに属します。 App Server 内で使用されるメモリ、または複数の会話からのツールが重複している間に起動される child には、証明可能な論理スレッド所有者が存在しない可能性があります。 **blind control**です。 これは、supervisor が何も見ていないという意味ではありません。システム headroom と低下速度、アプリと child プロセスの成長、アクティブな会話、および現在のツールの種類をまだ認識しています。 一部の成長の所有者だけが不明です。

その制限内で、アプリ コントローラーは次の順序で CLI ポリシーを保存します。

1. **パフォーマンスを第一に保ちます。** 使用頻度が高くても安定した使用、またはシステムの headroom の損失を説明できないアプリの増加は、アプリに起因する会話制限を引き起こしません。 コントローラーは、危険にさらされるまでの残り時間とブレーキに必要な時間を比較し、最新の安全点まで待機します。 成長の大きな部分に証明できる会話の所有者がいない場合、コントローラーは候補を 1 つずつ試して結果を測定するのに必要な時間のみを追加します。 不確実性だけでは早期のスロットリングは発生しません。
2. **最初に新しい高メモリのクッションが開始されます。** アプリの持続的な成長がリスクの原因となっており、計算された停止距離に入ったが、会話の所有者が不明な場合は、ビルドやテストなどの将来の高メモリ アプリの開始のみがアプリ全体で待機します。 実行中の作業、結果、メッセージ、ステータス、およびリカバリは引き続き利用可能です。
3. **リスクを説明する最小のセットのみを絞り込みます。** 成長に正確な所有者がいる場合、コントローラーはそれを説明するために必要な最小限の会話のみを選択し、通常はそれぞれの将来の作業範囲を 1 段階下に移動します。 所有権が不明瞭な場合は、現在の重いツール、subagent の役割、および最近のアクティビティによって候補がランク付けされます。 最初の blind candidate を狭めてから再測定し、成長が鈍化したときに停止し、危険が続く場合にのみ別の値に移動します。 残り時間が少なすぎる場合でも、リスク境界の前に必要な最小限のセットのみをバッチ処理します。 推定された証拠により候補者をランク付けできます。 会話固有のプロセスを一時停止する権限は決して付与されません。
4. **利用可能な最小の物理ブレーキを使用します。** 小さな論理アクションがすべて失敗した後、順序は、正確に所有されまだ成長中の 1 つの child プロセス、次にアプリに属しているが特定の会話に属していないことが知られているまだ成長中の 1 つの child、そして最後に共有の App Server です。 サーバーはアクティブな会話が行われるたびにのみ一時停止でき、subagent は最終段階を認識しました。この段階では、結果の配信、ステータス、回復などの簡単な作業のみが残っています。 サーバーの増大自体が主な原因であり続ける必要があり、これより小さな選択肢を残すことはできません。 独立したリカバリ ガードは、一定の遅延の後にそのサーバーを再開します。 一時停止してもメモリはすぐには解放されません。 これにより、さらなる成長が停止されるため、他の作業が完了し、システムが回復できるようになります。

回復は逆に一度に 1 段階ずつ再開されます。 影響を受ける各会話は、次の hook で理由と現在のスコープを受け取ります。 blind child または共有サーバーのブレーキは、影響を受ける可能性のあるすべてのアクティブな会話に報告されます。 アプリ hook のルートが切断された場合、supervisor は、新しい会話ごとの制限や物理的なブレーキが適用されたかのように見せかけません。 次に利用可能な保護としてカウントすることを停止し、劣化状態を報告し、新しい作業の開始とアプリ プロセスの監視に関するシステム全体の決定を継続します。

CLI とアプリを同時に実行しても、どちらかが監視対象から外れるわけではありません。 1 つの PID 空間 (1 つの OS、WSL ディストリビューション、VM など) 内で、単一の supervisor が CLI プロセス ツリーと Codex App サーバーの両方を監視します。 それらを 1 つのセッションにマージしません。

- 各 CLI セッションは、**独自の端末、lead PID、および子孫プロセス ツリーを持つ独立した lead のままです**。
- App Server は 1 つの端末として扱われません。 これは、**複数の会話で共有される 1 つの物理プロセス ホストです**。 その下の各 `session_id` は個別の論理 lead ですが、サーバー PID とその内部メモリは 1 回だけカウントされます。

両方のサーフェスは同じローカル メモリ評価と新しい作業 admission の決定を共有しますが、制御ターゲットは別々のままです。 アプリ属性の blind cushion は、アプリ hook 呼び出しにのみ適用され、通常の CLI リクエストもブロックしません。 どちらの表面のメモリも機械のリスクに寄与しますが、一方の表面の増加によって自動的にもう一方の表面がブレーキの対象になるわけではありません。 各ターゲットには、依然として独自の成長と帰属の証拠が必要です。

Federation は、同じ物理メモリをめぐって競合する **supervisor インスタンス** に参加します。 アプリの会話や端末はマージされません。 Windows、各 WSL ディストリビューション、ダイナミック メモリ VM、およびプロセス分離コンテナは、10 秒以内の新しい作業の決定のみを使用し、最も厳密な新しい決定を適用します。 別のインスタンスの会話リストや PID をローカル制御にマージすることはなく、各 supervisor は独自の PID 空間内の CLI およびアプリ プロセスのみを制御します。 たとえば、WSL のアプリによって引き起こされる `DRAIN` により、Windows CLI は新しい subagent または大きなタスクを federation まで遅延させることができますが、WSL supervisor はその Windows CLI を一時停止できず、Windows supervisor は WSL を一時停止できません App Server。

<a id="how-are-terminals-and-agents-controlled"></a>
## 端末とエージェントはどのように制御されますか?

<a id="1-claude-code-and-codex-cli"></a>
### 1. Claude Code および Codex CLI

CLI パスでは、Claude Code と Codex は端末に直接接続されたままになります。 バックグラウンド モニターは、同じローカル プロセス空間内の関連プログラムを監視します。 コントロールは 2 つのレイヤーに分割されています。

- 開始前チェック: まだ開始されていない作業を許可または遅延します。
- プログラムの一時停止: 危険が続く場合は、オペレーティング システムを通じて検証済みの PID を 1 つだけ一時停止します。

```text
A. User work path

                ┌──────────────────────┐
                │ Exact user terminal  │
                │ Commands and results │
                └──────────┬───────────┘
                           │ direct attachment
                           ▼
                ┌──────────────────────┐
                │ Claude / Codex lead  │
                │ Main agent           │
                └──────────┬───────────┘
                           │ before supported actions
                           ▼
                ┌──────────────────────┐
                │ Before-tool hook     │
                │ Reads latest decision│
                │ Returns reason       │
                └──────────┬───────────┘
                           │ decision
           ┌───────────────┴───────────────┐
           ▼                               ▼
┌──────────────────────┐        ┌──────────────────────┐
│ ALLOW / OBSERVE      │        │ HOLD / DRAIN         │
│ Requested work runs  │        │ Targeted work waits  │
│ No start is delayed  │        │ In-flight work stays │
└──────────────────────┘        └──────────────────────┘

B. Background protection path

┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ Measure/brake/recover│                │ Latest hook decision │
└──────────────────────┘                └──────────┬───────────┘                └──────────────────────┘
                                                   │ when protection acts
                               ┌───────────────────┴───────────────────┐
                               ▼                                       ▼
                    ┌──────────────────────┐                ┌──────────────────────┐
                    │ Notice + lead context│                │ One verified PID     │
                    │ Exact terminal: now  │                │ Final stage only     │
                    │ Lead: next hook once │                │ Pause + auto-resume  │
                    └──────────────────────┘                └──────────────────────┘

Windows, Linux, and macOS hosts with independent environments layered on top

                         ┌────────────────────────────────────┐
                         │ Shared federation decision         │
                         │ Shares new-work decisions only     │
                         │ Valid for 10 seconds               │
                         │ Strictest fresh decision wins      │
                         └─────────────────┬──────────────────┘
                                           ↕
                         only boundaries competing for shared RAM connect

       ┌────────────────────────────┐  ┌────────────────────────────┐  ┌────────────────────────────┐
       │ WSL distro / VM / container│  │ VM / container             │  │ VM / container             │
       │ each: local Supervisor     │  │ local Supervisor           │  │ local Supervisor           │
       └──────────────▲─────────────┘  └──────────────▲─────────────┘  └──────────────▲─────────────┘
                      │ runs on                       │ runs on                       │ runs on
       ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐
       │ Windows base OS            │  │ Linux base OS              │  │ macOS base OS              │
       │ host Supervisor            │  │ host Supervisor            │  │ host Supervisor            │
       └────────────────────────────┘  └────────────────────────────┘  └────────────────────────────┘

                  Each Supervisor controls only its own state, hooks, and PID space
                              No RAM pooling · no cross-environment PID control
```

CLI lead の認識は固定された順序に従います。

1. supervisor は、まず原因、ターゲット、アクティブな制限、および回復パスをインシデント台帳に記録します。
2. 作業を遅らせる hook は、同じ呼び出しで理由を返します。
3. 物理的なプロセス アクションは、正確な端末に即座に表示されます。 別個の端末を持たない Worker インシデントは、lead の次のリアル hook で一度配信されます。
4. 選択すると、OS、Discord、および Telegram は 1 つの保護開始通知と 1 つの完全回復通知を受け取ります。

たとえば、Windows 上のクロード lead が編集をパッケージ化している間に、WSL の Codex が subagent と大規模なテストを開始しようとしていて、共有物理メモリ headroom が急速に減少し始めるとします。 WSL supervisor が `DRAIN` を記録すると、federation はその決定を Windows に伝えます。 両方の hooks は新しい subagent のみを遅延させてテストします。 編集、結果、メッセージは開いたままになります。 外部 VM が原因の場合、AI PID は一時停止されません。 持続的な回復の後、新しい仕事が再開されます。 同じ AI worker からの検証済みの成長のみが、lead で段階的な論理制限に達し、最後に正確なローカル PID 一時停止に至ることができます。

完全な状態フロー、複数端末のレイアウト、障害境界については、[アーキテクチャとランタイム トポロジ](guides/architecture.ja.md)を参照してください。

<a id="2-codex-desktop-app"></a>
### 2. Codex Desktop App

Codex Desktop App では、各会話は `session_id` で識別される 1 つの論理スレッドです。 異なるセッション ID は独立したリードです。 複数のウィンドウで同じ会話を開いた場合でも、1 つの論理スレッドと 1 つの lead としてカウントされます。 これにより、supervisor は、会話ごとに hook レベルの作業範囲と通知を管理できるようになります。 各スレッドに個別の PID やメモリ プールは作成されません。スレッドはすべて 1 つの App Server を共有します。 この図は、正確な所有権を blind candidates から分離しながら、論理的な会話とエージェント台帳が物理的なプロセスとメモリの観察とどのように組み合わされるかを示しています。

```text
                                        ┌──────────────────────┐
                                        │ Codex Desktop App    │
                                        │ Logical App threads  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Shared App Server    │
                                        │ One PID + shared RAM │
                                        └──────────┬───────────┘
                                                   │ hooks + process view
                           ┌───────────────────────┴───────────────────────┐
                           ▼                                               ▼
                ┌──────────────────────┐                        ┌──────────────────────┐
                │ Conversation ledger  │                        │ Process + memory map │
                │ session ID = lead    │                        │ exact / blind pool   │
                │ agent ID = subagent  │                        │ Shared RAM once      │
                └──────────┬───────────┘                        └──────────┬───────────┘
                           └───────────────────────┬───────────────────────┘
                                                   ▼
┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ App-specific planner │                │ Hook-confirmed stage │
│ Sustained App growth │                │ Cause + braking room │                │ Recovery + notice    │
└──────────────────────┘                └──────────┬───────────┘                └──────────┬───────────┘
                                                   │                                       │
                                                   ▼                                       ▼
                                        ┌──────────────────────┐                ┌──────────────────────┐
                                        │ App staged cushion   │                │ Affected lead context│
                                        │ New heavy starts wait│                │ Scope + recovery     │
                                        │ Chosen sessions only │                └──────────────────────┘
                                        └──────────┬───────────┘
                                                   │ if danger persists
                                                   ▼
                                        ┌──────────────────────┐
                                        │ One subprocess PID   │
                                        │ Exact owner first    │
                                        │ Blind: one-by-one    │
                                        └──────────┬───────────┘
                                                   │ absolute last stage
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Final server brake   │
                                        │ All App work pauses  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Independent recovery │
                                        │ Timed auto-resume    │
                                        └──────────────────────┘
```

会話 A がビルドを開始し、会話 B が回答を準備しているとします。 ビルド プロセスが A の hook に正確にリンクされている場合、supervisor は最初に A の新しい作業を絞り込み、B をそのまま残します。 危険が続く場合は、共有サーバーではなく、そのビルド プロセスが最初に考えられる物理的なブレーキとなります。

プロセスが A または B に属することが証明できない場合、supervisor は恣意的に B を責めることはありません。まず、アプリ全体で高メモリの新しい開始のみを保持し、次に、負荷の高い作業を実行している、または観察された増加に最も一致する 1 つの会話の今後の作業を絞り込みます。 アクションの効果を測定するための短いウィンドウの後、記憶力の低下が遅くなったかどうかをチェックし、有用な変化がなかった場合にのみ別の候補を追加します。 この一連の調査時間は、開始からのアプリ停止距離に含まれるため、不必要に早期にパフォーマンスを低下させることなく、リスク境界の前に候補チェックを終了できます。

メカニズムは CLI とは異なりますが、ポリシーの結果は同じです。実行中の結果の前に新しい作業を減らし、リードよりも subagents を優先し、リスクを説明する最小セットのみを制御し、物理的なブレーキとリカバリを一度に 1 つのターゲットに適用します。 正確に所有されている child が最初に来ます。 blind child は、関連するすべての会話の後でのみ一時停止でき、subagent は実際に最終論理段階を受け取りました。 共有された App Server 、つまりすべての会話を一時停止することは、小さな選択肢がすべてなくなった後の最後の手段です。 Federation ベース OS、WSL、VM、およびコンテナー間の境界は、各 supervisor が独自の PID 空間のみを制御するというルールと同様に、CLI 設計と同じままです。 完全な安全条件については、[Codex Desktop App](guides/codex-app.ja.md) を参照してください。

<a id="installation"></a>
## インストール

ご使用の環境の **ターミナル** を開き、以下に一致する 1 行のコマンドを貼り付けます。 Git、Python、Rust、または別個のインストーラーを準備する必要はありません。 通常のインストールはユーザー アカウントに限定されており、`sudo` や管理者シェルは必要ありません。

<a id="1-install-memory-supervisor"></a>
### 1. Memory Supervisor をインストールします

<a id="linux-wsl2-or-macos-terminal"></a>
#### Linux、WSL2、または macOS ターミナル

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

コマンドが完了すると、バックグラウンド サービスが実行され、検出された Claude Code および Codex hooks が自動的に接続されます。 実行中の AI プログラムを閉じたり、進行中の作業を中断したりすることはありません。

<a id="windows-powershell-terminal"></a>
#### Windows PowerShell ターミナル

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

コマンドが完了すると、バックグラウンド サービスが実行され、検出された Claude Code および Codex hooks が自動的に接続されます。 実行中の AI プログラムを閉じたり、進行中の作業を中断したりすることはありません。

> [！重要]
> Windows 実行可能ファイルは現在、[SignPath Foundation](https://signpath.org/) によってレビュー中です。
> Windows 11 では、**Windows セキュリティ → アプリとブラウザのコントロール → Smart App Control** を維持する必要があります
> レビューが完了するまで、ネイティブ ビルドをインストールして使用している間、`Off`。

| ウィンドウの状態 | ネイティブ ビルドはインストールできますか? |
| --- | --- |
| 64 ビット Windows 10 | はい。 Smart App Control は使用できないため、SAC 設定は必要ありません。 SmartScreen が表示された場合は、ダウンロードがこのリポジトリのリリースからのものであることを確認してください。 |
| Windows 11 24H2 ビルド 26100.8117 以降、25H2 ビルド 26200.8117 以降、または再有効化コントロールが表示される新しい Windows 11 ビルド | はい、Smart App Controlを`Off`に設定した後です。 未署名ビルドの使用を停止した後は、同じ設定ページから再度有効にすることができます。 |
| 古い W​​indows 11 ビルド、または段階的に展開されるコントロールをまだ受け取っていない現在のビルド | オフにした後は可能ですが、再度オンにするには、Windows のリセットまたは再インストールが必要になる場合があります。 無効にする前にこれを確認してください。 |
| Smart App Control はすでに `Off` です | 変更せずにインストールしてください。 別の SmartScreen ダウンロード レピュテーション プロンプトが表示された場合は、発行者とファイル ソースを確認してください。 |
| S モードの Windows 11、または実行可能ファイルをブロックする組織のアプリ制御ポリシー | ネイティブ Windows パスではサポートされていません。 このインストーラーは、S モードまたは管理者ポリシーをバイパスできません。 |

`Win + R` から `winver` を実行して、Windows のバージョンとビルドを確認します。 Smart App Control をオフにする前に、設定ページでオンに戻す方法が提供されているかどうかも確認してください。 ロールアウトはデバイスごとです。 ソース基準については、Microsoft の [Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions) および [初期ロールアウト ノート](https://support.microsoft.com/en-au/help/5079391) を参照してください。

Codex App ウィンドウが表示されるオペレーティング システムではなく、**App Server とそのツールが実際に実行される環境**にインストールします。

- Windows 上の Codex App が WSL エンジンを使用する場合、その WSL 環境にインストールされている Supervisor は、その WSL App Server、タスクごとの論理スレッド、および WSL 側のツールを保護します。 このパスではネイティブ Windows Supervisor が実行されないため、Smart App Control をオフにする必要はありません。 Windows アプリ UI プロセスおよび個別の Windows ネイティブ Claude Code または Codex CLI は、WSL インストールの測定および制御境界の外側に残ります。
- App Server または CLI が Windows、macOS、または Linux 上で直接実行される場合は、そのオペレーティング システムにインストールします。 ネイティブ Windows インストールでは、上記の Smart App Control 要件が使用されます。
- App Server または CLI が別の WSL ディストリビューション、仮想マシン、または分離されたコンテナーで実行される場合は、そのような各環境にインストールします。 Windows と WSL は、federation パスを自動的に見つけます。 macOS または Linux ホスト、ダイナミック メモリ VM、およびコンテナーは、同じマシン上の共有フォルダーに接続します。 接続されると、同じ物理メモリを競合する環境は、新しい作業の決定を共有します。 固定メモリ VM と他のコンピューターまたはクラウド サーバーは、独立して自身を保護します。 境界については、[プラットフォームと複数環境の動作](guides/platforms.ja.md)を参照してください。

<a id="2-set-up-claude-code"></a>
### 2. Claude Code を設定します

インストーラーは、Memory Supervisor ユーザー hook に自動的に接続します。 Claude Code では、設定、承認、または有効にするものは何もありません。

**インストール中に Claude Code がすでに実行されていた場合:** 作業を続けます。 Claude Code はユーザー設定の変更を自動的に再ロードするため、通常は再起動は必要ありません。

**確認するには:** ステップ 5 の `memory-status --connections` 出力の `Claude Code CONNECTED` を確認します。 hook の詳細を確認したい場合のみ、読み取り専用の `/hooks` 画面を開いて `User Settings` を調べてください。 このオプションのビューにエントリが表示されない例外的な場合にのみ、現在の作業の後に Claude Code を再起動します。

<a id="3-set-up-codex-cli"></a>
### 3. Codex CLI を設定します

1. 使用するCodex CLIで`/hooks`を開きます。
2. 7 つの Memory Supervisor hooks がすべて **信頼されており、オン**であることを確認します。
3. レビュー用にマークされたエントリを信頼し、無効になっているエントリをオンにします。
4. `/hooks` を閉じて作業を続けてください。

**インストール中に Codex CLI がすでに実行されていた場合:** 確認したばかりの CLI で続行します。 再起動する必要はありません。 インストール前にすでに開いていた他の Codex CLI については、現在の作業を終了し、その CLI のみを 1 回再起動します。

<a id="4-set-up-codex-desktop-app"></a>
### 4. Codex Desktop App を設定します

1. Codex App を開き、**設定 → Hooks** に移動します。 Memory Supervisor エントリがまだ存在しない場合は、最大 60 秒待ってから設定を再度開きます。
2. Memory Supervisor hooks の 7 つすべてを信頼してオンにします。 **すべて信頼** では、以前に無効にされたスイッチはオンにならないため、両方の状態を確認してください。
3. 既存のタスクに戻り、送信する予定だった次のリクエストを送信します。 続行する既存のタスクがない場合にのみ、新しいタスクを作成します。

**インストール中にCodex App がすでに実行されていた場合:** アプリとその既存のタスクを開いたままにして、手順 1 ～ 3 に従います。 アプリを再起動したり、新しいタスクを作成したりする必要はありません。

<a id="5-verify-the-installation"></a>
### 5. インストールを確認する

```bash
memory-status --connections
```

使用するプログラムの行を確認してください。

- `Core daemon CONNECTED`: バックグラウンド サービスは正常です。
- `Claude Code CONNECTED`: サポートされているバージョンとユーザー hook が接続されています。
- `Codex CONNECTED`: 7 つの CLI hooks がすべてインストールされ、有効化され、信頼されています。
- `Codex App ACTIVE`: 7 つのアプリ hooks がすべて準備ができており、既存または新しいタスクから実際の呼び出しが到着しました。
- `NOT DETECTED` は、使用していないプログラム、またはインストールされていないプログラムでは正常です。

回線が正常でない場合は、回線が報告する内容にのみ対処してください。

- `disabled` または `not trusted`: Codex CLI で `/hooks` を使用するか、Codex App で **設定 → Hooks** を使用して、名前付きエントリを信頼して有効にします。
- `missing`、`stale`、`DEGRADED`、または `NOT RUNNING`: `memory-supervisor update` を実行し、このチェックを繰り返します。
- `NEEDS ATTENTION`: 報告されたプログラム バージョンまたは hook の要件を満たしてから、`memory-supervisor update` を実行します。
- `Core daemon OFF`: `memory-supervisor on` を実行します。
- 7 つのアプリ hooks がすべて正しいように見えても、リクエスト後にアプリがまだ `ACTIVE` にならない場合は、アプリを一度再起動し、既存のタスクで次のリクエストを送信して、再度確認してください。
- 新規インストールで `memory-status` コマンドが見つからない場合は、ターミナルのみを再度開き、再度実行します。 Claude Code、Codex CLI、および Codex App は、この PATH 更新のために再起動する必要はありません。

Codex hook trust は管理者アクセスではありません。 Codex が実行する正確なローカル コマンドを承認することになります。 組織のポリシーまたは Windows セキュリティ ポリシーによってインストールがブロックされている場合にのみ、管理者ポリシーを確認してください。 基礎となる信頼ルールについては、[Claude Code hooks ガイド](https://code.claude.com/docs/en/hooks) および [Codex hooks ガイド](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks) を参照してください。

これらのコマンドは、最新のパブリック リリースをインストールします。 Rust ビルド ツールは必要ありません。 そのリリースに含まれる検証済みの実行可能ファイルが自動的に使用されます。

<a id="6-uninstall"></a>
### 6. アンインストール

Calando を削除するには、それがインストールされている各環境でこれを 1 回実行します。

```bash
memory-supervisor uninstall
```

状態とユーザー設定を維持しながら、バックグラウンド サービス、実行可能ファイル、Calando が所有する hook およびスキル接続を削除します。

<a id="supported-environments"></a>
## サポートされている環境

保護は、サポートされているすべての環境で同じように動作します。 supervisor は利用可能なメモリとその減少率を監視し、新しい作業を段階的に絞り込み、危険が残っている場合にのみ検証済みの Claude Code または Codex プロセスを一時停止し、安定した回復後に再開します。 メモリの読み取りとプロセスの一時停止に使用されるオペレーティング システムのメカニズムのみが異なります。

| 環境 | テストカバレッジ |
| --- | --- |
| 64 ビット Intel/AMD 上の Linux および WSL2 | 物理的な WSL2 および自動 Linux チェック |
| macOS アップルシリコン | 自動化された Apple Silicon チェック |
| 64 ビット Intel/AMD 上の Windows 10 または 11 | 物理的な Windows 11 E2E、自動化された Windows Server 2022 チェック、および Windows 10 ランタイム/API 互換性レビュー |
| IntelベースのmacOS | Rosetta による自動互換性 |

接続される製品は、Claude Code 2.1.217 以降、Codex CLI 0.145.0 以降、`hooks stable true`、Codex Desktop App です。 同じ保護ポリシーが CLI とアプリに適用されます。

<a id="measured-resident-memory"></a>
### 常駐メモリの測定値

これらのオペレーティング システムの合計は、ウォームアップ後に 0.2 秒間隔で 20 個のサンプルで測定されました。

| テスト環境 | 最小 | 平均 | 最大 | OSメトリクス |
| --- | ---: | ---: | ---: | --- |
| WSL2 Linux、物理サービス | 4.88 MiB | 4.88 MiB | 4.88 MiB | RSS |
| 64 ビット Intel/AMD 上の Ubuntu、自動テスト | 3.50 MiB | 3.52 MiB | 3.54 MiB | RSS |
| 64 ビット Intel/AMD 上の Windows、自動テスト | 4.15 MiB | 4.20 MiB | 4.25 MiB | ワーキングセット |
| macOS Apple Silicon、自動テスト | 3.38 MiB | 4.35 MiB | 5.13 MiB | RSS |

容量計画の場合は、最小サンプルではなく、**インストールされているモニターごとに 10 MiB** を使用します。 詳細な条件と生データについては、[パフォーマンス測定](guides/performance.ja.md)を参照してください。

1 台の物理コンピューターに複数の実行環境 (Windows、WSL ディストリビューション、仮想マシン、分離コンテナー) がある場合、Claude Code または Codex を実行するすべての環境にインストールします。 1つの環境内の複数の端末が1つのモニターを共有します。 各環境でのインストールと federation パスのセットアップ後、実行されているカーネルの数に関係なく、コンピュータ全体が最新の新しい作業の決定を自動的に共有します。 各モニターは依然として独自の環境のみを測定および制御するため、別の環境の PID で動作することはありません。 インストーラーは、Windows と WSL の同じローカル共有フォルダーに接続します。 VM またはコンテナは、ホスト共有ローカル フォルダーを federation パスとして使用します。 ネットワーク フォルダーは、別の物理コンピューターやクラウド サーバーへの接続には使用されません。 セットアップの詳細については、[プラットフォームとマルチ環境の動作](guides/platforms.ja.md)を参照してください。

<a id="notifications"></a>
## 通知

Memory Supervisor は、メモリ読み取りごとに通知しません。 実際の保護アクションが開始されるか完全に回復するたび、または接続または保護状態にユーザーの注意が必要な場合に、通知が 1 つ送信されます。

| ルート | 出現する場所 | いつ、何を報告するか |
| --- | --- | --- |
| ターミナル | 影響を受ける Claude Code または Codex CLI プロセスを実行している正確な端末 | プロセスが一時停止または再開されたとき、または回復を確認するためにlead 一時停止が一度解除されたときに、理由、PID、および回復コマンドが即座に表示されます。 このルートは常にオンです。 |
| OS | Linux、WSL、macOS、または Windows デスクトップ通知 | 保護が最初に機能したとき、または完全に回復したとき、または federation 接続または保護に注意が必要なときに表示されます。 このオプションのルートは、デスクトップ通知が利用可能な場合に機能します。 |
| 電報 | ユーザーが選択したボットのプライベート チャットまたはグループ | 重要なアクションの開始と回復、および接続または保護の問題を報告します。 メモリの状態、理由、ターゲットが存在する場合のPID、次のアクションが含まれており、ユーザーが外出中に確認できる履歴を残します。 |
| 不和 | 接続されたチャネル、Webhook、またはダイレクト メッセージ | 同じ重要な行動や回復、注目アイテムを掲載しています。 このオプションのルートは、チーム チャネルまたは個人通知を目的としています。 |

端末、OS、テレグラム、および Discord の配信は、イベントが記録された直後に試行されます。 変更されていない状態は繰り返し送信されません。 lead は、次の hook 境界で同じ状況と回復状態を受け取ります。 終端ルートは常に接続されたままになります。 次のコマンドを使用して、オプションのルートを構成およびテストします。

```bash
memory-supervisor notifications show
memory-supervisor notifications routes os
memory-supervisor notifications discord-webhook
memory-supervisor notifications telegram
memory-supervisor notifications test
```

Discord Webhook URL、Discord または Telegram ボット トークンをコマンド ラインに入力しないでください。 setup コマンドの実行後に表示される非表示のプロンプトにこれを入力します。 変更は、supervisor や AI プログラムを再起動しなくても、次の通知に適用されます。 ルートの選択と削除、Discord チャンネルと DM の設定、Telegram グループの設定、およびトラブルシューティングについては、[通知の設定](guides/notifications.ja.md)を参照してください。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## Claude Code と Codex のスキルとコマンド

インストーラーは、自動決定を行う **hooks**、エージェントにステータスを理解して説明する方法を教える **スキル**、そのワークフローを呼び出す **短いコマンド**の 3 つの個別の部分を接続します。 Hooks ユーザーによる呼び出しなしで実行されます。 スキルはメモリ ポリシー自体を強制しません。

| 使用される場所 | 何を入力するか | 何をするのか |
| --- | --- | --- |
| Claude Code | 「メモリの状態を確認してください」と尋ね、`/memory-supervisor` を使用するか、`/memory-status` を使用してください | インストールされたスキルまたはショートカットは完全なステータスを読み取り、原因、自動回復、および必要なコマンドを説明します。 |
| Codex CLI | `$memory-supervisor check memory status` を使用します。 `/skills` を使用して発見を確認します。 `/prompts:memory-status` は互換性ショートカットです。 | Codex のプライマリ スキル パスを通じて同じステータス ワークフローを実行します。 Hook 信頼と有効化は、`/hooks` では分離されたままになります。 |
| Codex Desktop App | `$memory-supervisor check memory status` を使用するか、タスク内で自然に質問します | 各タスクで同じユーザーレベルの Codex スキルを使用します。 個別のアプリスキルはありません。 hooks は **設定 → Hooks** で管理します。 |
| オペレーティング システム端末 | `memory-status` または `memory-supervisor ...` を使用してください | これらはスキルではなく、実際のステータス、セットアップ、回復コマンドです。 `resume`、`terminate`、および `kill` は、明示的なユーザー要求の後にのみ実行されます。 |

スキルは「`memory-status --all`」を読み取り、原因と次のアクションを説明しますが、ユーザーの承認なしにプロセスを再開または終了することはありません。 Claude Code または Codex が Memory Supervisor の後にインストールされている場合は、`memory-supervisor update` を実行して、`memory-status --connections` との接続を確認します。 詳しい違いについては、[Claude Code ガイド](guides/usage-claude.ja.md) と [Codex ガイド](guides/usage-codex.ja.md) を参照してください。

<a id="security"></a>
## 安全

Memory Supervisor は、オペレーティング システムのメモリとプロセス情報に加えて、セッション、エージェント、ツール、作業ディレクトリ、および接続状態の情報と、Claude Code および Codex hooks によって提供されるコマンド プレフィックスを読み取ります。 この情報は、新しい作業を開始できるかどうかを決定し、正確な制御ターゲットを特定するためにのみ使用されます。

自動制御は、今後の Claude Code または Codex の作業を遅らせると停止し、最終保護段階で 1 つの検証済みローカル作業プロセスを一時停止して再開します。 プログラムを自動的に終了したり、無関係なプログラムを制御したりすることはありません。 通常の監視では外部要求は行われません。 GitHub のインストールと更新、およびオペレーターが有効にした Discord または Telegram の通知のみがネットワークを使用します。

**これは完全な検査と管理の境界です。 Memory Supervisor は外部の何も処理しません。** 制御決定の hook ペイロードに存在する可能性があるプロンプト、会話テキスト、模範応答、またはファイルの内容を使用せず、それらを保持しません。 プロジェクト ファイルやプロセス メモリを直接開いたり、ブラウザや IDE の内部データ、Claude や ChatGPT の資格情報、オペレーティング システムのカーネル、メモリ、スワップ、ファイアウォールの設定を検査したり変更したりすることはありません。 保存されたデータ、同一マシンの federation フィールド、および安全対策の完全なリストについては、[セキュリティとデータ/制御境界](guides/security.ja.md)を参照してください。

<a id="control-and-recovery"></a>
## 制御と回復

メモリが再び安定すると、一時停止していた作業が 1 項目ずつ自動的に再開されます。 lead 自体のメモリが増加し続けたため一時停止された場合は、supervisor が結果を確認できるように自動的に再開されます。 同じ成長が戻った場合、lead は再び一時停止し、ユーザーの決定を待ちます。 手動で再開するには、まず現在のステータスを確認し、そこに表示されている PID を使用します。

```bash
memory-status
memory-supervisor resume [pid]
```

lead の一時停止は意図的に非常にまれです。 これは**最終保護段階**であり、新たな作業が段階的に遅延し、subagentとツール制御によって危険が除去されず、同じleadとその正確な終端からの持続的な成長が確認された場合にのみ使用されます。 ほとんどのインシデントは、作業範囲の縮小、worker の一時停止、または自動回復によって早期に終了します。

Claude Code または Codex が誤って終了した場合、CLI は会話を復元し、インストールされた `SessionStart` hook は、保持されているメモリ インシデントと現在の決定を lead に 1 回送信します。

```bash
claude --resume
codex resume
```

意図的に保護をオフまたはオンにする場合は、これら 2 つのコマンドのみを使用してください。 `off` は、インストールされている Claude Code および Codex hooks をサイレント パススルー モードで接続したまま、バックグラウンド サービスを停止して無効にします。 選択は再起動後も有効であり、`memory-supervisor update`; 1 つの `on` コマンドで保護が復元されます。

```bash
memory-supervisor off
memory-supervisor on
```

`off` は、supervisor が管理する一時停止された PID または進行中のプロセス アクションを保留することを拒否します。 最初にリストされた PID を解決します。 意図的な `off` なしにサービスが停止した場合でも、hooks は 10 秒後に古い決定を破棄し、**保護が利用できない** と警告します。

```bash
memory-status --connections
memory-supervisor update
```

固定制限が必要な場合は、ローカル環境内のすべての Claude Code および Codex プログラムに対して 1 つの合計メモリ上限をオプションで設定できます。

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

コマンドは、制御対象ごとにグループ化されています。

- `memory-status` コマンドは読み取り専用です: ローカル原因、federation、サービス、hook、および通知接続。
- `on` と `off` は、現在のインストール全体を制御します。 1 つのコマンドで、接続されているすべての Claude Code および Codex セッションがカバーされます。 その環境内で別の OS、WSL ディストリビューション、または VM を切り替える必要があります。
- `resume` は、supervisor によって一時停止されたプロセスを続行します。 `terminate` および `kill` は、原因を調査した後にオペレーターが選択したプロセス終了です。
- `budget` は、コンピューター全体や Chrome ではなく、現在の環境の Claude Code と Codex にのみオプションのキャップを適用します。
- `update` はサービスを再適用し、CLI 接続を検出します。 `notifications` は、オプションの OS、Discord、および Telegram ルートを制御します。 lead hooks および正確な最終通知は引き続き必須です。

<a id="common-commands"></a>
## 共通コマンド

| 指示 | 目的 |
| --- | --- |
| `memory-status` | 地域の健康状態、原因、次のアクション |
| `memory-status --all` | 同じコンピューター上の Windows、WSL、仮想マシン、およびコンテナーの状態 |
| `memory-status --connections` | バックグラウンド サービス、AI CLI、および通知接続 |
| `memory-supervisor on` / `off` | この環境での保護を永続的に有効または無効にします。 接続済み hooks オフ時に通過 |
| `memory-supervisor update` | 検出された CLI を更新して再接続する |
| `memory-supervisor budget` | この環境の適応能力とオプションの上限を表示します |
| `memory-supervisor budget set <GiB>` / `budget off` | 集約ローカル Claude Code および Codex の上限を設定または削除します |
| `memory-supervisor resume [pid]` | supervisor で一時停止されたプロセスを再開します。 1 つだけが一時停止されている場合にのみ PID を省略します |
| `memory-supervisor terminate <pid>` | 1 つの検証済み管理プロセスを正常に終了する |
| `memory-supervisor kill <pid>` | 最後の手段として、検証済みの 1 つのプロセスを強制終了する |
| `memory-supervisor notifications show` | シークレットを非表示にした通知設定を表示する |
| `memory-supervisor 通知ルート <all\|なし\|ルート>` | オプションの OS、Discord、および Telegram ルートを選択します |
| `memory-supervisor notifications test` | 有効になっているオプションの通知ルートをテストする |
| `memory-supervisor uninstall` | 状態を維持しながらサービスと AI CLI 接続を削除します |

<a id="verification"></a>
## 検証

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust ユニット、統合、およびインストーラのテストでは、ポリシー、プロセスの安全性、Claude Code および Codex の配線、federation、リカバリ、およびリリース バンドルがカバーされます。 GitHub Actions は、Rosetta 上の Linux x86-64、Windows x86-64、Apple Silicon macOS、および macOS x86-64 上のビルドとプラットフォーム コントラクトをチェックします。 実際のほぼ枯渇の境界は、境界のある物理マシン検証と決定論的シミュレーションによってカバーされます。 [テストカバレッジ](testing/test-matrix.ja.md)を参照してください。

<a id="documentation"></a>
## ドキュメント

| ガイド | 用途: |
| --- | --- |
| [すべてのドキュメント](README.ja.md) | インストール、使用法、セキュリティ境界、および公開テストに関するドキュメントを見つける |
| [アーキテクチャ](guides/architecture.ja.md) | バックグラウンド監視、開始前チェック、状態ファイル、プログラム制御 |
| [Codex Desktop App](guides/codex-app.ja.md) | 論理的な会話、blind control、および共有されたApp Server 内のリカバリ |
| [適応停止距離](testing/stopping-distance.ja.md) | 計算、境界の測定、段階的なブレーキ、および回復 |
| [プラットフォームと複数環境での動作](guides/platforms.ja.md) | オペレーティング システムと仮想環境が新しい作業の決定をどのように共有するか |
| [セキュリティとデータ/制御境界](guides/security.ja.md) | 情報の読み取り、保存、共有、および自動および手動の制御制限 |
| [テストカバレッジ](testing/test-matrix.ja.md) | 公開テストの対象となる製品の動作とプラットフォーム |
| [Claude Code](guides/usage-claude.ja.md) / [Codex](guides/usage-codex.ja.md) | CLI とデスクトップ アプリの統合とセッションの動作 |
| [通知](guides/notifications.ja.md) | 端末、OS、Discord、Telegram配信 |
| [パフォーマンス](guides/performance.ja.md) | バックグラウンドメモリ使用量と開始前チェック時間 |
| [セキュリティ ポリシー](../.github/SECURITY.ja.md) | プライベート脆弱性報告ルート |
| [貢献](../.github/CONTRIBUTING.ja.md) | 変更原則と提出前チェック |

<a id="license"></a>
## ライセンス

[MIT](../LICENSE)
