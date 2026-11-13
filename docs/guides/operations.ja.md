# 操作、通知、および回復

<p align="center">
  <a href="operations.md">English</a> · <a href="operations.ko.md">한국어</a> · <a href="operations.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

<a id="common-commands"></a>
## 共通コマンド

ターミナルのコマンドは、その OS 環境全体に作用します。Windows、各 WSL ディストリビューション、VM、コンテナはそれぞれ別の環境なので、必要な環境ごとに実行します。

| コマンド | 目的 |
| --- | --- |
| `calando status [--json]` | この環境、CLI の接続、同じマシン上の他のすべての環境 |
| `calando on` / `off` | この環境の保護をオンまたはオフにします。`off` はまず Calando が一時停止したものをすべて再開してから切ります |
| `calando telegram [CHAT_ID]` / `telegram off` | Telegram 通知の設定と削除 |
| `calando cap [<GB> \| off]` | 上級: この環境の全エージェントにメモリ上限をかけます |
| `calando update` | 今すぐ更新 |
| `calando update auto on` / `off` | 自動更新（既定はオン） |
| `calando uninstall` | 削除（設定と状態は残します） |
| `calando lock on` / `off` | 管理者のみ: オンの間は `off` とアンインストールを拒否します |
| `calando help` | ヘルプ |

0.3.x の間は `memory-supervisor` と `memory-status` も別名として使えます。

`off` の状態は再起動と更新のあとも保たれ、`calando on` 一つで元に戻ります。`off` の間もフックは接続されたままで、何もせずに通します。意図的な `off` なしにサービスが止まった場合、フックは 10 秒より古い判断を捨て、**保護が利用できない**と警告します。そのときは `calando status` で確認し、`calando update` で再接続します。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## セッション内のコマンド

セッションの中では、入力したそのセッションだけに作用するコマンドを使えます。Claude Code は `/calando`、Codex CLI と App は `$calando` です。

```text
/calando on | off | status | help
/calando cap <GB> | off
```

- UserPromptSubmit フックが答えるので、プロンプトはモデルに届かず、トークンもコンテキストも使いません。
- 対象はフックが受け取ったセッション ID のセッションです。PID やセッション一覧を指定することはありません。
- `/calando off` にしたセッションも測定は続きますが、待機・一時停止・通知の対象にはなりません。
- `/calando status` はそのセッションのエージェントが見ている状態を表示し、必要なら更新の案内も 1 行加えます。
- `/calando cap <GB>` は最初の返答で説明し、`/calando cap <GB> yes` で適用します。上限はそのセッションのプロセスツリーにかかります。90% に近づくと重い作業の開始を待たせ、上限を超えると増え続けているサブエージェントを一つ一時停止し、セッションが 90% を下回ると再開します。

これは `calando` という一つのスキルとしてインストールされます（`~/.claude/skills`、`~/.agents/skills`、`~/.codex/skills` にリンクではなくコピー）。以前の `memory-status` コマンドファイルは削除されます。詳しくは [Claude Code ガイド](usage-claude.ja.md) と [Codex ガイド](usage-codex.ja.md) を参照してください。

<a id="notifications"></a>
## 通知

Calando は実際に保護の操作をしたときだけ通知します。エージェントの文脈、フックの 1 行メッセージ、一時停止したプロセスの端末への通知は常にオンです。外部へのプッシュ通知は Telegram だけで、`calando telegram` で設定します。形式と送られる条件は[通知](notifications.ja.md)を参照してください。

<a id="control-and-recovery"></a>
## 一時停止と回復

手動で再開するコマンドはありません。

- メモリ逼迫のために一時停止したプロセスは、メモリが回復すると一つずつ自動的に再開します。
- 主エージェント自身の増加が続いて一時停止した場合は、見守り付きで一度だけ自動的に再開します。同じ増加が戻ると再び一時停止します。
- 主エージェントが一時停止したままの場合は、その端末を閉じて CLI 自身の再開機能（`claude --resume`、`codex resume`）で続けるか、`calando off` でその場で再開します。
- CLI で再開したあとの最初のフックは、前のプロセスが一時停止したまま終了したこと、その実行のサブエージェントの結果が欠けている可能性があることを伝えます。
- Codex App の共有 App Server は、見守り付きの再開に失敗しても一時停止したままにはしません。スレッドごとの制限に戻ります。

主エージェントの一時停止は意図的にまれにしています。新しい作業を段階的に遅らせ、サブエージェントとツールの制御でも危険が残り、同じ主エージェントの増加が続くことを確認できた場合だけの最終段階です。

<a id="memory-cap"></a>
## メモリ上限（上級）

```bash
calando cap 6
calando cap off
```

`calando cap` はこの環境のすべてのエージェントに合計の上限をかけます。説明を表示してから `[y/N]` で確認します。対話できない呼び出し元は `--yes` を付けます。値は、この環境で今使える量と照らして検証します。

- 上限に近づくと新しい重い作業を待たせ、上限を超えると増え続けているサブエージェントを一つ一時停止します。
- 一時停止しても、すでに使ったメモリは戻りません。使用量が一時的に上限を超えることもあります。
- バイト単位で正確な上限が必要なら、cgroup・コンテナ・VM の制限を使ってください。

通常の利用には勧めません。厳密な研究やベンチマークの実行向けです。

<a id="updates"></a>
## 更新

インストールされた監視プログラムは 1 日 1 回、最新の GitHub リリースを確認します。一時停止中のものがなく、メモリが落ち着いているときに、自分のサービスの外で更新を実行します。忙しいときは 1 時間ごとに再試行します。

- **Claude Code:** フックの変更も含めて自動で適用されます。新しいセッションから反映されます。
- **Codex:** Codex が接続されていて、フック定義が変わるリリース（0.3 → 0.4 のようなマイナーバージョン）は自動適用しません。Codex は変更されたフックを、CLI の `/hooks` または App の **設定 → Hooks** で信頼するまで黙って飛ばすためです。`calando status` と Telegram（1 回）が `calando update` の実行とフックの信頼を案内します。パッチリリースではフックは変わりません。
- **自動更新がオフ:** `calando status`、`/calando status`、Telegram（バージョンごとに 1 回）が新しいバージョンを知らせます。エージェントのフックは更新について触れません。
- 毎日の確認では、Claude Code が `settings.json` から Calando のフックを消していた場合に元に戻します。

0.2.x から 0.3.0 への更新ではフックのコマンドが変わるため、Codex では Calando のフックを一度信頼し直す必要があります。古い `memory-supervisor` のフォルダーは一度だけ移動され、Unix では古い名前がリンクとして残ります。

<a id="administrator-lock"></a>
## 管理者ロック

```bash
sudo "$(command -v calando)" lock on
```

Windows では管理者の PowerShell で `calando lock on` を実行します。オンの間は、ターミナルとセッションの `off`、およびアンインストールが拒否されます。同じユーザーで動くエージェントが Calando を切るのを防ぐ唯一の方法です。ロックは管理者だけが変更できるポリシーファイル（`/etc/calando/lock` または `%ProgramData%\Calando\lock`）で、サンドボックスではありません。限界は[セキュリティ](security.ja.md)を参照してください。

<a id="security"></a>
## 安全

Calando は OS のメモリとプロセス情報、Claude Code と Codex のフックが渡すセッション・エージェント・ツール・作業ディレクトリ・コマンドの先頭部分を読みます。使い道は、新しい作業を始めてよいかの判断と、制御対象の特定だけです。プロンプト、会話、モデルの応答、ファイルの内容は判断に使わず、保存もしません。自動の物理制御は、確認済みのローカル作業プロセス一つの一時停止と再開までで、終了はしません。ネットワークを使うのは、インストール、更新、Telegram 通知のときだけです。詳しくは[セキュリティとデータ/制御境界](security.ja.md)を参照してください。

<a id="verification"></a>
## 検証

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust のユニット・統合・インストーラーのテストは、ポリシー、プロセスの安全性、Claude Code と Codex の配線、federation、回復、リリースバンドルを対象にしています。詳しくは[テストカバレッジ](../testing/test-matrix.ja.md)を参照してください。
