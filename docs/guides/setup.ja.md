# 設置・接続・対応環境

<p align="center">
  <a href="setup.md">English</a> · <a href="setup.ko.md">한국어</a> · <a href="setup.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

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
- App Server または CLI が別の WSL ディストリビューション、仮想マシン、または分離されたコンテナーで実行される場合は、そのような各環境にインストールします。 Windows と WSL は、federation パスを自動的に見つけます。 macOS または Linux ホスト、ダイナミック メモリ VM、およびコンテナーは、同じマシン上の共有フォルダーに接続します。 接続されると、同じ物理メモリを競合する環境は、新しい作業の決定を共有します。 固定メモリ VM と他のコンピューターまたはクラウド サーバーは、独立して自身を保護します。 境界については、[プラットフォームと複数環境の動作](platforms.ja.md)を参照してください。

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

容量計画の場合は、最小サンプルではなく、**インストールされているモニターごとに 10 MiB** を使用します。 詳細な条件と生データについては、[パフォーマンス測定](performance.ja.md)を参照してください。

1 台の物理コンピューターに複数の実行環境 (Windows、WSL ディストリビューション、仮想マシン、分離コンテナー) がある場合、Claude Code または Codex を実行するすべての環境にインストールします。 1つの環境内の複数の端末が1つのモニターを共有します。 各環境でのインストールと federation パスのセットアップ後、実行されているカーネルの数に関係なく、コンピュータ全体が最新の新しい作業の決定を自動的に共有します。 各モニターは依然として独自の環境のみを測定および制御するため、別の環境の PID で動作することはありません。 インストーラーは、Windows と WSL の同じローカル共有フォルダーに接続します。 VM またはコンテナは、ホスト共有ローカル フォルダーを federation パスとして使用します。 ネットワーク フォルダーは、別の物理コンピューターやクラウド サーバーへの接続には使用されません。 セットアップの詳細については、[プラットフォームとマルチ環境の動作](platforms.ja.md)を参照してください。
