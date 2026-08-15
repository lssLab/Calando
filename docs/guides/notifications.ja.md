# 通知設定 — ファイルを開かずにターミナルで完了します

<p align="center">
  <a href="notifications.md">English</a> · <a href="notifications.ko.md">한국어</a> · <a href="notifications.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

Memory Supervisor がインストールされると、同じ `memory-supervisor notifications ...` コマンドが Linux、WSL、macOS、および Windows PowerShell で機能します。 設定ファイルを見つけたり、変数名や引用符を手動で入力したりする必要はありません。 ルートと資格情報の変更は、次の通知イベントで有効になります。 daemon および Claude Code または Codex CLI は再起動する必要はありません。

Discord Webhook URL、Discord、または Telegram ボット トークンをコマンド ラインに入力しないでください。 コマンドを開始した後、`(hidden)` プロンプトに貼り付けます。 値はエコーされず、シェル履歴にも記録されません。 Memory Supervisor は、Unix ではモード `600` を使用して、OS ごとのプライベート ファイルにアトミックに保存します。

<a id="check-the-current-state-first"></a>
## まずは現状を確認

この 1 行を任意の端末にコピーします。

```bash
memory-supervisor notifications show
```

出力には、有効なルート、選択した Discord メソッド、および Telegram チャットが表示されますが、元の Webhook やトークンは表示されません。

<a id="enable-or-disable-optional-routes"></a>
## オプションのルートを有効または無効にする

通知は常に**実際の保護措置のみ**を対象としています。 緑/黄/オレンジ/赤の遷移と未確認の漏洩観察は、`memory-status` とインシデント台帳に残ります。 以下のコマンドは、同じアクション通知が配信される場所を選択します。 詳細レベルは変更されません。

`hook` は、メインエージェントの認識および回復契約です。 `terminal` は、一時停止された lead が独自の hook を実行できない場合に、正確な回復コマンドを提供します。 どちらも必須であり、コマンド、構成ファイル、または環境変数によって無効にすることはできません。 `os,discord,telegram` のみがオプションです。

すべてのオプションのルートを有効にします。

```bash
memory-supervisor notifications routes all
```

ネイティブ OS 通知のみを必須の hook およびターミナル ルートに追加します。

```bash
memory-supervisor notifications routes os
```

OS 通知と Discord を使用します。

```bash
memory-supervisor notifications routes os,discord
```

hook と端末配信を維持しながら、オプションのルートをすべて無効にします。

```bash
memory-supervisor notifications routes none
```

有効なオプション名は `os,discord,telegram` です。 `hook` または `terminal` の指定は、これらのルートが必須であるため拒否されます。 Discord または Telegram のセットアップ コマンドは、そのルートを現在の選択に自動的に追加します。 対応する認証情報を持たずに `all` を選択すると、未設定のリモート ルートが単純にスキップされます。

終了通知は色変更ごとに印刷されるわけではありません。 lead 一時停止、再開、または保護観察などの実際のアクションの場合、supervisor はターゲット PID の正確な TTY または Windows コンソールを再検証し、1 つのプレーンテキスト通知を書き込みます。 入力を挿入したり、端末モードを変更したりしません。 全画面 TUI では 1 ～ 2 行を再描画し、次の再描画時にビューを復元できます。 正確な端末を検証して書き込むことができない場合、supervisor はlead を一時停止したままにしません。 AI CLI ツール呼び出しが拒否された場合も、その理由が lead に直接返されます。

OS ルートは、Linux では `notify-send`、WSL からの Windows ホスト通知パス、macOS では `osascript`、Windows では NotifyIcon を使用します。

<a id="discord-a-connect-a-webhook-recommended"></a>
## Discord A — Webhook を接続します (推奨)

これはボットを必要としないため、最も簡単なオプションです。

1. Discord デスクトップまたは Web でターゲットサーバーのテキストチャネルを開きます。
2. **[チャネルの編集] → [統合] → [Webhook] → [新しい Webhook] ** を選択します。
3. 名前とターゲット チャネルを確認し、[**Webhook URL をコピー**] を選択します。
4. 走る：

```bash
memory-supervisor notifications discord-webhook
```

5. `Discord webhook URL (hidden):` に URL を貼り付けて Enter キーを押します。 表示される文字は期待されません。
6. 接続をテストします。

```bash
memory-supervisor notifications test
```

コマンドが `discord: delivered` を出力し、チャネルがテスト メッセージを受信すると、セットアップは完了です。 setup コマンドは Discord を有効にし、以前の Discord 配信方法を置き換えます。

Webhook URL は、チャネルに書き込むことができるシークレットです。 漏洩した場合は、Discord でその Webhook を削除し、新しい Webhook を作成して、setup コマンドを再度実行します。

<a id="discord-b-send-to-a-channel-through-an-existing-bot"></a>
## Discord B — 既存のボットを通じてチャネルに送信します

すでにDiscord Botを運用している場合のみご利用ください。

1. Discord 開発者ポータルからトークンを取得し、サーバーに招待し、ターゲット チャネルで **メッセージの送信** を許可します。
2. **[ユーザー設定] → [詳細設定] → [開発者モード]** を有効にします。
3. ターゲット チャネルを右クリックし、**チャネル ID のコピー** を選択します。
4. 以下の数字をそのチャネル ID に置き換えます。

```bash
memory-supervisor notifications discord-channel 123456789012345678
```

5. トークンを `Discord bot token (hidden):` に貼り付け、Enter キーを押してテストします。

```bash
memory-supervisor notifications test
```

トークンに `Bot ` プレフィックスを追加しないでください。 Memory Supervisor はそれを API リクエストに追加します。

<a id="discord-c-send-a-direct-message-through-an-existing-bot"></a>
## Discord C — 既存のボットを通じてダイレクトメッセージを送信する

サーバーをボットと共有し、そのサーバーからの DM を許可する必要があります。

1. Discord 開発者モードを有効にし、プロフィールを右クリックして、**ユーザー ID をコピー** を選択します。
2. 以下の数字をユーザー ID に置き換えます。

```bash
memory-supervisor notifications discord-dm 123456789012345678
```

3. 非表示のプロンプトにボット トークンを貼り付けてテストします。

```bash
memory-supervisor notifications test
```

最初の送信時に、ボットは DM チャネルを作成し、そのチャネル ID のみをローカルにキャッシュします。

Discord 認証情報を削除し、そのルートを 1 行で無効にします。

```bash
memory-supervisor notifications disable-discord
```

<a id="telegram-connect-a-bot-and-discover-its-chat"></a>
## Telegram — ボットに接続し、そのチャットを発見します

Memory Supervisor は、Telegram コマンドを受け入れるパブリック Webhook サーバーを作成しません。 Bot API `sendMessage` メソッドを通じてのみ通知を送信します。

1. `@BotFather` を開き、`/newbot` でボットを作成し、そのトークンをコピーします。
2. 個人的なアラートについては、新しいボットの会話を開きます。 グループアラートの場合は、グループに追加します。
3. 走る：

```bash
memory-supervisor notifications telegram
```

4. トークンを`Telegram bot token (hidden):`に貼り付けてEnterを押します。 このコマンドは最初に保留中の更新をチェックします。 存在しない場合は、`waiting 120 seconds` が出力されます。 待機中に、新しい `/start` またはメッセージをそのボットに送信するか、ターゲット グループに新しいメッセージを送信します。 チャットが 1 つだけ表示されると、コマンドはその ID を保存し、Telegram を有効にします。
5. 接続をテストします。

```bash
memory-supervisor notifications test
```

コマンドが `telegram: delivered` を出力し、Telegram がテストを受信するとセットアップは完了です。

ボットの更新に複数のチャットが表示される場合、コマンドは何も保存せずにその ID とラベルを一覧表示します。 1 つを選択し、その ID を使用して再実行します。 グループ ID は通常、負の値になります。

```bash
memory-supervisor notifications telegram -1001234567890
```

同じトークンを再度貼り付けます。 120 秒以内にチャットが表示されない場合は、コマンドを再実行し、待機中のメッセージが表示された後、そのトークンとペアになっている正確なボットに新しいメッセージを送信します。 古い `/start` を再び読み取れるとは考えないでください。

検出エラーは個別に報告されます。

| エラー | 意味 | アクション |
| --- | --- | --- |
| `HTTP 401` | BotFather トークンが無効か取り消されています | `@BotFather` から現在のトークンをコピーして再実行します |
| `HTTP 409` | このボットにはすでに Webhook または別の `getUpdates` コンシューマーが含まれています | 専用の Memory Supervisor ボットを使用します。 既存の統合は自動的には削除されません |
| `connection failed or timed out` | Telegram API ネットワーク接続に失敗しました | インターネット、ファイアウォール、プロキシを確認してから再実行してください |
| `No Telegram update arrived within 120 seconds` | 正確なボットまたはグループから新しい更新は到着しませんでした | コマンドの待機中に新しい `/start` またはメッセージを送信します |

失敗すると、トークンとチャット ID は保存されません。 Memory Supervisor が `deleteWebhook` を自動的に呼び出すことはありません。これは、既存のボット統合が壊れる可能性があるためです。

Telegram 認証情報を削除し、次のコマンドでルートを無効にします。

```bash
memory-supervisor notifications disable-telegram
```

<a id="verify-connections-and-read-test-results"></a>
## 接続を確認し、テスト結果を読み取る

現在の構成を表示します。

```bash
memory-supervisor notifications show
```

有効な OS ルートと設定されたリモート ルートを介してテストを送信します。

```bash
memory-supervisor notifications test
```

| 結果 | 意味 | 次のアクション |
| --- | --- | --- |
| `delivered` | ルートはテストを受けました | 終わり |
| `disabled` | ルートが選択されていません | 必要に応じて `routes ...` を追加します |
| `not configured` | ルートは有効ですが、認証情報が不完全です | 上記の Discord または Telegram セットアップ コマンドを実行します |
| `unavailable` | この GUI/セッションでは OS 通知トランスポートを使用できません | デスクトップセッションまたはリモートルートを使用する |
| `failed` | API、権限、またはネットワークエラー | トークン、ID、権限、ネットワークを確認し、再度構成してテストします。 |

`hook` および `terminal` には、実際の AI CLI hook または実際の保護アクションの正確なターゲットが必要であるため、テスト コマンドはそれらのメッセージを合成しません。 `memory-status --connections` は、daemon、hook、および選択されたルート配線をレポートします。 `memory-status` は、各リアルイベントの `delivered|failed|skipped|unavailable` の結果を記録します。

通常の使用ではバッキング ファイルを開く必要はありません。

| 環境 | プライベートな内部位置 |
| --- | --- |
| Linux、WSL、macOS | `~/.config/memory-supervisor/notifications.conf` |
| 窓 | `$HOME\.config\memory-supervisor\notifications.conf` |

明示的に設定された `MEMORY_SUPERVISOR_NOTIFICATION_*` 環境変数は、保存された値をオーバーライドします。 `show` およびセットアップ コマンドは、これらのオーバーライド名について警告します。 保存した変更が有効にならない場合は、まず設定を解除してください。

<a id="when-notifications-are-sent"></a>
## 通知が送信されるとき

- いずれかの `HOLD|DRAIN`、ライブ論理制限、管理対象停止 PID、または lead 保護観察が最初にアクティブになったとき、1 つの `pressure-episode / active`
- これらの条件がすべてクリアされた後の最後の 1 つ `recovered`、または再開が確認される前に停止した worker が消えたときの `ended-with-loss`
- 正確な端末 PID の一時停止/再開の安全に関する通知
- 以前は新鮮だった federation ピアが失効し、その後の回復
- ライブ daemon なしで hooks がフェールオープンしているときに、レート制限された保護が利用できないという警告が表示されます。
- センサー/ランタイム/通知保護の機能低下や保護観察の失敗など、アクションが必要な障害

アクションを引き起こしていない未処理の使用状況の推移と漏洩の疑いは、インシデント台帳にのみ残ります。 通常の `SessionStart/End`、`SubagentStart/Stop`、安定した `ACTIVE` 状態、および変化のない `HOLD/DRAIN` ティックでは、別のユーザー通知は作成されません。 ライフサイクル インベントリは、それ自体でユーザーに表示される論理制御エポックを進めることはありません。 内部スポーン拒否、worker-開始遅延、論理クッション、PID ごとの一時停止/再開イベント、および通常の保護観察段階も `importance=detail` です。 拒否された hook は、その `systemMessage` を直接その lead に返します。 同じファクトを別の Discord、Telegram、または OS メッセージに複製することはありません。

境界線はイベント名ではなく意図によるものです。supervisor が証拠を lead に対する明示的なプロアクティブな認識指示に変える場合、その指示はユーザーに見えるアクションであり、一度配信されます。 センサーのサンプルや、誰にも何も要求しない変更されていない境界は台帳に残り、モデルのコンテキストを消費しません。

lead インシデント メッセージには、PID、直接プロセスまたは機械圧力の証拠、個別に推定される `agent|external|mixed|unknown` システムの属性、および自動回復を待つか手動コマンドを使用するかが含まれます。 一時停止した lead がその hook を実行できない場合でも、正確なターミナルおよびリモート ルートは引き続き機能します。 端末、OS、およびリモート配信は直ちに試行されます。 モデルと lead の認識は次の hook の境界に到達します。 すべての一時停止、猶予期間、成功/失敗、手動再開、および外部再開メッセージには、そのタイミングの違いが示されます。 繰り返しは、イベント タイプ、ステータス、ソース、およびインシデント/セッション エポックによって抑制されます。 本当の回復は新たな移行であり、一度だけ実現されます。 単に安定しているだけの境界はそうではありません。

Hooks、`memory-status`、正確な端末、OS、Discord、および Telegram はすべて、同じユーザー境界を通じてその構造化イベントをレンダリングします。 古いリリースによって書き込まれたランタイム レコードもそこで正規化されるため、`Some(...)` などの古いデバッグ テキストは更新後に再生されません。

リモート チャネルの履歴は、ポップアップとは異なり、ユーザーが離れている間も表示されたままになります。 権威あるインシデント記録は依然として `runtime.json` および `state.json` のローカル通知台帳です。 Discord と Telegram はベストエフォート型のコピーであり、障害によって検出や保護が妨げられることはありません。
