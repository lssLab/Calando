<p align="center">
  <img src="assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <a href="README.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

<p align="center">
  <em>Claude Code と Codex が長時間実行される大規模なワークロードを処理しながらメモリ使用量を制御し、端末やアプリのフリーズや予期しないセッションの終了を防ぎます。</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 or newer"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 or newer"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 or newer and Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="Test"></a>
  <a href="docs/guides/setup.ja.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="docs/guides/performance.ja.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="docs/guides/security.ja.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#installation"><strong>インストール</strong></a> ·
  <a href="#how-it-works-in-30-seconds">仕組み</a> ·
  <a href="#common-commands">コマンド</a> ·
  <a href="#documentation">ドキュメント</a> ·
  <a href="docs/detailed-guide.ja.md">詳細ガイド</a>
</p>

<a id="why-calando"></a>
## 何を解決するのか

Claude CodeやCodex CLI、Codex Desktop Appで大きな作業を長時間続けると、サブエージェント、ビルド、テスト、ブラウザツールが同時に重なることがあります。空きメモリが急速に減ると、CLIではターミナルが応答しなくなったりセッションが終了したりし、Desktop AppではApp Serverを共有する複数の会話がまとめて影響を受けることがあります。どちらの場合も、まだ受け取っていない結果や作業の流れが途切れかねません。

Calandoは、メモリ使用量が多いという理由だけで作業を制限しません。CLIでもDesktop Appでも、実際の危険が近づいたときに新しい作業から段階的に遅らせ、進行中の作業と結果の受け渡しはできる限り維持して、セッションの突然の終了を防ぎます。

保護の強さは一気に上がりません。危険に近づくにつれて次の段階を一つずつ適用し、状態が回復すると逆の順序で解除します。

1. **自動判定** — 普段どおり`claude`や`codex`を起動するか、Codex Desktop Appで会話を始めるだけです。CalandoがCLIセッションとアプリの会話を自動的に区別し、メモリ容量、現在の空き、減少速度、次の作業に必要な余裕を基に保護基準を自動設定します。予算を設定したり、状態を常に確認したりする必要はありません。
2. **制限せずに実行** — メモリ使用量が多くても、残りの余裕と減少速度が安定していればエージェントやツールを制限しません。
3. **性能を保ったまま観察** — 十分な余裕があれば、急速に減っているという理由だけですぐには制限しません。すべての作業を許可したまま、減少が続くか、実際の危険が近づいているかを確認します。
4. **新しいサブエージェント・ワークフロー・タスクの作成から待機** — 空きメモリの減少が続いて危険が近づくか、新しい作業を始める余裕が足りなくなったときは、進行中の作業には触れず、新しいサブエージェント・ワークフロー・タスクの作成だけを一時的に待たせます。この段階ではビルドやテストの開始を止めず、実行中のプログラムも一時停止しないため、現在の作業を終えてメモリが戻るまでの時間を確保できます。
5. **作業範囲を段階的に縮小** — 危険がさらに近づくと、まず新しいサブエージェント・ワークフロー・タスクの作成をすべて止めます。空きメモリの減少がAIの作業によるものだと信頼できる根拠がある場合、またはユーザーが設定した上限を超えた場合にだけ、既存エージェントが次に行える作業を`すべての作業 → 新しいサブエージェント・ワークフロー・タスクの作成なし → ビルドやテストなどメモリを多く使う新規作業なし → 引き継ぎ・調整・状態確認・停止・復旧と小さな読み取りのみ`の順で狭めます。

   サブエージェント全体を一度に制限することはありません。時間に余裕があれば一つのサブエージェントについて、次のツール呼び出しから一段階だけ狭めます。時間が少なければ、回復ラインに達する前に必要な最小グループだけを制限し、メモリを測り直します。選ばれていないエージェントと実行中の作業はそのままです。先にツール範囲を狭めるサブエージェントは、(1) 関連プロセスの異常な増加を再確認できたもの、(2) 現在または直前のツールがエージェント・ワークフロー・タスクの作成、またはビルド・テストなどの重い作業だったもの、(3) すでに狭い段階にあるもの、(4) 関連プロセスが回復ラインに達するまでの時間が短いもの、(5) より新しく開始したもの、の順に選びます。

   主エージェントを制限するのは、すべてのサブエージェントが最も狭い段階になっても危険が残る場合だけです。ただし主エージェントが再確認済みの主因で、サブエージェントから制限していては間に合わない場合は、主エージェントを先に一段階だけ制限します。外部プログラムだけが原因なら既存のAI作業は維持し、新しいサブエージェント・ワークフロー・タスクの作成と、OSのメモリ逼迫が深刻なときの重い作業開始だけを待たせます。
6. **最後の手段として実行プロセスを一つだけ一時停止** — それでも危険が続き、Claude CodeまたはCodexに属する特定の実行プロセスが増え続けていることを確認できた場合にだけ、そのプロセスを終了せず一時停止します。処置はターミナルにすぐ表示され、主エージェントも次の作業前に同じ内容を受け取ります。
7. **逆の順序で復旧** — メモリの状態が安定すると、結果の受け渡しだけを許可していた段階から作業範囲を一段階ずつ戻し、一時停止したプロセスも一つずつ再開します。

目的はメモリ使用量を減らすことではありません。Claude Code・Codex CLIのターミナルセッションとCodex Desktop Appの会話を守りながら、できるだけ高い性能を長く維持することです。

<a id="installation"></a>
## インストール

ご使用の環境の**ターミナル**を開き、一致する 1 行のコマンドを貼り付けます。 Git、Python、Rust、または別個のインストーラーを準備する必要はありません。 インストールの範囲は現在のユーザーに限定されるため、`sudo` や管理者シェルは必要ありません。

<a id="linux-wsl2-macos-terminal"></a>
### Linux・WSL2・macOS端末

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

<a id="windows-powershell-terminal"></a>
### Windows PowerShell ターミナル

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

コマンドが完了するとバックグラウンドサービスが起動し、検出されたClaude CodeとCodexのHookも自動的に接続されます。実行中のAIプログラムや作業は終了しません。

> [!IMPORTANT]
> Windows向け実行ファイルは現在、[SignPath Foundation](https://signpath.org/)の認証審査中です。
> 審査が完了するまで、Windows 11ではSmart App Controlを**オフ**にして使用してください。
>
> - **Windows 11:** インストール前にSmart App Controlを**オフ**にし、Windows版を使用している間はそのままにします。
> - **Windows 10:** Smart App Controlはないため、追加設定なしでインストールできます。
> - **Codex AppのWSLエンジン:** WSL用のターミナルコマンドでインストールします。Windowsのセキュリティ設定を変更する必要はありません。
>
> Windows 11で再びオンにできる条件や、インストールがブロックされる環境については、
> [インストール・接続・対応環境](docs/guides/setup.ja.md#windows-powershell-terminal)を参照してください。

<a id="connect-the-programs-you-use"></a>
### 使用するプログラムを接続する

| プログラム | インストール後にやるべきこと |
| --- | --- |
| **Claude Code** | Hookは自動的に接続されます。すでに作業中なら、そのまま続けます。 |
| **Codex CLI** | 使用するCLIで`/hooks`を開き、Memory Supervisorの7項目がすべて**信頼済み・オン**であることを確認して、そのまま作業します。インストール前から別に開いていたCLIだけは、現在の作業を終えてから一度再起動します。 |
| **Codex Desktop App** | **設定 → Hooks**で7項目をすべて信頼してオンにします。既存の会話に戻って、もともと送る予定だった次のリクエストを送ればよく、アプリや会話を開き直す必要はありません。項目がまだ表示されない場合は、最大60秒待ってから設定を開き直します。 |

<a id="verify-installation"></a>
### インストールの確認

```bash
memory-status --connections
```

- `Core daemon CONNECTED`: バックグラウンド スーパーバイザは正常です。
- `Claude Code CONNECTED`: サポートされているバージョンとユーザー フックが接続されています。
- `Codex CONNECTED`: 7 つの CLI フックがすべてインストールされ、有効化され、信頼されています。
- `Codex App ACTIVE`: アプリフックの準備が整い、既存または新しいタスクから実際の呼び出しが到着しました。
- `NOT DETECTED` は、使用していないプログラムでは正常です。

行が異なる場合は、その行が報告する内容のみに基づいて行動してください。 すべての例外と正確なライブインストール動作は、[インストール、接続、およびサポートされる環境](docs/guides/setup.ja.md)に記載されています。

<a id="uninstall"></a>
### アンインストール

Calando を削除するには、Calando がインストールされている各環境でこれを 1 回実行します。

```bash
memory-supervisor uninstall
```

状態とユーザー設定を維持しながら、バックグラウンド サービス、実行可能ファイル、および Calando が所有するフックとスキルの接続を削除します。

<a id="how-it-works-in-30-seconds"></a>
## 30秒でわかる仕組み

CalandoはClaude CodeやCodexの前に入り込み、代わりにコマンドを実行するものではありません。OS環境ごとに小さな監視プログラムが一つ動き、空きメモリ、減少速度、メモリ逼迫の兆候、AI作業による増加を確認します。Hookは新しい作業を始める直前に最新の判断を受け取ります。

```text
┌──────────────────────┐    memory / PID    ┌──────────────────────┐
│ OS environment       │ ─────────────────► │ Calando              │
└──────────────────────┘                    │ forecast / brake     │
                                            └──────────┬───────────┘
                                                       │ decision
┌──────────────────────┐      pre-run hook  ┌──────────▼───────────┐
│ Claude Code / Codex  │ ─────────────────► │ allow / hold         │
│ CLI / App thread     │ ◄── reason/state ─ │ explain / recover    │
└──────────────────────┘                    └──────────────────────┘
```

1. **自動判定** — メモリ容量、残りの余裕、短期・長期の減少速度、次の作業で予想される増加量から、保護基準と制動距離を自動計算します。
2. **性能を優先** — 使用量が多くても安定していれば制限せず、十分な余裕がある急減も、実際の危険が近づくまでは観察だけにとどめます。
3. **新しい作業から余裕を作る** — 危険が近づいたときだけ、新しいサブエージェント・ワークフロー・タスクの作成を先に待たせます。必要なら、選んだエージェント一つの次のツールから段階的に範囲を狭めます。
4. **可逆的な最後の手段** — すべての緩衝策を取っても危険が続き、増え続けるClaude Code・Codexのプロセスを正確に確認できた場合にだけ、一つを終了せず一時停止します。
5. **逆の順序で復旧** — 空きメモリが安定すると作業範囲を一段階ずつ戻し、一時停止した作業も一つずつ再開します。

<a id="cli-versus-codex-desktop-app"></a>
### CLI と Codex Desktop App

| Claude Code および Codex CLI | Codex Desktop App |
| --- | --- |
| 端末セッションと子プロセスが分離されているため、原因となるプロセスと制御対象を比較的正確に結び付けることができます。 | 会話は 1 つの App Server 内の **論理スレッド** として区別されますが、メモリは共有されます。 これらは、独立した CLI プロセスのようには測定されません。 |
| フックはリード、サブエージェント、ツールを識別します。 最後の手段では、再検証されたローカル PID を 1 つだけ一時停止します。 | フックは会話ごとに新しい作業を制御し、最近のツール、サブエージェント、アクティビティ時間、および App Server の増加を関連付けます。 帰属が不確かな場合、スーパーバイザは共有メモリが 1 つのスレッドに属しているとはみなしません。 共有リスクに対して新しい作業を緩衝します。 App Server を一時停止することは、段階的な制御と持続的な成長が確認された後の、非常にまれな最後のステップです。 |

各 Windows、WSL2、macOS、Linux、VM、または分離コンテナ環境で 1 つのスーパーバイザが実行されます。 同じ物理メモリを共有する環境がフェデレーションを通じて接続されている場合、各スーパーバイザは独自のプロセスのみを制御しながら、いつ新しい作業を開始できるか、いつ制限を解除できるかを一緒に決定します。

完全なステージ ポリシー、両方のアーキテクチャ、およびフェデレーション トポロジは、[Calando の仕組み](docs/guides/how-it-works.ja.md) に保存されます。

<a id="common-commands"></a>
## 共通コマンド

| 目的 | 指示 |
| --- | --- |
| 現在のメモリと保護状態 | `memory-status` |
| あらゆる接続環境 | `memory-status --all` |
| Claude Code および Codex フック接続 | `memory-status --connections` |
| プログラムを更新し、統合を再接続します | `memory-supervisor update` |
| この環境で保護をオフまたはオンにする | `memory-supervisor off` / `memory-supervisor on` |
| 通知ルートを表示する | `memory-supervisor notifications show` |

一時停止された作業の処理、自動回復、手動再開、オプションのメモリのハードキャップ構成、Discord と Telegram の通知設定については、[操作、通知、および回復](docs/guides/operations.ja.md)を参照してください。

<a id="supported-environments-and-safety-boundary"></a>
## サポートされる環境と安全境界

| アイテム | サポートまたは境界 |
| --- | --- |
| **オペレーティング システム** | 64 ビット Intel/AMD 上の Linux および WSL2、Apple Silicon および Intel 上の macOS、64 ビット Intel/AMD 上の Windows 10 または 11 |
| **AI プログラム** | Claude Code 2.1.217 以降、Codex CLI 0.145.0 以降、Codex Desktop App |
| **常駐メモリ** | 各OSでの実測最大値は5.13 MiB。インストールした監視プログラム一つあたりの設計値は10 MiB未満 |
| **ネットワーク** | 通常の監視では、ネットワーク トラフィックや使用状況テレメトリは送信されません。 ネットワーク アクセスは、インストールとアップデートの場合、またはユーザーが明示的に有効にした Discord と Telegram の通知の場合にのみ発生します。 |
| **決して読まない** | プロンプト、会話、モデル応答、プロジェクト ファイルの内容、プロセス メモリの内容、または Claude/ChatGPT 資格情報 |
| **決してコントロールしない** | ブラウザや IDE などの他のプログラム、別の OS 環境の PID、またはメモリ、スワップ、VM 設定 |
| **自動的な物理制御** | 正確に再確認したClaude Code・Codexの作業プロセス一つを可逆的に一時停止するところまでです。自動終了や強制終了は行いません。 |

完全なデータとプロセス境界については[セキュリティ](docs/guides/security.ja.md)を、測定については[パフォーマンス](docs/guides/performance.ja.md)を、プラットフォーム固有の条件については[インストール、接続、およびサポートされる環境](docs/guides/setup.ja.md)を参照してください。

<a id="documentation"></a>
## ドキュメント

| トピック | 文書 |
| --- | --- |
| インストール、ライブセッション接続、フックトラスト、Windows、WSL2、macOS、Linux | [インストール、接続、サポート環境](docs/guides/setup.ja.md) |
| 段階的ブレーキ、CLI および Codex App アーキテクチャ、blind control、およびフェデレーション | [Memory Supervisor の仕組み](docs/guides/how-it-works.ja.md) |
| ターミナル、OS、Discord、および Telegram の通知、コマンド、一時停止、および回復 | [操作、通知、および回復](docs/guides/operations.ja.md) |
| 元の詳細な README を 1 つの文書で継続的に読む | [詳細ガイド](docs/detailed-guide.ja.md) |
| セキュリティ、パフォーマンス、テスト、およびあらゆる専門家のリファレンスを見つける | [ドキュメントインデックス](docs/README.ja.md) |

<a id="verification"></a>
## 検証

このプロジェクトでは、自動化された Rust テスト、E2E のインストール/更新/アンインストール、フック契約チェック、リポジトリのプライバシー境界チェック、および Linux、Windows、および macOS プラットフォームの検証が実行されます。 公開検証範囲については、[テスト範囲](docs/testing/test-matrix.ja.md)と[適応停止距離](docs/testing/stopping-distance.ja.md)を参照してください。

脆弱性を報告するには [セキュリティ ポリシー](.github/SECURITY.ja.md) を参照し、プロジェクトに取り組むには [貢献ガイド](.github/CONTRIBUTING.ja.md) を参照してください。

<a id="license"></a>
## ライセンス

[MIT](LICENSE)
