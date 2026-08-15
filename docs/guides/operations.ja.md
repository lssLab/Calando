# 操作、通知、および回復

<p align="center">
  <a href="operations.md">English</a> · <a href="operations.ko.md">한국어</a> · <a href="operations.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

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

Discord Webhook URL、Discord または Telegram ボット トークンをコマンド ラインに入力しないでください。 setup コマンドの実行後に表示される非表示のプロンプトにこれを入力します。 変更は、supervisor や AI プログラムを再起動しなくても、次の通知に適用されます。 ルートの選択と削除、Discord チャンネルと DM の設定、Telegram グループの設定、およびトラブルシューティングについては、[通知の設定](notifications.ja.md)を参照してください。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## Claude Code と Codex のスキルとコマンド

インストーラーは、自動決定を行う **hooks**、エージェントにステータスを理解して説明する方法を教える **スキル**、そのワークフローを呼び出す **短いコマンド**の 3 つの個別の部分を接続します。 Hooks ユーザーによる呼び出しなしで実行されます。 スキルはメモリ ポリシー自体を強制しません。

| 使用される場所 | 何を入力するか | 何をするのか |
| --- | --- | --- |
| Claude Code | 「メモリの状態を確認してください」と尋ね、`/memory-supervisor` を使用するか、`/memory-status` を使用してください | インストールされたスキルまたはショートカットは完全なステータスを読み取り、原因、自動回復、および必要なコマンドを説明します。 |
| Codex CLI | `$memory-supervisor check memory status` を使用します。 `/skills` を使用して発見を確認します。 `/prompts:memory-status` は互換性ショートカットです。 | Codex のプライマリ スキル パスを通じて同じステータス ワークフローを実行します。 Hook 信頼と有効化は、`/hooks` では分離されたままになります。 |
| Codex Desktop App | `$memory-supervisor check memory status` を使用するか、タスク内で自然に質問します | 各タスクで同じユーザーレベルの Codex スキルを使用します。 個別のアプリスキルはありません。 hooks は **設定 → Hooks** で管理します。 |
| オペレーティング システム端末 | `memory-status` または `memory-supervisor ...` を使用してください | これらはスキルではなく、実際のステータス、セットアップ、回復コマンドです。 `resume`、`terminate`、および `kill` は、明示的なユーザー要求の後にのみ実行されます。 |

スキルは「`memory-status --all`」を読み取り、原因と次のアクションを説明しますが、ユーザーの承認なしにプロセスを再開または終了することはありません。 Claude Code または Codex が Memory Supervisor の後にインストールされている場合は、`memory-supervisor update` を実行して、`memory-status --connections` との接続を確認します。 詳しい違いについては、[Claude Code ガイド](usage-claude.ja.md) と [Codex ガイド](usage-codex.ja.md) を参照してください。

<a id="security"></a>
## 安全

Memory Supervisor は、オペレーティング システムのメモリとプロセス情報に加えて、セッション、エージェント、ツール、作業ディレクトリ、および接続状態の情報と、Claude Code および Codex hooks によって提供されるコマンド プレフィックスを読み取ります。 この情報は、新しい作業を開始できるかどうかを決定し、正確な制御ターゲットを特定するためにのみ使用されます。

自動制御は、今後の Claude Code または Codex の作業を遅らせると停止し、最終保護段階で 1 つの検証済みローカル作業プロセスを一時停止して再開します。 プログラムを自動的に終了したり、無関係なプログラムを制御したりすることはありません。 通常の監視では外部要求は行われません。 GitHub のインストールと更新、およびオペレーターが有効にした Discord または Telegram の通知のみがネットワークを使用します。

**これは完全な検査と管理の境界です。 Memory Supervisor は外部の何も処理しません。** 制御決定の hook ペイロードに存在する可能性があるプロンプト、会話テキスト、模範応答、またはファイルの内容を使用せず、それらを保持しません。 プロジェクト ファイルやプロセス メモリを直接開いたり、ブラウザや IDE の内部データ、Claude や ChatGPT の資格情報、オペレーティング システムのカーネル、メモリ、スワップ、ファイアウォールの設定を検査したり変更したりすることはありません。 保存されたデータ、同一マシンの federation フィールド、および安全対策の完全なリストについては、[セキュリティとデータ/制御境界](security.ja.md)を参照してください。

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

Rust ユニット、統合、およびインストーラのテストでは、ポリシー、プロセスの安全性、Claude Code および Codex の配線、federation、リカバリ、およびリリース バンドルがカバーされます。 GitHub Actions は、Rosetta 上の Linux x86-64、Windows x86-64、Apple Silicon macOS、および macOS x86-64 上のビルドとプラットフォーム コントラクトをチェックします。 実際のほぼ枯渇の境界は、境界のある物理マシン検証と決定論的シミュレーションによってカバーされます。 [テストカバレッジ](../testing/test-matrix.ja.md)を参照してください。
