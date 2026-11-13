# 通知

<p align="center">
  <a href="notifications.md">English</a> · <a href="notifications.ko.md">한국어</a> · <a href="notifications.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

Calando が実際に保護の操作をしたときだけ通知します。使用量の推移や、何も処置をしていない状態の変化は通知せず、`calando status` と内部の記録に残すだけです。状態はいつでも `calando status`（ターミナル）または `/calando status`（セッション内）で確認できます。

<a id="always-on"></a>
## 常に届く通知

次の三つは常にオンで、オフにはできません。

- **エージェントの文脈** — 主エージェントは次のフックで同じ内容を受け取ります。理由と回復方法も含まれます。
- **フックの 1 行メッセージ** — フックがターミナルに表示する短いメッセージです。
- **一時停止したプロセスの端末** — 一時停止したプロセスの端末に、平文の通知を 1 回だけ書き込みます。Calando は対象 PID の TTY または Windows コンソールを再確認してから書き込み、入力の挿入や端末モードの変更はしません。全画面 TUI では 1～2 行が崩れることがありますが、次の再描画で戻ります。

<a id="telegram"></a>
## Telegram

外部へのプッシュ通知は Telegram だけです。OS の通知（Windows のバルーン、macOS、`notify-send`）と Discord はありません。

Calando は Telegram のコマンドを受け付けるサーバーを立てません。Bot API の `sendMessage` で送るだけです。

1. `@BotFather` を開き、`/newbot` でボットを作ってトークンをコピーします。
2. 個人で受け取るなら新しいボットとの会話を開き、グループで受け取るならボットをグループに追加します。
3. 実行します。

```bash
calando telegram
```

4. トークンを非表示のプロンプトに貼り付けます。まだボットにメッセージがない場合は 120 秒待つので、その間にボット（またはグループ）にメッセージを送ります。チャットが一つだけ見つかると保存し、テストメッセージを送ります。

複数のチャットが見つかった場合は、何も保存せずに ID と名前を一覧表示します。一つ選んで、その ID で再実行してください（グループの ID は通常負の値です）。

```bash
calando telegram -1001234567890
```

| エラー | 意味 | 対処 |
| --- | --- | --- |
| `HTTP 401` | トークンが無効か取り消されています | `@BotFather` から現在のトークンをコピーして再実行します |
| `HTTP 409` | このボットにはすでに Webhook か別の `getUpdates` 利用者があります | Calando 専用のボットを使います。既存の Webhook は変更しません |
| `No message reached the bot within 120 seconds` | 待機中にメッセージが届きませんでした | 再実行し、待っている間にボットへメッセージを送ります |

失敗したときは何も保存しません。トークンをコマンドラインに書かないでください。

Telegram の設定を消すには次を実行します。

```bash
calando telegram off
```

設定はプライベートなファイル（`~/.config/calando/notifications.conf`、Unix ではモード `600`）に保存されます。普段開く必要はありません。

<a id="message-format"></a>
## メッセージの形式

メッセージは英語です。1 行目が状態、2 行目が環境で、そのあとに `ラベル: 値` が 1 行ずつ続きます。

```text
Calando — Memory protection on
Windows (LAPTOP-1)
Free: 1.5 of 15.7 GB
WSL VM: holds 9.6 GB, uses 3.2 GB
Action: New heavy work is on hold. Running work continues.
```

使うラベルは `Free`、`WSL VM`（Windows のみ）、`Restricted agents`、`Paused processes`、`Duration`、`Action` です。値がないラベルは表示しません。

<a id="when-notifications-are-sent"></a>
## 通知が送られるとき

- 保護が始まったとき（`Memory protection on`）と、安定確認のあと解除されたとき（`Memory protection off`）。一時停止した worker が再開前に終了した場合はその旨を伝えます。
- プロセスの一時停止と再開、見守り付きの自動再開とその結果
- 同じメモリを共有する別環境が報告しなくなったときと、再び報告し始めたとき
- 保護が低下し、対処が必要な障害
- 新しいバージョンがあるとき（自動更新がオフの場合、または Codex のフック信頼が必要な場合。Telegram にはバージョンごとに 1 回）

同じ内容は、イベントの種類・状態・発生元・エピソードで重複を抑えます。通常のセッション開始・終了、サブエージェントの開始・終了、変化のない `HOLD/DRAIN` では通知しません。

<a id="notice-scope"></a>
## 通知の範囲

通知はそれを必要とするセッションにだけ届きます。プロセスに関する通知は、そのプロセスを持つセッションだけに送られます。他のセッションについては、件数とエージェントのメモリ合計だけを伝え、名前・パス・PID は出しません。

Telegram は手元を離れているときのための写しです。届かなくても検出と保護には影響しません。正式な記録はローカルの `runtime.json` と `state.json` です。
