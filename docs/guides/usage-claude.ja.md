# Claude Codeの使用法

<p align="center">
  <a href="usage-claude.md">English</a> · <a href="usage-claude.ko.md">한국어</a> · <a href="usage-claude.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

<a id="supported-contract"></a>
## サポートされている契約

Memory Supervisor `0.2.1` は、Claude Code **2.1.217 以降** をサポートします。 これは、段階的論理制御コントラクトの固定された最新のサポートされるベースラインです。 古いリリースには、縮小されたマッチャー セットや互換性ポリシーが適用されません。

```bash
claude --version
claude update
```

インストーラは、バージョンと hook の配線を 2 つの別個の事実としてチェックします。 アクティブな `PATH` と、ネイティブ、NVM、fnm、asdf、Volta、Windows npm パスなどの既知のユーザー インストール場所を検索し、検証できる最新のサポートされている Claude Code を使用します。 したがって、非ログインプロセスの `PATH` で以前に実行された古い実行可能ファイルは、現在のユーザーのインストールを隠すことができません。

サポートされている実行可能ファイルを検証できない場合、インストールではバージョンの問題が報告されますが、既存の Memory Supervisor hook は保持されます。 バージョン プローブの失敗は、有効な hook を削除する必要があるという証拠にはなりません。 `memory-status --connections` は、hook の健全性を個別に表示し続け、バージョンと hook の両方の準備が整うまで、保護されたプロバイダーを呼び出しません。 `claude update` が元のインストール方法で利用できない場合は、同じパッケージマネージャーまたはインストーラーを使用して Claude Code をアップグレードしてから、`memory-supervisor update` を実行します。

Claude Code には、サポートされている CLI の最も広範な統合が文書化されています。 `PreToolUse` はすべてのツールパスを観察し、実際の入力を分類し、マシン admission での新しい拡張のみをゲートし、名前付き論理エージェントが存在する場合はその将来の作業クッションを適用します。 すでに起動したツールは元に戻されません。

プラットフォームのインストーラーを実行します。 無関係な hooks を置き換えることなく、これらのイベントをアトミックに `~/.claude/settings.json` にマージします。

- `SessionStart`: 起動時にリソースコントラクトを挿入します。 再開/クリア/コンパクト 目に見えない停止事件がない場合にのみ沈黙してください。
- `UserPromptSubmit`: 非 GREEN アダプティブ admission および回復後であっても未確認のインシデントを注入します。
- `PreToolUse`: すべてのツールを分類します。 マシンの圧力下で新しい拡張を遅らせて差し戻したり、マシンの故障が重大な間は新しい分類された高メモリの開始を保留したり、ターゲットの論理状態によって除外された将来の作業クラスのみを拒否したりできます。
- `SubagentStart`: ライフサイクル観察と 12 秒の RED のみのフォールバック。 ORANGE は、すでに承認されている worker を遅らせることはありません。
- `SubagentStop`: 結果を部分的にした可能性があるsupervisor の拒否を保持しながら、論理ライフサイクル レコードを閉じます。
- `PostToolUse` および `PostToolBatch`: 進行状況を記録します。 lead 境界は、目に見えないインシデントのコンテキストを提供します。 subagent 境界は、lead のインシデント カーソルを消費できません。 どちらも固定の RED スリープを追加しません。
- `Stop` および `SessionEnd`: 通常の終了をブロックすることなく、lead/セッションのライフサイクル状態を閉じます。

<a id="hook-activation-workspace-trust-and-reload"></a>
## Hook アクティベーション、ワークスペースの信頼、およびリロード

Claude Code は、Codex のhook ごとのハッシュ承認を使用しません。 インストーラーは、Memory Supervisor hook を `~/.claude/settings.json` のユーザー設定に書き込みます。 そのユーザー hook には、個別の承認/有効化手順はありません。 それでも、対話型 Claude Code は、ユーザーが現在のフォルダーまたはその親のいずれかに対するワークスペースの信頼を受け入れるまで、このユーザー hook を含むすべての設定ファイル hook を保持します。 クロードの `/hooks` 画面は読み取り専用ブラウザなので、その信頼を与えることができません。

ワークスペースの信頼は、Memory Supervisor 固有の Hook のレビューではなく、フォルダー レベルの 1 つの決定です。 信頼できる作業フォルダーに対してのみ受け入れてください。 その決定後、現在の Claude Code は設定ファイルを監視するため、実行中のセッションは通常、後のユーザー hook の変更を取得します。 しばらく待ってもエントリが表示されない場合にのみ再起動するか、特にセッションごとに 1 回のイベント `SessionStart` を実行することが目的の場合は、新しいセッションを開きます。

プレーンな非インタラクティブな `claude -p` の実行では、同じユーザー設定と hooks がロードされるため、追加のセットアップ手順なしで Memory Supervisor がこれをカバーします。 Claude Code は、このモードではワークスペースの信頼検証をスキップします。 `--bare` が追加された場合、Claude Code は意図的にすべての hooks をスキップし、Memory Supervisor はその呼び出しを監視できません。

インストール後、または `memory-supervisor update` 後、`memory-status --connections` を実行します。 その Claude `CONNECTED` の結果は、サポートされている実行可能ファイル、スキル、および現在のユーザー - hook の配線を検証します。 必要に応じて、Claude の読み取り専用 `/hooks` ビューを使用して、`User Settings` の下のエントリを確認します。 どちらのチェックでも、現在のフォルダーに対するワークスペースの信頼性は証明されません。 管理専用の hooks や `disableAllHooks` などの組織ポリシーによっても、ユーザー hook のアクセスが妨げられる可能性があり、管理者のアクションが必要です。

インストールされたコマンド hooks はオプションの `statusMessage` を意図的に省略しているため、通常の hook の実行では TUI に Memory Supervisor の進行状況行が保持されません。 ユーザーに表示されるテキストは、実際の保護措置または目に見えないインシデントに対してのみ表示されます。 すでに実行中のセッションに古いルーチン hook の進行状況ラインがまだ表示されている場合は、`memory-supervisor update` を実行して新しい Claude Code セッションを開き、AI CLI が現在の hook 定義を再ロードします。

Admission は、`MEMORY_SUPERVISOR_FEDERATION_DIR` からの最悪の新しい適応アクションを使用するため、ホスト、WSL、または VM の圧力により、プロセスの一時停止がローカルのままである間、新しいクロード ファンアウトがどこにでも保持されます。 Raw 利用色だけではファンアウトはブロックされません。

Claudeのleadが`PAUSED_BY_SUPERVISOR`の場合、一時停止中はプロセス内hookを実行できません。そのためsupervisorは、再確認した対象ターミナルに原因と正確な復旧方針を書き込み、OS・Discord・Telegramへの通知も別々にキューへ入れます。自動の試験再開、成功、失敗、手動再開、外部からの直接再開には、段階ごとに同じ案内が適用されます。hookは次のプロンプトまたはツール境界で、その内容をユーザーとモデルへ一度だけ渡します。この通知はOSレベルの再開そのものより遅れる場合があります。`memory-supervisor resume`は同じPIDとメモリ上のセッションを継続します。Claudeを終了して`--resume`で起動した場合は、`SessionStart source=resume`がリソース事象を伝え、会話自体はClaudeの履歴復元機能が別途復元します。

`StructuredOutput` およびその他の結果/メッセージ/ステータス ツールは、`HANDOFF_ONLY` で引き続き許可されます。 Supervisor 拒否は、ツール、理由、時間、論理エポックとともに記録され、次の完了/プロンプト境界で lead に要約されます。 通常の成功したツール結果文字列として到着するプロバイダー固有のクォータの枯渇には、構造化された失敗信号がないため、依然として subagent によって報告される必要があります。

意図的な決定は、終了コード 0 の JSON です。安定したラッパーは、Rust ゲート、状態、またはポリシーのエラーをサイレント終了 0 に変換するため、内部エラーが誤って Claude Code の終了コード 2 プロンプト ブロックになることはありません。

確認する：

```bash
bash tests/run.sh
memory-status --connections
memory-status
printf '{}' | runtime/hooks/gate.sh SessionStart
```

契約に関する参照:

- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code 権限とワークスペースの信頼](https://code.claude.com/docs/en/permissions)
- [Claude Code のインストールとアップデート](https://code.claude.com/docs/en/installation)
- [Claude Code インストールのトラブルシューティング](https://code.claude.com/docs/en/troubleshoot-install)

Windows では次を使用します。

```powershell
'{}' | powershell -File .\runtime\hooks\gate.ps1 SessionStart
```

個人スキルは`~/.claude/skills/memory-supervisor`にリンクされています。 新しく作成されたトップレベルのスキル ディレクトリでは、検出する前に新しい Claude Code セッションが必要になる場合があります。

プロセスを一時停止した場合は、まず通知に表示された対応手順に従います。メモリ逼迫で一時停止されたworkersと、持続的な実増加を確認して一時停止されたleadの最初の試験再開は、自動復旧の対象です。許可された手動操作では、生の`kill -CONT`ではなく`memory-supervisor resume <pid>`（一時停止中のPIDが一つだけなら`memory-supervisor resume`）を使用してください。これによりdaemonがプロセスの開始IDを確認し、状態を解除し、`RESUMED`事象を保存して、再開後のクールダウンを適用できます。

<a id="if-a-hook-blocks-every-prompt"></a>
## hook がすべてのプロンプトをブロックする場合

ブロックされたセッションからアクティブな hook を編集し続けないでください。 別の端末から:

1. `~/.claude/settings.json` と現在の supervisor チェックアウトをバックアップします。
2. `printf '{}' | runtime/hooks/gate.sh UserPromptSubmit` を実行します。 安全な結果は、有効な JSON または出力なしであり、常に終了コード 0 が付きます。
3. `bash tests/run.sh` を実行します。
4. `memory-supervisor update` を実行して、所有されている supervisor hook エントリのみをアトミックに置き換え、サービスをリロードします。
5. hook 定義はセッション開始時にスナップショットされる可能性があるため、新しい Claude Code セッションを開きます。

この手順の後もすべてのプロンプトがブロックされたままの場合は、`memory-status --connections` とゲート終了コードを使用して、hook 配線障害を supervisor 状態から分離します。
